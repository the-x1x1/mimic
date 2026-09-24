//! Situations: what a message is *doing*.
//!
//! A person does not write the same way when they say no as when they say
//! thanks. The situational voice layer measures that, and it needs something
//! to say which of the user's messages were doing which. That is this module.
//!
//! The vocabulary is fixed and small on purpose — six situations, seeded by
//! migration 0007 — because a layer is only worth anything with twenty
//! messages behind it, and a long tail of situations would leave every one of
//! them below the floor.
//!
//! Classification is by rule: a set of cue phrases per situation, each with a
//! weight, summed and clamped, and written only above `RULE_THRESHOLD`. The
//! rules are tuned for precision over recall. A message that is left
//! unclassified costs the layer one sample; a message filed under the wrong
//! situation teaches the layer the wrong habit. Every row this module writes
//! carries `classified_by = 'rule'` and the cue that fired, so nothing it
//! decides can be mistaken for something the user said (CLAUDE.md: a situation
//! is never presented as a fact the user stated).
//!
//! A language model does this better, and with one on this computer the user
//! can have it read their messages (`read_with_model`): what it says replaces
//! what the rules said, message by message, and is stored as the model's
//! (`classified_by = 'model'`). Only a local model is ever asked. Sending
//! everything the user has written to a hosted provider in a background job is
//! not a thing Mimic does, so a hosted one is refused rather than used.
//!
//! The user can say what any one of their messages is doing (`decide`), which
//! beats both, and hand it back to the rules (`let_rules_decide`).
//!
//! A note the user types to go with a draft ("say no, busy that week") is
//! read by the rules alone (`classify_note`): it is read as they write it,
//! before the draft, and a model call there would slow every draft down for a
//! short text the note cues read well.

use std::collections::BTreeSet;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::db::{Db, DbError, Filing, Message};
use crate::jobs::{JobContext, JobError, JobExecutor, JobFuture};
use crate::providers::{GenerationRequest, ModelProvider, PromptMessage, ProviderError};

/// Summed cue weight at which a message is filed under a situation.
pub const RULE_THRESHOLD: f64 = 0.6;

/// Explaining is the one situation a short message cannot be in: "because I
/// said so" is not an explanation worth learning a register from.
const EXPLAINING_MIN_WORDS: usize = 20;

/// Messages read per page while classifying the corpus.
const PAGE: usize = 1000;

/// One situation in the built-in vocabulary. The ids are stable: they are
/// primary keys in `situations`, scope keys in `voice_profiles`, and values in
/// `drafts.situation_id`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Builtin {
    pub id: &'static str,
    /// What it is called on screen. Plain words, never a category name.
    pub label: &'static str,
    /// The heading of its voice layer: "When you say no".
    pub layer_label: &'static str,
    /// Past tense, for evidence: "23 times you said no".
    pub done: &'static str,
    /// For the prompt: "In this reply they are saying no to something."
    pub doing: &'static str,
}

pub const BUILTINS: [Builtin; 6] = [
    Builtin {
        id: "declining",
        label: "Saying no",
        layer_label: "When you say no",
        done: "said no",
        doing: "saying no to something",
    },
    Builtin {
        id: "scheduling",
        label: "Setting a time",
        layer_label: "When you set up a time",
        done: "set up a time",
        doing: "arranging a time",
    },
    Builtin {
        id: "apologising",
        label: "Apologising",
        layer_label: "When you apologise",
        done: "apologised",
        doing: "apologising",
    },
    Builtin {
        id: "thanking",
        label: "Saying thanks",
        layer_label: "When you thank someone",
        done: "thanked someone",
        doing: "thanking someone",
    },
    Builtin {
        id: "explaining",
        label: "Explaining something",
        layer_label: "When you explain something",
        done: "explained something",
        doing: "explaining something",
    },
    Builtin {
        id: "disagreeing",
        label: "Disagreeing",
        layer_label: "When you disagree",
        done: "disagreed",
        doing: "disagreeing",
    },
];

pub fn builtin(id: &str) -> Option<&'static Builtin> {
    BUILTINS.iter().find(|b| b.id == id)
}

/// A phrase that suggests a situation, and how strongly. Negative weights
/// take evidence away ("sorry to hear" is sympathy, not an apology).
struct Cue {
    phrase: &'static str,
    weight: f64,
}

const fn c(phrase: &'static str, weight: f64) -> Cue {
    Cue { phrase, weight }
}

/// Cues that hold in anything the user wrote.
const MESSAGE_DECLINING: &[Cue] = &[
    c("can't make it", 0.8),
    c("cannot make it", 0.8),
    c("won't be able to", 0.7),
    c("wont be able to", 0.7),
    c("not going to be able to", 0.7),
    c("not able to make", 0.7),
    c("unable to", 0.5),
    c("i'll have to pass", 0.9),
    c("i'll pass", 0.8),
    c("going to pass", 0.7),
    c("have to decline", 0.9),
    c("i must decline", 0.9),
    c("have to say no", 0.9),
    c("count me out", 0.8),
    c("no thanks", 0.7),
    c("no thank you", 0.7),
    c("not for me", 0.6),
    c("i can't do", 0.6),
    c("i can't this", 0.6),
    c("i'm afraid i can't", 0.9),
    c("afraid not", 0.7),
    c("not this time", 0.6),
    c("not interested", 0.7),
    c("unfortunately", 0.35),
    c("can't wait", -0.8),
];

const MESSAGE_SCHEDULING: &[Cue] = &[
    c("are you free", 0.7),
    c("free on", 0.5),
    c("what time", 0.5),
    c("what day", 0.5),
    c("works for me", 0.5),
    c("work for you", 0.6),
    c("works for you", 0.6),
    c("does that work", 0.5),
    c("reschedule", 0.8),
    c("schedule", 0.5),
    c("set up a time", 0.8),
    c("find a time", 0.8),
    c("availability", 0.6),
    c("calendar invite", 0.7),
    c("send an invite", 0.6),
    c("let's meet", 0.6),
    c("can we meet", 0.6),
    c("next week", 0.3),
    c("tomorrow", 0.25),
    c("monday", 0.3),
    c("tuesday", 0.3),
    c("wednesday", 0.3),
    c("thursday", 0.3),
    c("friday", 0.3),
    c("saturday", 0.3),
    c("sunday", 0.3),
];

