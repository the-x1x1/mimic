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
//!
//! The other evidence is which way of saying a draft the user chose
//! (0.10.0-alpha.97): with other ways written beside a first draft, sending
//! the shorter one is a vote for shorter, and sending the first with a
//! shorter one beside it is a vote against. A way chosen `MIN_AGREEING` times
//! and more often than not (`ChosenWay`) is how the next first draft is
//! written. The user's own edits outrank it on length in the same scope, and
//! a person's choice outranks everyone's (`applying`).

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::db::{ChoiceMade, Db, DbError};
use crate::generation::feedback::{self, diff_draft};
use crate::generation::Adjustment;

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

/// A way of saying a draft the user chose over the first draft beside it
/// (0.10.0-alpha.97): one adjustment, one scope, and whether it is enough to
/// write the next first draft that way.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChosenWay {
    pub adjustment: Adjustment,
    /// `None` for everyone; a participant id for one person.
    pub participant_id: Option<String>,
    pub participant_name: Option<String>,
    /// Times this way was sent instead of the first draft.
    pub chosen: usize,
    /// Times the choice went against it: this way was beside the one sent
    /// (and the first draft didn't already carry it), or the opposite way
    /// was sent.
    pub against: usize,
    /// `chosen` as a share of `chosen + against`, 0..1.
    pub share: f64,
    /// Whether the next first draft is written this way.
    pub holds: bool,
    /// Shorter or longer: the mean change in length of what was sent against
    /// the first draft, as a fraction of it, over the choices whose length
    /// moved that way (-0.4 = 40% shorter). `None` when none did, and for
    /// the register.
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
    /// Choices between ways of saying a draft the ways were computed from.
    #[serde(default)]
    pub choices_considered: usize,
    /// Ways that hold, then ways still forming, most chosen first.
    #[serde(default)]
    pub ways: Vec<ChosenWay>,
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
    let choices = db.choices_for_learning()?;
    let ways = chosen_ways(db, &choices)?;
    Ok(LearningOverview {
        drafts_considered: drafts.len(),
        patterns: out,
        choices_considered: choices.len(),
        ways,
        min_agreeing: MIN_AGREEING,
        notes: Vec::new(),
    })
}

/// What changes a draft to this person: the patterns and the chosen ways
/// that hold for them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Applying {
    pub patterns: Vec<LearnedPattern>,
    pub ways: Vec<ChosenWay>,
}

/// The patterns that change a draft to this person: a person's own pattern
/// on a habit wins over the global one on the same habit, whichever way
/// either points.
pub fn applicable(db: &Db, participant_id: Option<&str>) -> Result<Vec<LearnedPattern>, DbError> {
    Ok(applying(db, participant_id)?.patterns)
}

/// Everything learned that changes a draft to this person. A person's own
/// pattern on a habit wins over the global one on that habit, and a
/// person's chosen way over everyone's on its axis (shorter or longer; more
/// casual or more professional), whichever way either points. On length a
/// pattern and a way can both hold: the person's wins over everyone's, and
/// within one scope the edits — the user's own hand — win over the choice.
pub fn applying(db: &Db, participant_id: Option<&str>) -> Result<Applying, DbError> {
    let o = patterns(db)?;
    let mine_only = |p: Option<&str>| participant_id.is_some() && p == participant_id;
    let mine: Vec<LearnedPattern> =
        o.patterns.iter().filter(|p| p.holds && mine_only(p.participant_id.as_deref())).cloned().collect();
    let mut patterns: Vec<LearnedPattern> = o
        .patterns
        .into_iter()
        .filter(|p| p.holds && p.participant_id.is_none() && !mine.iter().any(|m| m.habit == p.habit))
        .collect();
    patterns.extend(mine);
    let my_ways: Vec<ChosenWay> =
        o.ways.iter().filter(|w| w.holds && mine_only(w.participant_id.as_deref())).cloned().collect();
    let mut ways: Vec<ChosenWay> = o
        .ways
        .into_iter()
        .filter(|w| {
            w.holds && w.participant_id.is_none() && !my_ways.iter().any(|m| axis(m.adjustment) == axis(w.adjustment))
        })
        .collect();
    ways.extend(my_ways);
    let way_theirs = ways.iter().find(|w| axis(w.adjustment) == Axis::Length).map(|w| w.participant_id.is_some());
    let pattern_theirs = patterns.iter().find(|p| p.habit == Habit::Length).map(|p| p.participant_id.is_some());
    if let (Some(way_theirs), Some(pattern_theirs)) = (way_theirs, pattern_theirs) {
        if pattern_theirs || !way_theirs {
            ways.retain(|w| axis(w.adjustment) != Axis::Length);
        } else {
            patterns.retain(|p| p.habit != Habit::Length);
        }
    }
    patterns.sort_by_key(|p| p.habit);
    ways.sort_by_key(|w| w.adjustment);
    Ok(Applying { patterns, ways })
}

