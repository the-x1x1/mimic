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

pub mod describe;
pub mod metrics;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::db::{Db, DbError, Message, SelfScope, VoiceLayer};
use crate::jobs::{JobContext, JobError, JobExecutor, JobFuture};
use crate::version::ANALYSIS_VERSION;
use metrics::{Accumulator, Sample, VoiceMetrics, MIN_SAMPLE};

pub const JOB_KIND: &str = "analyze_voice";

/// The same analysis, measuring only what changed ([`Mode::Changed`]). A kind
/// of its own because it runs by itself as mail comes in, and the screen says
/// nothing each time one finishes.
pub const CHANGED_JOB_KIND: &str = "measure_voice_changes";

/// Messages read per page while analyzing. A scope is read a page at a time
/// and counted as it goes, twice — once for its numbers, once for its
/// examples — so however many messages it holds, only a page of them is in
/// memory at once.
const PAGE: usize = 1000;

/// How many representative examples each scope keeps.
const EXAMPLES: usize = 6;

/// Which scopes an analysis measures.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Every scope, whatever was measured before: what the user asks for.
    #[default]
    Everything,
    /// Only what something changed under since it was measured — a profile
    /// marked stale, a scope never measured, a situation whose filing moved —
    /// and nothing else. What runs by itself after new messages come in.
    Changed,
}

