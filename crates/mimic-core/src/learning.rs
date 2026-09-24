//! The learning loop's second half: turning what the user changed in earlier
//! drafts into something the next draft does differently.
//!
//! The evidence is every draft the user sent — edited or not — compared with
//! what Mimic wrote. An edit that took a greeting out is evidence for leaving
//! greetings out; a draft sent unedited *with* a greeting is evidence against.
//! Both count, because a loop that only listens to edits learns that every
//! habit is wrong.
//!
//! The rule is the one the previous product's corrections system earned: a
//! pattern holds only when at least `MIN_AGREEING` observations agree on a
//! direction **and** they carry the majority of the weight, counting the
//! evidence against. One deleted greeting is an anecdote. Three deletions
//! against ten greetings left in place is still an anecdote.
//!
//! Patterns are computed for everyone (the global scope) and per person. A
//! person's pattern replaces the global one on the same habit, so learning
//! that Ada gets no greeting does not stop Mimic greeting anyone else.
//!
//! Nothing here is stored. Every call re-reads the drafts, which keeps a
//! deleted person's drafts from leaving a pattern behind and makes the result
//! a pure function of what is in the database.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::db::{Db, DbError};
use crate::generation::feedback::{self, diff_draft};

/// Agreeing observations a pattern needs before it changes anything.
pub const MIN_AGREEING: usize = 3;

/// The weight a draft sent at the same length contributes against a length
/// pattern: the size of the dead band `diff_draft` already treats as "not a
/// length preference".
const SAME_LENGTH_WEIGHT: f64 = 0.1;

/// A habit the loop can learn about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Habit {
    Greeting,
    SignOff,
    Emoji,
    TerminalPeriod,
    Length,
}

pub const HABITS: [Habit; 5] = [Habit::Greeting, Habit::SignOff, Habit::Emoji, Habit::TerminalPeriod, Habit::Length];

/// Which way the user pushed it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Direction {
    /// Took it out, or made it shorter.
    Less,
    /// Put it in, or made it longer.
    More,
}

/// One habit, one direction, one scope: what the drafts say, and whether it
/// is enough to act on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LearnedPattern {
    pub habit: Habit,
    pub direction: Direction,
    /// `None` for everyone; a participant id for one person.
    pub participant_id: Option<String>,
    pub participant_name: Option<String>,
    /// Drafts that moved this habit this way.
    pub agreeing: usize,
    /// Drafts that said something about this habit at all.
    pub observations: usize,
    /// The agreeing drafts' share of the weight, 0..1.
    pub share: f64,
    /// Whether this pattern changes the next draft.
    pub holds: bool,
    /// For length: the mean change the agreeing drafts made, as a fraction of
    /// the draft (0.4 = cut by 40%). `None` for the other habits.
    pub mean_change: Option<f64>,
    /// One sentence for the person, in Mimic's voice.
    pub summary: String,
}

/// Everything the loop knows, for the "How you write" drawer.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LearningOverview {
    /// Sent drafts the patterns were computed from.
    pub drafts_considered: usize,
    /// Patterns that hold, then patterns still forming, most evidence first.
    pub patterns: Vec<LearnedPattern>,
    pub min_agreeing: usize,
    /// What the user has told Mimic directly. These outrank everything.
    pub notes: Vec<StatedNote>,
}

/// Something the user told Mimic, in their words, and who it is about.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatedNote {
    /// The `voice_preferences` row, so it can be taken back.
    pub id: String,
    pub text: String,
    /// `None` when it is about everyone.
    pub participant_id: Option<String>,
    pub participant_name: Option<String>,
    pub updated_at: String,
}

/// Key prefix for a note the user typed, as opposed to a structured
/// preference such as `signOff`.
pub const NOTE_PREFIX: &str = "said:";

/// Longest note Mimic keeps. A note is an instruction, not a letter.
pub const NOTE_MAX_CHARS: usize = 500;

