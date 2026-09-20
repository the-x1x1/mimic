//! The layered voice engine.
//!
//! A person does not have one writing style. They have a default, a set of
//! per-channel adjustments (email is not SMS), a set of per-relationship
//! adjustments (their manager is not their brother), and situational habits
//! (declining is not thanking). Mimic models that as layers rather than as one
//! averaged profile, because the average of "Dear Dr Okafor" and "lol ok" is a
//! voice nobody has.
//!
//! Layers are computed independently over the user's own messages, each
//! narrowed by its scope. A layer with too few messages is written with its
//! sample size and no metrics, so the UI can say "not enough yet" instead of
//! drawing a confident line through three data points.
//!
//! Resolution at generation time is outermost-to-innermost: global, then
//! channel, then relationship, then situation, then the user's manual
//! overrides, which beat everything.

pub mod metrics;

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::db::{Db, DbError, Message, VoiceLayer};
use crate::jobs::{JobContext, JobError, JobExecutor, JobFuture};
use crate::version::ANALYSIS_VERSION;
use metrics::{Sample, VoiceMetrics, MIN_SAMPLE};

pub const JOB_KIND: &str = "analyze_voice";

/// Messages pulled per page while analyzing. Analysis never holds the whole
/// corpus; it accumulates counts.
const PAGE: usize = 1000;

/// How many representative examples each scope keeps.
const EXAMPLES: usize = 6;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisSummary {
    pub messages_considered: usize,
    pub profiles_written: usize,
    /// Scopes that were skipped for having fewer than `MIN_SAMPLE` messages.
    pub scopes_below_threshold: usize,
    pub examples_selected: usize,
}

/// The resolved voice for one generation: the layers that applied, folded in
/// order, with the evidence that produced each one.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedVoice {
    pub layers: Vec<ResolvedLayer>,
    /// Manual overrides, innermost last. These are applied after every layer.
    pub overrides: Vec<(String, serde_json::Value)>,
    pub examples: Vec<crate::db::RepresentativeExample>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedLayer {
    pub layer: String,
    pub scope_key: String,
    pub label: String,
    pub sample_size: i64,
    pub measurable: bool,
    pub metrics: VoiceMetrics,
    pub stale: bool,
}

/// Recompute every layer from the user's own messages.
pub fn analyze(db: &Db, on_progress: &mut dyn FnMut(&str, usize)) -> Result<AnalysisSummary, DbError> {
    let run_id = db.start_analysis_run("voice", ANALYSIS_VERSION, &json!({}))?;
    let mut summary = AnalysisSummary::default();

    // Global.
    on_progress("global", 0);
    let global = collect(db, None, None)?;
    summary.messages_considered += global.len();
    write_scope(db, VoiceLayer::Global, "", None, "Everything you write", &global, &mut summary)?;

    // Per channel.
    for (channel, _) in db.self_message_channels()? {
        on_progress(&channel, summary.messages_considered);
        let msgs = collect(db, Some(&channel), None)?;
        let label = format!("What you write over {channel}");
        write_scope(db, VoiceLayer::Channel, &channel, None, &label, &msgs, &mut summary)?;
    }

    // Per relationship. Only people the user has actually written to enough.
    for person in db.list_participants(10_000)? {
        if person.sent_by_user < MIN_SAMPLE as i64 {
            summary.scopes_below_threshold += 1;
            continue;
        }
        let id = person.participant.id.clone();
        on_progress(&person.participant.display_name, summary.messages_considered);
        let msgs = collect(db, None, Some(&id))?;
        let label = format!("What you write to {}", person.participant.display_name);
        write_scope(db, VoiceLayer::Relationship, &id, Some(&id), &label, &msgs, &mut summary)?;
    }

    db.finish_analysis_run(
        &run_id,
        "completed",
        summary.messages_considered as i64,
        summary.profiles_written as i64,
        None,
    )?;
    Ok(summary)
}