const MESSAGE_APOLOGISING: &[Cue] = &[
    c("sorry", 0.7),
    c("apologies", 0.8),
    c("i apologize", 0.9),
    c("i apologise", 0.9),
    c("my apologies", 0.9),
    c("my bad", 0.8),
    c("my fault", 0.8),
    c("i should have", 0.4),
    c("sorry to hear", -0.7),
    c("sorry for your loss", -0.7),
    c("sorry you", -0.4),
];

const MESSAGE_THANKING: &[Cue] = &[
    c("thank you", 0.8),
    c("thanks", 0.7),
    c("thx", 0.7),
    c("much appreciated", 0.8),
    c("i appreciate", 0.7),
    c("really appreciate", 0.8),
    c("grateful", 0.6),
    c("thanks in advance", -0.5),
    c("no thanks", -0.7),
    c("no thank you", -0.8),
];

const MESSAGE_EXPLAINING: &[Cue] = &[
    c("because", 0.3),
    c("the reason", 0.6),
    c("that's why", 0.5),
    c("which is why", 0.5),
    c("to clarify", 0.7),
    c("to explain", 0.7),
    c("let me explain", 0.8),
    c("what happened", 0.4),
    c("the way it works", 0.7),
    c("in other words", 0.5),
    c("so basically", 0.5),
    c("the issue is", 0.5),
    c("the problem is", 0.5),
    c("the idea is", 0.5),
];

const MESSAGE_DISAGREEING: &[Cue] = &[
    c("i disagree", 0.9),
    c("don't agree", 0.8),
    c("do not agree", 0.8),
    c("i see it differently", 0.9),
    c("not convinced", 0.7),
    c("i'd push back", 0.8),
    c("push back on", 0.6),
    c("that's not right", 0.7),
    c("that isn't right", 0.7),
    c("that's not what", 0.5),
    c("on the contrary", 0.7),
    c("i'd argue", 0.6),
    c("i don't think", 0.35),
    c("not sure that's", 0.5),
    c("i'm not sure that", 0.5),
    c("respectfully", 0.4),
    c("but i think", 0.35),
];

fn message_cues(id: &str) -> &'static [Cue] {
    match id {
        "declining" => MESSAGE_DECLINING,
        "scheduling" => MESSAGE_SCHEDULING,
        "apologising" => MESSAGE_APOLOGISING,
        "thanking" => MESSAGE_THANKING,
        "explaining" => MESSAGE_EXPLAINING,
        "disagreeing" => MESSAGE_DISAGREEING,
        _ => &[],
    }
}

/// Extra cues that only make sense in the user's note to Mimic — "say no
/// politely" is an instruction, not a message anyone sends.
const NOTE_DECLINING: &[Cue] = &[c("say no", 0.9), c("decline", 0.9), c("turn down", 0.9), c("turn it down", 0.9)];

const NOTE_SCHEDULING: &[Cue] =
    &[c("propose a time", 0.9), c("suggest a time", 0.9), c("book", 0.4), c("meeting", 0.3)];

const NOTE_APOLOGISING: &[Cue] = &[c("say sorry", 0.9), c("apologize", 0.9), c("apologise", 0.9)];

const NOTE_THANKING: &[Cue] = &[c("thank them", 0.9), c("say thanks", 0.9), c("thank", 0.7)];

const NOTE_EXPLAINING: &[Cue] = &[c("explain", 0.8), c("clarify", 0.7)];

const NOTE_DISAGREEING: &[Cue] = &[c("disagree", 0.9), c("push back", 0.9), c("argue", 0.6)];

fn note_cues(id: &str) -> &'static [Cue] {
    match id {
        "declining" => NOTE_DECLINING,
        "scheduling" => NOTE_SCHEDULING,
        "apologising" => NOTE_APOLOGISING,
        "thanking" => NOTE_THANKING,
        "explaining" => NOTE_EXPLAINING,
        "disagreeing" => NOTE_DISAGREEING,
        _ => &[],
    }
}

/// One situation a piece of text appears to be in, with the evidence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Classification {
    pub situation_id: String,
    /// Summed cue weight, clamped to [0, 1]. A rule score, not a probability.
    pub confidence: f64,
    /// The strongest phrase that fired, as it appeared in the text.
    pub cue: String,
}

/// Classify something the user wrote. Deterministic: same text, same answer,
/// strongest first, ties broken by situation id.
pub fn classify(body: &str) -> Vec<Classification> {
    let text = strip_closing_line(body);
    score(&text, false)
}

/// Classify the user's note to Mimic ("say no, busy that week"). Uses the
/// message cues plus the instruction-shaped ones, and does not strip a closing
/// line, because a note has none. Returns the single strongest situation.
pub fn classify_note(note: &str) -> Option<Classification> {
    let words: Vec<String> = normalize(note).split_whitespace().map(str::to_string).collect();
    // A note that accepts anything is not a refusal, however it opens: "no,
    // that works", "no worries, yes please", "say no problem and that friday
    // works".
    let accepts = ACCEPTING.iter().any(|a| {
        let a_words: Vec<&str> = a.split(' ').collect();
        words.windows(a_words.len()).any(|w| w.iter().map(String::as_str).eq(a_words.iter().copied()))
    });
    let mut all = score(note, true);
    if accepts {
        all.retain(|c| c.situation_id != "declining");
    }
    // "no, busy that week" — a note that opens with a bare "no" is a no, but
    // not "no rush", "no worries" or "no problem".
    let bare_no = words.first().map(String::as_str) == Some("no")
        && !words.get(1).is_some_and(|w| NOT_A_REFUSAL.contains(&w.as_str()))
        && !accepts;
    if bare_no && !all.iter().any(|c| c.situation_id == "declining") {
        all.push(Classification { situation_id: "declining".into(), confidence: 0.7, cue: "no".into() });
        sort(&mut all);
    }
    all.into_iter().next()
}