/// Remember something the user told Mimic, about one person or everyone.
/// It lands as a manual preference, so it outranks every measurement and
/// every learned pattern, and it can be taken back like any other.
pub fn remember_note(db: &Db, participant_id: Option<&str>, note: &str) -> Result<crate::db::VoicePreference, DbError> {
    let text = note.trim();
    if text.is_empty() {
        return Err(DbError::Invalid("There's nothing to remember in an empty note.".into()));
    }
    if text.chars().count() > NOTE_MAX_CHARS {
        return Err(DbError::Invalid(format!(
            "Keep it under {NOTE_MAX_CHARS} characters — one instruction at a time."
        )));
    }
    let (layer, scope) = match participant_id {
        Some(pid) => {
            if db.get_participant(pid)?.is_none() {
                return Err(DbError::NotFound(pid.to_string()));
            }
            (crate::db::VoiceLayer::Relationship, pid.to_string())
        }
        None => (crate::db::VoiceLayer::Global, String::new()),
    };
    let key = format!("{NOTE_PREFIX}{}", &crate::ids::new_id()[..8]);
    db.set_voice_preference(layer, &scope, &key, &serde_json::Value::String(text.to_string()), Some(text))
}

/// A preference as a line a person can read: a note is its own text; a
/// structured preference is "key: value".
pub fn preference_text(key: &str, value: &serde_json::Value) -> String {
    let rendered = match value {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    if key.starts_with(NOTE_PREFIX) {
        rendered
    } else {
        format!("{key}: {rendered}")
    }
}

/// The patterns and the notes, for the screen.
pub fn overview(db: &Db) -> Result<LearningOverview, DbError> {
    let mut o = patterns(db)?;
    let mut notes = Vec::new();
    for p in db.all_voice_preferences()? {
        let participant_id = (p.layer == "relationship").then(|| p.scope_key.clone());
        let participant_name = match &participant_id {
            Some(pid) => db.get_participant(pid)?.map(|x| x.display_name),
            None => None,
        };
        notes.push(StatedNote {
            id: p.id,
            text: preference_text(&p.key, &p.value),
            participant_id,
            participant_name,
            updated_at: p.updated_at,
        });
    }
    o.notes = notes;
    Ok(o)
}

/// What one sent draft says about each habit.
#[derive(Debug, Clone, Copy, Default)]
struct Signal {
    /// -1 took it out / shorter, +1 put it in / longer, 0 left alone.
    moved: i8,
    /// Whether the habit was present in the draft (binary habits only).
    present_before: bool,
    /// Weight of this observation.
    weight: f64,
    /// Length only: signed change as a fraction of the draft.
    change: f64,
}

fn signals(generated: &str, sent: &str) -> [(Habit, Signal); 5] {
    let d = diff_draft(generated, sent);
    let w = feedback::EDIT_WEIGHT;
    let binary = |removed: bool, added: bool, before: bool| Signal {
        moved: if removed {
            -1
        } else if added {
            1
        } else {
            0
        },
        present_before: before,
        weight: w,
        change: 0.0,
    };
    let length = {
        let change = if d.draft_words > 0 { d.word_delta as f64 / d.draft_words as f64 } else { 0.0 };
        match d.length_direction.as_str() {
            "shorter" => Signal { moved: -1, present_before: false, weight: change.abs().min(1.0) * w, change },
            "longer" => Signal { moved: 1, present_before: false, weight: change.abs().min(1.0) * w, change },
            _ => Signal { moved: 0, present_before: false, weight: SAME_LENGTH_WEIGHT * w, change },
        }
    };
    [
        (Habit::Greeting, binary(d.greeting_removed, d.greeting_added, feedback::has_greeting(generated))),
        (Habit::SignOff, binary(d.sign_off_removed, d.sign_off_added, feedback::has_sign_off(generated))),
        (Habit::Emoji, binary(d.emoji_removed, d.emoji_added, feedback::has_emoji(generated))),
        (
            Habit::TerminalPeriod,
            binary(d.terminal_period_removed, d.terminal_period_added, feedback::ends_with_period(generated)),
        ),
        (Habit::Length, length),
    ]
}

#[derive(Default)]
struct Tally {
    less: (usize, f64, f64),
    more: (usize, f64, f64),
    /// Evidence against removing: the habit was there and stayed.
    kept_present: f64,
    /// Evidence against adding: the habit was absent and stayed absent.
    kept_absent: f64,
    kept_present_n: usize,
    kept_absent_n: usize,
    /// Length: drafts sent at the same length.
    same: f64,
    same_n: usize,
}

fn add(t: &mut Tally, habit: Habit, s: Signal) {
    match s.moved {
        -1 => {
            t.less.0 += 1;
            t.less.1 += s.weight;
            t.less.2 += s.change;
        }
        1 => {
            t.more.0 += 1;
            t.more.1 += s.weight;
            t.more.2 += s.change;
        }
        _ if habit == Habit::Length => {
            t.same += s.weight;
            t.same_n += 1;
        }
        _ if s.present_before => {
            t.kept_present += s.weight;
            t.kept_present_n += 1;
        }
        _ => {
            t.kept_absent += s.weight;
            t.kept_absent_n += 1;
        }
    }
}

fn judge(habit: Habit, t: &Tally, participant_id: Option<&str>, participant_name: Option<&str>) -> Vec<LearnedPattern> {
    let mut out = Vec::new();
    for dir in [Direction::Less, Direction::More] {
        let (n, w, change_sum) = if dir == Direction::Less { t.less } else { t.more };
        if n == 0 {
            continue;
        }
        let (against, against_n) = match (habit, dir) {
            (Habit::Length, Direction::Less) => (t.more.1 + t.same, t.more.0 + t.same_n),
            (Habit::Length, Direction::More) => (t.less.1 + t.same, t.less.0 + t.same_n),
            (_, Direction::Less) => (t.more.1 + t.kept_present, t.more.0 + t.kept_present_n),
            (_, Direction::More) => (t.less.1 + t.kept_absent, t.less.0 + t.kept_absent_n),
        };
        let share = if w + against > 0.0 { w / (w + against) } else { 0.0 };
        let holds = n >= MIN_AGREEING && share > 0.5;
        let mean_change = (habit == Habit::Length).then(|| (change_sum / n as f64 * 100.0).round() / 100.0);
        out.push(LearnedPattern {
            habit,
            direction: dir,
            participant_id: participant_id.map(str::to_string),
            participant_name: participant_name.map(str::to_string),
            agreeing: n,
            observations: n + against_n,
            share: (share * 100.0).round() / 100.0,
            holds,
            mean_change,
            summary: summarize(habit, dir, n, n + against_n, holds, participant_name, mean_change),
        });
    }
    out
}

fn summarize(
    habit: Habit,
    dir: Direction,
    n: usize,
    of: usize,
    holds: bool,
    who: Option<&str>,
    mean_change: Option<f64>,
) -> String {
    let times = |k: usize| if k == 1 { "once".to_string() } else { format!("{k} times") };
    let to = who.map(|w| format!(" to {w}")).unwrap_or_default();
    let did = match (habit, dir) {
        (Habit::Greeting, Direction::Less) => format!("You took the greeting out of my drafts{to} {}", times(n)),
        (Habit::Greeting, Direction::More) => format!("You added a greeting to my drafts{to} {}", times(n)),
        (Habit::SignOff, Direction::Less) => format!("You took the sign-off out of my drafts{to} {}", times(n)),
        (Habit::SignOff, Direction::More) => format!("You added a sign-off to my drafts{to} {}", times(n)),
        (Habit::Emoji, Direction::Less) => format!("You took emoji out of my drafts{to} {}", times(n)),
        (Habit::Emoji, Direction::More) => format!("You added emoji to my drafts{to} {}", times(n)),
        (Habit::TerminalPeriod, Direction::Less) => {
            format!("You took the full stop off the end of my drafts{to} {}", times(n))
        }
        (Habit::TerminalPeriod, Direction::More) => {
            format!("You put a full stop at the end of my drafts{to} {}", times(n))
        }
        (Habit::Length, Direction::Less) => format!(
            "You cut my drafts{to} {}, by about {}% each time",
            times(n),
            (mean_change.unwrap_or(0.0).abs() * 100.0).round()
        ),
        (Habit::Length, Direction::More) => format!(
            "You lengthened my drafts{to} {}, by about {}% each time",
            times(n),
            (mean_change.unwrap_or(0.0).abs() * 100.0).round()
        ),
    };
    let of = if of > n { format!(" (out of {of} that said anything about it)") } else { String::new() };
    if holds {
        let now = match (habit, dir) {
            (Habit::Greeting, Direction::Less) => "so I've stopped adding one",
            (Habit::Greeting, Direction::More) => "so I open with one now",
            (Habit::SignOff, Direction::Less) => "so I've stopped signing off",
            (Habit::SignOff, Direction::More) => "so I sign off now",
            (Habit::Emoji, Direction::Less) => "so I've stopped using them",
            (Habit::Emoji, Direction::More) => "so I use them now",
            (Habit::TerminalPeriod, Direction::Less) => "so I leave it off",
            (Habit::TerminalPeriod, Direction::More) => "so I put one there",
            (Habit::Length, Direction::Less) => "so I write shorter",
            (Habit::Length, Direction::More) => "so I write a little more",
        };
        format!("{did}{of}, {now}.")
    } else if n < MIN_AGREEING {
        let more = MIN_AGREEING - n;
        format!(
            "{did}{of}. {} more and I'll take the hint.",
            if more == 1 { "Once".to_string() } else { format!("{more} times") }
        )
    } else {
        format!("{did}{of}, but you left it alone more often than not, so I haven't changed anything.")
    }
}

/// Every pattern the sent drafts support, for everyone and per person.
pub fn patterns(db: &Db) -> Result<LearningOverview, DbError> {
    let drafts = db.sent_drafts_for_learning()?;
    let mut global: BTreeMap<Habit, Tally> = BTreeMap::new();
    let mut per_person: BTreeMap<String, BTreeMap<Habit, Tally>> = BTreeMap::new();
    for (participant_id, generated, sent) in &drafts {
        for (habit, s) in signals(generated, sent) {
            add(global.entry(habit).or_default(), habit, s);
            if let Some(pid) = participant_id {
                add(per_person.entry(pid.clone()).or_default().entry(habit).or_default(), habit, s);
            }
        }
    }
    let mut out = Vec::new();
    for habit in HABITS {
        if let Some(t) = global.get(&habit) {
            out.extend(judge(habit, t, None, None));
        }
    }
    for (pid, tallies) in &per_person {
        let name = db.get_participant(pid)?.map(|p| p.display_name);
        for habit in HABITS {
            if let Some(t) = tallies.get(&habit) {
                out.extend(judge(habit, t, Some(pid), name.as_deref()));
            }
        }
    }
    out.sort_by(|a, b| {
        b.holds
            .cmp(&a.holds)
            .then(b.agreeing.cmp(&a.agreeing))
            .then(a.participant_name.cmp(&b.participant_name))
            .then(a.habit.cmp(&b.habit))
            .then(a.direction.cmp(&b.direction))
    });
    Ok(LearningOverview {
        drafts_considered: drafts.len(),
        patterns: out,
        min_agreeing: MIN_AGREEING,
        notes: Vec::new(),
    })
}

/// The patterns that change a draft to this person: a person's own pattern
/// on a habit wins over the global one on the same habit, whichever way
/// either points.
pub fn applicable(db: &Db, participant_id: Option<&str>) -> Result<Vec<LearnedPattern>, DbError> {
    let all = patterns(db)?.patterns;
    let mine: Vec<LearnedPattern> = all
        .iter()
        .filter(|p| p.holds && participant_id.is_some() && p.participant_id.as_deref() == participant_id)
        .cloned()
        .collect();
    let mut out: Vec<LearnedPattern> = all
        .into_iter()
        .filter(|p| p.holds && p.participant_id.is_none() && !mine.iter().any(|m| m.habit == p.habit))
        .collect();
    out.extend(mine);
    out.sort_by_key(|p| p.habit);
    Ok(out)
}

/// A pattern in two or three words, for the evidence line under a draft.
pub fn short_label(p: &LearnedPattern) -> &'static str {
    match (p.habit, p.direction) {
        (Habit::Greeting, Direction::Less) => "no greeting",
        (Habit::Greeting, Direction::More) => "a greeting",
        (Habit::SignOff, Direction::Less) => "no sign-off",
        (Habit::SignOff, Direction::More) => "a sign-off",
        (Habit::Emoji, Direction::Less) => "no emoji",
        (Habit::Emoji, Direction::More) => "emoji",
        (Habit::TerminalPeriod, Direction::Less) => "no full stop at the end",
        (Habit::TerminalPeriod, Direction::More) => "a full stop at the end",
        (Habit::Length, Direction::Less) => "shorter",
        (Habit::Length, Direction::More) => "longer",
    }
}