fn collect(db: &Db, channel: Option<&str>, participant_id: Option<&str>) -> Result<Vec<Message>, DbError> {
    let mut out = Vec::new();
    let mut cursor: Option<(String, String)> = None;
    loop {
        let page = db.page_self_messages(
            channel,
            participant_id,
            cursor.as_ref().map(|(a, b)| (a.as_str(), b.as_str())),
            PAGE,
        )?;
        if page.is_empty() {
            break;
        }
        let last = page.last().unwrap();
        cursor = Some((last.sent_at.clone().unwrap_or_default(), last.id.clone()));
        out.extend(page);
    }
    Ok(out)
}

fn write_scope(
    db: &Db,
    layer: VoiceLayer,
    scope_key: &str,
    participant_id: Option<&str>,
    label: &str,
    messages: &[Message],
    summary: &mut AnalysisSummary,
) -> Result<(), DbError> {
    let samples: Vec<Sample<'_>> = messages
        .iter()
        .map(|m| Sample { body: &m.body, response_latency_seconds: m.response_latency_seconds })
        .collect();
    let m = metrics::compute(&samples);
    let qualitative = json!({ "label": label });
    db.put_voice_profile(
        layer,
        scope_key,
        participant_id,
        &serde_json::to_value(&m).unwrap_or(json!({})),
        &qualitative,
        m.sample_size as i64,
        ANALYSIS_VERSION,
    )?;
    summary.profiles_written += 1;
    if !m.measurable {
        summary.scopes_below_threshold += 1;
        // No examples either: six messages cherry-picked from ten is not a
        // representative set, it is the set.
        db.put_representative_examples(layer, scope_key, participant_id, &[])?;
        return Ok(());
    }
    let chosen = select_examples(messages, &m, EXAMPLES);
    summary.examples_selected += chosen.len();
    db.put_representative_examples(layer, scope_key, participant_id, &chosen)?;
    Ok(())
}

/// Pick the messages that best show how this person writes in this scope.
///
/// Scoring is deliberately simple and deterministic: a message scores well
/// when its length is close to the scope's median and when it carries the
/// features the scope is characterized by (a greeting if they greet, a sign-off
/// if they sign off, an emoji if they use them). Ties break on message id so
/// two runs over the same data choose the same examples.
///
/// Near-duplicates are suppressed, because six copies of "sounds good" teach a
/// model less than one.
fn select_examples(messages: &[Message], m: &VoiceMetrics, limit: usize) -> Vec<(String, String, f64)> {
    let median = m.median_words_per_message.unwrap_or(0.0).max(1.0);
    let mut scored: Vec<(f64, String, String, String)> = messages
        .iter()
        .filter(|msg| msg.word_count >= 3)
        .map(|msg| {
            let len_ratio = (msg.word_count as f64 / median).min(median / msg.word_count.max(1) as f64);
            let mut score = len_ratio;
            let mut reasons: Vec<&str> = vec!["typical length"];
            if m.greeting_rate.unwrap_or(0.0) > 0.4 && msg.body.len() > 8 {
                let opens = m.top_greetings.iter().any(|(g, _)| msg.body.to_lowercase().starts_with(g));
                if opens {
                    score += 0.25;
                    reasons.push("your usual opener");
                }
            }
            if m.sign_off_rate.unwrap_or(0.0) > 0.4 {
                let closes = m
                    .top_sign_offs
                    .iter()
                    .any(|(s, _)| msg.body.to_lowercase().trim_end().rsplit('\n').next().unwrap_or("").contains(s));
                if closes {
                    score += 0.25;
                    reasons.push("your usual sign-off");
                }
            }
            let uses_phrase = m.top_phrases.iter().any(|(p, _)| msg.body.to_lowercase().contains(p.as_str()));
            if uses_phrase {
                score += 0.3;
                reasons.push("a phrase you repeat");
            }
            (score, msg.id.clone(), reasons.join(", "), normalize_for_dedupe(&msg.body))
        })
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal).then(a.1.cmp(&b.1)));

    let mut seen: Vec<String> = Vec::new();
    let mut out = Vec::new();
    for (score, id, reason, fingerprint) in scored {
        if seen.contains(&fingerprint) {
            continue;
        }
        seen.push(fingerprint);
        out.push((id, reason, (score * 1000.0).round() / 1000.0));
        if out.len() == limit {
            break;
        }
    }
    out
}

