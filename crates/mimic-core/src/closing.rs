//! Whether a message only closes a conversation: a thank-you, an okay, a
//! thumbs-up — something that asks nothing back.
//!
//! This exists because "the last word was theirs" is not the same as "they
//! are waiting on you". Before it, every "Thanks!" and every "ok 👍" sat on
//! the home screen as someone waiting on a reply, and assisted drafting spent
//! a model call answering each one.
//!
//! It decides from the words, and only when **every part** of the message is
//! one of a short list of closing phrases — thanks, okays, see-you-thens, a
//! name, a sign-off, an emoji. Precision over recall, as with automated mail
//! (`sources::automated`): a thank-you that slips through costs one glance,
//! while a question filed away as a thank-you is a reply the user never
//! writes. So anything with a question mark, anything long, and anything with
//! a word that turns a thank-you into an ask ("but", "please", "can", "need",
//! "let me know") is never closing. "This is exactly what I needed. Thank
//! you!" stays on the list: its first sentence is not on it.
//!
//! The result is a reading, like the others, and the screen says it that way;
//! the user's own call on a thread (`thread_marks`) outranks it.

/// Messages longer than this, in words, are never only closing.
const MOST_WORDS: usize = 25;

/// Closing phrases, as a whole fragment or at its start. Longest first, so
/// "thank you so much" is read before "thank you".
const ACKS: &[&str] = &[
    "thank you so much",
    "thank you very much",
    "thanks so much",
    "thanks very much",
    "thanks a lot",
    "thanks a ton",
    "thanks a million",
    "many thanks",
    "much appreciated",
    "really appreciate it",
    "appreciate it",
    "appreciated",
    "thank you",
    "thank u",
    "thankyou",
    "thanks",
    "thx",
    "tysm",
    "ty",
    "ta",
    "cheers",
    "got it",
    "gotcha",
    "sounds good",
    "sounds great",
    "sounds perfect",
    "perfect",
    "great",
    "awesome",
    "amazing",
    "wonderful",
    "lovely",
    "brilliant",
    "excellent",
    "fantastic",
    "cool",
    "nice",
    "noted",
    "will do",
    "all good",
    "all set",
    "okay",
    "ok",
    "kk",
    "k",
    "no worries",
    "no problem",
    "np",
    "you're welcome",
    "youre welcome",
    "my pleasure",
    "anytime",
    "any time",
    "received",
    "done",
];

/// What may follow a closing phrase in the same fragment.
const AFTER_ACK: &[&str] = &["so much", "very much", "a lot", "again", "too", "as always", "in advance"];

/// Whole fragments that close a conversation by themselves.
const PLEASANTRIES: &[&str] = &[
    "talk soon",
    "talk later",
    "speak soon",
    "chat soon",
    "catch you later",
    "take care",
    "enjoy",
    "you too",
    "same to you",
    "to you too",
    "see you then",
    "see you soon",
    "see you there",
    "see you later",
    "see ya",
    "see you",
    "no rush",
    "have fun",
    "good luck",
];

/// Words for a stretch of time a pleasantry can wish someone well for.
const WISHES: &[&str] = &["good", "great", "nice", "lovely", "wonderful", "fun", "safe"];
const SPANS: &[&str] =
    &["one", "day", "weekend", "evening", "night", "week", "trip", "time", "flight", "holiday", "holidays"];

/// A word anywhere in a fragment that makes it more than a thank-you.
const ASKING: &[&str] = &[
    "but",
    "however",
    "though",
    "although",
    "also",
    "please",
    "pls",
    "plz",
    "can",
    "could",
    "would",
    "should",
    "need",
    "needs",
    "lmk",
    "when",
    "if",
    "still",
    "question",
    "wait",
    "actually",
    "unfortunately",
    "sorry",
    "not",
    "don't",
    "dont",
    "didn't",
    "didnt",
    "can't",
    "cant",
    "won't",
    "wont",
    "never",
    "why",
    "how",
    "what",
    "where",
    "who",
    "which",
];

/// Lines that end a message: a sign-off, on its own or before a name.
const SIGN_OFFS: &[&str] = &[
    "best regards",
    "kind regards",
    "warm regards",
    "regards",
    "best wishes",
    "all the best",
    "best",
    "warmly",
    "sincerely",
    "yours",
];

/// Words a greeting line starts with.
const GREETINGS: &[&str] = &["hi", "hey", "hello", "dear", "hiya", "morning", "good morning", "evening", "yo"];

