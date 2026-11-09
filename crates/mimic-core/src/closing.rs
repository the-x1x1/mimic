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
//! Everything is compared word by word, each word lower-cased on its own, and
//! nothing is ever cut out of the text by a length measured on a lower-cased
//! copy: lower-casing can change how many bytes a letter takes (alpha.106
//! panicked on "ok ẞẞ", and the panic failed every query that read it).
//! A name is only a name when it is one of the names the caller passes — the
//! sender's and the user's own — and only where names go: after a thank-you
//! ("Thanks, Sam"), after a greeting, or on a line of its own at the end.
//! alpha.108 guessed names from capital letters, and a phone that capitalises
//! every line made "Thanks!" over "Resend Invoice" a thank-you to someone
//! called Resend Invoice. Only a few pictographs close anything
//! (a thumbs-up, a heart, a smile); a few more go with a thank-you but say
//! nothing alone (🙏, a laugh); any other — ❓, 🤔, 📞 — is not a thank-you.
//! And whether a thread is only closing is decided over everything they sent
//! since the user last wrote, not just the last of it (`repo_waiting`).
//!
//! The result is a reading, like the others, and the screen says it that way;
//! the user's own call on a thread (`thread_marks`) outranks it.

/// Messages longer than this, in words, are never only closing.
const MOST_WORDS: usize = 25;

/// Lines read before giving up: a long message is not a thank-you, and is not
/// read to the end to find that out.
const MOST_LINES: usize = 12;

/// Closing phrases, as a whole fragment or at its start, in words. Longest
/// first, so "thank you so much" is read before "thank you".
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
];

/// The closing phrases that thank, and so may say what for.
const THANKS: &[&str] = &[
    "thank you so much",
    "thank you very much",
    "thanks so much",
    "thanks very much",
    "thanks a lot",
    "thanks a ton",
    "thanks a million",
    "many thanks",
    "thank you",
    "thank u",
    "thankyou",
    "thanks",
    "thx",
    "tysm",
    "ty",
];

/// What may follow a closing phrase in the same fragment. Not "in advance":
/// thanks in advance is what comes after an ask, often one in the subject.
const AFTER_ACK: &[&str] = &["so much", "very much", "a lot", "again", "too", "as always"];

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

/// A word anywhere in a fragment, or a line taken for a name, that makes it
/// more than a thank-you: modals, conjunctions, question words, negations,
/// and words that ask for something done. A name is never one of these.
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
    "asap",
    "urgent",
    "urgently",
    "thoughts",
    "confirm",
    "advise",
    "call",
    "send",
    "remind",
    "check",
    "review",
    "reply",
    "respond",
    "help",
    "let",
    "tell",
    "update",
    "fix",
    "ring",
    "text",
    "email",
    "come",
    "bring",
    "pick",
    "grab",
    "get",
    "buy",
    "pay",
    "sign",
    "approve",
    "is",
    "are",
    "do",
    "does",
    "did",
    "will",
    "shall",
    "may",
    "might",
    "must",
    "now",
    "today",
    "tonight",
    "tomorrow",
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

/// Every way of writing a question mark, the pictographs among them.
const QUESTION_MARKS: [char; 14] = ['?', '？', '¿', '؟', '﹖', '❓', '❔', '⁉', '‽', '⸮', '⁇', '⁈', '፧', '՞'];

/// Bodies longer than this, in bytes, are never only closing, and are not
/// read to find that out: every thread's deciding message is read on every
/// load of the home screen.
const MOST_BYTES: usize = 8 * 1024;

