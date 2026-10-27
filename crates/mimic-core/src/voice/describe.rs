//! How a layer reads, in words: a model's reading of the measurements.
//!
//! Everything a profile holds is counted (`metrics`). A prompt can use the
//! numbers, but a sentence such as "short and warm, lowercase, signs off
//! with thanks" carries register better than twelve rates do, and turning
//! numbers into that sentence is the one thing here a model does better than
//! arithmetic. So, when the user asks, a model is given a layer's numbers —
//! only the numbers, and the greetings and sign-offs from Mimic's own short
//! lists; never a message, a phrase the user wrote, or who a layer is about —
//! and writes two or three sentences, kept with the profile as a reading
//! (`qualitative_json.description`), with the model that wrote it and a
//! digest of the numbers it was written from.
//!
//! A reading is shown and used only as what it is: a model's reading, next
//! to the numbers, never in place of them. A reading written from numbers
//! that have since changed is shown as out of date and not given to a draft.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::metrics::VoiceMetrics;
use crate::db::{Db, DbError, VoiceLayer};
use crate::jobs::{JobContext, JobError, JobExecutor, JobFuture};
use crate::providers::{GenerationRequest, ModelProvider, PromptMessage, ProviderError};
use crate::version::ANALYSIS_VERSION;

pub const JOB_KIND: &str = "describe_voice";

/// The most layers one run describes; the rest wait for the next.
pub const MAX_PER_RUN: usize = 24;

/// The longest reading kept, in characters.
const MAX_CHARS: usize = 600;

/// A reading as it is kept with a profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Description {
    pub text: String,
    pub provider: String,
    pub model: String,
    /// `metrics_digest` of the numbers it was written from.
    pub metrics_digest: String,
    pub written_at: String,
}

/// A reading as a layer carries it: whether it was written from the numbers
/// the layer has now.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reading {
    pub text: String,
    pub model: String,
    /// The provider it came from.
    pub provider: String,
    pub written_at: String,
    /// Written from the numbers the layer has now. A reading of older numbers
    /// is shown as that, and no draft is given it.
    pub current: bool,
}

/// A short digest of a layer's numbers, to tell whether a reading was written
/// from the ones it has now.
pub fn metrics_digest(m: &VoiceMetrics) -> String {
    let text = serde_json::to_string(m).unwrap_or_default();
    hex::encode(Sha256::digest(text.as_bytes()))[..16].to_string()
}

/// The reading kept in a profile's `qualitative`, as the layer with `metrics`
/// carries it.
pub fn reading_of(qualitative: &Value, metrics: &VoiceMetrics) -> Option<Reading> {
    let d: Description = serde_json::from_value(qualitative.get("description")?.clone()).ok()?;
    Some(Reading {
        current: d.metrics_digest == metrics_digest(metrics),
        text: d.text,
        model: d.model,
        provider: d.provider,
        written_at: d.written_at,
    })
}

