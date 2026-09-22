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
//! A language model would do this better. It would also mean sending every
//! message the user has ever written to it, which on a hosted provider is not
//! a thing to do in a background job. The rules run on the machine, over the
//! user's own messages only, and a model-backed classifier can replace
//! `classify` behind the same signature when there is a local one to run it.

use serde::{Deserialize, Serialize};

use crate::db::{Db, DbError};

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
    /// Messages filed under at least one situation.
    pub messages_classified: usize,
    /// Rows written, per situation id, in vocabulary order.
    pub per_situation: Vec<(String, usize)>,
}

/// Re-file every one of the user's own messages. Rule rows are replaced
/// wholesale; rows the user set by hand (`classified_by = 'user'`) are never
/// touched, and a rule never adds a row where the user already made the call.
///
/// Only `direction = 'self'` messages are read. What other people wrote is not
/// evidence of how the user says no.
pub fn classify_corpus(db: &Db, on_progress: &mut dyn FnMut(usize)) -> Result<ClassifySummary, DbError> {
    db.clear_rule_situations()?;
    let mut summary = ClassifySummary::default();
    let mut counts = vec![0usize; BUILTINS.len()];
    let mut cursor: Option<(String, String)> = None;
    loop {
        let page = db.page_self_messages(None, None, cursor.as_ref().map(|(a, b)| (a.as_str(), b.as_str())), PAGE)?;
        if page.is_empty() {
            break;
        }
        let last = page.last().unwrap();
        cursor = Some((last.sent_at.clone().unwrap_or_default(), last.id.clone()));
        let mut rows: Vec<(String, String, f64)> = Vec::new();
        for m in &page {
            let found = classify(&m.body);
            if !found.is_empty() {
                summary.messages_classified += 1;
            }
            for f in found {
                if let Some(i) = BUILTINS.iter().position(|b| b.id == f.situation_id) {
                    counts[i] += 1;
                }
                rows.push((m.id.clone(), f.situation_id, f.confidence));
            }
        }
        summary.messages_considered += page.len();
        db.insert_rule_situations(&rows)?;
        on_progress(summary.messages_considered);
    }
    summary.per_situation = BUILTINS.iter().zip(counts).map(|(b, n)| (b.id.to_string(), n)).collect();
    Ok(summary)
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
}