/// Which of two ways an adjustment moves a draft.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Axis {
    Length,
    Register,
}

fn axis(a: Adjustment) -> Axis {
    match a {
        Adjustment::Shorter | Adjustment::Longer => Axis::Length,
        Adjustment::MoreCasual | Adjustment::MoreProfessional => Axis::Register,
    }
}

/// The way that goes the other way on the same axis.
fn opposite(a: Adjustment) -> Adjustment {
    match a {
        Adjustment::Shorter => Adjustment::Longer,
        Adjustment::Longer => Adjustment::Shorter,
        Adjustment::MoreCasual => Adjustment::MoreProfessional,
        Adjustment::MoreProfessional => Adjustment::MoreCasual,
    }
}

fn adjustment(raw: &str) -> Option<Adjustment> {
    serde_json::from_value(serde_json::Value::String(raw.to_string())).ok()
}

/// The ways a first draft was already written with: those it learned
/// (`context.ways`), and the one it was asked for in Compose, if any.
fn carried(context: &serde_json::Value) -> BTreeSet<Adjustment> {
    let mut out: BTreeSet<Adjustment> = context
        .get("ways")
        .and_then(|w| w.as_array())
        .map(|ws| ws.iter().filter_map(|w| w.as_str().and_then(adjustment)).collect())
        .unwrap_or_default();
    out.extend(context.get("adjustment").and_then(|a| a.as_str()).and_then(adjustment));
    out
}

#[derive(Default)]
struct WayTally {
    chosen: usize,
    against: usize,
    change_sum: f64,
    change_n: usize,
}

/// What one choice says about each way. The way sent counts for itself and
/// against its opposite; a way beside it counts against itself, unless the
/// opposite was sent (already counted) or the first draft was written that
/// way already — passing over "shorter still" says the first was short
/// enough, not that shorter was wrong.
fn count_choice(t: &mut BTreeMap<Adjustment, WayTally>, c: &ChoiceMade) {
    let chosen = c.chosen.as_deref().and_then(adjustment);
    let already = carried(&c.first_context);
    if let Some(a) = chosen {
        let e = t.entry(a).or_default();
        e.chosen += 1;
        // How much shorter or longer is measured only against a first draft
        // written without that way (0.10.0-alpha.100): against one written
        // shorter already, "shorter still" says nothing of how much shorter
        // than the measurements the user wants.
        if axis(a) == Axis::Length && !already.contains(&a) {
            let d = diff_draft(&c.first_text, &c.sent_text);
            let moved = if a == Adjustment::Shorter { "shorter" } else { "longer" };
            if d.length_direction == moved && d.draft_words > 0 {
                e.change_sum += d.word_delta as f64 / d.draft_words as f64;
                e.change_n += 1;
            }
        }
        t.entry(opposite(a)).or_default().against += 1;
    }
    let passed: BTreeSet<Adjustment> = c.passed_over.iter().filter_map(|p| adjustment(p.as_str())).collect();
    for d in passed {
        if Some(d) == chosen || Some(opposite(d)) == chosen || already.contains(&d) {
            continue;
        }
        t.entry(d).or_default().against += 1;
    }
}