/// What a thank-you may be for, word for word after "for": a closed list, so
/// that "thank you for your prompt response" (a reply is wanted) and "thanks
/// for the reminder" (something is overdue) are not thank-yous.
const THANKED_FOR: &[&str] = &[
    "the update",
    "the updates",
    "the info",
    "the information",
    "the help",
    "your help",
    "all your help",
    "all the help",
    "the call",
    "the chat",
    "the catch up",
    "the catch-up",
    "letting me know",
    "the heads up",
    "the heads-up",
    "everything",
    "that",
    "this",
    "the note",
    "the reply",
    "your reply",
    "the quick reply",
    "the quick response",
    "getting back to me",
    "sending",
    "sending it",
    "sending that",
    "sending this",
    "sending them",
    "the link",
    "the photos",
    "the pics",
    "the pictures",
    "the invite",
    "the invitation",
    "having me",
    "having us",
    "the flowers",
    "the gift",
    "the card",
    "dinner",
    "lunch",
    "coffee",
    "tonight",
    "today",
    "yesterday",
    "checking",
    "checking in",
    "asking",
    "the kind words",
    "your kind words",
    "sharing",
    "sharing it",
    "sharing this",
    "sharing that",
    "the feedback",
    "your feedback",
    "the intro",
    "the introduction",
    "the recommendation",
    "the tip",
    "the advice",
    "your advice",
    "the lift",
    "the ride",
    "coming",
    "doing this",
    "doing that",
    "sorting it",
    "sorting that",
    "sorting this",
    "sorting that out",
    "sorting it out",
    "the help today",
    "the help yesterday",
    "the notes",
    "your notes",
    "the deck",
    "the doc",
    "the docs",
    "the document",
    "the slides",
    "the file",
    "the files",
    "the summary",
    "the recap",
    "the details",
    "the photo",
    "the pic",
    "the video",
    "the article",
    "the pointer",
    "the pointers",
];

/// Words a greeting line starts with.
const GREETINGS: &[&str] = &["hi", "hey", "hello", "dear", "hiya", "morning", "good morning", "evening", "yo"];

/// Pictographs that close a conversation on their own: an okay, a heart, a
/// smile, a cheer.
const CLOSING_EMOJI: &[char] = &[
    '👍', '👌', '✅', '🙌', '🎉', '🥳', '❤', '♥', '💙', '💚', '💛', '💜', '🧡', '😊', '🙂', '😀', '😃', '😄', '😁',
    '☺', '😍', '🤗', '💯', '👏', '✌',
];

/// Pictographs that go with a thank-you but say nothing alone: 🙏 is "please"
/// as often as "thanks", and a laugh is not a thank-you.
const NEUTRAL_EMOJI: &[char] = &['🙏', '😂', '🤣', '😅', '😆', '😉', '😎'];

/// Whether `body` only closes the conversation, knowing no names.
pub fn closes(body: &str) -> bool {
    closes_with(body, "")
}

/// The names a message may use without asking anything: every word of
/// `names` (the sender's display name and the user's, in SQL), lower-cased.
struct Names(Vec<String>);

impl Names {
    /// Every word of `names` — but never a word that asks or a closing
    /// phrase: a sender sets their own display name, and one called "Call
    /// Me" would otherwise make "Thanks, Call Me" a thank-you.
    fn of(names: &str) -> Self {
        Names(
            names
                .split(|c: char| !c.is_alphabetic() && c != '\'' && c != '-')
                .filter(|w| w.chars().count() > 1)
                .map(str::to_lowercase)
                .filter(|w| {
                    !ASKING.contains(&w.as_str()) && !ACKS.contains(&w.as_str()) && !GREETINGS.contains(&w.as_str())
                })
                .collect(),
        )
    }

    /// One to three words, each one of these names: "Sam", "Maya Chen".
    fn all(&self, words: &[(String, String)]) -> bool {
        !words.is_empty() && words.len() <= 3 && words.iter().all(|(_, low)| self.0.contains(low))
    }
}

/// Whether `body` only closes the conversation, where `names` are the names
/// it may use (the sender's and the user's). See the module comment.
pub fn closes_with(body: &str, names: &str) -> bool {
    let names = Names::of(names);
    let Some(mut lines) = own_lines(body) else { return false };
    // Asked before anything is set aside: "Thanks!" over "Friday?" asks.
    if lines.iter().any(|l| l.contains(QUESTION_MARKS)) {
        return false;
    }
    // A greeting line ("Hi Sam,") says nothing either way, when it is only a
    // greeting and perhaps a name.
    if lines.len() > 1 && is_greeting(&lines[0], &names) {
        lines.remove(0);
    }
    // Names and sign-offs at the end ("Best,", "Maya") say nothing either
    // way; a closing sign-off ("Thanks,") is kept as words. At most two.
    let mut popped = 0;
    while lines.len() > 1 && popped < 2 {
        let last = &lines[lines.len() - 1];
        if names.all(&words_of(last)) || is_sign_off(last, &names) {
            lines.pop();
            popped += 1;
        } else {
            break;
        }
    }
    let text = lines.join("\n");
    if text.split_whitespace().count() > MOST_WORDS {
        return false;
    }
    // Every part closes, and at least one is a thank-you rather than a name or
    // a neutral pictograph: "Maya" on its own is not one, nor is "🙏".
    let read: Vec<Part> = fragments(&text).iter().map(|f| part(&f.text, f.after_comma, &names)).collect();
    let mut closing = false;
    for (i, p) in read.iter().enumerate() {
        match p {
            Part::Closing => closing = true,
            Part::Neutral => {}
            // A name only straight after a thank-you, as in "Thanks, Sam".
            Part::Name => {
                if i == 0 || read[i - 1] != Part::Closing {
                    return false;
                }
            }
            Part::Other => return false,
        }
    }
    closing
}

