//! Deterministic voice metrics.
//!
//! Everything in this file is arithmetic over message text. No model is
//! involved and none is needed: "how long are this person's messages", "do
//! they end sentences with a full stop", "do they start with a greeting" are
//! counting problems, and counting them is both cheaper and more honest than
//! asking a language model to estimate them.
//!
//! Two rules hold throughout:
//!
//! * A metric that cannot be computed is `None`, never zero. `emojiRate: 0.0`
//!   means "this person does not use emoji"; `null` means "we have not seen
//!   enough of their writing to say".
//! * Every metric is defined in one sentence in `docs/VOICE_ENGINE.md` and
//!   computed in exactly one place, here.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Below this many messages a profile reports its sample size and nothing
/// else. Rates over a handful of messages are noise wearing a decimal point.
pub const MIN_SAMPLE: usize = 20;

/// How many phrases and openers a profile keeps.
const TOP_N: usize = 8;

/// `serde(default)` matters: a profile row written by an older build, or a
/// hand-set override that carries only one metric, must still deserialize into
/// a usable struct rather than collapsing to "nothing was measured".
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct VoiceMetrics {
    pub sample_size: usize,
    /// True when `sample_size >= MIN_SAMPLE`. When false every rate below is
    /// `None` and the UI says so rather than drawing a chart of nothing.
    pub measurable: bool,

    pub avg_words_per_message: Option<f64>,
    pub median_words_per_message: Option<f64>,
    pub p90_words_per_message: Option<f64>,
    pub avg_sentences_per_message: Option<f64>,
    pub multi_paragraph_rate: Option<f64>,

    pub terminal_period_rate: Option<f64>,
    pub question_rate: Option<f64>,
    pub exclamation_rate: Option<f64>,
    pub ellipsis_rate: Option<f64>,

    pub emoji_rate: Option<f64>,
    pub lowercase_start_rate: Option<f64>,
    pub all_lowercase_rate: Option<f64>,
    pub contractions_per_100_words: Option<f64>,

    pub greeting_rate: Option<f64>,
    pub sign_off_rate: Option<f64>,
    pub top_greetings: Vec<(String, usize)>,
    pub top_sign_offs: Vec<(String, usize)>,
    pub top_phrases: Vec<(String, usize)>,

    /// Median seconds between a message from someone else and the user's
    /// reply. `None` when no pair in the sample had both timestamps.
    pub median_response_seconds: Option<i64>,
}

/// One message as the analyzer sees it.
#[derive(Debug, Clone)]
pub struct Sample<'a> {
    pub body: &'a str,
    pub response_latency_seconds: Option<i64>,
}

impl<'a> Sample<'a> {
    pub fn new(body: &'a str) -> Self {
        Self { body, response_latency_seconds: None }
    }
}