fn chosen_ways(db: &Db, choices: &[ChoiceMade]) -> Result<Vec<ChosenWay>, DbError> {
    let mut global: BTreeMap<Adjustment, WayTally> = BTreeMap::new();
    let mut per_person: BTreeMap<String, BTreeMap<Adjustment, WayTally>> = BTreeMap::new();
    for c in choices {
        count_choice(&mut global, c);
        if let Some(pid) = &c.participant_id {
            count_choice(per_person.entry(pid.clone()).or_default(), c);
        }
    }
    let mut out: Vec<ChosenWay> = global.iter().filter_map(|(a, t)| judge_way(*a, t, None, None)).collect();
    for (pid, tallies) in &per_person {
        let name = db.get_participant(pid)?.map(|p| p.display_name);
        out.extend(tallies.iter().filter_map(|(a, t)| judge_way(*a, t, Some(pid), name.as_deref())));
    }
    out.sort_by(|a, b| {
        b.holds
            .cmp(&a.holds)
            .then(b.chosen.cmp(&a.chosen))
            .then(a.participant_name.cmp(&b.participant_name))
            .then(a.adjustment.cmp(&b.adjustment))
    });
    Ok(out)
}

fn judge_way(
    a: Adjustment,
    t: &WayTally,
    participant_id: Option<&str>,
    participant_name: Option<&str>,
) -> Option<ChosenWay> {
    if t.chosen == 0 {
        return None;
    }
    let share = t.chosen as f64 / (t.chosen + t.against) as f64;
    let holds = t.chosen >= MIN_AGREEING && share > 0.5;
    let mean_change = (t.change_n > 0).then(|| (t.change_sum / t.change_n as f64 * 100.0).round() / 100.0);
    Some(ChosenWay {
        adjustment: a,
        participant_id: participant_id.map(str::to_string),
        participant_name: participant_name.map(str::to_string),
        chosen: t.chosen,
        against: t.against,
        share: (share * 100.0).round() / 100.0,
        holds,
        mean_change,
        summary: summarize_way(a, t.chosen, t.chosen + t.against, holds, participant_name),
    })
}

fn summarize_way(a: Adjustment, n: usize, of: usize, holds: bool, who: Option<&str>) -> String {
    let times = |k: usize| if k == 1 { "once".to_string() } else { format!("{k} times") };
    let to = who.map(|w| format!(" to {w}")).unwrap_or_default();
    let drafts = if n == 1 { "my first draft" } else { "my first drafts" };
    let did = format!("You chose the {} way over {drafts}{to} {}", a.label(), times(n));
    let of = if of > n { format!(" (out of {of} choices that said anything about it)") } else { String::new() };
    if holds {
        let now = match a {
            Adjustment::Shorter => "so I write shorter from the start",
            Adjustment::Longer => "so I give it more room from the start",
            Adjustment::MoreCasual => "so I start more casually",
            Adjustment::MoreProfessional => "so I start more formally",
        };
        format!("{did}{of}, {now}.")
    } else if n < MIN_AGREEING {
        let more = MIN_AGREEING - n;
        format!(
            "{did}{of}. {} more and I'll take the hint.",
            if more == 1 { "Once".to_string() } else { format!("{more} times") }
        )
    } else {
        format!("{did}{of}, but you went another way more often than not, so I haven't changed anything.")
    }
}