/// What a fragment of a message is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    /// A thank-you, an okay, a pleasantry, a closing pictograph.
    Closing,
    /// A pictograph that goes with a thank-you but says nothing alone.
    Neutral,
    /// A name, which may follow a thank-you after a comma.
    Name,
    /// Anything else.
    Other,
}

/// The lines the writer wrote: before a quoted reply or a signature, without
/// quoted lines, trimmed, empty ones dropped. `None` once it is plainly too
/// long to be a thank-you, without reading the rest.
fn own_lines(body: &str) -> Option<Vec<String>> {
    if body.len() > MOST_BYTES {
        return None;
    }
    let mut out = Vec::new();
    let mut words = 0;
    let mut lines = body.lines();
    while let Some(raw) = lines.next() {
        let line = raw.trim();
        if line.starts_with("-----Original Message") {
            // Outlook's: what follows is the message answered, unquoted.
            break;
        }
        if line == "--" {
            // A signature. Nothing in it may ask: a chat message with a "--"
            // line is not an email's signature.
            // A P.S. under it is the writer's own words, and anything that
            // asks there asks.
            if lines.clone().any(|l| {
                let low = l.trim_start().to_lowercase();
                l.contains(QUESTION_MARKS)
                    || asks(&words_of(l), ASKING)
                    || low.starts_with("p.s")
                    || low.starts_with("ps")
            }) {
                return None;
            }
            break;
        }
        let first = line.split_whitespace().next().unwrap_or_default();
        if first.eq_ignore_ascii_case("on") && line.ends_with("wrote:") {
            // What follows is quoted; anything written under it, unquoted, is
            // a reply written below the quote, which is not read here.
            if lines.clone().any(|l| !l.trim().is_empty() && !l.trim_start().starts_with('>')) {
                return None;
            }
            break;
        }
        if line.starts_with('>') || line.is_empty() {
            continue;
        }
        words += line.split_whitespace().count();
        // Room for a greeting and a two-line sign-off above the limit.
        if out.len() >= MOST_LINES || words > MOST_WORDS + 12 {
            return None;
        }
        out.push(line.to_string());
    }
    Some(out)
}

/// The words of a line or fragment, each lower-cased on its own and with the
/// punctuation round it taken off, beside the word as written.
fn words_of(text: &str) -> Vec<(String, String)> {
    text.split_whitespace()
        .filter_map(|w| {
            let bare = w.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'' && c != '’');
            if !bare.is_empty() {
                return Some((bare.to_string(), bare.to_lowercase().replace('’', "'")));
            }
            // A word of nothing but symbols: ordinary punctuation says
            // nothing, but "▶" or "©" is a word, and not a thank-you.
            (!w.chars().all(is_plain_punctuation)).then(|| (w.to_string(), w.to_string()))
        })
        .collect()
}

/// Punctuation that says nothing either way.
fn is_plain_punctuation(c: char) -> bool {
    ".!,;:-~'\"()…".contains(c)
}

/// Whether `words` (lower-cased) start with `phrase`, word for word; the
/// number of words it took.
fn starts_with_phrase(words: &[(String, String)], phrase: &str) -> Option<usize> {
    let p: Vec<&str> = phrase.split(' ').collect();
    (words.len() >= p.len() && words.iter().zip(&p).all(|((_, low), w)| low == w)).then_some(p.len())
}

fn is_greeting(line: &str, names: &Names) -> bool {
    let words = words_of(line);
    for g in GREETINGS {
        if let Some(n) = starts_with_phrase(&words, g) {
            // Only a greeting, and at most a name after it: "Hi Sam," yes;
            // "Hey call me asap" no.
            let rest = &words[n..];
            return rest.is_empty() || names.all(rest);
        }
    }
    false
}