impl Mode {
    /// The mode a job of `kind` measures in.
    pub fn of_kind(kind: &str) -> Self {
        if kind == CHANGED_JOB_KIND {
            Mode::Changed
        } else {
            Mode::Everything
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisSummary {
    /// The user's own messages, all of them read to file them by situation.
    pub messages_considered: usize,
    /// Of the user's own messages, how many were filed under a situation.
    pub messages_classified: usize,
    pub profiles_written: usize,
    /// Profiles left as they were: nothing in them changed since they were
    /// measured. Always none when everything is measured.
    pub profiles_kept: usize,
    /// Profiles removed because what they described is gone — a channel the
    /// user no longer has a message in, someone they now have too few
    /// messages to, a situation nothing is filed under.
    pub profiles_removed: usize,
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
    /// A model's reading of the numbers, in words, when one was asked for
    /// (`describe`): shown beside the numbers, never instead of them.
    pub reading: Option<describe::Reading>,
}

/// Recompute every layer from the user's own messages.
pub fn analyze(db: &Db, on_progress: &mut dyn FnMut(&str, usize)) -> Result<AnalysisSummary, DbError> {
    analyze_in(db, Mode::Everything, on_progress)
}

/// Measure the layers `mode` asks for from the user's own messages, and
/// remove the ones whose scope is gone.
///
/// Every message is in the layer over everything, so that layer is read in
/// full whenever anything of the user's changed; a channel, a person or a
/// situation nothing changed in is left as it was.
pub fn analyze_in(db: &Db, mode: Mode, on_progress: &mut dyn FnMut(&str, usize)) -> Result<AnalysisSummary, DbError> {
    let run_id = db.start_analysis_run("voice", ANALYSIS_VERSION, &json!({ "mode": mode }))?;
    let mut summary = AnalysisSummary::default();

    // Situations first: the situational layers below are computed over what
    // this pass files, and a stale filing would describe last week's corpus.
    on_progress("what each message is doing", 0);
    let classified = crate::situations::classify_corpus(db, &mut |_| {})?;
    summary.messages_considered = classified.messages_considered;
    summary.messages_classified = classified.messages_classified;

    // What was measured before, and whether something changed under it since.
    let before: HashMap<(String, String), bool> =
        db.list_voice_profiles(ANALYSIS_VERSION)?.into_iter().map(|p| ((p.layer, p.scope_key), p.stale)).collect();
    let due = |layer: VoiceLayer, key: &str| {
        mode == Mode::Everything
            || before.get(&(layer.as_str().to_string(), key.to_string())).is_none_or(|stale| *stale)
    };
    let gone = |layer: VoiceLayer, kept: &HashSet<&str>| -> Vec<String> {
        before
            .keys()
            .filter(|(l, k)| l == layer.as_str() && !kept.contains(k.as_str()))
            .map(|(_, k)| k.clone())
            .collect()
    };

    // Global.
    if due(VoiceLayer::Global, "") {
        on_progress("global", 0);
        measure(db, VoiceLayer::Global, "", None, "Everything you write", SelfScope::default(), &mut summary)?;
    } else {
        summary.profiles_kept += 1;
    }

    // Per channel.
    let channels = db.self_message_channels()?;
    for (channel, _) in &channels {
        if !due(VoiceLayer::Channel, channel) {
            summary.profiles_kept += 1;
            continue;
        }
        on_progress(channel, summary.profiles_written);
        let scope = SelfScope { channel: Some(channel.as_str()), ..Default::default() };
        let label = format!("What you write over {channel}");
        measure(db, VoiceLayer::Channel, channel, None, &label, scope, &mut summary)?;
    }
    let kept: HashSet<&str> = channels.iter().map(|(c, _)| c.as_str()).collect();
    for channel in gone(VoiceLayer::Channel, &kept) {
        db.delete_voice_scope(VoiceLayer::Channel, &channel)?;
        summary.profiles_removed += 1;
    }

    // Per relationship. Only people the user has actually written to enough.
    let people = db.people_written_to()?;
    let mut enough: HashSet<&str> = HashSet::new();
    for (id, name, sent) in &people {
        if *sent < MIN_SAMPLE as i64 {
            summary.scopes_below_threshold += 1;
            continue;
        }
        enough.insert(id.as_str());
        if !due(VoiceLayer::Relationship, id) {
            summary.profiles_kept += 1;
            continue;
        }
        on_progress(name, summary.profiles_written);
        let scope = SelfScope { participant_id: Some(id.as_str()), ..Default::default() };
        let label = format!("What you write to {name}");
        measure(db, VoiceLayer::Relationship, id, Some(id.as_str()), &label, scope, &mut summary)?;
    }
    for id in gone(VoiceLayer::Relationship, &enough) {
        db.delete_voice_scope(VoiceLayer::Relationship, &id)?;
        summary.profiles_removed += 1;
    }

    // Per situation. A situation with nothing filed under it has no layer;
    // one with a few messages writes its sample size and no metrics, like any
    // other scope below the floor, so the screen can say how far off it is.
    for (situation_id, n) in &classified.per_situation {
        if *n == 0 {
            if before.contains_key(&(VoiceLayer::Situational.as_str().to_string(), situation_id.clone())) {
                summary.profiles_removed += 1;
            }
            db.delete_voice_scope(VoiceLayer::Situational, situation_id)?;
            continue;
        }
        let Some(b) = crate::situations::builtin(situation_id) else { continue };
        // Filing marked a layer a message joined or left stale already.
        if !due(VoiceLayer::Situational, situation_id) {
            summary.profiles_kept += 1;
            continue;
        }
        on_progress(b.label, summary.profiles_written);
        let scope = SelfScope { situation_id: Some(situation_id.as_str()), ..Default::default() };
        measure(db, VoiceLayer::Situational, situation_id, None, b.layer_label, scope, &mut summary)?;
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

/// Whether an analysis of what changed has something to do: how the user
/// writes has been measured at this version, and something it was measured
/// over has changed since. Nothing before the first analysis — that one is
/// the user's to start.
pub fn wants_measuring(db: &Db) -> Result<bool, DbError> {
    let profiles = db.list_voice_profiles(ANALYSIS_VERSION)?;
    if !profiles.iter().any(|p| p.layer == VoiceLayer::Global.as_str()) {
        return Ok(false);
    }
    if profiles.iter().any(|p| p.stale) {
        return Ok(true);
    }
    // A situation with messages filed under it and no layer yet — the user or
    // a model filed the first ones — has nothing to mark stale.
    let has_layer =
        |id: &str| profiles.iter().any(|p| p.layer == VoiceLayer::Situational.as_str() && p.scope_key == id);
    Ok(db.self_situation_counts()?.iter().any(|(id, n)| *n > 0 && !has_layer(id)))
}

/// Hand every one of the user's messages in a scope to `f`, oldest first, a
/// page at a time. How many there were.
fn each_message(db: &Db, scope: SelfScope<'_>, f: &mut dyn FnMut(&Message)) -> Result<usize, DbError> {
    let mut n = 0;
    let mut cursor: Option<(String, String)> = None;
    loop {
        let page = db.page_self_messages_in(&scope, cursor.as_ref().map(|(a, b)| (a.as_str(), b.as_str())), PAGE)?;
        let Some(last) = page.last() else { break };
        cursor = Some((last.sent_at.clone().unwrap_or_default(), last.id.clone()));
        n += page.len();
        for m in &page {
            f(m);
        }
    }
    Ok(n)
}

fn sample(m: &Message) -> Sample<'_> {
    Sample { body: &m.body, response_latency_seconds: m.response_latency_seconds }
}

/// Measure one scope and choose its examples: one read of its messages for
/// the numbers, a second to score each message against them.
fn measure(
    db: &Db,
    layer: VoiceLayer,
    scope_key: &str,
    participant_id: Option<&str>,
    label: &str,
    scope: SelfScope<'_>,
    summary: &mut AnalysisSummary,
) -> Result<(), DbError> {
    let mut acc = Accumulator::default();
    each_message(db, scope, &mut |m| acc.push(&sample(m)))?;
    let m = acc.finish();
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
    let mut picker = ExamplePicker::new(&m, EXAMPLES);
    each_message(db, scope, &mut |msg| picker.offer(msg))?;
    let chosen = picker.finish();
    summary.examples_selected += chosen.len();
    db.put_representative_examples(layer, scope_key, participant_id, &chosen)?;
    Ok(())
}

/// Picks the messages that best show how this person writes in a scope, one
/// message at a time.
///
/// Scoring is deliberately simple and deterministic: a message scores well
/// when its length is close to the scope's median and when it carries the
/// features the scope is characterized by (a greeting if they greet, a
/// sign-off if they sign off, a phrase they repeat). Ties break on message id
/// so two runs over the same data choose the same examples.
///
/// Near-duplicates are suppressed, because six copies of "sounds good" teach a
/// model less than one: each wording counts once, by its best message.
///
/// Only `limit` wordings are held. That loses nothing: a wording is let go
/// only when `limit` others each have a better message, and then it could
/// never have been among the best `limit`.
struct ExamplePicker<'m> {
    metrics: &'m VoiceMetrics,
    median: f64,
    limit: usize,
    /// Wording -> its best message so far: `(score, id, reasons)`.
    held: HashMap<String, (f64, String, String)>,
}

/// How two `(score, message id)` rank: the higher score first, then the
/// smaller id.
fn rank(a: (f64, &str), b: (f64, &str)) -> std::cmp::Ordering {
    a.0.total_cmp(&b.0).then_with(|| b.1.cmp(a.1))
}

impl<'m> ExamplePicker<'m> {
    fn new(metrics: &'m VoiceMetrics, limit: usize) -> Self {
        let median = metrics.median_words_per_message.unwrap_or(0.0).max(1.0);
        Self { metrics, median, limit, held: HashMap::new() }
    }

    fn offer(&mut self, msg: &Message) {
        if msg.word_count < 3 || self.limit == 0 {
            return;
        }
        let (score, reasons) = self.score(msg);
        let wording = normalize_for_dedupe(&msg.body);
        if let Some(best) = self.held.get_mut(&wording) {
            if rank((score, &msg.id), (best.0, &best.1)).is_gt() {
                *best = (score, msg.id.clone(), reasons);
            }
            return;
        }
        if self.held.len() == self.limit {
            let worst = self
                .held
                .iter()
                .min_by(|(_, a), (_, b)| rank((a.0, &a.1), (b.0, &b.1)))
                .map(|(w, c)| (w.clone(), c.0, c.1.clone()));
            match worst {
                Some((w, s, id)) if rank((score, &msg.id), (s, &id)).is_gt() => {
                    self.held.remove(&w);
                }
                _ => return,
            }
        }
        self.held.insert(wording, (score, msg.id.clone(), reasons));
    }

    fn score(&self, msg: &Message) -> (f64, String) {
        let m = self.metrics;
        let median = self.median;
        let len_ratio = (msg.word_count as f64 / median).min(median / msg.word_count.max(1) as f64);
        let mut score = len_ratio;
        let mut reasons: Vec<&str> = vec!["typical length"];
        let lower = msg.body.to_lowercase();
        if m.greeting_rate.unwrap_or(0.0) > 0.4 && msg.body.len() > 8 {
            let opens = m.top_greetings.iter().any(|(g, _)| lower.starts_with(g));
            if opens {
                score += 0.25;
                reasons.push("your usual opener");
            }
        }
        if m.sign_off_rate.unwrap_or(0.0) > 0.4 {
            let last_line = lower.trim_end().rsplit('\n').next().unwrap_or("");
            if m.top_sign_offs.iter().any(|(s, _)| last_line.contains(s)) {
                score += 0.25;
                reasons.push("your usual sign-off");
            }
        }
        if m.top_phrases.iter().any(|(p, _)| lower.contains(p.as_str())) {
            score += 0.3;
            reasons.push("a phrase you repeat");
        }
        (score, reasons.join(", "))
    }

    /// The best `limit` wordings' messages, best first:
    /// `(message id, reasons, score)`.
    fn finish(self) -> Vec<(String, String, f64)> {
        let mut chosen: Vec<(f64, String, String)> = self.held.into_values().collect();
        chosen.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        chosen.into_iter().map(|(score, id, reasons)| (id, reasons, (score * 1000.0).round() / 1000.0)).collect()
    }
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
///
/// `situation_id` is what the reply is doing, when that is known — chosen by
/// the user or read from their note. Its layer sits inside the relationship
/// layer, and its examples go first, because six times the user said no are
/// a better guide to saying no than six typical messages.
pub fn resolve(
    db: &Db,
    channel: &str,
    participant_id: Option<&str>,
    situation_id: Option<&str>,
) -> Result<ResolvedVoice, DbError> {
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
    if let Some(sid) = situation_id {
        if let Some(row) = db.get_voice_profile(VoiceLayer::Situational, sid, ANALYSIS_VERSION)? {
            layers.push(to_layer(row));
            scopes.push((VoiceLayer::Situational, sid.to_string()));
        }
        // Half the set from the situation, the rest from whatever was chosen
        // above, without repeating a message.
        let situational = db.representative_examples(VoiceLayer::Situational, sid, EXAMPLES / 2)?;
        if !situational.is_empty() {
            let mut merged = situational;
            for e in examples {
                if merged.len() == EXAMPLES {
                    break;
                }
                if !merged.iter().any(|m| m.message_id == e.message_id) {
                    merged.push(e);
                }
            }
            examples = merged;
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
        reading: describe::reading_of(&row.qualitative, &metrics),
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

/// The voice worked out afresh from the user's own messages outside some
/// conversations. Measuring the drafts uses it, so that no layer a model is
/// shown was measured on a reply it is trying to predict — the stored
/// profiles were measured over everything. Layers are folded as `resolve`
/// folds them; each scope is computed once per measurement and kept.
/// Representative examples are not carried: a prompt never shows them.
pub struct LeavingOut<'a> {
    db: &'a Db,
    held: std::collections::HashSet<String>,
    cache: std::cell::RefCell<std::collections::HashMap<(&'static str, String), Option<ResolvedLayer>>>,
}

impl<'a> LeavingOut<'a> {
    pub fn new(db: &'a Db, conversations: &[String]) -> Self {
        Self { db, held: conversations.iter().cloned().collect(), cache: Default::default() }
    }

    /// `resolve`, over the messages outside the held-out conversations.
    pub fn resolve(
        &self,
        channel: &str,
        participant_id: Option<&str>,
        situation_id: Option<&str>,
    ) -> Result<ResolvedVoice, DbError> {
        let mut layers = Vec::new();
        let mut scopes: Vec<(VoiceLayer, String)> = vec![(VoiceLayer::Global, String::new())];
        if let Some(l) = self.layer(VoiceLayer::Global, "", SelfScope::default(), "Everything you write")? {
            layers.push(l);
        }
        let over = format!("What you write over {channel}");
        if let Some(l) =
            self.layer(VoiceLayer::Channel, channel, SelfScope { channel: Some(channel), ..Default::default() }, &over)?
        {
            layers.push(l);
            scopes.push((VoiceLayer::Channel, channel.to_string()));
        }
        if let Some(pid) = participant_id {
            let name = self.db.get_participant(pid)?.map(|p| p.display_name).unwrap_or_default();
            let to = format!("What you write to {name}");
            let scope = SelfScope { participant_id: Some(pid), ..Default::default() };
            if let Some(l) = self.layer(VoiceLayer::Relationship, pid, scope, &to)? {
                layers.push(l);
                scopes.push((VoiceLayer::Relationship, pid.to_string()));
            }
        }
        if let Some((sid, b)) = situation_id.and_then(|sid| crate::situations::builtin(sid).map(|b| (sid, b))) {
            let scope = SelfScope { situation_id: Some(sid), ..Default::default() };
            if let Some(l) = self.layer(VoiceLayer::Situational, sid, scope, b.layer_label)? {
                layers.push(l);
                scopes.push((VoiceLayer::Situational, sid.to_string()));
            }
        }
        // What the user told Mimic directly is theirs, not measured from any
        // message, so it stands as it does for every draft.
        let overrides =
            self.db.voice_preferences_for(&scopes)?.into_iter().map(|p| (p.key, p.value)).collect::<Vec<_>>();
        Ok(ResolvedVoice { layers, overrides, examples: Vec::new() })
    }

    /// One scope's layer over the messages outside the held-out
    /// conversations; none when no message is left in it.
    fn layer(
        &self,
        layer: VoiceLayer,
        key: &str,
        scope: SelfScope<'_>,
        label: &str,
    ) -> Result<Option<ResolvedLayer>, DbError> {
        if let Some(hit) = self.cache.borrow().get(&(layer.as_str(), key.to_string())) {
            return Ok(hit.clone());
        }
        let mut acc = Accumulator::default();
        each_message(self.db, scope, &mut |m| {
            if !self.held.contains(&m.conversation_id) {
                acc.push(&sample(m));
            }
        })?;
        let computed = (!acc.is_empty()).then(|| {
            let metrics = acc.finish();
            ResolvedLayer {
                layer: layer.as_str().to_string(),
                scope_key: key.to_string(),
                label: label.to_string(),
                sample_size: metrics.sample_size as i64,
                measurable: metrics.measurable,
                metrics,
                stale: false,
                // Measured afresh, from other messages: no reading was
                // written from these numbers.
                reading: None,
            }
        });
        self.cache.borrow_mut().insert((layer.as_str(), key.to_string()), computed.clone());
        Ok(computed)
    }
}

pub struct AnalyzeExecutor;

impl AnalyzeExecutor {
    pub fn shared() -> Arc<dyn JobExecutor> {
        Arc::new(AnalyzeExecutor)
    }
}

impl JobExecutor for AnalyzeExecutor {
    fn kinds(&self) -> &'static [&'static str] {
        &[JOB_KIND, CHANGED_JOB_KIND]
    }
    fn resumable(&self, _kind: &str) -> bool {
        // Not resumed where it stopped: a layer measured before a stop and
        // one after it could describe two different sets of messages. Run
        // again, an analysis of what changed picks up whatever a stopped one
        // left stale.
        false
    }
    fn execute(&self, ctx: JobContext) -> JobFuture {
        Box::pin(async move {
            let db = ctx.db.clone();
            let mode = Mode::of_kind(&ctx.job.kind);
            let progress_ctx = ctx.clone();
            let mut on_progress =
                move |scope: &str, done: usize| progress_ctx.progress(done as i64, 0, &format!("analyzing {scope}"));
            let summary =
                tokio::task::block_in_place(|| analyze_in(&db, mode, &mut on_progress)).map_err(JobError::Db)?;
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

        let v = resolve(&db, "chat", Some(&ada), None).unwrap();
        assert_eq!(v.layers.len(), 3);
        assert_eq!(v.layers[0].layer, "global");
        assert_eq!(v.layers[2].layer, "relationship");
        assert_eq!(v.overrides.last().unwrap().1, json!("c"), "the innermost override wins");
        assert!(!v.examples.is_empty());

        // A channel the user has never used contributes no layer.
        let email = resolve(&db, "email", None, None).unwrap();
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
        let v = resolve(&db, "chat", Some(&ada), None).unwrap();
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

    fn add_self_messages(db: &Db, prefix: &str, bodies: &[String]) {
        let source = db.list_sources().unwrap()[0].id.clone();
        let convo = db.upsert_conversation(&source, prefix, "chat", None).unwrap();
        let batch: Vec<NewMessage> = bodies
            .iter()
            .enumerate()
            .map(|(i, body)| NewMessage {
                conversation_id: convo.clone(),
                source_id: source.clone(),
                participant_id: None,
                external_id: format!("{prefix}{i}"),
                direction: "self".into(),
                channel: "chat".into(),
                sent_at: Some(format!("2026-03-{:02}T10:00:00Z", (i % 27) + 1)),
                sequence_index: i as i64,
                body: body.clone(),
                reply_to_external_id: None,
                metadata: serde_json::Value::Null,
            })
            .collect();
        db.insert_messages(&batch).unwrap();
    }

    /// A conversation with `name` over `channel` holding `n` messages of the
    /// user's. Returns (conversation, person).
    fn add_person(db: &Db, name: &str, channel: &str, n: usize) -> (String, String) {
        let source = db.list_sources().unwrap()[0].id.clone();
        let convo = db.upsert_conversation(&source, name, channel, None).unwrap();
        let handle = format!("@{}", name.to_lowercase());
        let person =
            db.resolve_participant(name, &[IdentifierInput::new(IdentifierKind::Handle, &handle)], false).unwrap();
        db.link_conversation_participant(&convo, &person).unwrap();
        add_to(db, &convo, channel, name, 0, n);
        (convo, person)
    }

    fn add_to(db: &Db, convo: &str, channel: &str, prefix: &str, from: usize, n: usize) {
        let source = db.list_sources().unwrap()[0].id.clone();
        let batch: Vec<NewMessage> = (from..from + n)
            .map(|i| NewMessage {
                conversation_id: convo.to_string(),
                source_id: source.clone(),
                participant_id: None,
                external_id: format!("{prefix}-{i}"),
                direction: "self".into(),
                channel: channel.into(),
                sent_at: Some(format!("2026-04-{:02}T10:00:00Z", (i % 27) + 1)),
                sequence_index: i as i64,
                body: format!("right, that is all from me on number {i}"),
                reply_to_external_id: None,
                metadata: serde_json::Value::Null,
            })
            .collect();
        db.insert_messages(&batch).unwrap();
    }

    #[test]
    fn an_analysis_of_what_changed_measures_that_and_leaves_the_rest() {
        let (db, ada) = seeded("chat", 30);
        let (bob_convo, bob) = add_person(&db, "Bob", "email", 25);
        let all = analyze(&db, &mut |_, _| {}).unwrap();
        assert_eq!((all.profiles_written, all.profiles_kept), (5, 0), "global, chat, email, Ada, Bob");

        let nothing = analyze_in(&db, Mode::Changed, &mut |_, _| {}).unwrap();
        assert_eq!((nothing.profiles_written, nothing.profiles_kept), (0, 5), "nothing changed, nothing measured");

        // Three more to Bob: what they are in is measured again, and only that.
        add_to(&db, &bob_convo, "email", "Bob", 25, 3);
        db.mark_conversations_changed(std::slice::from_ref(&bob_convo)).unwrap();
        let stale: Vec<(String, String)> = db
            .list_voice_profiles(ANALYSIS_VERSION)
            .unwrap()
            .into_iter()
            .filter(|p| p.stale)
            .map(|p| (p.layer, p.scope_key))
            .collect();
        assert_eq!(stale.len(), 3, "{stale:?}");
        assert!(!stale.contains(&("relationship".to_string(), ada.clone())), "Ada was not in it");
        let changed = analyze_in(&db, Mode::Changed, &mut |_, _| {}).unwrap();
        assert_eq!((changed.profiles_written, changed.profiles_kept), (3, 2));
        let rel = db.get_voice_profile(VoiceLayer::Relationship, &bob, ANALYSIS_VERSION).unwrap().unwrap();
        assert_eq!(rel.sample_size, 28);
        assert!(!rel.stale);
        let global = db.get_voice_profile(VoiceLayer::Global, "", ANALYSIS_VERSION).unwrap().unwrap();
        assert_eq!(global.sample_size, 58, "every message is in the layer over everything");

        // Measured afresh or only what changed, the numbers are the same.
        let before = db.list_voice_profiles(ANALYSIS_VERSION).unwrap();
        analyze(&db, &mut |_, _| {}).unwrap();
        let after = db.list_voice_profiles(ANALYSIS_VERSION).unwrap();
        let numbers = |ps: &[crate::db::VoiceProfileRow]| {
            ps.iter().map(|p| (p.layer.clone(), p.scope_key.clone(), p.metrics.clone())).collect::<Vec<_>>()
        };
        assert_eq!(numbers(&before), numbers(&after));
    }

    #[test]
    fn a_profile_whose_scope_is_gone_is_removed() {
        let (db, _) = seeded("chat", 30);
        let (_, bob) = add_person(&db, "Bob", "email", 25);
        analyze(&db, &mut |_, _| {}).unwrap();
        assert!(db.get_voice_profile(VoiceLayer::Channel, "email", ANALYSIS_VERSION).unwrap().is_some());
        assert!(db.get_voice_profile(VoiceLayer::Relationship, &bob, ANALYSIS_VERSION).unwrap().is_some());

        // Bob's messages go, but for a few.
        db.conn().execute("DELETE FROM messages WHERE external_id LIKE 'Bob-%' AND sequence_index >= 5", []).unwrap();
        db.mark_profiles_stale(None).unwrap();
        let summary = analyze_in(&db, Mode::Changed, &mut |_, _| {}).unwrap();
        assert_eq!(summary.profiles_removed, 1, "too few to Bob now for a profile of their own");
        assert!(db.get_voice_profile(VoiceLayer::Relationship, &bob, ANALYSIS_VERSION).unwrap().is_none());
        assert!(db.representative_examples(VoiceLayer::Relationship, &bob, 10).unwrap().is_empty());
        let email = db.get_voice_profile(VoiceLayer::Channel, "email", ANALYSIS_VERSION).unwrap().unwrap();
        assert_eq!(email.sample_size, 5, "a channel with a few messages still says how many");

        // And the rest, and the channel is gone too.
        db.conn().execute("DELETE FROM messages WHERE external_id LIKE 'Bob-%'", []).unwrap();
        db.mark_profiles_stale(None).unwrap();
        let summary = analyze_in(&db, Mode::Changed, &mut |_, _| {}).unwrap();
        assert_eq!(summary.profiles_removed, 1);
        assert!(db.get_voice_profile(VoiceLayer::Channel, "email", ANALYSIS_VERSION).unwrap().is_none());
    }

    #[test]
    fn only_a_voice_measured_before_and_changed_since_wants_measuring() {
        let (db, _) = seeded("chat", 30);
        assert!(!wants_measuring(&db).unwrap(), "the first analysis is the user's to start");
        analyze(&db, &mut |_, _| {}).unwrap();
        assert!(!wants_measuring(&db).unwrap());
        db.mark_profiles_stale(None).unwrap();
        assert!(wants_measuring(&db).unwrap());
        analyze_in(&db, Mode::Changed, &mut |_, _| {}).unwrap();
        assert!(!wants_measuring(&db).unwrap());

        // The first message filed under a situation, by the user, is measured.
        let first = db.page_self_messages(None, None, None, 1).unwrap().remove(0);
        crate::situations::decide(&db, &first.id, &["thanking".to_string()]).unwrap();
        assert!(wants_measuring(&db).unwrap(), "a situation with messages and no layer");
        analyze_in(&db, Mode::Changed, &mut |_, _| {}).unwrap();
        assert!(db.get_voice_profile(VoiceLayer::Situational, "thanking", ANALYSIS_VERSION).unwrap().is_some());
        assert!(!wants_measuring(&db).unwrap());
    }

    #[test]
    fn a_job_measures_what_changed_only_when_it_is_that_kind() {
        assert_eq!(Mode::of_kind(JOB_KIND), Mode::Everything);
        assert_eq!(Mode::of_kind(CHANGED_JOB_KIND), Mode::Changed);
        assert_eq!(AnalyzeExecutor.kinds(), &[JOB_KIND, CHANGED_JOB_KIND]);
    }

    #[test]
    fn examples_chosen_a_message_at_a_time_are_the_ones_sorting_them_all_would_choose() {
        // A scope's messages: many lengths, many repeats of a few wordings.
        let wordings = [
            "hi there, let me know if that works",
            "sounds good to me",
            "let me know if that works for you on friday",
            "ok",
            "can we move it to next week instead of this one",
            "thanks, that is really helpful",
            "yeah let me know",
            "I will send the numbers over in the morning",
            "no rush at all on this",
            "let me know when you are free",
        ];
        let messages: Vec<Message> = (0..300)
            .map(|i| {
                let body = format!("{}{}", wordings[(i * 7) % wordings.len()], if i % 3 == 0 { "" } else { " ok" });
                Message {
                    id: format!("m{:03}", (i * 37) % 300),
                    conversation_id: "c".into(),
                    source_id: "s".into(),
                    participant_id: None,
                    external_id: format!("e{i}"),
                    direction: "self".into(),
                    channel: "chat".into(),
                    sent_at: None,
                    sequence_index: i as i64,
                    word_count: crate::db::word_count(&body),
                    char_count: body.chars().count() as i64,
                    body,
                    reply_to_message_id: None,
                    response_latency_seconds: None,
                    metadata: serde_json::Value::Null,
                }
            })
            .collect();
        let m = metrics::compute(&messages.iter().map(sample).collect::<Vec<_>>());
        for limit in [1, 3, EXAMPLES, 40] {
            let mut picker = ExamplePicker::new(&m, limit);
            messages.iter().for_each(|msg| picker.offer(msg));
            let streamed = picker.finish();

            // The reference: score everything, sort, take the best of each wording.
            let reference = ExamplePicker::new(&m, limit);
            let mut all: Vec<(f64, String, String, String)> = messages
                .iter()
                .filter(|msg| msg.word_count >= 3)
                .map(|msg| {
                    let (score, reasons) = reference.score(msg);
                    (score, msg.id.clone(), reasons, normalize_for_dedupe(&msg.body))
                })
                .collect();
            all.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
            let mut seen = HashSet::new();
            let sorted: Vec<(String, String, f64)> = all
                .into_iter()
                .filter(|(_, _, _, w)| seen.insert(w.clone()))
                .take(limit)
                .map(|(score, id, reasons, _)| (id, reasons, (score * 1000.0).round() / 1000.0))
                .collect();
            assert_eq!(streamed, sorted, "limit {limit}");
        }
    }

    #[test]
    fn analysis_files_messages_by_situation_and_measures_each_one() {
        let (db, _) = seeded("chat", 30);
        let refusals: Vec<String> = (0..22).map(|i| format!("sorry, can't make it {i}")).collect();
        let thanks: Vec<String> = (0..4).map(|i| format!("thank you so much for this {i}")).collect();
        add_self_messages(&db, "no", &refusals);
        add_self_messages(&db, "ty", &thanks);
        let summary = analyze(&db, &mut |_, _| {}).unwrap();
        assert_eq!(summary.messages_classified, 26);

        let declining = db.get_voice_profile(VoiceLayer::Situational, "declining", ANALYSIS_VERSION).unwrap().unwrap();
        assert_eq!(declining.sample_size, 22);
        assert_eq!(declining.qualitative["label"], "When you say no");
        assert!(!db.representative_examples(VoiceLayer::Situational, "declining", 10).unwrap().is_empty());

        // Four thank-yous is a layer with a sample size and nothing measured.
        let thanking = db.get_voice_profile(VoiceLayer::Situational, "thanking", ANALYSIS_VERSION).unwrap().unwrap();
        assert_eq!(thanking.sample_size, 4);
        assert_eq!(thanking.metrics["measurable"], false);
        assert!(db.representative_examples(VoiceLayer::Situational, "thanking", 10).unwrap().is_empty());

        // A situation nothing was filed under has no layer at all.
        assert!(db.get_voice_profile(VoiceLayer::Situational, "disagreeing", ANALYSIS_VERSION).unwrap().is_none());

        // Resolution with a situation stacks it innermost and leads with its examples.
        let v = resolve(&db, "chat", None, Some("declining")).unwrap();
        assert_eq!(v.layers.last().unwrap().layer, "situational");
        assert!(v.examples[0].body.starts_with("sorry, can't make it"));
        assert!(v.examples.len() <= EXAMPLES);
        let eff = effective_metrics(&v);
        assert_eq!(eff.sample_size, 22, "the innermost measurable layer is the situation");
    }

    #[test]
    fn received_messages_are_never_filed_under_a_situation() {
        let (db, _) = seeded("chat", 30);
        // Ada's side of the seeded conversation says "any thoughts?"; make it
        // an apology instead and check it still does not count.
        db.conn().execute("UPDATE messages SET body = 'so sorry, my fault' WHERE direction = 'other'", []).unwrap();
        let summary = analyze(&db, &mut |_, _| {}).unwrap();
        assert_eq!(summary.messages_classified, 0);
        assert!(db.self_situation_counts().unwrap().is_empty());
    }

    #[test]
    fn a_situation_that_empties_loses_its_layer_on_the_next_analysis() {
        let (db, _) = seeded("chat", 30);
        let refusals: Vec<String> = (0..22).map(|i| format!("can't make it {i}")).collect();
        add_self_messages(&db, "no", &refusals);
        analyze(&db, &mut |_, _| {}).unwrap();
        assert!(db.get_voice_profile(VoiceLayer::Situational, "declining", ANALYSIS_VERSION).unwrap().is_some());
        db.conn().execute("UPDATE messages SET body = 'fine by me' WHERE external_id LIKE 'no%'", []).unwrap();
        analyze(&db, &mut |_, _| {}).unwrap();
        assert!(db.get_voice_profile(VoiceLayer::Situational, "declining", ANALYSIS_VERSION).unwrap().is_none());
        assert!(db.representative_examples(VoiceLayer::Situational, "declining", 10).unwrap().is_empty());
    }
}