/// Every metric over a set of messages held in memory. The same arithmetic as
/// feeding them one at a time to an [`Accumulator`], which is what analysis
/// does so that no scope is ever held whole.
pub fn compute(samples: &[Sample<'_>]) -> VoiceMetrics {
    let mut acc = Accumulator::default();
    for s in samples {
        acc.push(s);
    }
    acc.finish()
}

/// Past this many distinct phrases a scope's phrase counts are thinned: the
/// phrases seen least so far are forgotten until half the room is free. Below
/// it (a few thousand messages) every phrase is counted exactly; above it a
/// habit, which recurs, survives, and a phrase said once does not.
const PHRASE_ROOM: usize = 200_000;

/// The metrics of a scope worked out one message at a time, in memory that
/// does not grow with the scope's text: word counts are kept as a histogram
/// (a message is some whole number of words), phrases in a table thinned at
/// [`PHRASE_ROOM`], and one number per timed reply.
#[derive(Debug, Default)]
pub struct Accumulator {
    n: usize,
    words: std::collections::BTreeMap<i64, usize>,
    total_words: f64,
    sentences: f64,
    paragraphs: f64,
    periods: f64,
    questions: f64,
    bangs: f64,
    ellipses: f64,
    emoji: f64,
    lower_start: f64,
    all_lower: f64,
    contractions: f64,
    greetings: f64,
    sign_offs: f64,
    greeting_counts: HashMap<String, usize>,
    sign_off_counts: HashMap<String, usize>,
    phrase_counts: HashMap<String, usize>,
    latencies: Vec<i64>,
}

impl Accumulator {
    /// How many messages have been pushed.
    pub fn len(&self) -> usize {
        self.n
    }

    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    pub fn push(&mut self, s: &Sample<'_>) {
        self.n += 1;
        let body = s.body.trim();
        let tokens = crate::db::word_count(body);
        *self.words.entry(tokens).or_default() += 1;
        self.total_words += tokens as f64;
        self.sentences += count_sentences(body) as f64;
        if body.contains("\n\n") {
            self.paragraphs += 1.0;
        }
        match last_meaningful_char(body) {
            Some('.') => self.periods += 1.0,
            Some('?') => self.questions += 1.0,
            Some('!') => self.bangs += 1.0,
            _ => {}
        }
        if body.contains("...") || body.contains('…') {
            self.ellipses += 1.0;
        }
        if body.chars().any(is_emoji) {
            self.emoji += 1.0;
        }
        if body.chars().find(|c| c.is_alphabetic()).is_some_and(char::is_lowercase) {
            self.lower_start += 1.0;
        }
        if body.chars().any(char::is_alphabetic) && !body.chars().any(char::is_uppercase) {
            self.all_lower += 1.0;
        }
        self.contractions += count_contractions(body) as f64;
        if let Some(g) = opening_greeting(body) {
            self.greetings += 1.0;
            *self.greeting_counts.entry(g).or_default() += 1;
        }
        if let Some(sg) = closing_sign_off(body) {
            self.sign_offs += 1.0;
            *self.sign_off_counts.entry(sg).or_default() += 1;
        }
        for phrase in phrases(body) {
            *self.phrase_counts.entry(phrase).or_default() += 1;
        }
        thin(&mut self.phrase_counts, PHRASE_ROOM);
        if let Some(l) = s.response_latency_seconds.filter(|l| *l >= 0) {
            self.latencies.push(l);
        }
    }

    pub fn finish(self) -> VoiceMetrics {
        let n = self.n;
        let mut m = VoiceMetrics { sample_size: n, measurable: n >= MIN_SAMPLE, ..Default::default() };
        if !m.measurable {
            return m;
        }
        let fnn = n as f64;
        m.avg_words_per_message = Some(round2(self.total_words / fnn));
        m.median_words_per_message = Some(round2(percentile(&self.words, n, 0.5)));
        m.p90_words_per_message = Some(round2(percentile(&self.words, n, 0.9)));
        m.avg_sentences_per_message = Some(round2(self.sentences / fnn));
        m.multi_paragraph_rate = Some(round3(self.paragraphs / fnn));
        m.terminal_period_rate = Some(round3(self.periods / fnn));
        m.question_rate = Some(round3(self.questions / fnn));
        m.exclamation_rate = Some(round3(self.bangs / fnn));
        m.ellipsis_rate = Some(round3(self.ellipses / fnn));
        m.emoji_rate = Some(round3(self.emoji / fnn));
        m.lowercase_start_rate = Some(round3(self.lower_start / fnn));
        m.all_lowercase_rate = Some(round3(self.all_lower / fnn));
        m.contractions_per_100_words =
            Some(if self.total_words > 0.0 { round2(self.contractions * 100.0 / self.total_words) } else { 0.0 });
        m.greeting_rate = Some(round3(self.greetings / fnn));
        m.sign_off_rate = Some(round3(self.sign_offs / fnn));
        m.top_greetings = top(self.greeting_counts, TOP_N, 1);
        m.top_sign_offs = top(self.sign_off_counts, TOP_N, 1);
        // A phrase seen once is a sentence, not a habit.
        m.top_phrases = top(self.phrase_counts, TOP_N, 3);
        let mut latencies = self.latencies;
        if !latencies.is_empty() {
            latencies.sort_unstable();
            m.median_response_seconds = Some(latencies[latencies.len() / 2]);
        }
        m
    }
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}
fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

/// Past `room` entries, forget the phrases seen least — once, then twice, and
/// so on — until at most half the room is taken.
fn thin(counts: &mut HashMap<String, usize>, room: usize) {
    if counts.len() <= room {
        return;
    }
    let mut floor = 2;
    while counts.len() > room / 2 {
        counts.retain(|_, c| *c >= floor);
        floor += 1;
    }
}

/// The value at quantile `q` of `n` word counts held as a histogram: the
/// `round((n - 1) * q)`th smallest, as it would be in the sorted list.
fn percentile(histogram: &std::collections::BTreeMap<i64, usize>, n: usize, q: f64) -> f64 {
    if n == 0 {
        return 0.0;
    }
    let idx = ((n - 1) as f64 * q).round() as usize;
    let mut seen = 0usize;
    for (value, count) in histogram {
        seen += count;
        if seen > idx {
            return *value as f64;
        }
    }
    0.0
}

/// Most frequent first, then alphabetical, so the output is stable across runs.
fn top(counts: HashMap<String, usize>, limit: usize, min_count: usize) -> Vec<(String, usize)> {
    let mut v: Vec<(String, usize)> = counts.into_iter().filter(|(_, c)| *c >= min_count).collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    v.truncate(limit);
    v
}

fn last_meaningful_char(body: &str) -> Option<char> {
    body.chars().rev().find(|c| !c.is_whitespace() && !is_emoji(*c) && *c != '"' && *c != '\'' && *c != ')')
}

fn count_sentences(body: &str) -> usize {
    let n = body.split(['.', '!', '?']).filter(|s| s.chars().any(char::is_alphanumeric)).count();
    n.max(if body.chars().any(char::is_alphanumeric) { 1 } else { 0 })
}

/// Apostrophes that sit between letters. Counts both typewriter and typographic
/// forms, and does not count a possessive-looking `s'` at a word boundary.
fn count_contractions(body: &str) -> usize {
    let chars: Vec<char> = body.chars().collect();
    let mut n = 0;
    for i in 1..chars.len().saturating_sub(1) {
        if (chars[i] == '\'' || chars[i] == '\u{2019}') && chars[i - 1].is_alphabetic() && chars[i + 1].is_alphabetic()
        {
            n += 1;
        }
    }
    n
}

/// Emoji and pictographic symbols. Deliberately a range check rather than a
/// dependency: the exact boundary matters far less than being stable.
fn is_emoji(c: char) -> bool {
    matches!(c as u32,
        0x1F300..=0x1FAFF   // pictographs, emoticons, symbols, supplements
        | 0x1F000..=0x1F2FF // mahjong, dominoes, enclosed
        | 0x2600..=0x27BF   // misc symbols and dingbats
        | 0x2190..=0x21FF   // arrows
        | 0xFE0F            // variation selector-16
        | 0x2B00..=0x2BFF)
}

const GREETINGS: [&str; 12] = [
    "hi",
    "hey",
    "hello",
    "morning",
    "good morning",
    "good afternoon",
    "good evening",
    "yo",
    "hiya",
    "heya",
    "dear",
    "greetings",
];

const SIGN_OFFS: [&str; 14] = [
    "thanks",
    "thank you",
    "cheers",
    "best",
    "best regards",
    "regards",
    "kind regards",
    "talk soon",
    "later",
    "sincerely",
    "all the best",
    "ta",
    "much appreciated",
    "appreciate it",
];

/// The greeting a message opens with, normalized, or `None`.
fn opening_greeting(body: &str) -> Option<String> {
    let first = body.lines().find(|l| !l.trim().is_empty())?.trim();
    // Only the opening clause counts: "Hi Ada," or "hey —" or "Hello!".
    let clause = first.split([',', '!', '-', '—', ':']).next().unwrap_or(first).trim().to_lowercase();
    let words: Vec<&str> = clause.split_whitespace().collect();
    for take in [2usize, 1] {
        if words.len() >= take {
            let candidate = words[..take].join(" ");
            if GREETINGS.contains(&candidate.as_str()) {
                return Some(candidate);
            }
        }
    }
    None
}

/// The sign-off a message closes with, normalized, or `None`.
fn closing_sign_off(body: &str) -> Option<String> {
    let last = body.lines().rev().find(|l| !l.trim().is_empty())?.trim();
    // A sign-off line is short; a long final sentence that happens to contain
    // "thanks" is not one.
    if crate::db::word_count(last) > 5 {
        return None;
    }
    let clause = last.split([',', '!', '.', '—']).next().unwrap_or(last).trim().to_lowercase();
    let words: Vec<&str> = clause.split_whitespace().collect();
    for take in [3usize, 2, 1] {
        if words.len() >= take {
            let candidate = words[..take].join(" ");
            if SIGN_OFFS.contains(&candidate.as_str()) {
                return Some(candidate);
            }
        }
    }
    None
}

/// Word 3-grams, lowercased, punctuation stripped, stopword-only grams
/// dropped. These are the phrases the prompt can quote back as "things this
/// person actually says".
fn phrases(body: &str) -> Vec<String> {
    const STOP: [&str; 24] = [
        "the", "a", "an", "and", "or", "but", "of", "to", "in", "on", "at", "for", "is", "it", "that", "this", "with",
        "as", "be", "are", "was", "i", "you", "we",
    ];
    let tokens: Vec<String> = body
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'').to_lowercase())
        .filter(|w| !w.is_empty())
        .collect();
    let mut out = Vec::new();
    for window in tokens.windows(3) {
        if window.iter().all(|w| STOP.contains(&w.as_str())) {
            continue;
        }
        out.push(window.join(" "));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn samples(bodies: &[&str]) -> Vec<Sample<'static>> {
        // Leak is fine in a test and keeps the signature honest elsewhere.
        bodies.iter().map(|b| Sample::new(Box::leak(b.to_string().into_boxed_str()) as &'static str)).collect()
    }

    fn repeat(body: &'static str, n: usize) -> Vec<Sample<'static>> {
        (0..n).map(|_| Sample::new(body)).collect()
    }

    #[test]
    fn a_small_sample_reports_its_size_and_refuses_to_guess() {
        let m = compute(&samples(&["hi", "hello"]));
        assert_eq!(m.sample_size, 2);
        assert!(!m.measurable);
        assert_eq!(m.avg_words_per_message, None, "two messages is not a style");
        assert_eq!(m.emoji_rate, None);
        assert!(m.top_phrases.is_empty());
    }

    #[test]
    fn length_statistics_describe_the_distribution_not_just_the_mean() {
        let mut bodies: Vec<&str> = vec!["ok"; 19];
        bodies.push("one two three four five six seven eight nine ten eleven twelve");
        let m = compute(&samples(&bodies));
        assert!(m.measurable);
        assert_eq!(m.median_words_per_message, Some(1.0));
        assert_eq!(m.p90_words_per_message, Some(1.0));
        assert_eq!(m.avg_words_per_message, Some(1.55), "one long message moves the mean, not the median");
    }

    #[test]
    fn punctuation_habits_are_counted_from_the_last_real_character() {
        let m = compute(&[repeat("sounds good", 10), repeat("Sounds good.", 10)].concat());
        assert_eq!(m.terminal_period_rate, Some(0.5));
        let q = compute(&repeat("does tuesday work?", 20));
        assert_eq!(q.question_rate, Some(1.0));
        assert_eq!(q.terminal_period_rate, Some(0.0), "zero means never, and is not the same as unknown");
        // A trailing emoji does not hide the full stop before it.
        let e = compute(&repeat("on my way. \u{1F680}", 20));
        assert_eq!(e.terminal_period_rate, Some(1.0));
        assert_eq!(e.emoji_rate, Some(1.0));
    }

    #[test]
    fn capitalization_is_measured_two_ways() {
        let m = compute(&[repeat("yeah that works", 10), repeat("Yes, that works for me", 10)].concat());
        assert_eq!(m.lowercase_start_rate, Some(0.5));
        assert_eq!(m.all_lowercase_rate, Some(0.5));
        // Starting lowercase but shouting later is not "all lowercase".
        let shouty = compute(&repeat("yes but ONLY on Tuesday", 20));
        assert_eq!(shouty.lowercase_start_rate, Some(1.0));
        assert_eq!(shouty.all_lowercase_rate, Some(0.0));
    }

    #[test]
    fn contractions_are_per_hundred_words_and_ignore_possessives() {
        let m = compute(&repeat("I can't and I won't", 20));
        assert_eq!(m.contractions_per_100_words, Some(40.0), "2 contractions in 5 words");
        let p = compute(&repeat("the dogs' bowls are here", 20));
        assert_eq!(p.contractions_per_100_words, Some(0.0));
        let typographic = compute(&repeat("I can\u{2019}t today", 20));
        assert!(typographic.contractions_per_100_words.unwrap() > 0.0, "a smart apostrophe still counts");
    }

    #[test]
    fn greetings_and_sign_offs_are_recognized_where_they_belong() {
        let m = compute(&repeat("Hi Ada,\n\nCan you send the numbers?\n\nThanks, C", 20));
        assert_eq!(m.greeting_rate, Some(1.0));
        assert_eq!(m.sign_off_rate, Some(1.0));
        assert_eq!(m.top_greetings, vec![("hi".to_string(), 20)]);
        assert_eq!(m.top_sign_offs, vec![("thanks".to_string(), 20)]);
    }

    #[test]
    fn a_greeting_word_in_the_middle_is_not_a_greeting() {
        let m = compute(&repeat("I said hi to her yesterday and she said thanks", 20));
        assert_eq!(m.greeting_rate, Some(0.0));
        assert_eq!(m.sign_off_rate, Some(0.0), "a long closing sentence is not a sign-off");
    }

    #[test]
    fn phrases_need_to_recur_before_they_count() {
        let mut bodies = vec!["let me know if that works"; 5];
        bodies.extend(vec!["completely different sentence here"; 2]);
        bodies.extend(vec!["another unrelated thing entirely"; 13]);
        let m = compute(&samples(&bodies));
        let found: Vec<&str> = m.top_phrases.iter().map(|(p, _)| p.as_str()).collect();
        assert!(found.contains(&"let me know"), "{found:?}");
        assert!(!found.contains(&"completely different sentence"), "seen twice is not a habit: {found:?}");
    }

    #[test]
    fn response_latency_is_the_median_of_what_was_actually_timed() {
        let mut s = repeat("ok", 20);
        s[0].response_latency_seconds = Some(10);
        s[1].response_latency_seconds = Some(100);
        s[2].response_latency_seconds = Some(1000);
        let m = compute(&s);
        assert_eq!(m.median_response_seconds, Some(100));
        assert_eq!(compute(&repeat("ok", 20)).median_response_seconds, None, "untimed means unknown");
    }

    #[test]
    fn quantiles_of_lengths_are_read_from_counts_as_from_the_sorted_list() {
        // 1..=20 words: the median is the 11th smallest (index round(9.5)),
        // the 90th percentile the 18th (index round(17.1)).
        let bodies: Vec<String> = (1..=20).map(|n| vec!["w"; n].join(" ")).collect();
        let refs: Vec<&str> = bodies.iter().map(String::as_str).collect();
        let m = compute(&samples(&refs));
        assert_eq!(m.median_words_per_message, Some(11.0));
        assert_eq!(m.p90_words_per_message, Some(18.0));
        let mut acc = Accumulator::default();
        for b in refs.iter().rev() {
            acc.push(&Sample::new(b));
        }
        assert_eq!(acc.len(), 20);
        assert_eq!(acc.finish(), m, "the order messages arrive in changes nothing");
    }

    #[test]
    fn phrases_past_the_room_forget_the_rarest_first() {
        let mut counts: HashMap<String, usize> = HashMap::new();
        counts.insert("let me know".into(), 5);
        counts.insert("sounds good to".into(), 2);
        for i in 0..10 {
            counts.insert(format!("once only {i}"), 1);
        }
        thin(&mut counts, 20);
        assert_eq!(counts.len(), 12, "within the room nothing is forgotten");
        thin(&mut counts, 8);
        assert_eq!(counts.len(), 2, "said once goes first: {counts:?}");
        counts.insert("once more".into(), 1);
        thin(&mut counts, 2);
        assert_eq!(counts.keys().collect::<Vec<_>>(), vec!["let me know"], "then twice; a habit outlasts the rest");
    }

    #[test]
    fn metrics_are_deterministic() {
        let s = samples(&[
            "Hi Ada,\n\nlet me know if that works.\n\nThanks, C",
            "yeah sounds good 🙂",
            "let me know if that works for you",
        ]);
        let many: Vec<Sample<'_>> = (0..7).flat_map(|_| s.clone()).collect();
        assert_eq!(compute(&many), compute(&many));
    }
}