/// What a model is shown for one layer: its kind, and its numbers. Nothing a
/// person wrote, and not who it is about.
pub fn numbers(layer: &str, m: &VoiceMetrics) -> String {
    let kind = match layer {
        "global" => "everything they write",
        "channel" => "what they write over one channel",
        "relationship" => "what they write to one person",
        _ => "what they write when doing one kind of thing (saying no, thanking, and so on)",
    };
    let mut lines = vec![format!("Measured over {kind}: {} messages.", m.sample_size)];
    let pct = |r: Option<f64>| r.map(|r| format!("{:.0}%", r * 100.0));
    let mut add = |what: &str, v: Option<String>| {
        if let Some(v) = v {
            lines.push(format!("{what}: {v}"));
        }
    };
    add(
        "Words per message",
        m.median_words_per_message.map(|med| {
            format!(
                "median {med:.0}, 90th percentile {:.0}, mean {:.1}",
                m.p90_words_per_message.unwrap_or(med),
                m.avg_words_per_message.unwrap_or(med)
            )
        }),
    );
    add("Sentences per message", m.avg_sentences_per_message.map(|v| format!("{v:.1}")));
    add("Messages in more than one paragraph", pct(m.multi_paragraph_rate));
    add("Ending with a full stop", pct(m.terminal_period_rate));
    add("Ending with a question mark", pct(m.question_rate));
    add("Ending with an exclamation mark", pct(m.exclamation_rate));
    add("With an ellipsis", pct(m.ellipsis_rate));
    add("With an emoji", pct(m.emoji_rate));
    add("Starting in lowercase", pct(m.lowercase_start_rate));
    add("Entirely lowercase", pct(m.all_lowercase_rate));
    add("Contractions per 100 words", m.contractions_per_100_words.map(|v| format!("{v:.1}")));
    add("Opening with a greeting", pct(m.greeting_rate));
    add("Closing with a sign-off", pct(m.sign_off_rate));
    let listed = |words: &[(String, usize)]| {
        (!words.is_empty())
            .then(|| words.iter().take(4).map(|(w, n)| format!("\"{w}\" ({n})")).collect::<Vec<_>>().join(", "))
    };
    add("Greetings used", listed(&m.top_greetings));
    add("Sign-offs used", listed(&m.top_sign_offs));
    lines.join("\n")
}

const INSTRUCTIONS: &str =
    "You describe how someone writes, for another writer who will draft messages in their voice. \
You are given measurements of their messages — never the messages. Write two or three plain sentences about their \
register: how formal or casual, how long, how they open and close, their punctuation and capitalization. Say only \
what the numbers support, and nothing about what they write about. No lists, no headings, no preamble.";

/// Ask `provider` for a reading of one layer's numbers.
pub fn describe(provider: &dyn ModelProvider, layer: &str, m: &VoiceMetrics) -> Result<String, ProviderError> {
    let request = GenerationRequest {
        system: INSTRUCTIONS.to_string(),
        messages: vec![PromptMessage::user(numbers(layer, m))],
        max_output_tokens: 180,
        temperature: 0.2,
    };
    let text = provider.generate(&request)?.text;
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let flat = flat.trim_matches('"').trim().to_string();
    if flat.is_empty() {
        return Err(ProviderError::Malformed("an empty reading".into()));
    }
    if let Some(why) = contradiction(&flat, m) {
        return Err(ProviderError::Malformed(format!("a reading that {why}, so it wasn't kept")));
    }
    Ok(match flat.char_indices().nth(MAX_CHARS) {
        Some((cut, _)) => format!("{}…", &flat[..cut]),
        None => flat,
    })
}

/// What a reading says that its own numbers rule out, if anything
/// (0.10.0-alpha.90, .93, .95). Run for real, llama3.2:3b read a writer who
/// starts most messages in lowercase and contracts freely as "formal and
/// highly polished" and as avoiding contractions (job 312). Only what the
/// numbers can settle is checked — formal or not, contractions or not — a
/// sentence at a time, and only what the sentence says of them: a claim is
/// turned round by a word just before it in the same clause ("not formal",
/// "never formally", "more casual than formal", "no formal greetings", "low
/// formality"), not by one anywhere in the sentence, so "formal rather than
/// chatty" still calls them formal; formal greetings or sign-offs are about
/// greetings, not register; and "avoid" has to be about contractions
/// ("avoids slang and contractions", not "rarely capitalizes, uses
/// contractions" or "no emoji, and plenty of contractions") and not turned
/// round itself ("doesn't avoid contractions"). A comma ends a clause.
pub fn contradiction(text: &str, m: &VoiceMetrics) -> Option<String> {
    let lowercase = m.lowercase_start_rate.is_some_and(|r| r >= 0.6);
    let contracting = m.contractions_per_100_words.is_some_and(|r| r >= 4.0);
    for sentence in text.split(['.', '!', '?', ';', ':']) {
        let s = sentence.to_lowercase().replace('\u{2019}', "'").replace(',', " , ");
        let words: Vec<&str> =
            s.split(|c: char| !(c.is_alphanumeric() || c == '\'' || c == ',')).filter(|w| !w.is_empty()).collect();
        let called_formal = words.iter().enumerate().any(|(i, w)| {
            matches!(*w, "formal" | "formally" | "formality")
                && !turned(&words, i)
                && !words.get(i + 1).is_some_and(|next| PARTS.contains(next))
        });
        if called_formal {
            if lowercase {
                return Some("called them formal, but most of their messages start in lowercase".into());
            }
            if contracting {
                return Some("called them formal, but they use contractions freely".into());
            }
        }
        if contracting && words.iter().enumerate().any(|(i, w)| w.starts_with("contraction") && avoided(&words, i)) {
            return Some("said they avoid contractions, but they use them freely".into());
        }
    }
    None
}

