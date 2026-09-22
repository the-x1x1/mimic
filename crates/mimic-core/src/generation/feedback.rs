//! What the difference between a draft and what was actually sent tells us.
//!
//! This is the learning loop's evidence-gathering half, and it inherits a
//! lesson from the previous product's corrections system: a single edit is an
//! anecdote. One person deleting one greeting does not mean they never greet.
//! So this module *describes* a diff and assigns it a weight; deciding what to
//! do about a pattern of them happens later, over many, and only above a
//! threshold.
//!
//! The weights encode one rule: what the user said explicitly outranks what
//! Mimic inferred from watching them.

use serde::{Deserialize, Serialize};

/// Weight of an explicit correction the user typed.
pub const PREFERENCE_WEIGHT: f64 = 3.0;
/// Weight of a difference inferred from an edit.
pub const EDIT_WEIGHT: f64 = 1.0;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftDiff {
    pub draft_words: i64,
    pub sent_words: i64,
    /// Positive when the user made it longer.
    pub word_delta: i64,
    /// `shorter`, `longer` or `same_length`.
    pub length_direction: String,
    pub greeting_removed: bool,
    pub greeting_added: bool,
    pub sign_off_removed: bool,
    pub sign_off_added: bool,
    pub emoji_removed: bool,
    pub emoji_added: bool,
    pub terminal_period_removed: bool,
    pub terminal_period_added: bool,
    /// True when the user replaced the draft outright rather than editing it.
    pub rewritten: bool,
    /// Rough share of the draft's words that survived, 0..1.
    pub retained: f64,
}

/// Compare a generated draft with what was sent.
pub fn diff_draft(draft: &str, sent: &str) -> DraftDiff {
    let draft_words = crate::db::word_count(draft);
    let sent_words = crate::db::word_count(sent);
    let retained = retention(draft, sent);
    let delta = sent_words - draft_words;
    // A ±10% wobble is editing, not a length preference.
    let threshold = (draft_words as f64 * 0.1).ceil() as i64;
    DraftDiff {
        draft_words,
        sent_words,
        word_delta: delta,
        length_direction: if delta.abs() <= threshold {
            "same_length".into()
        } else if delta < 0 {
            "shorter".into()
        } else {
            "longer".into()
        },
        greeting_removed: has_greeting(draft) && !has_greeting(sent),
        greeting_added: !has_greeting(draft) && has_greeting(sent),
        sign_off_removed: has_sign_off(draft) && !has_sign_off(sent),
        sign_off_added: !has_sign_off(draft) && has_sign_off(sent),
        emoji_removed: has_emoji(draft) && !has_emoji(sent),
        emoji_added: !has_emoji(draft) && has_emoji(sent),
        terminal_period_removed: ends_with_period(draft) && !ends_with_period(sent),
        terminal_period_added: !ends_with_period(draft) && ends_with_period(sent),
        // Almost nothing of the draft left, and the reply is more than a
        // one-word acknowledgement: the user wrote their own.
        rewritten: retained < 0.3 && sent_words >= 3,
        retained: (retained * 100.0).round() / 100.0,
    }
}

/// Share of the draft's distinct words that appear in what was sent.
fn retention(draft: &str, sent: &str) -> f64 {
    let d: std::collections::HashSet<String> = words(draft);
    if d.is_empty() {
        return 1.0;
    }
    let s = words(sent);
    d.iter().filter(|w| s.contains(*w)).count() as f64 / d.len() as f64
}

fn words(text: &str) -> std::collections::HashSet<String> {
    text.split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'').to_lowercase())
        .filter(|w| !w.is_empty())
        .collect()
}

pub(crate) fn has_greeting(text: &str) -> bool {
    const OPENERS: [&str; 8] = ["hi", "hey", "hello", "morning", "good morning", "dear", "yo", "hiya"];
    let first = text.lines().find(|l| !l.trim().is_empty()).unwrap_or_default().trim().to_lowercase();
    let clause = first.split([',', '!', '-', ':']).next().unwrap_or(&first).trim().to_string();
    let w: Vec<&str> = clause.split_whitespace().collect();
    (1..=2).any(|n| w.len() >= n && OPENERS.contains(&w[..n].join(" ").as_str()))
}