/// Whether `body` only closes the conversation. See the module comment.
pub fn closes(body: &str) -> bool {
    let lines = own_lines(body);
    if lines.is_empty() {
        return false;
    }
    let mut lines: Vec<&str> = lines.iter().map(String::as_str).collect();
    // A greeting line ("Hi Sam,") says nothing either way.
    if lines.len() > 1 && is_greeting(lines[0]) {
        lines.remove(0);
    }
    // Names and sign-offs at the end ("Best,", "Maya") say nothing either
    // way; a closing sign-off ("Thanks,") is kept as words.
    while lines.len() > 1 {
        let last = lines[lines.len() - 1];
        if is_name(last) || is_sign_off(last) {
            lines.pop();
        } else {
            break;
        }
    }
    let text = lines.join("\n");
    if text.contains('?') || text.split_whitespace().count() > MOST_WORDS {
        return false;
    }
    // Every part closes, and at least one is more than a name: "Maya" on its
    // own is not a thank-you.
    let read: Vec<Part> = fragments(&text).iter().map(|f| part(f)).collect();
    read.iter().all(|p| *p != Part::Other) && read.contains(&Part::Closing)
}

/// What a fragment of a message is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    /// A thank-you, an okay, a pleasantry, a pictograph.
    Closing,
    /// A name on its own, as in "Thanks, Maya".
    Name,
    /// Anything else.
    Other,
}

/// The lines the writer wrote: before a quoted reply or a signature, without
/// quoted lines, trimmed, empty ones dropped.
fn own_lines(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw in body.lines() {
        let line = raw.trim();
        if line == "--" || line == "-- " || line.starts_with("-----Original Message") {
            break;
        }
        let lower = line.to_lowercase();
        if lower.starts_with("on ") && lower.ends_with("wrote:") {
            break;
        }
        if line.starts_with('>') || line.is_empty() {
            continue;
        }
        out.push(line.to_string());
    }
    out
}

fn is_greeting(line: &str) -> bool {
    let lower = line.to_lowercase();
    let words: Vec<&str> = lower.split_whitespace().collect();
    if words.is_empty() || words.len() > 4 || line.contains('?') {
        return false;
    }
    let lead = words[0].trim_matches(|c: char| !c.is_alphanumeric());
    let two =
        words.iter().take(2).map(|w| w.trim_matches(|c: char| !c.is_alphanumeric())).collect::<Vec<_>>().join(" ");
    GREETINGS.contains(&lead) || GREETINGS.contains(&two.as_str())
}

/// One to three words, each capitalised, none of them a closing phrase: a
/// name, or initials. "- Maya" and "Maya C." both count.
fn is_name(line: &str) -> bool {
    let line = line.trim_start_matches(['-', '–', '—', '~']).trim();
    let words: Vec<&str> = line.split_whitespace().collect();
    !words.is_empty()
        && words.len() <= 3
        && words.iter().all(|w| {
            let w = w.trim_end_matches([',', '.']);
            let mut chars = w.chars();
            chars.next().is_some_and(char::is_uppercase)
                && chars.all(|c| c.is_alphabetic() || c == '\'' || c == '-' || c == '.')
                && !ACKS.contains(&w.to_lowercase().as_str())
        })
}

/// "Best," or "Kind regards, Maya": a sign-off, with at most a name after it.
fn is_sign_off(line: &str) -> bool {
    let lower = line.to_lowercase();
    SIGN_OFFS.iter().any(|s| {
        lower.strip_prefix(s).is_some_and(|rest| {
            let rest = rest.trim_start_matches([',', '.', '!']).trim();
            let original = &line[line.len() - rest.len()..];
            rest.is_empty() || is_name(original)
        })
    })
}