/// Words that turn round what follows them closely: "not formal", "less
/// formal", "casual than formal", "instead of formal", "no formal …",
/// "avoids formal …", "low formality".
const TURNING: [&str; 33] = [
    "not", "never", "hardly", "barely", "rarely", "seldom", "less", "neither", "nor", "isn't", "aren't", "doesn't",
    "don't", "wasn't", "than", "instead", "no", "without", "lack", "lacks", "lacking", "avoid", "avoids", "avoiding",
    "skip", "skips", "skipping", "drop", "drops", "dropping", "low", "little", "minimal",
];

/// Where a clause ends, looking back: a claim after one of these isn't turned
/// round by a word before it ("not chatty, but formal").
const CLAUSE: [&str; 8] = ["but", "yet", ",", "while", "whereas", "though", "although", "however"];

/// What "formal" says of when it is about greetings and the like, not how
/// they write: "formal greetings", "formal sign-offs".
const PARTS: [&str; 13] = [
    "greeting",
    "greetings",
    "sign",
    "signoff",
    "signoffs",
    "salutation",
    "salutations",
    "closing",
    "closings",
    "opening",
    "openings",
    "title",
    "titles",
];

/// Whether the claim at `at` is turned round by one of the three words
/// before it in its clause ("far from formal" too).
fn turned(words: &[&str], at: usize) -> bool {
    for k in (at.saturating_sub(3)..at).rev() {
        let w = words[k];
        if CLAUSE.contains(&w) {
            return false;
        }
        if TURNING.contains(&w) || (w == "far" && words.get(k + 1) == Some(&"from")) {
            return true;
        }
    }
    false
}

/// Words that say something is avoided, before it ("avoids …", "no …",
/// "never uses …") or after ("… are rare").
const AVOIDING: [&str; 12] =
    ["avoid", "avoids", "avoiding", "avoided", "no", "without", "rarely", "seldom", "never", "few", "hardly", "barely"];
const AVOIDED: [&str; 7] = ["rare", "avoided", "absent", "scarce", "uncommon", "infrequent", "missing"];

/// Where looking for what is said of contractions stops: another clause, or
/// a word saying how much of something there is.
const STOPS: [&str; 27] = [
    "but",
    "yet",
    ",",
    "while",
    "whereas",
    "though",
    "although",
    "however",
    "with",
    "plenty",
    "lots",
    "many",
    "much",
    "frequent",
    "frequently",
    "often",
    "freely",
    "regular",
    "regularly",
    "common",
    "commonly",
    "liberal",
    "liberally",
    "everywhere",
    "throughout",
    "constantly",
    "always",
];

/// Whether the words round `at` ("contractions") say they are avoided: an
/// avoiding word in the four before, itself not turned round ("doesn't
/// avoid"), or an avoided one in the three after, with nothing turning it
/// ("are not rare").
fn avoided(words: &[&str], at: usize) -> bool {
    for k in (at.saturating_sub(4)..at).rev() {
        let w = words[k];
        if STOPS.contains(&w) {
            break;
        }
        if AVOIDING.contains(&w) {
            return !words[k.saturating_sub(2)..k].iter().any(|n| {
                matches!(*n, "not" | "never" | "isn't" | "aren't" | "doesn't" | "don't" | "didn't" | "won't")
            });
        }
    }
    for w in words.iter().skip(at + 1).take(3) {
        if STOPS.contains(w) || TURNING.contains(w) {
            return false;
        }
        if AVOIDED.contains(w) {
            return true;
        }
    }
    false
}