pub(crate) fn has_sign_off(text: &str) -> bool {
    const CLOSERS: [&str; 8] = ["thanks", "thank you", "cheers", "best", "regards", "talk soon", "later", "sincerely"];
    let last = text.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or_default().trim().to_lowercase();
    if crate::db::word_count(&last) > 5 {
        return false;
    }
    let clause = last.split([',', '!', '.']).next().unwrap_or(&last).trim().to_string();
    let w: Vec<&str> = clause.split_whitespace().collect();
    (1..=2).any(|n| w.len() >= n && CLOSERS.contains(&w[..n].join(" ").as_str()))
}

pub(crate) fn has_emoji(text: &str) -> bool {
    text.chars().any(|c| matches!(c as u32, 0x1F300..=0x1FAFF | 0x1F000..=0x1F2FF | 0x2600..=0x27BF | 0x2B00..=0x2BFF))
}

pub(crate) fn ends_with_period(text: &str) -> bool {
    text.trim_end().ends_with('.')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_light_edit_is_not_reported_as_a_rewrite_or_a_length_preference() {
        let d = diff_draft("Sure, I'll send the deck tomorrow morning.", "Sure, I'll send the deck tomorrow.");
        assert!(!d.rewritten);
        assert_eq!(d.length_direction, "same_length", "one word out of seven is editing");
        assert!(d.retained > 0.8);
    }

    #[test]
    fn a_real_shortening_is_recognized() {
        let d = diff_draft(
            "Thanks for getting back to me so quickly. I have had a look at the deck and I think Tuesday afternoon would work well for a call.",
            "Tuesday afternoon works.",
        );
        assert_eq!(d.length_direction, "shorter");
        assert!(d.word_delta < -15);
        assert!(d.rewritten, "almost nothing of the draft survived");
    }

    #[test]
    fn a_lengthening_is_recognized_too() {
        let d = diff_draft("yes", "yes, and I will bring the updated numbers with me on Tuesday");
        assert_eq!(d.length_direction, "longer");
        assert!(d.word_delta > 0);
        assert!(!d.rewritten, "the draft is inside what was sent");
    }

    #[test]
    fn removing_a_greeting_or_a_sign_off_is_noticed_in_both_directions() {
        let removed = diff_draft("Hi Ada,\n\nTuesday works.\n\nThanks, C", "Tuesday works.");
        assert!(removed.greeting_removed && removed.sign_off_removed);
        assert!(!removed.greeting_added && !removed.sign_off_added);

        let added = diff_draft("Tuesday works.", "Hi Ada,\n\nTuesday works.\n\nThanks, C");
        assert!(added.greeting_added && added.sign_off_added);
        assert!(!added.greeting_removed);
    }

    #[test]
    fn punctuation_and_emoji_changes_are_recorded() {
        let d = diff_draft("on my way.", "on my way 🚀");
        assert!(d.terminal_period_removed);
        assert!(d.emoji_added);
        let back = diff_draft("on my way 🚀", "on my way.");
        assert!(back.terminal_period_added && back.emoji_removed);
    }

    #[test]
    fn an_unedited_send_produces_a_diff_of_nothing() {
        let text = "Hi Ada,\n\nTuesday works. 🙂\n\nThanks, C";
        let d = diff_draft(text, text);
        assert_eq!(d.word_delta, 0);
        assert_eq!(d.length_direction, "same_length");
        assert_eq!(d.retained, 1.0);
        assert!(!d.rewritten);
        assert!(!d.greeting_removed && !d.sign_off_added && !d.emoji_removed && !d.terminal_period_added);
    }

    #[test]
    fn a_stated_preference_outweighs_an_inferred_edit() {
        // The gap is large enough that one stated preference is not drowned
        // out by two or three incidental edits.
        const _: () = assert!(PREFERENCE_WEIGHT >= EDIT_WEIGHT * 3.0);
    }

    #[test]
    fn an_empty_draft_does_not_divide_by_zero() {
        let d = diff_draft("", "anything at all");
        assert_eq!(d.retained, 1.0);
        assert_eq!(d.draft_words, 0);
        assert!(!d.rewritten);
    }
}