/// The text split at sentence ends, commas, and line ends, with emoji kept
/// as fragments of their own.
fn fragments(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in text.chars() {
        if matches!(c, '.' | '!' | ',' | ';' | '\n' | '…' | ':' | '(' | ')') || is_emoji(c) {
            if !cur.trim().is_empty() {
                out.push(cur.trim().to_string());
            }
            cur.clear();
            if is_emoji(c) {
                out.push(c.to_string());
            }
        } else {
            cur.push(c);
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

/// Pictographs and the joiners and variation selectors that build them.
fn is_emoji(c: char) -> bool {
    matches!(c as u32,
        0x1F000..=0x1FAFF | 0x2600..=0x27BF | 0x2B00..=0x2BFF | 0xFE0F | 0x200D)
}

fn part(fragment: &str) -> Part {
    if fragment.chars().all(|c| is_emoji(c) || c.is_whitespace()) {
        // Every pictograph closes: a thumbs-up, a heart, a laugh. A question
        // is never only a pictograph, and a question mark was ruled out.
        return Part::Closing;
    }
    let lower = fragment.to_lowercase().replace(['’', '‘'], "'");
    let words: Vec<&str> = lower.split_whitespace().collect();
    // First, so that "Sorry" is never read as a name.
    if words.iter().any(|w| ASKING.contains(&w.trim_matches(|c: char| !c.is_alphanumeric() && c != '\''))) {
        return Part::Other;
    }
    let text = words.join(" ");
    if PLEASANTRIES.contains(&text.as_str()) || is_wish(&words) {
        return Part::Closing;
    }
    for ack in ACKS {
        let Some(rest) = text.strip_prefix(ack) else { continue };
        if !(rest.is_empty() || rest.starts_with(' ')) {
            continue;
        }
        if rest_closes(rest.trim(), &fragment[fragment.len() - rest.trim_start().len()..]) {
            return Part::Closing;
        }
    }
    if is_name(fragment) {
        return Part::Name;
    }
    Part::Other
}

/// What follows a closing phrase: nothing, "so much", "for the update", or a
/// name ("thanks Sam").
fn rest_closes(rest: &str, original: &str) -> bool {
    if rest.is_empty() || AFTER_ACK.contains(&rest) {
        return true;
    }
    if let Some(what) = rest.strip_prefix("for ") {
        // "for the update", "for letting me know", "for everything": a few
        // words of what the thanks is for.
        let n = what.split_whitespace().count();
        return (1..=5).contains(&n);
    }
    if let Some(more) = AFTER_ACK.iter().find_map(|a| rest.strip_prefix(a).map(str::trim)) {
        if more.is_empty() {
            return true;
        }
        let at = original.len() - more.len();
        return rest_closes(more, &original[at..]);
    }
    // "Thanks Sam", "thank you Dr Patel": a name after it, written as one.
    is_name(original)
}

fn is_wish(words: &[&str]) -> bool {
    // "have a good weekend", "have a great one", "enjoy your trip".
    let w: Vec<&str> = words.iter().map(|w| w.trim_matches(|c: char| !c.is_alphanumeric())).collect();
    match w.as_slice() {
        ["have", "a", wish, span] => WISHES.contains(wish) && SPANS.contains(span),
        ["have", "a", "really", wish, span] => WISHES.contains(wish) && SPANS.contains(span),
        ["enjoy", "your", span] | ["enjoy", "the", span] => SPANS.contains(span),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::closes;

    #[test]
    fn a_thank_you_and_an_okay_close_a_conversation() {
        for body in [
            "Thanks!",
            "thank you so much",
            "Thanks Sam",
            "Thanks, Sam!",
            "Got it, thanks.",
            "Perfect, thanks!",
            "Sounds good",
            "ok",
            "k",
            "Ok 👍",
            "👍",
            "🙏🙏",
            "Will do!",
            "Noted, thanks",
            "Great, see you then!",
            "Thanks for the update.",
            "Thank you for letting me know!",
            "Thanks again, have a great weekend!",
            "No worries!",
            "You're welcome!",
            "Anytime!",
            "Much appreciated.",
            "Cheers",
            "Awesome, thanks Maya!",
            "thx",
        ] {
            assert!(closes(body), "{body:?} should close");
        }
    }

    #[test]
    fn a_greeting_a_sign_off_and_a_name_say_nothing_either_way() {
        assert!(closes("Hi Sam,\n\nThanks so much!\n\nMaya"));
        assert!(closes("Hey Sam,\nGot it, thank you.\nBest,\nJonah"));
        assert!(closes("Thanks,\nMaya"));
        assert!(closes("Thank you!\n\nKind regards, Priya Nair"));
        assert!(closes("Thanks!\n\n-- \nMaya Chen\nHead of Marketing, Northwind\n+1 555 0100"));
        assert!(closes(
            "Thanks!\n\nOn Mon, Nov 3, 2026 at 9:00 AM Sam wrote:\n> Here's the deck. Can you check slide 4?"
        ));
    }

    #[test]
    fn anything_that_asks_for_something_does_not() {
        for body in [
            "Thanks! Can you send the deck too?",
            "thanks, can you send the deck too",
            "Thanks, but the numbers look off",
            "Great. Call me tomorrow.",
            "Thanks for the update, please send the slides as well",
            "Got it. When is the next one?",
            "ok but not the blue one",
            "Thanks for the update but we need to talk",
            "Thanks! Let me know when it's ready",
            "Perfect, I'll need the invoice by Friday",
            "Thanks, one question though",
            "thanks, actually could we move it",
            "Sorry, thanks",
            "Thank you, it didn't work",
        ] {
            assert!(!closes(body), "{body:?} should not close");
        }
    }

    #[test]
    fn anything_more_than_a_thank_you_stays_on_the_list() {
        for body in [
            "This is exactly what I needed. Thank you!",
            "Anytime! She's lovely. Coffee accepted.",
            "Got it, thanks Sam. Passing to accounts.",
            "did you see the game last night",
            "yes",
            "no",
            "lol",
            "Hi Sam,\n\nLeadership wants to move the launch up to the 14th.\n\nMaya",
            "",
            "   ",
            "Maya",
            "Hi Sam,",
            "Thanks for everything you did on the launch over the last three months, it really made a difference to the whole team and we could not have done it without you, honestly",
        ] {
            assert!(!closes(body), "{body:?} should not close");
        }
    }

    #[test]
    fn a_thank_you_for_something_long_is_not_only_a_thank_you() {
        assert!(closes("Thanks for sending the revised numbers"));
        assert!(!closes("Thanks for sending the revised numbers to the whole board yesterday"));
    }
}