/// What a run did.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DescribeSummary {
    pub described: usize,
    /// Layers with a reading of their numbers as they are now, left alone.
    pub current: usize,
    pub failed: usize,
    pub model: String,
    /// What the provider said the last time it failed, in one line.
    pub last_error: Option<String>,
    /// Whether the run was stopped before it had asked about every layer due.
    #[serde(default)]
    pub stopped: bool,
}

/// Write a reading for every measurable layer that has none of its numbers
/// as they are now, up to `limit`, the layer over everything first.
pub fn describe_layers(
    db: &Db,
    provider: &dyn ModelProvider,
    limit: usize,
    on_progress: &mut dyn FnMut(usize, usize),
    should_stop: &dyn Fn() -> bool,
) -> Result<DescribeSummary, DbError> {
    let info = provider.info();
    let mut summary = DescribeSummary { model: info.model.clone(), ..Default::default() };
    let order = |layer: &str| ["global", "channel", "situational", "relationship"].iter().position(|l| *l == layer);
    let mut due = Vec::new();
    for row in db.list_voice_profiles(ANALYSIS_VERSION)? {
        let metrics: VoiceMetrics = serde_json::from_value(row.metrics.clone()).unwrap_or_default();
        if !metrics.measurable {
            continue;
        }
        if reading_of(&row.qualitative, &metrics).is_some_and(|r| r.current) {
            summary.current += 1;
            continue;
        }
        due.push((row, metrics));
    }
    due.sort_by_key(|(row, m)| (order(&row.layer), std::cmp::Reverse(m.sample_size)));
    due.truncate(limit);
    let total = due.len();
    for (i, (row, metrics)) in due.into_iter().enumerate() {
        if should_stop() {
            summary.stopped = true;
            break;
        }
        on_progress(i, total);
        let Some(layer) = VoiceLayer::parse(&row.layer) else { continue };
        match describe(provider, &row.layer, &metrics) {
            Ok(text) => {
                let d = Description {
                    text,
                    provider: info.id.clone(),
                    model: info.model.clone(),
                    metrics_digest: metrics_digest(&metrics),
                    written_at: crate::ids::now_rfc3339(),
                };
                if db.set_voice_description(layer, &row.scope_key, ANALYSIS_VERSION, &d)? {
                    summary.described += 1;
                }
            }
            Err(e) => {
                tracing::warn!(target: "voice", error = %e, "a layer could not be described");
                summary.failed += 1;
                summary.last_error = Some(e.to_string().lines().next().unwrap_or("").chars().take(160).collect());
            }
        }
    }
    on_progress(total, total);
    Ok(summary)
}

impl Db {
    /// Keep a reading with one profile. False when the profile is gone.
    pub fn set_voice_description(
        &self,
        layer: VoiceLayer,
        scope_key: &str,
        analysis_version: &str,
        description: &Description,
    ) -> Result<bool, DbError> {
        let value = serde_json::to_string(description).unwrap_or_else(|_| "null".into());
        let n = self.conn().execute(
            "UPDATE voice_profiles SET qualitative_json = json_set(qualitative_json, '$.description', json(?4))
             WHERE layer = ?1 AND scope_key = ?2 AND analysis_version = ?3",
            rusqlite::params![layer.as_str(), scope_key, analysis_version, value],
        )?;
        Ok(n > 0)
    }
}

/// Runs `describe_layers` as a background job with the provider the user
/// chose.
pub struct DescribeExecutor {
    provider: Arc<dyn Fn() -> Option<Arc<dyn ModelProvider>> + Send + Sync>,
}

impl DescribeExecutor {
    pub fn shared(provider: Arc<dyn Fn() -> Option<Arc<dyn ModelProvider>> + Send + Sync>) -> Arc<dyn JobExecutor> {
        Arc::new(DescribeExecutor { provider })
    }
}