/// "Best," or "Kind regards, Maya": a sign-off, with at most a name after it.
fn is_sign_off(line: &str, names: &Names) -> bool {
    let words = words_of(line);
    SIGN_OFFS.iter().any(|s| starts_with_phrase(&words, s).is_some_and(|n| n == words.len() || names.all(&words[n..])))
}

/// Whether any of `words` is one of `list`.
fn asks(words: &[(String, String)], list: &[&str]) -> bool {
    words.iter().any(|(_, low)| list.contains(&low.as_str()))
}

/// A part of a message, and whether a comma came before it.
struct Fragment {
    text: String,
    after_comma: bool,
}

/// The text split at sentence ends, commas, dashes and line ends, with each
/// pictograph a fragment of its own.
fn fragments(text: &str) -> Vec<Fragment> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut comma = false;
    let push = |cur: &mut String, comma: bool, out: &mut Vec<Fragment>| {
        if !cur.trim().is_empty() {
            out.push(Fragment { text: cur.trim().to_string(), after_comma: comma });
        }
        cur.clear();
    };
    let chars: Vec<char> = text.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        // A hyphen between letters is part of a word; with space round it,
        // it is a dash.
        let dash = matches!(c, '–' | '—')
            || (c == '-'
                && chars.get(i.wrapping_sub(1)).is_some_and(|p| p.is_whitespace())
                && chars.get(i + 1).is_none_or(|n| n.is_whitespace()));
        if matches!(c, '.' | '!' | ',' | ';' | '\n' | '…' | ':' | '(' | ')' | '/' | '&' | '+')
            || dash
            || is_pictograph(c)
        {
            let was_comma = c == ',';
            push(&mut cur, comma, &mut out);
            comma = was_comma;
            if is_pictograph(c) {
                out.push(Fragment { text: c.to_string(), after_comma: false });
            }
        } else if is_joiner(c) {
            // Skin tones, variation selectors and joiners belong to the
            // pictograph before them.
        } else {
            cur.push(c);
        }
    }
    push(&mut cur, comma, &mut out);
    out
}

/// Pictographs and symbols drawn as emoji.
fn is_pictograph(c: char) -> bool {
    matches!(c as u32, 0x1F000..=0x1FAFF | 0x2600..=0x27BF | 0x2B00..=0x2BFF | 0x2190..=0x21FF | 0x2300..=0x23FF)
        && !is_joiner(c)
}

/// What joins or modifies a pictograph without being one.
fn is_joiner(c: char) -> bool {
    matches!(c as u32, 0xFE0E | 0xFE0F | 0x200D | 0x1F3FB..=0x1F3FF | 0x20E3)
}

fn part(fragment: &str, after_comma: bool, names: &Names) -> Part {
    let mut chars = fragment.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        if is_pictograph(c) {
            return if CLOSING_EMOJI.contains(&c) {
                Part::Closing
            } else if NEUTRAL_EMOJI.contains(&c) {
                Part::Neutral
            } else {
                Part::Other
            };
        }
    }
    let words = words_of(fragment);
    if words.is_empty() {
        // Only ordinary punctuation says nothing either way; any other
        // symbol ("▶", "‼", "©") is not a thank-you.
        return if fragment.chars().all(|c| c.is_whitespace() || is_plain_punctuation(c)) {
            Part::Neutral
        } else {
            Part::Other
        };
    }
    let lows: Vec<&str> = words.iter().map(|(_, l)| l.as_str()).collect();
    if PLEASANTRIES.contains(&lows.join(" ").as_str()) || is_wish(&lows) {
        return Part::Closing;
    }
    // A closing phrase, then nothing that asks: "will do" closes, "thanks,
    // will you call" does not.
    for ack in ACKS {
        if let Some(n) = starts_with_phrase(&words, ack) {
            let rest = &words[n..];
            if rest.first().is_some_and(|(_, w)| w == "for") {
                // "Thanks for the update", "thank you for letting me know":
                // what for, from a closed list, after a thank-you only.
                let what: Vec<&str> = rest[1..].iter().map(|(_, w)| w.as_str()).collect();
                if THANKS.contains(ack) && THANKED_FOR.contains(&what.join(" ").as_str()) {
                    return Part::Closing;
                }
            } else if !asks(rest, ASKING) && rest_closes(rest, names) {
                return Part::Closing;
            }
        }
    }
    // A name, only where a comma set it off from a thank-you before it.
    if after_comma && names.all(&words) {
        return Part::Name;
    }
    Part::Other
}