/// Words that follow "no" without refusing anything.
const NOT_A_REFUSAL: [&str; 14] = [
    "rush", "worries", "worry", "problem", "problems", "need", "hurry", "pressure", "doubt", "question", "issue",
    "stress", "bother", "thanks",
];

/// Phrases in a note that accept something. A note containing one is not read
/// as a refusal.
const ACCEPTING: [&str; 14] = [
    "yes",
    "yeah",
    "yep",
    "sure",
    "ok",
    "okay",
    "works",
    "fine",
    "sounds good",
    "happy to",
    "confirm",
    "accept",
    "go ahead",
    "that's great",
];

/// Whether the words just before a cue negate it: "don't decline", "don't
/// say sorry", "never push back". Checked in notes only — in a message,
/// "don't agree" is itself the cue.
fn negated(hay: &str, phrase: &str) -> bool {
    let Some(at) = hay.find(&format!(" {phrase} ")) else { return false };
    let before: Vec<&str> = hay[..at].split_whitespace().rev().take(2).collect();
    before.iter().any(|w| matches!(*w, "don't" | "dont" | "not" | "never" | "without"))
        || (before.first() == Some(&"to") && before.get(1) == Some(&"need"))
}

fn score(text: &str, is_note: bool) -> Vec<Classification> {
    let hay = format!(" {} ", normalize(text));
    let words = hay.split_whitespace().count();
    let has_time = mentions_clock_time(&hay);
    let mut out = Vec::new();
    for b in &BUILTINS {
        if b.id == "explaining" && !is_note && words < EXPLAINING_MIN_WORDS {
            continue;
        }
        let mut total = 0.0;
        let mut best: Option<(&str, f64)> = None;
        let extra: &[Cue] = if is_note { note_cues(b.id) } else { &[] };
        for cue in message_cues(b.id).iter().chain(extra.iter()) {
            // In a note every cue can be negated ("don't say sorry"); in a
            // message the negation is part of the cue ("don't agree").
            if hay.contains(&format!(" {} ", cue.phrase)) && !(is_note && negated(&hay, cue.phrase)) {
                total += cue.weight;
                if cue.weight > 0.0 && best.is_none_or(|(_, w)| cue.weight > w) {
                    best = Some((cue.phrase, cue.weight));
                }
            }
        }
        if b.id == "scheduling" && has_time {
            total += 0.35;
            if best.is_none() {
                best = Some(("a time of day", 0.35));
            }
        }
        let confidence = total.clamp(0.0, 1.0);
        if let Some((cue, _)) = best {
            if confidence >= RULE_THRESHOLD {
                out.push(Classification {
                    situation_id: b.id.to_string(),
                    confidence: (confidence * 100.0).round() / 100.0,
                    cue: cue.to_string(),
                });
            }
        }
    }
    sort(&mut out);
    out
}

fn sort(v: &mut [Classification]) {
    v.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.situation_id.cmp(&b.situation_id))
    });
}

/// Lowercase, straighten apostrophes, and reduce everything that is not a
/// letter, digit or apostrophe to single spaces, so cue phrases match on word
/// boundaries. A colon survives only between two digits ("10:30"), so
/// "differently:" still matches "differently".
fn normalize(text: &str) -> String {
    let chars: Vec<char> = text
        .chars()
        .map(|ch| match ch {
            '\u{2019}' | '\u{2018}' | '`' => '\'',
            other => other,
        })
        .collect();
    let mut s = String::with_capacity(text.len());
    for (i, &ch) in chars.iter().enumerate() {
        let clock_colon =
            ch == ':' && i > 0 && chars[i - 1].is_ascii_digit() && chars.get(i + 1).is_some_and(|c| c.is_ascii_digit());
        if ch.is_alphanumeric() || ch == '\'' || clock_colon {
            s.extend(ch.to_lowercase());
        } else {
            s.push(' ');
        }
    }
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// "3pm", "10:30", "9 am". Enough to tell "see you at 3pm" from "see you".
fn mentions_clock_time(hay: &str) -> bool {
    let tokens: Vec<&str> = hay.split_whitespace().collect();
    for (i, t) in tokens.iter().enumerate() {
        let digits: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() || digits.len() > 2 {
            continue;
        }
        let rest = &t[digits.len()..];
        if rest == "am" || rest == "pm" {
            return true;
        }
        if rest.is_empty() && matches!(tokens.get(i + 1), Some(&"am") | Some(&"pm")) {
            return true;
        }
        if let Some(mins) = rest.strip_prefix(':') {
            let m = mins.trim_end_matches("am").trim_end_matches("pm");
            if m.len() == 2 && m.chars().all(|c| c.is_ascii_digit()) {
                return true;
            }
        }
    }
    false
}

/// Drop the closing block: up to two short trailing lines ("Thanks," then a
/// name). "Thanks" at the bottom of every message is a sign-off, not a
/// situation; without this, the thanking layer would be a copy of the global
/// one for anyone who signs off with thanks.
fn strip_closing_line(body: &str) -> String {
    let mut lines: Vec<&str> = body.trim_end().lines().collect();
    for _ in 0..2 {
        while lines.last().is_some_and(|l| l.trim().is_empty()) {
            lines.pop();
        }
        let has_body_above = lines.len() >= 2 && lines[..lines.len() - 1].iter().any(|l| !l.trim().is_empty());
        match lines.last() {
            Some(last) if has_body_above && last.split_whitespace().count() <= 3 => {
                lines.pop();
            }
            _ => break,
        }
    }
    lines.join("\n")
}

/// What `classify_corpus` did.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClassifySummary {
    pub messages_considered: usize,
    /// Messages the rules filed under at least one situation.
    pub messages_classified: usize,
    /// The user's own messages filed under each situation afterwards, by the
    /// rules or by the user, in vocabulary order.
    pub per_situation: Vec<(String, usize)>,
    /// Situations a message joined or left in this pass. Their layers are
    /// marked stale as it goes.
    pub changed: BTreeSet<String>,
}