fn normalize_for_dedupe(body: &str) -> String {
    body.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Assemble the voice that applies to one generation.
pub fn resolve(db: &Db, channel: &str, participant_id: Option<&str>) -> Result<ResolvedVoice, DbError> {
    let mut layers = Vec::new();
    let mut scopes: Vec<(VoiceLayer, String)> = vec![(VoiceLayer::Global, String::new())];
    if let Some(row) = db.get_voice_profile(VoiceLayer::Global, "", ANALYSIS_VERSION)? {
        layers.push(to_layer(row));
    }
    if let Some(row) = db.get_voice_profile(VoiceLayer::Channel, channel, ANALYSIS_VERSION)? {
        layers.push(to_layer(row));
        scopes.push((VoiceLayer::Channel, channel.to_string()));
    }
    let mut examples = db.representative_examples(VoiceLayer::Global, "", EXAMPLES)?;
    if let Some(pid) = participant_id {
        if let Some(row) = db.get_voice_profile(VoiceLayer::Relationship, pid, ANALYSIS_VERSION)? {
            layers.push(to_layer(row));
            scopes.push((VoiceLayer::Relationship, pid.to_string()));
        }
        // Examples from the actual relationship are worth more than generic
        // ones, so they replace rather than supplement when they exist.
        let theirs = db.representative_examples(VoiceLayer::Relationship, pid, EXAMPLES)?;
        if !theirs.is_empty() {
            examples = theirs;
        }
    }
    let overrides = db.voice_preferences_for(&scopes)?.into_iter().map(|p| (p.key, p.value)).collect::<Vec<_>>();
    Ok(ResolvedVoice { layers, overrides, examples })
}

fn to_layer(row: crate::db::VoiceProfileRow) -> ResolvedLayer {
    let metrics: VoiceMetrics = serde_json::from_value(row.metrics).unwrap_or_default();
    ResolvedLayer {
        layer: row.layer,
        scope_key: row.scope_key,
        label: row.qualitative.get("label").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
        sample_size: row.sample_size,
        measurable: metrics.measurable,
        metrics,
        stale: row.stale,
    }
}

/// Fold the layers into the numbers a prompt should actually use: the
/// innermost layer that is measurable wins each metric.
pub fn effective_metrics(voice: &ResolvedVoice) -> VoiceMetrics {
    let mut out = VoiceMetrics::default();
    for layer in &voice.layers {
        if !layer.measurable {
            continue;
        }
        out.sample_size = layer.sample_size as usize;
        out.measurable = true;
        let m = &layer.metrics;
        macro_rules! take {
            ($($f:ident),*) => { $( if m.$f.is_some() { out.$f = m.$f; } )* };
        }
        take!(
            avg_words_per_message,
            median_words_per_message,
            p90_words_per_message,
            avg_sentences_per_message,
            multi_paragraph_rate,
            terminal_period_rate,
            question_rate,
            exclamation_rate,
            ellipsis_rate,
            emoji_rate,
            lowercase_start_rate,
            all_lowercase_rate,
            contractions_per_100_words,
            greeting_rate,
            sign_off_rate,
            median_response_seconds
        );
        if !m.top_greetings.is_empty() {
            out.top_greetings = m.top_greetings.clone();
        }
        if !m.top_sign_offs.is_empty() {
            out.top_sign_offs = m.top_sign_offs.clone();
        }
        if !m.top_phrases.is_empty() {
            out.top_phrases = m.top_phrases.clone();
        }
    }
    out
}

pub struct AnalyzeExecutor;

impl AnalyzeExecutor {
    pub fn shared() -> Arc<dyn JobExecutor> {
        Arc::new(AnalyzeExecutor)
    }
}

impl JobExecutor for AnalyzeExecutor {
    fn kinds(&self) -> &'static [&'static str] {
        &[JOB_KIND]
    }
    fn resumable(&self, _kind: &str) -> bool {
        // Analysis is a full recompute; resuming a partial one would leave
        // layers from two different corpora side by side.
        false
    }
    fn execute(&self, ctx: JobContext) -> JobFuture {
        Box::pin(async move {
            let db = ctx.db.clone();
            let progress_ctx = ctx.clone();
            let mut on_progress =
                move |scope: &str, done: usize| progress_ctx.progress(done as i64, 0, &format!("analyzing {scope}"));
            let summary = tokio::task::block_in_place(|| analyze(&db, &mut on_progress)).map_err(JobError::Db)?;
            Ok(serde_json::to_value(summary).unwrap_or(serde_json::Value::Null))
        })
    }
}