/// What follows a closing phrase: nothing, "so much", "for the update", or a
/// name ("thanks Sam").
fn rest_closes(rest: &[(String, String)], names: &Names) -> bool {
    if rest.is_empty() {
        return true;
    }
    for a in AFTER_ACK {
        if let Some(n) = starts_with_phrase(rest, a) {
            return rest_closes(&rest[n..], names);
        }
    }
    // "Thanks Sam": one of the names after it.
    names.all(rest)
}

fn is_wish(words: &[&str]) -> bool {
    // "have a good weekend", "have a great one", "enjoy your trip".
    match words {
        ["have", "a", wish, span] => WISHES.contains(wish) && SPANS.contains(span),
        ["have", "a", "really", wish, span] => WISHES.contains(wish) && SPANS.contains(span),
        ["enjoy", "your", span] | ["enjoy", "the", span] => SPANS.contains(span),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    /// The sender's name and the user's, as the query passes them.
    fn closes(body: &str) -> bool {
        super::closes_with(body, "Maya Chen Sam Rivera Jonah Okafor Priya Nair")
    }

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
            "👍🏽",
            "Thanks 🙏",
            "Thank you ❤️",
            "Will do!",
            "Noted, thanks",
            "Great, see you then!",
            "Thanks for the update.",
            "Thank you for letting me know!",
            "Thanks again, have a great weekend!",
            "Thanks, Sam",
            "Thanks for this!",
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
        assert!(closes("Thanks for sending it"));
        assert!(!closes("Thanks for sending the revised numbers to the whole board yesterday"));
    }

    #[test]
    fn a_request_in_capitals_or_title_case_is_not_a_name() {
        for body in [
            "Thanks!\nPlease Advise",
            "Ok.\nPLEASE CONFIRM",
            "Thanks.\nAsap",
            "OK. CALL ME.",
            "THANKS SEND IT NOW",
            "Thanks. Thoughts",
            "Great. Thoughts",
            "Thanks, Confirm Receipt",
            "Ok, Urgent",
            "Thanks Call Me",
            "ok Remind Me",
            "Thanks for the invite, Is It Friday",
            "Thanks. Maya",
            "Maya, thanks",
        ] {
            assert!(!closes(body), "{body:?} should not close");
        }
    }

    #[test]
    fn a_greeting_that_asks_and_an_ask_after_a_dash_do_not_close() {
        for body in [
            "Hey call me asap\nThanks",
            "Hi, send the deck\nThanks!",
            "Thanks for this - send the final",
            "Thanks for this — call me tomorrow",
            "Thanks for this – the final version tonight",
        ] {
            assert!(!closes(body), "{body:?} should not close");
        }
    }

    #[test]
    fn only_pictographs_that_close_close() {
        for body in ["Invoice ❓", "Ok 🤔", "Report 🙏", "ok 👍 📞", "🙏", "😂", "📞", "ok ❓"] {
            assert!(!closes(body), "{body:?} should not close");
        }
        assert!(closes("👍👍"));
        assert!(closes("❤️"));
    }

    #[test]
    fn thanks_in_advance_is_what_comes_after_an_ask() {
        assert!(!closes("Thanks in advance!\nMaya"));
        assert!(!closes("Thanks in advance"));
    }

    #[test]
    fn letters_that_change_size_when_lower_cased_never_panic() {
        // Each of these took a slice at a length measured on the lower-cased
        // text in alpha.106, and panicked.
        for body in [
            "ok ẞẞ",
            "ok \u{212A}\u{212A}\u{212A}\u{212A} x",
            "ok İİİİİİİİİİ",
            "Thanks İstanbul",
            "Best, ẞtefan",
            "thanks \u{212A}elvin",
            "ȺȺȺ thanks",
            "Ok ǅ",
            "thanks Ⱥ",
        ] {
            let _ = closes(body);
        }
    }

    #[test]
    fn a_long_message_is_not_read_to_the_end() {
        let long = "word ".repeat(10_000);
        assert!(!closes(&format!("Thanks!\n{long}")));
        assert!(!closes(&"Thanks!\n".repeat(100)));
    }

    #[test]
    fn a_question_mark_on_a_last_line_is_still_a_question() {
        for body in [
            "Thanks!\nFriday?",
            "Thanks\nSam?",
            "Got it\nSame Link?",
            "ok\nZoom?",
            "Thanks!\nMonday？",
            "Thanks؟",
            "thanks﹖",
        ] {
            assert!(!closes(body), "{body:?} should not close");
        }
    }

    #[test]
    fn only_the_names_given_are_names() {
        for body in [
            "Thanks!\nResend Invoice",
            "Thanks\nResend",
            "Thanks, Zoom Link",
            "Great, Share Deck",
            "Thanks Book Room",
            "Thanks Forward Deck",
            "Ok Ship It",
            "Cool, Discuss Monday",
            "k\nDeadline Friday",
            "Thanks, Schedule Meeting",
            "thx, Eta",
            "ok, Status",
            "Thanks!\nRsvp",
            "Thanks!\nBest,\nResend Deck",
            "Dear Resend\nThanks",
        ] {
            assert!(!closes(body), "{body:?} should not close");
        }
        // The same names, given, are names.
        assert!(closes("Thanks, Maya"));
        assert!(closes("Hi Sam,\nThanks!\nMaya Chen"));
        // And knowing no names, no word is one.
        assert!(!super::closes("Thanks, Maya"));
        assert!(super::closes("Thanks!"));
    }

    #[test]
    fn a_thank_you_for_something_is_only_that() {
        for body in [
            "thanks for the draft send the final",
            "thanks for that call me later",
            "thanks for this review it tonight",
            "Thanks for the update tell me more",
            "thanks for the deck sign and return",
            "thanks for the invoice pay by friday",
            "ok for me to call you",
            "Thanks for nothing",
        ] {
            assert!(!closes(body), "{body:?} should not close");
        }
        for body in
            ["Thanks for the update", "thank you for the call", "Thanks for your help!", "thanks for letting me know"]
        {
            assert!(closes(body), "{body:?} should close");
        }
    }

    #[test]
    fn what_is_written_under_a_quote_or_a_dashed_line_is_read() {
        assert!(!closes("Thanks!\nOn Monday Sam wrote:\nalso send the deck"));
        assert!(!closes("ok\n--\nalso can you resend the deck"));
        assert!(closes("Thanks!\nOn Monday Sam wrote:\n> here's the deck"));
        assert!(closes("Thanks!\n--\nMaya Chen\nNorthwind"));
    }

    #[test]
    fn a_ps_under_a_signature_is_read() {
        for body in [
            "Thanks!\n-- \nMaya\nP.S. send me the deck by Friday",
            "Thanks!\n--\nMaya\nPS - the invoice is overdue, pay by Friday",
            "Thanks!\n--\nmeeting moved to 8am, bring the contract",
            "Thanks\n--\nreview the attached and sign",
        ] {
            assert!(!closes(body), "{body:?} should not close");
        }
    }

    #[test]
    fn a_display_name_that_asks_is_not_a_name() {
        for (body, names) in [
            ("Thanks, Call Me", "Call Me Sam Rivera"),
            ("Thanks\nCall me", "Call Me Sam Rivera"),
            ("Thanks!\nsend me\nthe invoice", "Send Me The Invoice Sam Rivera"),
            ("Thanks, Sign Here", "Sign Here"),
            ("Hi Call Me,\nThanks", "Call Me"),
            ("Ok, pay Bill", "Bill Pay"),
            ("ok, will grant", "Will Grant"),
        ] {
            assert!(!super::closes_with(body, names), "{body:?} with {names:?} should not close");
        }
    }

    #[test]
    fn a_thank_you_that_asks_for_attention_or_brings_bad_news_does_not_close() {
        for body in [
            "Thank you for your prompt response.",
            "Thank you for your prompt attention.",
            "Thanks for your cooperation",
            "Thank you for your consideration",
            "Thanks for the reminder",
            "Thanks for the overdue invoice",
            "Thanks for the late payment",
            "Thanks for the deadline Friday",
            "Done.",
            "Done 👍",
            "Received.",
        ] {
            assert!(!closes(body), "{body:?} should not close");
        }
    }

    #[test]
    fn symbols_that_are_not_thank_yous_are_not() {
        for body in ["ok ⁇", "ok⁈", "ok፧", "ok՞", "ok ▶", "ok ◻", "ok ‼", "ok ©", "ok ㊙", "ok #️⃣"]
        {
            assert!(!closes(body), "{body:?} should not close");
        }
        assert!(closes("ok!!"));
        assert!(closes("thanks..."));
    }

    #[test]
    fn a_large_body_is_not_read() {
        assert!(!closes(&"thanks.".repeat(3_000)));
    }
}