/// The instruction a chosen way adds to the prompt.
pub fn way_instruction(w: &ChosenWay) -> String {
    let pct = |c: f64| (c.abs() * 100.0).round().min(100.0);
    match (w.adjustment, w.mean_change) {
        (Adjustment::Shorter, Some(c)) => {
            format!("Write about {}% shorter than the measurements alone would suggest.", pct(c))
        }
        (Adjustment::Shorter, None) => "Write shorter than the measurements alone would suggest.".into(),
        (Adjustment::Longer, Some(c)) => {
            format!("Write about {}% longer than the measurements alone would suggest.", pct(c))
        }
        (Adjustment::Longer, None) => "Give it a little more room than the measurements alone would suggest.".into(),
        (Adjustment::MoreCasual, _) => {
            "Pitch it a little more casually than the measurements alone would suggest, while still sounding like them."
                .into()
        }
        (Adjustment::MoreProfessional, _) => {
            "Pitch it a little more formally than the measurements alone would suggest, while still sounding like them."
                .into()
        }
    }
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
        assert_eq!(o.choices_considered, 0, "nothing was beside them to choose between");
        assert!(o.ways.is_empty());

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

    // Ways of saying one reply, for the choices between them
    // (0.10.0-alpha.97): 12 words, 4 and 25.
    const MIDDLE: &str = "Tuesday works for me, and I can bring the notes along too.";
    const SHORT: &str = "Tuesday works for me.";
    const LONG: &str = "Tuesday works for me, and I can bring the notes along too, plus the figures from last week so we can go through them together.";

    fn written(
        db: &Db,
        to: Option<&str>,
        text: &str,
        context: serde_json::Value,
        alternative_to: Option<&str>,
    ) -> String {
        db.create_draft(&NewDraft {
            participant_id: to.map(str::to_string),
            conversation_id: None,
            channel: "chat".into(),
            situation_id: None,
            incoming_message: None,
            incoming_message_id: None,
            intent: None,
            generated_text: text.into(),
            provider: "mock".into(),
            model: "m".into(),
            context,
            prompt_hash: "h".into(),
            evidence: serde_json::json!({}),
            alternative_to: alternative_to.map(str::to_string),
        })
        .unwrap()
        .id
    }

    fn text_of(way: &str) -> &'static str {
        match way {
            "shorter" => SHORT,
            "longer" => LONG,
            _ => MIDDLE,
        }
    }

    /// A first draft to `to`, written already with the ways in `carried`,
    /// with a way of saying it beside it for each of `offered`; those in
    /// `aside` put aside first ("Not this one"), then the one `send` names
    /// sent as written — the first draft when `None`.
    fn choice(db: &Db, to: Option<&str>, carried: &[&str], offered: &[&str], aside: &[&str], send: Option<&str>) {
        let first = written(db, to, MIDDLE, serde_json::json!({ "ways": carried }), None);
        let mut ids = BTreeMap::new();
        for way in offered {
            ids.insert(*way, written(db, to, text_of(way), serde_json::json!({ "adjustment": way }), Some(&first)));
        }
        for way in aside {
            db.resolve_draft(&ids[way], "discarded", None).unwrap();
        }
        let (id, text) = match send {
            Some(w) => (ids[w].clone(), text_of(w)),
            None => (first, MIDDLE),
        };
        db.resolve_draft(&id, "sent_unedited", Some(text)).unwrap();
    }

    fn way(o: &LearningOverview, a: Adjustment, who: Option<&str>) -> Option<ChosenWay> {
        o.ways.iter().find(|w| w.adjustment == a && w.participant_id.as_deref() == who).cloned()
    }

    fn ways_for(db: &Db, who: Option<&str>) -> Vec<(Adjustment, Option<String>)> {
        applying(db, who).unwrap().ways.into_iter().map(|w| (w.adjustment, w.participant_id)).collect()
    }

    #[test]
    fn a_way_chosen_three_times_over_the_first_draft_is_how_the_next_one_is_written() {
        let (db, ada, _) = db_with_people();
        for _ in 0..2 {
            choice(&db, Some(&ada), &[], &["shorter"], &[], Some("shorter"));
        }
        let o = patterns(&db).unwrap();
        assert_eq!(o.choices_considered, 2);
        assert_eq!(o.drafts_considered, 0, "a way chosen isn't an edit");
        let s = way(&o, Adjustment::Shorter, None).unwrap();
        assert!(!s.holds);
        assert_eq!(
            s.summary,
            "You chose the shorter way over my first drafts 2 times. Once more and I'll take the hint."
        );
        assert!(ways_for(&db, None).is_empty(), "two choices are an anecdote");

        choice(&db, Some(&ada), &[], &["shorter", "moreCasual"], &[], Some("shorter"));
        let o = patterns(&db).unwrap();
        let s = way(&o, Adjustment::Shorter, None).unwrap();
        assert!(s.holds);
        assert_eq!((s.chosen, s.against, s.share), (3, 0, 1.0));
        assert_eq!(s.mean_change, Some(-0.67), "what was sent ran two thirds shorter than the first draft");
        assert_eq!(
            s.summary,
            "You chose the shorter way over my first drafts 3 times, so I write shorter from the start."
        );
        assert_eq!(way_instruction(&s), "Write about 67% shorter than the measurements alone would suggest.");
        let hers = way(&o, Adjustment::Shorter, Some(&ada)).unwrap();
        assert!(hers.summary.starts_with("You chose the shorter way over my first drafts to Ada 3 times"));
        assert!(way(&o, Adjustment::Longer, None).is_none(), "never chosen, so nothing to say of it");
        assert!(way(&o, Adjustment::MoreCasual, None).is_none(), "passed over once, never chosen");

        assert_eq!(ways_for(&db, None), [(Adjustment::Shorter, None)]);
        assert_eq!(ways_for(&db, Some(&ada)), [(Adjustment::Shorter, Some(ada.clone()))], "hers, not everyone's");
        assert!(applying(&db, Some(&ada)).unwrap().patterns.is_empty());
    }

    #[test]
    fn sending_the_first_draft_beside_a_way_or_the_opposite_way_counts_against_it() {
        let (db, _, _) = db_with_people();
        for _ in 0..3 {
            choice(&db, None, &[], &["shorter"], &[], Some("shorter"));
        }
        choice(&db, None, &[], &["shorter"], &[], None);
        choice(&db, None, &[], &["longer"], &[], Some("longer"));
        let s = way(&patterns(&db).unwrap(), Adjustment::Shorter, None).unwrap();
        assert_eq!((s.chosen, s.against), (3, 2));
        assert!(s.holds, "three of five");

        // Put aside on its own, then the first draft sent.
        choice(&db, None, &[], &["shorter"], &["shorter"], None);
        let o = patterns(&db).unwrap();
        let s = way(&o, Adjustment::Shorter, None).unwrap();
        assert_eq!((s.chosen, s.against, s.share), (3, 3, 0.5));
        assert!(!s.holds);
        assert_eq!(
            s.summary,
            "You chose the shorter way over my first drafts 3 times (out of 6 choices that said anything about it), but you went another way more often than not, so I haven't changed anything."
        );
        let l = way(&o, Adjustment::Longer, None).unwrap();
        assert_eq!((l.chosen, l.against), (1, 3), "each shorter one sent went against it");
        assert_eq!(l.mean_change, Some(1.08));
        assert!(ways_for(&db, None).is_empty());
    }

    #[test]
    fn passing_over_shorter_still_says_the_first_was_short_enough_not_that_shorter_was_wrong() {
        let (db, _, _) = db_with_people();
        for _ in 0..3 {
            choice(&db, None, &[], &["shorter"], &[], Some("shorter"));
        }
        for _ in 0..3 {
            choice(&db, None, &["shorter"], &["shorter"], &[], None);
        }
        let s = way(&patterns(&db).unwrap(), Adjustment::Shorter, None).unwrap();
        assert_eq!((s.chosen, s.against), (3, 0));
        assert!(s.holds);

        // Shorter still, chosen over a first draft written shorter: it counts,
        // but how much shorter stays measured from first drafts written
        // without it (0.10.0-alpha.100).
        let first = written(&db, None, SHORT, serde_json::json!({ "ways": ["shorter"] }), None);
        let still = written(&db, None, "Tuesday works.", serde_json::json!({ "adjustment": "shorter" }), Some(&first));
        db.resolve_draft(&still, "sent_unedited", Some("Tuesday works.")).unwrap();
        let s = way(&patterns(&db).unwrap(), Adjustment::Shorter, None).unwrap();
        assert_eq!((s.chosen, s.against, s.mean_change), (4, 0, Some(-0.67)), "not -0.63");

        // Longer sent instead of a first draft written shorter does count.
        choice(&db, None, &["shorter"], &["shorter", "longer"], &[], Some("longer"));
        let s = way(&patterns(&db).unwrap(), Adjustment::Shorter, None).unwrap();
        assert_eq!((s.chosen, s.against), (4, 1));
    }

    #[test]
    fn a_persons_choice_outranks_everyones_and_on_length_their_edits_outrank_their_choice() {
        let (db, ada, bob) = db_with_people();
        for _ in 0..4 {
            choice(&db, Some(&bob), &[], &["moreCasual"], &[], Some("moreCasual"));
        }
        for _ in 0..3 {
            choice(&db, Some(&ada), &[], &["moreProfessional"], &[], Some("moreProfessional"));
        }
        let o = patterns(&db).unwrap();
        assert!(way(&o, Adjustment::MoreCasual, None).unwrap().holds, "four of seven, for everyone");
        assert!(!way(&o, Adjustment::MoreProfessional, None).unwrap().holds);
        let hers = way(&o, Adjustment::MoreProfessional, Some(&ada)).unwrap();
        assert_eq!(
            hers.summary,
            "You chose the more professional way over my first drafts to Ada 3 times, so I start more formally."
        );
        assert_eq!(ways_for(&db, None), [(Adjustment::MoreCasual, None)]);
        assert_eq!(ways_for(&db, Some(&bob)), [(Adjustment::MoreCasual, Some(bob.clone()))]);
        assert_eq!(
            ways_for(&db, Some(&ada)),
            [(Adjustment::MoreProfessional, Some(ada.clone()))],
            "her own way on the register, not everyone's"
        );

        // Longer, chosen three times for Ada; and drafts to nobody cut down
        // by hand three times, which is everyone's length.
        for _ in 0..3 {
            choice(&db, Some(&ada), &[], &["longer"], &[], Some("longer"));
        }
        for _ in 0..3 {
            sent(&db, None, LONG, SHORT);
        }
        let everyone = applying(&db, None).unwrap();
        assert_eq!(
            everyone.patterns.iter().map(|p| (p.habit, p.direction)).collect::<Vec<_>>(),
            [(Habit::Length, Direction::Less)]
        );
        assert!(
            !everyone.ways.iter().any(|w| w.adjustment == Adjustment::Longer),
            "everyone's longer, chosen, gives way to everyone's edits: {:?}",
            everyone.ways
        );
        let to_ada = applying(&db, Some(&ada)).unwrap();
        assert!(to_ada.patterns.is_empty(), "her choice outranks everyone's edits: {:?}", to_ada.patterns);
        assert!(to_ada
            .ways
            .iter()
            .any(|w| w.adjustment == Adjustment::Longer && w.participant_id.as_deref() == Some(ada.as_str())));

        // Her own edits outrank her own choice.
        for _ in 0..3 {
            sent(&db, Some(&ada), LONG, SHORT);
        }
        let to_ada = applying(&db, Some(&ada)).unwrap();
        assert!(to_ada
            .patterns
            .iter()
            .any(|p| p.habit == Habit::Length && p.participant_id.as_deref() == Some(ada.as_str())));
        assert!(!to_ada.ways.iter().any(|w| w.adjustment == Adjustment::Longer), "{:?}", to_ada.ways);
        assert!(
            to_ada.ways.iter().any(|w| w.adjustment == Adjustment::MoreProfessional),
            "the register is another axis"
        );
    }

    #[test]
    fn a_set_put_aside_whole_or_sent_by_itself_or_with_nothing_beside_it_chose_nothing() {
        let (db, _, _) = db_with_people();
        let first = written(&db, None, MIDDLE, serde_json::json!({}), None);
        written(&db, None, SHORT, serde_json::json!({ "adjustment": "shorter" }), Some(&first));
        db.resolve_draft(&first, "discarded", None).unwrap();

        let first = written(&db, None, MIDDLE, serde_json::json!({}), None);
        let shorter = written(&db, None, SHORT, serde_json::json!({ "adjustment": "shorter" }), Some(&first));
        db.resolve_draft(&shorter, "sent_unedited", Some(SHORT)).unwrap();
        db.mark_sent_on_its_own(&shorter).unwrap();

        let lone = written(&db, None, SHORT, serde_json::json!({ "adjustment": "shorter" }), None);
        db.resolve_draft(&lone, "sent_unedited", Some(SHORT)).unwrap();

        let o = patterns(&db).unwrap();
        assert_eq!(o.choices_considered, 0);
        assert!(o.ways.is_empty());
    }
}