/// The instruction a pattern adds to the prompt.
pub fn instruction(p: &LearnedPattern) -> String {
    match (p.habit, p.direction) {
        (Habit::Greeting, Direction::Less) => "Do not open with a greeting.".into(),
        (Habit::Greeting, Direction::More) => "Open with a short greeting.".into(),
        (Habit::SignOff, Direction::Less) => "Do not sign off.".into(),
        (Habit::SignOff, Direction::More) => "End with a short sign-off.".into(),
        (Habit::Emoji, Direction::Less) => "Do not use emoji.".into(),
        (Habit::Emoji, Direction::More) => "An emoji is welcome where it fits.".into(),
        (Habit::TerminalPeriod, Direction::Less) => "Do not end the message with a full stop.".into(),
        (Habit::TerminalPeriod, Direction::More) => "End the message with a full stop.".into(),
        (Habit::Length, Direction::Less) => format!(
            "Write about {}% shorter than the measurements alone would suggest.",
            (p.mean_change.unwrap_or(-0.3).abs() * 100.0).round()
        ),
        (Habit::Length, Direction::More) => format!(
            "Write about {}% longer than the measurements alone would suggest.",
            (p.mean_change.unwrap_or(0.3).abs() * 100.0).round().min(100.0)
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{IdentifierInput, IdentifierKind, NewDraft};

    fn db_with_people() -> (Db, String, String) {
        let db = Db::open_in_memory().unwrap();
        let ada =
            db.resolve_participant("Ada", &[IdentifierInput::new(IdentifierKind::Handle, "@ada")], false).unwrap();
        let bob =
            db.resolve_participant("Bob", &[IdentifierInput::new(IdentifierKind::Handle, "@bob")], false).unwrap();
        (db, ada, bob)
    }

    fn sent(db: &Db, to: Option<&str>, generated: &str, final_text: &str) {
        let d = db
            .create_draft(&NewDraft {
                participant_id: to.map(str::to_string),
                conversation_id: None,
                channel: "chat".into(),
                situation_id: None,
                incoming_message: None,
                incoming_message_id: None,
                intent: None,
                generated_text: generated.into(),
                provider: "mock".into(),
                model: "m".into(),
                context: serde_json::json!({}),
                prompt_hash: "h".into(),
                evidence: serde_json::json!({}),
                alternative_to: None,
            })
            .unwrap();
        let outcome = if generated == final_text { "sent_unedited" } else { "sent_edited" };
        db.resolve_draft(&d.id, outcome, Some(final_text)).unwrap();
    }

    const GREETED: &str = "Hi Ada,\n\nTuesday works for me.";
    // One word shorter than GREETED: inside diff_draft's dead band, so these
    // tests are about the greeting and not about length.
    const BARE: &str = "Tuesday works well for me.";

    fn find(o: &LearningOverview, habit: Habit, dir: Direction, who: Option<&str>) -> Option<LearnedPattern> {
        o.patterns
            .iter()
            .find(|p| p.habit == habit && p.direction == dir && p.participant_id.as_deref() == who)
            .cloned()
    }

    #[test]
    fn two_edits_are_an_anecdote_and_three_are_a_pattern() {
        let (db, ada, _) = db_with_people();
        sent(&db, Some(&ada), GREETED, BARE);
        sent(&db, Some(&ada), GREETED, BARE);
        let two = find(&patterns(&db).unwrap(), Habit::Greeting, Direction::Less, Some(&ada)).unwrap();
        assert!(!two.holds);
        assert!(two.summary.contains("Once more"), "{}", two.summary);
        assert!(applicable(&db, Some(&ada)).unwrap().is_empty());

        sent(&db, Some(&ada), GREETED, BARE);
        let three = find(&patterns(&db).unwrap(), Habit::Greeting, Direction::Less, Some(&ada)).unwrap();
        assert!(three.holds);
        assert_eq!(three.agreeing, 3);
        assert!(three.summary.ends_with("so I've stopped adding one."), "{}", three.summary);
        let apply = applicable(&db, Some(&ada)).unwrap();
        assert_eq!(apply.len(), 1);
        assert_eq!(instruction(&apply[0]), "Do not open with a greeting.");
    }

    #[test]
    fn drafts_sent_unedited_count_against_a_pattern() {
        let (db, ada, _) = db_with_people();
        for _ in 0..3 {
            sent(&db, Some(&ada), GREETED, BARE);
        }
        for _ in 0..4 {
            sent(&db, Some(&ada), GREETED, GREETED);
        }
        let p = find(&patterns(&db).unwrap(), Habit::Greeting, Direction::Less, Some(&ada)).unwrap();
        assert_eq!(p.agreeing, 3);
        assert_eq!(p.observations, 7);
        assert!(!p.holds, "three removals against four greetings kept is not a majority");
        assert!(p.summary.contains("left it alone more often than not"), "{}", p.summary);
        assert!(applicable(&db, Some(&ada)).unwrap().is_empty());
    }

    #[test]
    fn a_persons_pattern_stays_with_that_person() {
        let (db, ada, bob) = db_with_people();
        for _ in 0..3 {
            sent(&db, Some(&ada), GREETED, BARE);
        }
        for _ in 0..3 {
            sent(&db, Some(&bob), GREETED, GREETED);
        }
        // Globally: three removals, three kept — not a majority.
        assert!(applicable(&db, None).unwrap().is_empty());
        assert!(applicable(&db, Some(&bob)).unwrap().is_empty(), "Bob keeps his greeting");
        assert_eq!(applicable(&db, Some(&ada)).unwrap().len(), 1, "Ada loses hers");
    }

    #[test]
    fn a_persons_pattern_replaces_the_global_one_on_the_same_habit() {
        let (db, ada, bob) = db_with_people();
        for _ in 0..4 {
            sent(&db, Some(&bob), GREETED, BARE);
        }
        for _ in 0..3 {
            sent(&db, Some(&ada), BARE, GREETED);
        }
        let for_ada = applicable(&db, Some(&ada)).unwrap();
        let greeting: Vec<_> = for_ada.iter().filter(|p| p.habit == Habit::Greeting).collect();
        assert_eq!(greeting.len(), 1);
        assert_eq!(greeting[0].direction, Direction::More, "Ada gets greeted even though others don't");
        assert_eq!(greeting[0].participant_id.as_deref(), Some(ada.as_str()));
    }

    #[test]
    fn length_is_learned_with_how_much() {
        let (db, _, _) = db_with_people();
        let long = "Thanks for sending this over. I have had a look and I think Tuesday afternoon would work well for the call, if that suits you.";
        for _ in 0..3 {
            sent(&db, None, long, "Tuesday afternoon works for the call.");
        }
        let p = find(&patterns(&db).unwrap(), Habit::Length, Direction::Less, None).unwrap();
        assert!(p.holds);
        let change = p.mean_change.unwrap();
        assert!(change < -0.5 && change > -1.0, "{change}");
        assert!(instruction(&p).starts_with("Write about "), "{}", instruction(&p));
        assert!(p.summary.contains("by about"), "{}", p.summary);
    }

    #[test]
    fn discarded_and_unresolved_drafts_are_not_evidence() {
        let (db, ada, _) = db_with_people();
        for _ in 0..5 {
            let d = db
                .create_draft(&NewDraft {
                    participant_id: Some(ada.clone()),
                    conversation_id: None,
                    channel: "chat".into(),
                    situation_id: None,
                    incoming_message: None,
                    incoming_message_id: None,
                    intent: None,
                    generated_text: GREETED.into(),
                    provider: "mock".into(),
                    model: "m".into(),
                    context: serde_json::json!({}),
                    prompt_hash: "h".into(),
                    evidence: serde_json::json!({}),
                    alternative_to: None,
                })
                .unwrap();
            db.resolve_draft(&d.id, "discarded", None).unwrap();
        }
        let o = patterns(&db).unwrap();
        assert_eq!(o.drafts_considered, 0);
        assert!(o.patterns.is_empty());
    }

    /// A draft written with an adjustment was asked to differ from how Mimic
    /// writes; what the user changed in it is measured against that request,
    /// not against Mimic's habits, so it teaches nothing.
    #[test]
    fn a_draft_asked_for_another_way_is_not_evidence_of_a_habit() {
        let (db, ada, _) = db_with_people();
        for adjustment in ["longer", "moreProfessional", "shorter"] {
            let d = db
                .create_draft(&NewDraft {
                    participant_id: Some(ada.clone()),
                    conversation_id: None,
                    channel: "chat".into(),
                    situation_id: None,
                    incoming_message: None,
                    incoming_message_id: None,
                    intent: None,
                    generated_text: GREETED.into(),
                    provider: "mock".into(),
                    model: "m".into(),
                    context: serde_json::json!({ "adjustment": adjustment }),
                    prompt_hash: "h".into(),
                    evidence: serde_json::json!({}),
                    alternative_to: None,
                })
                .unwrap();
            db.resolve_draft(&d.id, "sent_edited", Some(BARE)).unwrap();
        }
        let o = patterns(&db).unwrap();
        assert_eq!(o.drafts_considered, 0, "three edits the same way, none of them evidence");
        assert!(o.patterns.is_empty());

        // The same edits to drafts written as Mimic writes are.
        for _ in 0..3 {
            sent(&db, Some(&ada), GREETED, BARE);
        }
        assert_eq!(patterns(&db).unwrap().drafts_considered, 3);
    }

    #[test]
    fn nothing_sent_means_nothing_learned_rather_than_a_default() {
        let db = Db::open_in_memory().unwrap();
        let o = patterns(&db).unwrap();
        assert_eq!(o.drafts_considered, 0);
        assert!(o.patterns.is_empty());
        assert_eq!(o.min_agreeing, MIN_AGREEING);
    }

    #[test]
    fn the_result_is_the_same_every_time() {
        let (db, ada, bob) = db_with_people();
        for _ in 0..3 {
            sent(&db, Some(&ada), GREETED, BARE);
            sent(&db, Some(&bob), "on my way 🙂", "on my way");
        }
        let a = serde_json::to_value(patterns(&db).unwrap()).unwrap();
        let b = serde_json::to_value(patterns(&db).unwrap()).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn a_note_is_remembered_for_the_right_person_and_can_be_taken_back() {
        let (db, ada, _) = db_with_people();
        let everyone = remember_note(&db, None, "  never use exclamation marks ").unwrap();
        assert_eq!(everyone.layer, "global");
        let hers = remember_note(&db, Some(&ada), "I call her Ada, never Mrs Lovelace").unwrap();
        assert_eq!(hers.layer, "relationship");
        assert_eq!(hers.scope_key, ada);

        let o = overview(&db).unwrap();
        assert_eq!(o.notes.len(), 2);
        let n = o.notes.iter().find(|n| n.participant_id.as_deref() == Some(ada.as_str())).unwrap();
        assert_eq!(n.text, "I call her Ada, never Mrs Lovelace");
        assert_eq!(n.participant_name.as_deref(), Some("Ada"));
        assert!(o.notes.iter().any(|n| n.text == "never use exclamation marks" && n.participant_id.is_none()));

        db.delete_voice_preference(&everyone.id).unwrap();
        assert_eq!(overview(&db).unwrap().notes.len(), 1);
    }

    #[test]
    fn an_empty_or_rambling_note_is_refused() {
        let (db, _, _) = db_with_people();
        assert!(remember_note(&db, None, "   ").is_err());
        assert!(remember_note(&db, None, &"x".repeat(NOTE_MAX_CHARS + 1)).is_err());
        assert!(remember_note(&db, Some("nobody"), "hi").is_err());
    }

    #[test]
    fn a_note_reads_as_itself_and_a_setting_reads_as_a_setting() {
        assert_eq!(preference_text("said:abcd1234", &serde_json::json!("keep it short")), "keep it short");
        assert_eq!(preference_text("signOff", &serde_json::json!("— C")), "signOff: — C");
    }
}