/// File every one of the user's own messages again, a page at a time, each
/// page in one transaction. A rule row changes only where the rules now say
/// something else — so filing a corpus that has not changed writes nothing,
/// and a stop part way leaves every message filed one way or the other, never
/// half. Rows the user set (`classified_by = 'user'`) are never touched, and a
/// rule never adds a row where the user already made the call.
///
/// Only `direction = 'self'` messages are read. What other people wrote is not
/// evidence of how the user says no.
pub fn classify_corpus(db: &Db, on_progress: &mut dyn FnMut(usize)) -> Result<ClassifySummary, DbError> {
    db.forget_rule_filing_of_others()?;
    let mut summary = ClassifySummary::default();
    let mut cursor: Option<(String, String)> = None;
    loop {
        let page = db.page_self_messages(None, None, cursor.as_ref().map(|(a, b)| (a.as_str(), b.as_str())), PAGE)?;
        let Some(last) = page.last() else { break };
        cursor = Some((last.sent_at.clone().unwrap_or_default(), last.id.clone()));
        let filed: Vec<(String, Vec<(String, f64)>)> = page
            .iter()
            .map(|m| {
                let found = classify(&m.body);
                if !found.is_empty() {
                    summary.messages_classified += 1;
                }
                (m.id.clone(), found.into_iter().map(|f| (f.situation_id, f.confidence)).collect())
            })
            .collect();
        summary.messages_considered += page.len();
        summary.changed.extend(db.refile_by_rule(&filed)?);
        on_progress(summary.messages_considered);
    }
    let counts = db.self_situation_counts()?;
    summary.per_situation = BUILTINS
        .iter()
        .map(|b| {
            let n = counts.iter().find(|(id, _)| id == b.id).map_or(0, |(_, n)| *n as usize);
            (b.id.to_string(), n)
        })
        .collect();
    Ok(summary)
}

/// The user says what one of their own messages is doing: these situations,
/// or none of them. Their decision stands over the rules and a model.
pub fn decide(db: &Db, message_id: &str, situation_ids: &[String]) -> Result<Filing, DbError> {
    let mut ids: Vec<String> = Vec::new();
    for id in situation_ids {
        if builtin(id).is_none() {
            return Err(DbError::Invalid(format!("{id:?} is not something a message can be doing")));
        }
        if !ids.contains(id) {
            ids.push(id.clone());
        }
    }
    db.decide_situations(message_id, &ids)?;
    db.filing_of(message_id)?.ok_or_else(|| DbError::NotFound(format!("message {message_id}")))
}

/// Forget what the user (or a model) decided about one of their messages,
/// and file it as the rules read it now.
pub fn let_rules_decide(db: &Db, message_id: &str) -> Result<Filing, DbError> {
    let body: String =
        db.get_message(message_id)?.ok_or_else(|| DbError::NotFound(format!("message {message_id}")))?.body;
    let found: Vec<(String, f64)> = classify(&body).into_iter().map(|c| (c.situation_id, c.confidence)).collect();
    db.hand_back_to_rules(message_id, &found)?;
    db.filing_of(message_id)?.ok_or_else(|| DbError::NotFound(format!("message {message_id}")))
}

// ------------------------------------------------------------ a model reads

pub const READ_JOB_KIND: &str = "read_situations";

/// Messages a model is shown at once.
const MODEL_BATCH: usize = 8;

/// The most messages one reading goes through, the most recent first. The
/// rest wait for the next, which starts from the next unread message.
pub const MODEL_READS_PER_RUN: usize = 400;

/// Stored as a model reading's confidence. A model says which situations,
/// not how sure it is; this sits above what the rules usually reach and
/// below a person's 1.0, and nothing reads it as a probability.
pub const MODEL_CONFIDENCE: f64 = 0.9;

/// How a model is asked, recorded with each reading beside the model's name,
/// so a reading can be told from one asked another way.
const MODEL_PROMPT_VERSION: &str = "situations_v1";

/// Characters of each message a model is shown.
const MODEL_CHARS: usize = 800;

/// Said when the provider chosen is not on this computer.
pub const NOT_LOCAL: &str =
    "Reading what your messages are doing sends all of them to the model, so it only runs on a model on this computer.";