/// Counts for the Voice screen. Every field is measured, not estimated.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceOverview {
    pub analysis_version: String,
    pub own_messages: i64,
    pub channels: Vec<(String, i64)>,
    pub profiles: Vec<ResolvedLayer>,
    pub people_with_profiles: usize,
    pub stale: bool,
    pub last_analyzed_at: Option<String>,
    /// How many of the user's own messages are still needed before the global
    /// profile becomes measurable. `0` once it is.
    pub messages_until_measurable: i64,
}

pub fn overview(db: &Db) -> Result<VoiceOverview, DbError> {
    let profiles: Vec<ResolvedLayer> = db.list_voice_profiles(ANALYSIS_VERSION)?.into_iter().map(to_layer).collect();
    let own = db.count_self_messages(None, None)?;
    let last = db.last_analysis_run("voice")?;
    Ok(VoiceOverview {
        analysis_version: ANALYSIS_VERSION.to_string(),
        own_messages: own,
        channels: db.self_message_channels()?,
        people_with_profiles: profiles.iter().filter(|p| p.layer == "relationship").count(),
        stale: profiles.iter().any(|p| p.stale) || profiles.is_empty(),
        last_analyzed_at: last.and_then(|r| r.completed_at),
        messages_until_measurable: (MIN_SAMPLE as i64 - own).max(0),
        profiles,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{IdentifierInput, IdentifierKind, NewMessage, NewSource};

    /// Build a database with `n` messages from the user to `person`, plus the
    /// other side of the conversation.
    fn seeded(channel: &str, n: usize) -> (Db, String) {
        let db = Db::open_in_memory().unwrap();
        db.set_user_identity("C").unwrap();
        db.add_user_identifier(IdentifierKind::Handle, "@c").unwrap();
        let source = db
            .create_source(&NewSource {
                connector: "test".into(),
                name: "T".into(),
                channel: channel.into(),
                location: None,
                config: serde_json::Value::Null,
            })
            .unwrap();
        let convo = db.upsert_conversation(&source.id, "t1", channel, Some("Thread")).unwrap();
        let ada =
            db.resolve_participant("Ada", &[IdentifierInput::new(IdentifierKind::Handle, "@ada")], false).unwrap();
        db.link_conversation_participant(&convo, &ada).unwrap();
        let mut batch = Vec::new();
        for i in 0..n {
            batch.push(NewMessage {
                conversation_id: convo.clone(),
                source_id: source.id.clone(),
                participant_id: Some(ada.clone()),
                external_id: format!("in{i}"),
                direction: "other".into(),
                channel: channel.into(),
                sent_at: Some(format!("2026-02-{:02}T09:00:00Z", (i % 27) + 1)),
                sequence_index: (i * 2) as i64,
                body: "any thoughts?".into(),
                reply_to_external_id: None,
                metadata: serde_json::Value::Null,
            });
            batch.push(NewMessage {
                conversation_id: convo.clone(),
                source_id: source.id.clone(),
                participant_id: None,
                external_id: format!("out{i}"),
                direction: "self".into(),
                channel: channel.into(),
                sent_at: Some(format!("2026-02-{:02}T09:05:00Z", (i % 27) + 1)),
                sequence_index: (i * 2 + 1) as i64,
                body: format!("yeah let me know if that works, thing {i}"),
                reply_to_external_id: None,
                metadata: serde_json::Value::Null,
            });
        }
        db.insert_messages(&batch).unwrap();
        db.link_replies(&convo).unwrap();
        db.refresh_conversation_stats(&convo).unwrap();
        (db, ada)
    }

    #[test]
    fn analysis_writes_a_layer_per_scope_and_says_what_it_could_not_measure() {
        let (db, ada) = seeded("chat", 30);
        let summary = analyze(&db, &mut |_, _| {}).unwrap();
        assert_eq!(summary.messages_considered, 30);
        assert_eq!(summary.profiles_written, 3, "global, chat, and Ada");
        assert!(summary.examples_selected > 0);

        let global = db.get_voice_profile(VoiceLayer::Global, "", ANALYSIS_VERSION).unwrap().unwrap();
        assert_eq!(global.sample_size, 30);
        assert!(!global.stale);
        let rel = db.get_voice_profile(VoiceLayer::Relationship, &ada, ANALYSIS_VERSION).unwrap().unwrap();
        assert_eq!(rel.sample_size, 30);
        assert_eq!(rel.participant_id.as_deref(), Some(ada.as_str()));
    }

    #[test]
    fn a_person_the_user_barely_writes_to_gets_no_profile() {
        let (db, _) = seeded("chat", 5);
        let summary = analyze(&db, &mut |_, _| {}).unwrap();
        assert_eq!(summary.profiles_written, 2, "global and channel are still written, with their sample size");
        assert!(summary.scopes_below_threshold >= 1);
        let global = db.get_voice_profile(VoiceLayer::Global, "", ANALYSIS_VERSION).unwrap().unwrap();
        assert_eq!(global.metrics["sampleSize"], 5);
        assert_eq!(global.metrics["measurable"], false);
        assert!(global.metrics["avgWordsPerMessage"].is_null(), "no numbers from five messages");
        assert!(db.representative_examples(VoiceLayer::Global, "", 10).unwrap().is_empty());
    }

    #[test]
    fn resolution_stacks_layers_and_puts_overrides_last() {
        let (db, ada) = seeded("chat", 30);
        analyze(&db, &mut |_, _| {}).unwrap();
        db.set_voice_preference(VoiceLayer::Global, "", "signOff", &json!("— C"), None).unwrap();
        db.set_voice_preference(VoiceLayer::Relationship, &ada, "signOff", &json!("c"), None).unwrap();

        let v = resolve(&db, "chat", Some(&ada)).unwrap();
        assert_eq!(v.layers.len(), 3);
        assert_eq!(v.layers[0].layer, "global");
        assert_eq!(v.layers[2].layer, "relationship");
        assert_eq!(v.overrides.last().unwrap().1, json!("c"), "the innermost override wins");
        assert!(!v.examples.is_empty());

        // A channel the user has never used contributes no layer.
        let email = resolve(&db, "email", None).unwrap();
        assert_eq!(email.layers.len(), 1);
    }

    #[test]
    fn effective_metrics_prefer_the_innermost_measurable_layer() {
        let (db, ada) = seeded("chat", 30);
        analyze(&db, &mut |_, _| {}).unwrap();
        // Hand-write a relationship profile that differs from the global one.
        db.put_voice_profile(
            VoiceLayer::Relationship,
            &ada,
            Some(&ada),
            &json!({"sampleSize": 40, "measurable": true, "avgWordsPerMessage": 3.0}),
            &json!({"label": "to Ada"}),
            40,
            ANALYSIS_VERSION,
        )
        .unwrap();
        let v = resolve(&db, "chat", Some(&ada)).unwrap();
        let eff = effective_metrics(&v);
        assert_eq!(eff.avg_words_per_message, Some(3.0), "the relationship layer overrides the global one");
        // A metric the inner layer does not carry still comes from outside it.
        assert!(eff.terminal_period_rate.is_some());
    }

    #[test]
    fn example_selection_is_deterministic_and_avoids_duplicates() {
        let (db, _) = seeded("chat", 30);
        analyze(&db, &mut |_, _| {}).unwrap();
        let a: Vec<String> =
            db.representative_examples(VoiceLayer::Global, "", 10).unwrap().into_iter().map(|e| e.message_id).collect();
        analyze(&db, &mut |_, _| {}).unwrap();
        let b: Vec<String> =
            db.representative_examples(VoiceLayer::Global, "", 10).unwrap().into_iter().map(|e| e.message_id).collect();
        assert_eq!(a, b, "re-analyzing the same corpus picks the same examples");
        assert_eq!(a.len(), EXAMPLES);
    }

    #[test]
    fn near_duplicate_messages_are_not_all_chosen() {
        let db = Db::open_in_memory().unwrap();
        db.set_user_identity("C").unwrap();
        db.add_user_identifier(IdentifierKind::Handle, "@c").unwrap();
        let source = db
            .create_source(&NewSource {
                connector: "test".into(),
                name: "T".into(),
                channel: "chat".into(),
                location: None,
                config: serde_json::Value::Null,
            })
            .unwrap();
        let convo = db.upsert_conversation(&source.id, "t", "chat", None).unwrap();
        let mut batch = Vec::new();
        for i in 0..25 {
            // Twenty identical messages plus five distinct ones.
            let body = if i < 20 { "sounds good to me".to_string() } else { format!("distinct message number {i}") };
            batch.push(NewMessage {
                conversation_id: convo.clone(),
                source_id: source.id.clone(),
                participant_id: None,
                external_id: format!("m{i}"),
                direction: "self".into(),
                channel: "chat".into(),
                sent_at: Some(format!("2026-02-01T09:{:02}:00Z", i)),
                sequence_index: i as i64,
                body,
                reply_to_external_id: None,
                metadata: serde_json::Value::Null,
            });
        }
        db.insert_messages(&batch).unwrap();
        analyze(&db, &mut |_, _| {}).unwrap();
        let bodies: Vec<String> =
            db.representative_examples(VoiceLayer::Global, "", 10).unwrap().into_iter().map(|e| e.body).collect();
        let identical = bodies.iter().filter(|b| *b == "sounds good to me").count();
        assert_eq!(identical, 1, "one copy of a repeated message is enough: {bodies:?}");
    }

    #[test]
    fn the_overview_reports_what_is_missing_rather_than_a_score() {
        let db = Db::open_in_memory().unwrap();
        let o = overview(&db).unwrap();
        assert_eq!(o.own_messages, 0);
        assert_eq!(o.messages_until_measurable, MIN_SAMPLE as i64);
        assert!(o.stale, "no analysis has been run");
        assert!(o.profiles.is_empty());
        assert_eq!(o.last_analyzed_at, None);

        let (db, _) = seeded("chat", 30);
        analyze(&db, &mut |_, _| {}).unwrap();
        let o = overview(&db).unwrap();
        assert_eq!(o.own_messages, 30);
        assert_eq!(o.messages_until_measurable, 0);
        assert!(!o.stale);
        assert_eq!(o.people_with_profiles, 1);
        assert!(o.last_analyzed_at.is_some());
    }
}