impl JobExecutor for DescribeExecutor {
    fn kinds(&self) -> &'static [&'static str] {
        &[JOB_KIND]
    }

    fn resumable(&self, _kind: &str) -> bool {
        // It may call a provider that charges, and only when asked.
        false
    }

    fn execute(&self, ctx: JobContext) -> JobFuture {
        let resolve = self.provider.clone();
        Box::pin(async move {
            let provider = resolve().ok_or_else(|| JobError::Failed("no model provider is configured".to_string()))?;
            let db = ctx.db.clone();
            let progress_ctx = ctx.clone();
            let cancel_ctx = ctx.clone();
            let summary = tokio::task::block_in_place(move || {
                let mut on_progress = |done: usize, total: usize| {
                    progress_ctx.progress(done as i64, total as i64, "describing how you write")
                };
                let should_stop = || cancel_ctx.check_cancel().is_err();
                describe_layers(&db, provider.as_ref(), MAX_PER_RUN, &mut on_progress, &should_stop)
            })
            .map_err(JobError::Db)?;
            // Stopped is stopped, whatever was written before it: what was
            // described is kept, and the run does not say it finished.
            if summary.stopped {
                return Err(JobError::Canceled);
            }
            // Every layer asked and none described is a failure, not a run
            // that finished: the screen would show nothing and say it was done.
            if summary.described == 0 && summary.failed > 0 {
                return Err(JobError::Failed(format!(
                    "No layer could be put into words: {}",
                    summary.last_error.as_deref().unwrap_or("the model gave no answer")
                )));
            }
            Ok(serde_json::to_value(summary).unwrap_or(json!(null)))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::mock::MockProvider;

    fn measured() -> VoiceMetrics {
        let bodies: Vec<String> =
            (0..24).map(|i| format!("hi Ada, the zanzibar plan is fine by me, number {i}\n\nthanks")).collect();
        let samples: Vec<super::super::metrics::Sample<'_>> =
            bodies.iter().map(|b| super::super::metrics::Sample::new(b)).collect();
        super::super::metrics::compute(&samples)
    }

    #[test]
    fn a_model_is_shown_numbers_and_never_words_the_user_wrote() {
        let m = measured();
        assert!(!m.top_phrases.is_empty(), "the numbers do carry phrases");
        let shown = numbers("relationship", &m);
        assert!(shown.contains("Measured over what they write to one person: 24 messages."), "{shown}");
        assert!(shown.contains("Opening with a greeting: 100%"), "{shown}");
        assert!(shown.contains("\"hi\" (24)"), "a greeting from Mimic's own list: {shown}");
        assert!(!shown.contains("zanzibar"), "no phrase the user wrote: {shown}");
        assert!(!shown.contains("Ada"), "and not who it is about: {shown}");
    }

    #[test]
    fn a_reading_is_one_short_paragraph() {
        let provider = MockProvider::answering("  \"Short and warm.\n\nAlways opens with hi.\"  ");
        let text = describe(&provider, "global", &measured()).unwrap();
        assert_eq!(text, "Short and warm. Always opens with hi.");
        let request = provider.last_request().unwrap();
        assert_eq!(request.system, INSTRUCTIONS);
        assert!(describe(&MockProvider::answering("   "), "global", &measured()).is_err());
        let long = MockProvider::answering(&"word ".repeat(400));
        assert!(describe(&long, "global", &measured()).unwrap().chars().count() <= MAX_CHARS + 1);
    }

    fn casual() -> VoiceMetrics {
        VoiceMetrics {
            measurable: true,
            lowercase_start_rate: Some(0.95),
            contractions_per_100_words: Some(6.0),
            ..Default::default()
        }
    }

    // What llama3.2:3b and qwen3:14b wrote for the same layer (job 312).
    const LLAMA: &str = "The writer's register is formal and highly polished, with a focus on concise and precise language. They consistently use lowercase letters and avoid contractions, punctuation, and emotive elements, creating a tone that is professional and reserved.";
    const QWEN: &str = "They write in a very casual, concise register, using short messages with few words and no punctuation at the end. Their messages start in lowercase and are entirely in lowercase, with a moderate use of contractions.";

    #[test]
    fn a_reading_its_numbers_rule_out_is_not_kept() {
        let why = contradiction(LLAMA, &casual()).expect("formal, of a lowercase writer");
        assert_eq!(why, "called them formal, but most of their messages start in lowercase");
        assert_eq!(contradiction(QWEN, &casual()), None);

        let err = describe(&MockProvider::answering(LLAMA), "global", &casual()).unwrap_err();
        assert!(err.to_string().contains("called them formal") && err.to_string().contains("wasn't kept"), "{err}");
        assert!(describe(&MockProvider::answering(QWEN), "global", &casual()).is_ok());

        // Contractions, on their own.
        let only_contracting = VoiceMetrics { lowercase_start_rate: Some(0.1), ..casual() };
        assert_eq!(
            contradiction("They avoid contractions.", &only_contracting).as_deref(),
            Some("said they avoid contractions, but they use them freely")
        );
        assert_eq!(
            contradiction("Fairly formal.", &only_contracting).as_deref(),
            Some("called them formal, but they use contractions freely")
        );

        // A word that turns something round elsewhere in the sentence, or one
        // that turns round something else, doesn't save it (0.10.0-alpha.93).
        for ruled_out in [
            "Formal rather than chatty, and brief.",
            "They are not chatty but formal.",
            "They write formally and never use emoji.",
            "A formal register; no emoji.",
            "Their tone is formal and measured.",
            "A formal, professional voice.",
        ] {
            assert!(contradiction(ruled_out, &casual()).is_some(), "{ruled_out}");
        }
        for avoiding in [
            "They avoid slang and contractions.",
            "Short messages without contractions.",
            "They never use contractions.",
            "Contractions are rare.",
            "Rarely if ever uses contractions.",
        ] {
            assert_eq!(
                contradiction(avoiding, &only_contracting).as_deref(),
                Some("said they avoid contractions, but they use them freely"),
                "{avoiding}"
            );
        }
    }

    #[test]
    fn a_reading_the_numbers_allow_is_kept_however_it_is_put() {
        for fine in [
            "They write casually, never formally.",
            "More casual than formal, and short.",
            "Not at all formal: lowercase, short, contracted.",
            "Rather relaxed instead of formal.",
            "They don\u{2019}t sound formal.",
            "No formal greetings or sign-offs.",
            "Far from formal.",
            // Polished isn't formal: lowercase writing can be careful.
            "Polished but relaxed, in lowercase.",
            // "No" and "never" about something else.
            "No emoji, and plenty of contractions.",
            "They don't avoid contractions.",
            "Contractions are not rare here.",
            "Never long; contractions throughout.",
            // Greetings, not register; a comma ends a clause (0.10.0-alpha.95).
            "They avoid formal greetings.",
            "Low formality.",
            "Formal sign-offs are rare.",
            "Drops capitals and formal sign-offs.",
            "Doesn't bother much with formal greetings.",
            "Rarely capitalizes, uses contractions.",
            "No greeting, contractions everywhere.",
            "Never long, contractions throughout.",
            "Short messages, few capitals, and contractions are everywhere.",
        ] {
            assert_eq!(contradiction(fine, &casual()), None, "{fine}");
        }
        // A writer the numbers say is formal may be called it.
        let formal = VoiceMetrics {
            measurable: true,
            lowercase_start_rate: Some(0.02),
            contractions_per_100_words: Some(0.2),
            ..Default::default()
        };
        assert_eq!(contradiction(LLAMA, &formal), None);
        // And with nothing measured about it, nothing is ruled out.
        assert_eq!(contradiction(LLAMA, &VoiceMetrics::default()), None);
    }

    #[test]
    fn a_reading_outlives_measuring_again_and_says_when_its_numbers_changed() {
        let db = Db::open_in_memory().unwrap();
        let m = measured();
        let put = |m: &VoiceMetrics| {
            db.put_voice_profile(
                VoiceLayer::Global,
                "",
                None,
                &serde_json::to_value(m).unwrap(),
                &json!({"label": "Everything you write"}),
                m.sample_size as i64,
                ANALYSIS_VERSION,
            )
            .unwrap()
        };
        put(&m);
        let provider = MockProvider::answering("Short and warm.");
        let first = describe_layers(&db, &provider, 10, &mut |_, _| {}, &|| false).unwrap();
        assert_eq!((first.described, first.current), (1, 0));
        let second = describe_layers(&db, &provider, 10, &mut |_, _| {}, &|| false).unwrap();
        assert_eq!((second.described, second.current), (0, 1), "a current reading is not asked for again");
        assert_eq!(provider.call_count(), 1);

        // Measured again, the same numbers: the reading stands, and the label.
        let row = put(&m);
        assert_eq!(row.qualitative["label"], "Everything you write");
        let r = reading_of(&row.qualitative, &m).unwrap();
        assert_eq!((r.text.as_str(), r.current, r.model.as_str()), ("Short and warm.", true, "mock-1"));

        // New numbers: kept, shown as of older numbers, and written again.
        let mut more = m.clone();
        more.sample_size += 5;
        let row = put(&more);
        assert!(!reading_of(&row.qualitative, &more).unwrap().current);
        let third = describe_layers(&db, &provider, 10, &mut |_, _| {}, &|| false).unwrap();
        assert_eq!(third.described, 1);
        let layer = crate::voice::overview(&db).unwrap().profiles.remove(0);
        assert!(layer.reading.unwrap().current);
    }

    #[test]
    fn a_provider_that_fails_says_why_and_describes_nothing() {
        let db = Db::open_in_memory().unwrap();
        let m = measured();
        db.put_voice_profile(
            VoiceLayer::Global,
            "",
            None,
            &serde_json::to_value(&m).unwrap(),
            &json!({"label": "Everything you write"}),
            24,
            ANALYSIS_VERSION,
        )
        .unwrap();
        let down = MockProvider::failing("connection refused");
        let s = describe_layers(&db, &down, 10, &mut |_, _| {}, &|| false).unwrap();
        assert_eq!((s.described, s.failed), (0, 1));
        assert!(!s.stopped);
        assert!(s.last_error.unwrap().contains("connection refused"));
        let row = db.get_voice_profile(VoiceLayer::Global, "", ANALYSIS_VERSION).unwrap().unwrap();
        assert_eq!(reading_of(&row.qualitative, &m), None);
    }

    #[test]
    fn a_run_stopped_before_a_layer_says_so_and_describes_nothing() {
        let db = Db::open_in_memory().unwrap();
        let m = measured();
        db.put_voice_profile(
            VoiceLayer::Global,
            "",
            None,
            &serde_json::to_value(&m).unwrap(),
            &json!({"label": "Everything you write"}),
            24,
            ANALYSIS_VERSION,
        )
        .unwrap();
        let provider = MockProvider::answering("Short and warm.");
        let s = describe_layers(&db, &provider, 10, &mut |_, _| {}, &|| true).unwrap();
        assert!(s.stopped);
        assert_eq!((s.described, s.failed, provider.call_count()), (0, 0, 0));
        let s = describe_layers(&db, &provider, 10, &mut |_, _| {}, &|| false).unwrap();
        assert!(!s.stopped);
        assert_eq!(s.described, 1);
    }

    #[test]
    fn a_reading_is_current_only_for_the_numbers_it_was_written_from() {
        let m = measured();
        let d = Description {
            text: "Short and warm.".into(),
            provider: "local".into(),
            model: "m".into(),
            metrics_digest: metrics_digest(&m),
            written_at: "t".into(),
        };
        let q = json!({ "label": "x", "description": d });
        assert!(reading_of(&q, &m).unwrap().current);
        let mut other = m.clone();
        other.sample_size += 1;
        assert!(!reading_of(&q, &other).unwrap().current);
        assert_eq!(reading_of(&json!({ "label": "x" }), &m), None);
    }
}