#[derive(Debug, thiserror::Error)]
pub enum ModelReadError {
    #[error("{}", NOT_LOCAL)]
    NotLocal,
    #[error("the model on this computer could not read them: {0}")]
    Provider(#[from] ProviderError),
    #[error(transparent)]
    Db(#[from] DbError),
}

/// What a model reading did.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelReadSummary {
    /// Messages the model read, and filed as it said.
    pub read: usize,
    /// Messages whose answer could not be read, left as the rules filed them.
    pub not_understood: usize,
    /// Situations a message joined or left.
    pub changed: BTreeSet<String>,
    pub model: String,
    /// Whether the reading was stopped before its limit or the last unread
    /// message.
    #[serde(default)]
    pub stopped: bool,
}

fn model_instructions() -> String {
    let mut s = String::from(
        "You sort short messages by what each one is doing. A message can be doing any of these, several of them, \
         or none:\n",
    );
    for b in &BUILTINS {
        s.push_str(&format!("- {}: {}\n", b.id, b.doing));
    }
    s.push_str(
        "A thanks or a name at the very end of a message is a sign-off, not thanking. Explaining means setting \
         out reasons at some length, not a short answer.\n\
         Answer with one JSON object and nothing else. Its keys are the message numbers; each value is the list of \
         ids above that the message is doing, or [] for none. For example: {\"1\": [\"thanking\"], \"2\": []}",
    );
    s
}

fn shown(body: &str) -> String {
    let text = strip_closing_line(body);
    let text = text.trim();
    match text.char_indices().nth(MODEL_CHARS) {
        Some((cut, _)) => format!("{}…", &text[..cut]),
        None => text.to_string(),
    }
}

/// What a model answered, message by message: the situations it named, or
/// nothing where the answer for that message cannot be read — missing, not
/// a list, or naming something that is not in the vocabulary. `None` when
/// no answer can be found at all.
pub fn parse_answers(text: &str, n: usize) -> Option<Vec<Option<Vec<String>>>> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    if end < start {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(&text[start..=end]).ok()?;
    let object = value.as_object()?;
    Some(
        (1..=n)
            .map(|i| {
                let list = object.get(&i.to_string())?.as_array()?;
                let mut ids: Vec<String> = Vec::new();
                for v in list {
                    let id = builtin(v.as_str()?)?.id.to_string();
                    if !ids.contains(&id) {
                        ids.push(id);
                    }
                }
                Some(ids)
            })
            .collect(),
    )
}

/// Ask the model about `batch`, together; where the answer cannot be read at
/// all, one message at a time.
fn ask(provider: &dyn ModelProvider, batch: &[Message]) -> Result<Vec<Option<Vec<String>>>, ProviderError> {
    let mut numbered = String::new();
    for (i, m) in batch.iter().enumerate() {
        numbered.push_str(&format!("Message {}:\n{}\n\n", i + 1, shown(&m.body)));
    }
    let request = GenerationRequest {
        system: model_instructions(),
        messages: vec![PromptMessage::user(numbered.trim_end())],
        max_output_tokens: 48 * batch.len() as u32 + 32,
        temperature: 0.0,
    };
    let text = provider.generate(&request)?.text;
    match parse_answers(&text, batch.len()) {
        Some(answers) => Ok(answers),
        None if batch.len() > 1 => {
            let mut answers = Vec::with_capacity(batch.len());
            for m in batch {
                answers.extend(ask(provider, std::slice::from_ref(m))?);
            }
            Ok(answers)
        }
        None => Ok(vec![None]),
    }
}

/// Have a model on this computer read up to `limit` of the user's messages
/// that nobody has decided about, the most recent first, and file each as it
/// says. What it read is kept as it goes, so a stop keeps it, and the next
/// reading starts from the next unread message. A hosted provider is refused.
pub fn read_with_model(
    db: &Db,
    provider: &dyn ModelProvider,
    limit: usize,
    on_progress: &mut dyn FnMut(usize, usize),
    should_stop: &dyn Fn() -> bool,
) -> Result<ModelReadSummary, ModelReadError> {
    let info = provider.info();
    if !info.local {
        return Err(ModelReadError::NotLocal);
    }
    let version = format!("{}/{MODEL_PROMPT_VERSION}", info.model);
    let mut summary = ModelReadSummary { model: info.model.clone(), ..Default::default() };
    let total = (db.filing_counts()?.by_rules.max(0) as usize).min(limit);
    let mut attempted = 0;
    let mut cursor: Option<(String, String)> = None;
    while attempted < limit {
        if should_stop() {
            summary.stopped = true;
            break;
        }
        let page = db.page_unread_self_messages(
            cursor.as_ref().map(|(a, b)| (a.as_str(), b.as_str())),
            MODEL_BATCH.min(limit - attempted),
        )?;
        let Some(last) = page.last() else { break };
        cursor = Some((last.sent_at.clone().unwrap_or_default(), last.id.clone()));
        attempted += page.len();
        let answers = ask(provider, &page)?;
        let mut readings: Vec<(String, Vec<String>)> = Vec::new();
        for (m, answer) in page.iter().zip(answers) {
            match answer {
                Some(situations) => readings.push((m.id.clone(), situations)),
                None => summary.not_understood += 1,
            }
        }
        summary.read += readings.len();
        summary.changed.extend(db.read_by_model(&readings, MODEL_CONFIDENCE, &version)?);
        on_progress(attempted, total);
    }
    Ok(summary)
}

/// Runs `read_with_model` as a background job, with the model on this
/// computer the shell hands it.
pub struct ReadExecutor {
    provider: Arc<dyn Fn() -> Option<Arc<dyn ModelProvider>> + Send + Sync>,
}

impl ReadExecutor {
    /// `provider` is the model on this computer to read with, resolved when
    /// the run starts, or none when there is none.
    pub fn shared(provider: Arc<dyn Fn() -> Option<Arc<dyn ModelProvider>> + Send + Sync>) -> Arc<dyn JobExecutor> {
        Arc::new(ReadExecutor { provider })
    }
}

impl JobExecutor for ReadExecutor {
    fn kinds(&self) -> &'static [&'static str] {
        &[READ_JOB_KIND]
    }

    fn resumable(&self, _kind: &str) -> bool {
        // It costs nothing but time on this computer, and every message read
        // is kept, so a reading cut short goes on from the next one.
        true
    }

    fn execute(&self, ctx: JobContext) -> JobFuture {
        let resolve = self.provider.clone();
        Box::pin(async move {
            let provider = resolve().ok_or_else(|| JobError::Failed(NOT_LOCAL.to_string()))?;
            let db = ctx.db.clone();
            let progress_ctx = ctx.clone();
            let cancel_ctx = ctx.clone();
            let summary = tokio::task::block_in_place(move || {
                let mut on_progress = |done: usize, total: usize| {
                    progress_ctx.progress(done as i64, total as i64, "reading what your messages are doing")
                };
                let should_stop = || cancel_ctx.check_cancel().is_err();
                read_with_model(&db, provider.as_ref(), MODEL_READS_PER_RUN, &mut on_progress, &should_stop)
            })
            .map_err(|e| JobError::Failed(e.to_string()))?;
            // What was read before the stop is kept either way; the run
            // does not say it finished.
            if summary.stopped {
                return Err(JobError::Canceled);
            }
            Ok(serde_json::to_value(summary).unwrap_or(serde_json::Value::Null))
        })
    }
}

/// One situation as the app shows it: what it is called, and how many of the
/// user's own messages are filed under it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SituationSummary {
    pub id: String,
    pub label: String,
    pub layer_label: String,
    pub own_messages: i64,
    /// Whether the situational layer has enough messages to measure anything.
    pub measurable: bool,
}

pub fn overview(db: &Db) -> Result<Vec<SituationSummary>, DbError> {
    let counts = db.self_situation_counts()?;
    Ok(BUILTINS
        .iter()
        .map(|b| {
            let n = counts.iter().find(|(id, _)| id == b.id).map(|(_, n)| *n).unwrap_or(0);
            SituationSummary {
                id: b.id.to_string(),
                label: b.label.to_string(),
                layer_label: b.layer_label.to_string(),
                own_messages: n,
                measurable: n >= crate::voice::metrics::MIN_SAMPLE as i64,
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn top(text: &str) -> Option<String> {
        classify(text).into_iter().next().map(|c| c.situation_id)
    }

    #[test]
    fn the_vocabulary_is_six_distinct_situations() {
        let mut ids: Vec<&str> = BUILTINS.iter().map(|b| b.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 6);
        for b in &BUILTINS {
            assert!(!message_cues(b.id).is_empty(), "{} has no cues", b.id);
        }
    }

    #[test]
    fn clear_messages_are_filed_where_a_person_would_file_them() {
        assert_eq!(top("I'm afraid I can't make it on the 12th, we're away."), Some("declining".into()));
        assert_eq!(top("Are you free Thursday at 3pm? Otherwise Friday works for me."), Some("scheduling".into()));
        assert_eq!(top("Sorry, I completely missed this one. My fault."), Some("apologising".into()));
        assert_eq!(top("Thank you so much for sorting that out, really appreciate it."), Some("thanking".into()));
        assert_eq!(
            top("I don't agree with that. I see it differently: the numbers were never the problem."),
            Some("disagreeing".into())
        );
        assert_eq!(
            top("The reason the build failed is that the cache was stale, which is why it passed locally and not on the server. To clarify, nothing in the code changed."),
            Some("explaining".into())
        );
    }

    #[test]
    fn ordinary_messages_are_left_alone() {
        for body in [
            "yeah let me know if that works, thing 4",
            "ok",
            "Sounds good, see you then",
            "Here are the files you asked for.",
            "Can't wait to see you all!",
        ] {
            assert!(classify(body).is_empty(), "{body:?} was classified as {:?}", classify(body));
        }
    }

    #[test]
    fn sympathy_is_not_an_apology() {
        assert!(classify("So sorry to hear about your dad. Thinking of you.")
            .iter()
            .all(|c| c.situation_id != "apologising"));
    }

    #[test]
    fn a_thanks_sign_off_does_not_make_every_message_a_thank_you() {
        let body = "Attached is the revised schedule for the audit.\n\nThanks,\nC";
        assert!(classify(body).iter().all(|c| c.situation_id != "thanking"), "{:?}", classify(body));
        // …but thanks in the body still counts.
        assert_eq!(top("Thanks for covering for me yesterday, I owe you one.\n\nC"), Some("thanking".into()));
    }

    #[test]
    fn no_thanks_is_a_no_not_a_thank_you() {
        let found = classify("No thanks, we're all set for now.");
        assert_eq!(found.first().map(|c| c.situation_id.as_str()), Some("declining"));
        assert!(found.iter().all(|c| c.situation_id != "thanking"));
    }

    #[test]
    fn a_message_can_be_doing_two_things() {
        let ids: Vec<String> =
            classify("Sorry, I can't make it tomorrow.").into_iter().map(|c| c.situation_id).collect();
        assert!(ids.contains(&"apologising".to_string()) && ids.contains(&"declining".to_string()), "{ids:?}");
    }

    #[test]
    fn a_short_message_is_never_an_explanation() {
        assert!(classify("because the reason is obvious").iter().all(|c| c.situation_id != "explaining"));
    }

    #[test]
    fn classification_is_deterministic_and_names_its_cue() {
        let a = classify("Sorry, I can't make it tomorrow.");
        let b = classify("Sorry, I can't make it tomorrow.");
        assert_eq!(a, b);
        assert!(a.iter().all(|c| !c.cue.is_empty() && c.confidence >= RULE_THRESHOLD && c.confidence <= 1.0));
    }

    #[test]
    fn curly_apostrophes_match_like_straight_ones() {
        assert_eq!(top("I\u{2019}m afraid I can\u{2019}t make it."), Some("declining".into()));
    }

    #[test]
    fn clock_times_count_toward_scheduling() {
        assert!(mentions_clock_time(" see you at 3pm "));
        assert!(mentions_clock_time(" 10:30 "));
        assert!(mentions_clock_time(" 9 am "));
        assert!(!mentions_clock_time(" version 2 "));
        assert!(!mentions_clock_time(" 2026 "));
    }

    #[test]
    fn notes_to_mimic_are_read_as_instructions() {
        assert_eq!(classify_note("say no politely").map(|c| c.situation_id), Some("declining".into()));
        assert_eq!(classify_note("no, busy that week").map(|c| c.situation_id), Some("declining".into()));
        assert_eq!(classify_note("thank them for the intro").map(|c| c.situation_id), Some("thanking".into()));
        assert_eq!(classify_note("push back on the deadline").map(|c| c.situation_id), Some("disagreeing".into()));
        assert_eq!(classify_note("propose a time next week").map(|c| c.situation_id), Some("scheduling".into()));
        assert_eq!(classify_note("yes, happy to, same as last year"), None);
        assert_eq!(classify_note(""), None);
    }

    /// Notes that start with "no" or name a situation but mean the opposite.
    #[test]
    fn a_note_that_only_sounds_like_a_refusal_is_not_read_as_one() {
        for note in [
            "no rush, tell her tuesday works",
            "No, that's fine - tell him yes",
            "no worries, yes please",
            "don't decline, say yes",
            "no problem at all",
        ] {
            let got = classify_note(note).map(|c| c.situation_id);
            assert_ne!(got.as_deref(), Some("declining"), "{note:?} was read as a refusal");
        }
        for note in [
            "no, that works",
            "no that's fine, friday works",
            "no issue, happy to",
            "no stress, go ahead",
            "say no problem and that friday works",
            "no thanks needed, just confirm friday",
        ] {
            let got = classify_note(note).map(|c| c.situation_id);
            assert_ne!(got.as_deref(), Some("declining"), "{note:?} was read as a refusal");
        }
        assert_ne!(classify_note("don't say sorry").map(|c| c.situation_id).as_deref(), Some("apologising"));
        assert_eq!(classify_note("no way, too busy").map(|c| c.situation_id), Some("declining".into()));
        assert_eq!(classify_note("say no, we're away").map(|c| c.situation_id), Some("declining".into()));
    }

    // ----------------------------------------------------- who decides

    mod deciding {
        use super::super::*;
        use crate::db::{FiledBy, FilingCounts, IdentifierKind, NewMessage, NewSource, VoiceLayer};
        use crate::providers::{GenerationResponse, ProviderInfo, ProviderResult};
        use std::sync::Mutex;

        fn with_messages(bodies: &[&str]) -> (Db, Vec<String>) {
            let db = Db::open_in_memory().unwrap();
            db.set_user_identity("C").unwrap();
            db.add_user_identifier(IdentifierKind::Handle, "@c").unwrap();
            let source = db
                .create_source(&NewSource {
                    connector: "t".into(),
                    name: "T".into(),
                    channel: "chat".into(),
                    location: None,
                    config: serde_json::Value::Null,
                })
                .unwrap();
            let convo = db.upsert_conversation(&source.id, "t", "chat", None).unwrap();
            let batch: Vec<NewMessage> = bodies
                .iter()
                .enumerate()
                .map(|(i, body)| NewMessage {
                    conversation_id: convo.clone(),
                    source_id: source.id.clone(),
                    participant_id: None,
                    external_id: format!("m{i}"),
                    direction: "self".into(),
                    channel: "chat".into(),
                    sent_at: Some(format!("2026-02-01T09:{i:02}:00Z")),
                    sequence_index: i as i64,
                    body: (*body).into(),
                    reply_to_external_id: None,
                    metadata: serde_json::Value::Null,
                })
                .collect();
            db.insert_messages(&batch).unwrap();
            let ids = (0..bodies.len())
                .map(|i| {
                    db.conn()
                        .query_row("SELECT id FROM messages WHERE external_id = ?1", [format!("m{i}")], |r| r.get(0))
                        .unwrap()
                })
                .collect();
            (db, ids)
        }

        #[test]
        fn a_person_decides_over_the_rules_until_they_hand_it_back() {
            let (db, ids) = with_messages(&["can't make it on friday, we're away", "sounds good"]);
            classify_corpus(&db, &mut |_| {}).unwrap();
            assert_eq!(db.filing_of(&ids[0]).unwrap().unwrap().situations, vec!["declining"]);
            db.put_voice_profile(
                VoiceLayer::Situational,
                "declining",
                None,
                &serde_json::json!({}),
                &serde_json::json!({}),
                1,
                "v",
            )
            .unwrap();

            // It was an apology, not a no.
            let filed = decide(&db, &ids[0], &["apologising".to_string()]).unwrap();
            assert_eq!(filed, Filing { by: FiledBy::You, situations: vec!["apologising".into()] });
            assert!(db.get_voice_profile(VoiceLayer::Situational, "declining", "v").unwrap().unwrap().stale);
            // The rules no longer touch it, however often they run.
            let again = classify_corpus(&db, &mut |_| {}).unwrap();
            assert!(again.changed.is_empty(), "{:?}", again.changed);
            assert_eq!(db.filing_of(&ids[0]).unwrap().unwrap().situations, vec!["apologising"]);

            // "Doing none of these" is a decision too.
            let none = decide(&db, &ids[1], &[]).unwrap();
            assert_eq!(none, Filing { by: FiledBy::You, situations: vec![] });
            assert_eq!(db.filing_counts().unwrap(), FilingCounts { by_rules: 0, by_model: 0, by_you: 2 });

            // Handed back, the rules read it again.
            let back = let_rules_decide(&db, &ids[0]).unwrap();
            assert_eq!(back, Filing { by: FiledBy::Rules, situations: vec!["declining".into()] });
            assert!(decide(&db, &ids[0], &["gloating".to_string()]).is_err(), "only the six");
        }

        #[test]
        fn only_the_users_own_messages_are_decided_about() {
            let (db, ids) = with_messages(&["thanks so much"]);
            db.conn().execute("UPDATE messages SET direction = 'other' WHERE id = ?1", [&ids[0]]).unwrap();
            assert!(matches!(decide(&db, &ids[0], &[]), Err(DbError::Invalid(_))));
            assert_eq!(db.filing_of(&ids[0]).unwrap(), None);
            assert!(matches!(decide(&db, "nope", &[]), Err(DbError::NotFound(_))));
        }

        /// A model on this computer that answers from what it is shown: an
        /// apology where it sees "sorry", thanks where it sees "thank", and
        /// for "garbled" something that is not an answer.
        struct Reader {
            local: bool,
            calls: Mutex<Vec<usize>>,
        }

        impl ModelProvider for Reader {
            fn info(&self) -> ProviderInfo {
                ProviderInfo {
                    id: "reader".into(),
                    display_name: "Reader".into(),
                    local: self.local,
                    model: "reader-1".into(),
                    description: String::new(),
                    requires_credential: false,
                }
            }
            fn generate(&self, request: &GenerationRequest) -> ProviderResult<GenerationResponse> {
                let text = &request.messages[0].content;
                let shown: Vec<&str> = text.split("Message ").skip(1).collect();
                self.calls.lock().unwrap().push(shown.len());
                if shown.iter().any(|m| m.contains("garbled")) {
                    return Ok(GenerationResponse {
                        text: "I think it is a greeting".into(),
                        provider: "reader".into(),
                        model: "reader-1".into(),
                        input_tokens: None,
                        output_tokens: None,
                    });
                }
                let answers: Vec<String> = shown
                    .iter()
                    .enumerate()
                    .map(|(i, m)| {
                        let ids: Vec<&str> = [("sorry", "apologising"), ("thank", "thanking")]
                            .iter()
                            .filter(|(cue, _)| m.to_lowercase().contains(cue))
                            .map(|(_, id)| *id)
                            .collect();
                        format!("\"{}\": {:?}", i + 1, ids)
                    })
                    .collect();
                Ok(GenerationResponse {
                    text: format!("Sure! ```json\n{{{}}}\n```", answers.join(", ")),
                    provider: "reader".into(),
                    model: "reader-1".into(),
                    input_tokens: None,
                    output_tokens: None,
                })
            }
            fn health(&self) -> ProviderResult<()> {
                Ok(())
            }
        }

        #[test]
        fn a_model_on_this_computer_reads_what_nobody_decided_and_keeps_to_its_lane() {
            let mut bodies: Vec<String> = (0..10).map(|i| format!("sorry about the delay {i}")).collect();
            bodies.push("thank you for the flowers".into());
            bodies.push("garbled text here".into());
            bodies.push("can't make it tuesday".into());
            bodies.push("count me out this time".into());
            let refs: Vec<&str> = bodies.iter().map(String::as_str).collect();
            let (db, ids) = with_messages(&refs);
            classify_corpus(&db, &mut |_| {}).unwrap();
            // The user decided about the last one already.
            decide(&db, &ids[12], &["declining".to_string()]).unwrap();

            let reader = Reader { local: true, calls: Mutex::new(vec![]) };
            let summary = read_with_model(&db, &reader, 100, &mut |_, _| {}, &|| false).unwrap();
            assert_eq!(summary.read, 12);
            assert_eq!(summary.not_understood, 1, "no answer, no reading");
            assert_eq!(summary.model, "reader-1");
            assert_eq!(summary.changed.iter().collect::<Vec<_>>(), vec!["declining"], "it read no refusal");
            let calls = reader.calls.lock().unwrap().clone();
            assert!(calls.iter().all(|n| *n <= MODEL_BATCH), "{calls:?}");
            assert!(calls.contains(&1), "a batch it could not read is asked again one at a time: {calls:?}");

            let flowers = db.filing_of(&ids[10]).unwrap().unwrap();
            assert_eq!(flowers, Filing { by: FiledBy::Model, situations: vec!["thanking".into()] });
            let garbled = db.filing_of(&ids[11]).unwrap().unwrap();
            assert_eq!(garbled.by, FiledBy::Rules, "not understood, left to the rules");
            let theirs = db.filing_of(&ids[12]).unwrap().unwrap();
            assert_eq!(theirs, Filing { by: FiledBy::You, situations: vec!["declining".into()] });
            assert_eq!(db.filing_counts().unwrap(), FilingCounts { by_rules: 1, by_model: 12, by_you: 1 });
            let out = db.filing_of(&ids[13]).unwrap().unwrap();
            assert_eq!(out, Filing { by: FiledBy::Model, situations: vec![] }, "the model's word over the rules'");

            // The rules leave what the model read; the next reading starts
            // from what is still unread.
            classify_corpus(&db, &mut |_| {}).unwrap();
            assert_eq!(db.filing_of(&ids[10]).unwrap().unwrap().by, FiledBy::Model);
            let next = read_with_model(&db, &reader, 100, &mut |_, _| {}, &|| false).unwrap();
            assert_eq!((next.read, next.not_understood), (0, 1));
        }

        #[test]
        fn a_reading_stops_when_asked_and_at_its_limit_keeping_what_it_read() {
            let bodies: Vec<String> = (0..20).map(|i| format!("thank you, number {i}")).collect();
            let refs: Vec<&str> = bodies.iter().map(String::as_str).collect();
            let (db, _) = with_messages(&refs);
            let reader = Reader { local: true, calls: Mutex::new(vec![]) };
            let first = read_with_model(&db, &reader, 5, &mut |_, _| {}, &|| false).unwrap();
            assert_eq!(first.read, 5);
            assert!(!first.stopped, "reaching the limit is finishing, not being stopped");
            assert_eq!(db.filing_counts().unwrap().by_model, 5);
            let stopped = read_with_model(&db, &reader, 100, &mut |_, _| {}, &|| true).unwrap();
            assert_eq!(stopped.read, 0);
            assert!(stopped.stopped);
            assert_eq!(db.filing_counts().unwrap().by_model, 5);
            let rest = read_with_model(&db, &reader, 100, &mut |_, _| {}, &|| false).unwrap();
            assert_eq!(rest.read, 15);
            assert!(!rest.stopped, "reading the last unread message is finishing");
        }

        #[test]
        fn a_hosted_model_is_never_sent_the_users_messages() {
            let (db, _) = with_messages(&["thank you"]);
            let hosted = Reader { local: false, calls: Mutex::new(vec![]) };
            let err = read_with_model(&db, &hosted, 100, &mut |_, _| {}, &|| false).unwrap_err();
            assert!(matches!(err, ModelReadError::NotLocal));
            assert!(hosted.calls.lock().unwrap().is_empty(), "nothing was sent");
            assert_eq!(db.filing_counts().unwrap().by_model, 0);
        }

        #[test]
        fn answers_are_read_only_where_they_name_the_vocabulary() {
            let parsed =
                parse_answers("```json\n{\"1\": [\"thanking\", \"thanking\"], \"2\": [], \"3\": \"no\"}\n```", 4);
            assert_eq!(parsed, Some(vec![Some(vec!["thanking".into()]), Some(vec![]), None, None]));
            assert_eq!(parse_answers("{\"1\": [\"thanking\", \"greeting\"]}", 1), Some(vec![None]));
            assert_eq!(parse_answers("no idea", 2), None);
            assert_eq!(parse_answers("} {", 1), None);
        }
    }
}
