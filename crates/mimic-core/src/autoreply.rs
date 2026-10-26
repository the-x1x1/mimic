//! Trusted mode: replies Mimic sends by itself (0.10.0-alpha.24).
//!
//! The owner decided on 2026-10-25 that Mimic should be able to answer on
//! its own. `docs/PRODUCT.md` is the account of what it will and will not
//! send; this module is that account, enforced here rather than in the UI:
//!
//! * **Off until the user turns it on** (`Rules.enabled`, stored under
//!   `autoreply`), and only with replies prepared in advance on
//!   (`assist.autoDraft`), since those are the drafts it sends. Turning it
//!   off stops every reply still held (`set_rules`).
//! * **Only through mailboxes the user named**, and only to the people they
//!   allowed: everyone they have written to themselves (`Who::Known`), or
//!   only the people they listed (`Who::Listed`).
//! * **Only what is waiting, exactly as the home screen reads it**: the
//!   thread's deciding message is the one the draft answers, it doesn't look
//!   automated, it hasn't gone quiet, nobody said it needs no reply, and the
//!   user hasn't answered or written since. Only a first draft prepared in
//!   advance — not one the user asked for, not one with other ways beside
//!   it — and only while it is undecided.
//! * **Once per message, and never in a loop**: a message that has had a
//!   reply held for it is never answered on its own again, whatever became of
//!   that reply; at most `PER_CONVERSATION_PER_DAY` replies on its own in one
//!   conversation a day, and at most `Rules.daily_limit` in all.
//! * **Held before it goes** (`Rules.hold_minutes`): an `outbox` row,
//!   `held`, shown under its message, where the user can stop it or send it
//!   now. Every rule is checked again when its time comes, and a reply that
//!   no longer passes is stopped with the reason (`stop_held_outgoing`).
//! * **It says what it is**: `Auto-Submitted: auto-replied`, so another
//!   program's automatic reply doesn't answer it.
//! * **It isn't the user's writing**: the draft is marked `sentOnItsOwn`
//!   (left out of the learning loop and the measured outcomes) and so is the
//!   message put in the conversation (left out of everything that measures
//!   or shows how the user writes: `db::written_by_the_user`).

use std::collections::HashSet;

use chrono::{DateTime, Duration, Local, TimeZone, Timelike, Utc};
use serde::{Deserialize, Serialize};

use crate::db::{Db, DbError, Draft, IdentifierInput, IdentifierKind, NewOutgoing, SendHow};
use crate::ids::fmt_rfc3339;
use crate::sending::{self, SendError};
use crate::sources::imap::{Credentials, ImapSourceConfig};
use crate::sources::outgoing;

/// Where the rules are kept.
pub const SETTING: &str = "autoreply";
/// Replies on its own in one conversation in a day: a second answers a
/// second question; a third is two programs talking to each other.
pub const PER_CONVERSATION_PER_DAY: i64 = 2;
/// How long a reply waits, at least and at most, in minutes.
pub const MIN_HOLD_MINUTES: i64 = 1;
pub const MAX_HOLD_MINUTES: i64 = 24 * 60;
pub const DEFAULT_HOLD_MINUTES: i64 = 10;
/// Replies on its own in a day, at most and unless changed.
pub const MAX_DAILY_LIMIT: i64 = 50;
pub const DEFAULT_DAILY_LIMIT: i64 = 10;
/// How far down the waiting list and the drafts a run looks.
const SCAN: usize = 500;

/// Why a held reply was stopped, or a draft not held: what the user is shown.
pub const TURNED_OFF: &str = "Replying by myself is off, so this wasn't sent.";
/// Rehearsing (0.10.0-alpha.45): the moment it would have gone.
pub const REHEARSED: &str = "Rehearsing: I'd have sent this by myself now, and didn't. It waits for you.";
/// Held while rehearsing, and rehearsing was turned off: shown as a
/// rehearsal, it doesn't go for real.
pub const REHEARSAL_OVER: &str = "I held this while rehearsing, so it waits for you rather than going for real.";
pub const DEALT_WITH: &str = "You dealt with this message yourself before it went.";
pub const NOT_WAITING: &str =
    "They aren't waiting on this reply any more — something came in or went out since — so it wasn't sent.";
pub const MAILBOX_OFF: &str = "Replying by myself is off for this mailbox, so this wasn't sent.";
pub const PERSON_OFF: &str = "Replying by myself to them is off, so this wasn't sent.";
pub const NO_PERSON: &str = "I don't know who this is from, so I don't reply to it by myself.";
pub const ASKED_FOR: &str = "You asked for this one yourself, so it waits for you.";
pub const CHOOSING: &str = "You're choosing between ways of saying this, so it waits for you.";
pub const ONCE: &str = "I've already answered this message by myself once.";
pub const DAILY_LIMIT: &str = "I've sent as many replies by myself today as you allow.";
pub const LOOPING: &str = "I've already replied by myself twice in this conversation today.";
pub const NOT_EMAIL: &str = "This isn't an email, so it can't be sent from here.";
pub const GONE: &str = "Its draft isn't here any more, so this wasn't sent.";
pub const BEFORE_ON: &str = "It came before replying by myself was turned on, so it waits for you.";
pub const OLD: &str = "I read it more than a day ago, or long after it was sent, so it waits for you.";
pub const LIST: &str = "It came through a mailing list, where a reply goes to everyone on it, so it waits for you.";
/// What a reply says, read before it goes by itself (0.10.0-alpha.49): a
/// model's draft can leave a gap to fill in, speak as what it is, or offer a
/// link nobody gave it — none of which the user would send unread.
pub const UNFINISHED: &str = "It has a gap left to fill in, like [this], so it waits for you.";
pub const NOT_IN_YOUR_VOICE: &str = "It talks about being an AI, which isn't you, so it waits for you.";
pub const NEW_LINK: &str = "It has a link that wasn't in their message, so it waits for you.";
pub const TOO_LONG: &str = "It's much longer than a reply I'd send without you, so it waits for you.";
/// The longest reply sent by itself, in characters.
pub const MOST_CHARS: usize = 3000;
/// Mail Mimic read longer ago than this is not answered by itself, however
/// it came to be allowed since (0.10.0-alpha.44): turned back on, a mailbox
/// or a person added, someone taken off the never list.
const READ_WITHIN_HOURS: i64 = 24;
/// Nor mail that reached Mimic longer than this after it says it was sent:
/// a mailbox catching up after failing for days.
const LATE_DAYS: i64 = 3;
pub const NOT_TO_YOU: &str =
    "It wasn't sent to you directly — a list, a copy you were hidden on, or I can't tell — so I don't answer it by myself.";
pub const SENT_FOLDER: &str = "It was in your Sent folder, so it may be yours; I don't answer it by myself.";
pub const NOT_THEIRS: &str =
    "A reply would go somewhere other than the address of the person who wrote, so I don't send it by myself.";
/// The mailbox's sending address changed while a reply was held: it would
/// go from an address the user didn't have when it was held.
pub const FROM_CHANGED: &str = "The address it would go from changed while I held it, so it waits for you.";

/// Who Mimic may answer by itself.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Who {
    /// Anyone the user has written to themselves.
    #[default]
    Known,
    /// Only the people in `Rules.people`.
    Listed,
}

/// What the user allowed. Every field has a default, so rules saved by an
/// earlier version read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Rules {
    pub enabled: bool,
    /// The mailboxes it may answer through (source ids).
    pub mailboxes: Vec<String>,
    pub who: Who,
    /// With `Who::Listed`, the people it may answer (participant ids).
    pub people: Vec<String>,
    /// People it never answers, whatever `who` says (0.10.0-alpha.37):
    /// those the user would rather answer themselves.
    #[serde(default)]
    pub never: Vec<String>,
    pub hold_minutes: i64,
    pub daily_limit: i64,
    /// When it was last turned on: only messages that arrived after are
    /// answered by it, so turning it on doesn't answer a backlog. Set by
    /// `set_rules`, whatever was asked for.
    pub enabled_at: Option<String>,
    /// The hours of the day replies may go by themselves, in this
    /// computer's time (0.10.0-alpha.39); any time when none.
    #[serde(default)]
    pub hours: Option<Hours>,
    /// Rehearsing (0.10.0-alpha.45): everything as when replying by itself
    /// — held, shown, checked again and timed — but at the moment a reply
    /// would go it is stopped instead (`REHEARSED`), and its draft waits for
    /// the user. Nothing is sent.
    #[serde(default)]
    pub rehearse: bool,
}

/// From `from` o'clock up to `to` o'clock, across midnight when `to` is the
/// earlier: `{ from: 8, to: 20 }` is eight in the morning to eight at night,
/// `{ from: 22, to: 6 }` the night.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hours {
    pub from: u32,
    pub to: u32,
}

impl Hours {
    fn contains(self, hour: u32) -> bool {
        if self.from < self.to {
            hour >= self.from && hour < self.to
        } else {
            hour >= self.from || hour < self.to
        }
    }
}

/// The first moment at or after `at` that falls within `hours` on `tz`'s
/// clock: `at` itself when it does, or the next time the clock reads `from`
/// o'clock (an hour later where that hour doesn't exist that day).
pub fn first_within<Tz: TimeZone>(hours: Option<Hours>, at: DateTime<Utc>, tz: &Tz) -> DateTime<Utc> {
    let Some(h) = hours else { return at };
    let local = at.with_timezone(tz);
    if h.contains(local.hour()) {
        return at;
    }
    let day = local.date_naive();
    let day = if local.hour() < h.from { day } else { day.succ_opt().unwrap_or(day) };
    (0..3)
        .filter_map(|late| day.and_hms_opt((h.from + late) % 24, 0, 0))
        .find_map(|naive| tz.from_local_datetime(&naive).earliest())
        .map(|t| t.with_timezone(&Utc))
        .unwrap_or(at)
}

impl Default for Rules {
    fn default() -> Self {
        Rules {
            enabled: false,
            mailboxes: Vec::new(),
            who: Who::Known,
            people: Vec::new(),
            never: Vec::new(),
            hold_minutes: DEFAULT_HOLD_MINUTES,
            daily_limit: DEFAULT_DAILY_LIMIT,
            enabled_at: None,
            hours: None,
            rehearse: false,
        }
    }
}

impl Rules {
    /// Within the bounds, whatever was asked for.
    fn bounded(mut self) -> Self {
        self.hold_minutes = self.hold_minutes.clamp(MIN_HOLD_MINUTES, MAX_HOLD_MINUTES);
        self.daily_limit = self.daily_limit.clamp(0, MAX_DAILY_LIMIT);
        self.mailboxes.dedup();
        self.people.dedup();
        self.never.dedup();
        // Within the day; the same hour twice is no window at all.
        self.hours = self.hours.map(|h| Hours { from: h.from.min(23), to: h.to.min(23) }).filter(|h| h.from != h.to);
        self
    }
}

/// The rules as saved, or the defaults (off).
pub fn rules(db: &Db) -> Result<Rules, DbError> {
    Ok(db.get_setting::<Rules>(SETTING)?.unwrap_or_default().bounded())
}

/// Save the rules. Turning it on records when; turning it off stops every
/// reply still held, at once.
pub fn set_rules(db: &Db, rules: Rules) -> Result<Rules, DbError> {
    let before = self::rules(db)?;
    let mut rules = rules.bounded();
    rules.enabled_at = match (before.enabled, rules.enabled) {
        (_, false) => None,
        (true, true) => before.enabled_at.or_else(|| Some(crate::ids::now_rfc3339())),
        (false, true) => Some(crate::ids::now_rfc3339()),
    };
    // Saved and stopped together: a look that reads the rules after finds
    // nothing held under the ones before — off, or what was shown as a
    // rehearsal, which doesn't go for real.
    let stop = if !rules.enabled {
        Some(TURNED_OFF)
    } else if before.rehearse && !rules.rehearse {
        Some(REHEARSAL_OVER)
    } else {
        None
    };
    match stop {
        Some(why) => {
            db.set_setting_stopping_held(SETTING, &rules, why)?;
        }
        None => db.set_setting(SETTING, &rules)?,
    }
    Ok(rules)
}

/// Whether it runs: on, and replies are being prepared in advance.
pub fn is_on(db: &Db, rules: &Rules) -> Result<bool, DbError> {
    Ok(rules.enabled && crate::assist::is_enabled(db)?)
}

/// What a run did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    /// Replies newly held to go by themselves.
    pub held: usize,
    pub sent: usize,
    /// Held replies the rules no longer allowed.
    pub stopped: usize,
    /// Held replies that didn't go.
    pub failed: usize,
    /// Held replies that may or may not have gone (0.10.0-alpha.44): never
    /// sent again until the user says they didn't.
    pub unsure: usize,
    /// Held replies whose time came while rehearsing (0.10.0-alpha.45):
    /// stopped, not sent.
    pub rehearsed: usize,
    /// Mailboxes with a reply whose hold has ended, waiting for the mailbox
    /// to be read again: a check of each is asked for, so it goes close to
    /// on time.
    #[serde(skip)]
    pub waiting_for_check: Vec<String>,
}

impl Run {
    pub fn changed_anything(&self) -> bool {
        self.held + self.sent + self.stopped + self.failed + self.unsure + self.rehearsed > 0
    }
}

/// `(conversation, message)` for every thread waiting on that message now,
/// leaving out what the user kept on the list against the headers or the
/// waiting window: those are theirs to answer.
fn waiting_now(db: &Db) -> Result<HashSet<(String, String)>, DbError> {
    Ok(db
        .threads_awaiting_reply(SCAN)?
        .into_iter()
        .filter(|t| t.automated.is_none() && !t.quiet)
        .map(|t| (t.conversation.id, t.last_message_id))
        .collect())
}

/// Why this draft may not go by itself now; `None` when it may. `except` is
/// the held reply being checked again, which is not counted against itself;
/// `holding` adds the limits, which only decide whether a reply is held.
fn refusal(
    db: &Db,
    rules: &Rules,
    draft: &Draft,
    waiting: &HashSet<(String, String)>,
    except: Option<&str>,
    holding: bool,
    now: DateTime<Utc>,
) -> Result<Option<&'static str>, SendError> {
    if !is_on(db, rules)? {
        return Ok(Some(TURNED_OFF));
    }
    if draft.outcome.is_some() {
        return Ok(Some(DEALT_WITH));
    }
    if draft.channel != "email" {
        return Ok(Some(NOT_EMAIL));
    }
    // Only a draft assisted drafting prepared: one the user asked for, even
    // with nothing typed, is theirs to send.
    if draft.intent.is_some()
        || draft.situation_id.is_some()
        || !draft.context.get("prepared").is_some_and(|p| p.as_bool() == Some(true))
    {
        return Ok(Some(ASKED_FOR));
    }
    if draft.alternative_to.is_some()
        || draft.context.get("adjustment").is_some_and(|a| !a.is_null())
        || !db.alternatives_of(&draft.id)?.is_empty()
    {
        return Ok(Some(CHOOSING));
    }
    let Some(incoming) = sending::answered(db, draft)? else {
        return Ok(Some(NOT_WAITING));
    };
    if !waiting.contains(&(incoming.conversation_id.clone(), incoming.id.clone())) {
        return Ok(Some(NOT_WAITING));
    }
    // The draft the home screen offers for it and no other: one the user
    // wrote since, or a decision about another, makes the message theirs.
    let offered = db.pending_draft_for_message(&incoming.conversation_id, &incoming.id, &incoming.body)?;
    if offered.map(|d| d.id).as_deref() != Some(draft.id.as_str()) {
        return Ok(Some(DEALT_WITH));
    }
    // Only what arrived since it was turned on.
    let on_since = rules.enabled_at.as_deref().and_then(|t| DateTime::parse_from_rfc3339(t).ok());
    let arrived = incoming.sent_at.as_deref().and_then(|t| DateTime::parse_from_rfc3339(t).ok());
    if !matches!((on_since, arrived), (Some(on), Some(at)) if at >= on) {
        return Ok(Some(BEFORE_ON));
    }
    // Not a backlog however it came to be allowed: read more than a day
    // ago, or reaching Mimic days after it says it was sent. Asked when it
    // is held; a hold, or hours, may then keep it a day or more.
    if holding {
        let read = db
            .message_imported_at(&incoming.id)?
            .and_then(|t| DateTime::parse_from_rfc3339(&t).ok())
            .map(|t| t.with_timezone(&Utc));
        let late =
            |read: DateTime<Utc>| arrived.is_some_and(|a| read - a.with_timezone(&Utc) > Duration::days(LATE_DAYS));
        if read.is_none_or(|r| r < now - Duration::hours(READ_WITHIN_HOURS) || late(r)) {
            return Ok(Some(OLD));
        }
    }
    // Through a list, a reply goes to the list (RFC 3834 §2).
    if incoming.metadata.get("list").and_then(serde_json::Value::as_bool) == Some(true) {
        return Ok(Some(LIST));
    }
    if let Some(why) = unfinished(&draft.generated_text, &incoming.body) {
        return Ok(Some(why));
    }
    if !rules.mailboxes.contains(&incoming.source_id) {
        return Ok(Some(MAILBOX_OFF));
    }
    if incoming.metadata.get("sentFolder").is_some() {
        return Ok(Some(SENT_FOLDER));
    }
    // Written to the user, by name — not a list, not a copy they were hidden
    // on, and not mail too old to say (RFC 3834).
    let mine = db.user_identifier_set()?;
    let to_me = ["to", "cc"]
        .iter()
        .filter_map(|k| incoming.metadata.get(*k).and_then(serde_json::Value::as_array))
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .any(|a| mine.contains(&IdentifierInput::new(IdentifierKind::Email, a).key()));
    if !to_me {
        return Ok(Some(NOT_TO_YOU));
    }
    let Some(person) = draft.participant_id.as_deref() else {
        return Ok(Some(NO_PERSON));
    };
    if incoming.participant_id.as_deref() != Some(person) {
        return Ok(Some(NOT_THEIRS));
    }
    if rules.never.iter().any(|p| p == person) {
        return Ok(Some(PERSON_OFF));
    }
    let allowed = match rules.who {
        Who::Listed => rules.people.iter().any(|p| p == person),
        // Written to by the user, not only by Mimic on its own.
        Who::Known => db.count_self_messages(None, Some(person))? > 0,
    };
    if !allowed {
        return Ok(Some(PERSON_OFF));
    }
    if db.answered_on_its_own(&incoming.id, except)? {
        return Ok(Some(ONCE));
    }
    if holding {
        let since = fmt_rfc3339(now - Duration::hours(24));
        if db.autonomous_since(&since, None, except)? >= rules.daily_limit {
            return Ok(Some(DAILY_LIMIT));
        }
        if db.autonomous_since(&since, Some(&incoming.conversation_id), except)? >= PER_CONVERSATION_PER_DAY {
            return Ok(Some(LOOPING));
        }
    }
    Ok(None)
}

/// What in a reply's words keeps it from going by itself (0.10.0-alpha.49),
/// if anything: a gap left to fill in (`[time]`, `{name}`, `<address>`), the
/// model speaking as itself, a link that isn't in the message it answers,
/// or more than `MOST_CHARS`. Words only: the signature is the user's.
pub fn unfinished(text: &str, answering: &str) -> Option<&'static str> {
    if text.chars().count() > MOST_CHARS {
        return Some(TOO_LONG);
    }
    if has_gap(text) {
        return Some(UNFINISHED);
    }
    let lower = text.to_lowercase().replace('\u{2019}', "'");
    const SELF_TALK: [&str; 6] =
        ["as an ai", "i am an ai", "i'm an ai", "language model", "ai assistant", "as an assistant"];
    if SELF_TALK.iter().any(|t| says_word(&lower, t)) {
        return Some(NOT_IN_YOUR_VOICE);
    }
    // Theirs however written: with or without `https://` or `www.`, or a
    // closing `/`.
    let theirs = answering.to_lowercase();
    if links_in(&lower).any(|link| !theirs.contains(plain_link(link))) {
        return Some(NEW_LINK);
    }
    None
}

/// Whether `phrase` is in `text` as words of its own: "i'm an ai", not
/// "i'm an aide".
fn says_word(text: &str, phrase: &str) -> bool {
    text.match_indices(phrase).any(|(i, _)| {
        let before = text[..i].chars().next_back();
        let after = text[i + phrase.len()..].chars().next();
        before.is_none_or(|c| !c.is_alphanumeric()) && after.is_none_or(|c| !c.is_alphanumeric())
    })
}

/// A link without its scheme, `www.` or closing `/`.
fn plain_link(link: &str) -> &str {
    let link = link.strip_prefix("https://").or_else(|| link.strip_prefix("http://")).unwrap_or(link);
    link.strip_prefix("www.").unwrap_or(link).trim_end_matches('/')
}

/// A gap left to fill in: brackets of any kind around something with a
/// letter in it, at most 40 characters long — not an address or a link
/// (`<sam@example.com>`, `<https://…>`).
fn has_gap(text: &str) -> bool {
    [('[', ']'), ('{', '}'), ('<', '>')].iter().any(|&(open, close)| {
        text.match_indices(open).any(|(i, _)| {
            let rest = &text[i + open.len_utf8()..];
            rest.find(close).is_some_and(|j| {
                let inside = &rest[..j];
                inside.chars().count() <= 40
                    && !inside.contains('\n')
                    && !inside.contains('@')
                    && !inside.contains("://")
                    && inside.chars().any(char::is_alphabetic)
            })
        })
    })
}

/// The links in already lower-cased words: `http://`, `https://` and
/// `www.` to the next space, closing mark or character outside ASCII (in
/// writing without spaces), without trailing punctuation.
fn links_in(lower: &str) -> impl Iterator<Item = &str> {
    lower.split(|c: char| c.is_whitespace() || matches!(c, '(' | ')' | '"' | '\'' | '<' | '>')).filter_map(|word| {
        let start = ["https://", "http://", "www."].iter().filter_map(|p| word.find(*p)).min()?;
        let link = word[start..].split(|c: char| !c.is_ascii()).next().unwrap_or_default();
        let link = link.trim_end_matches(['.', ',', '!', '?', ';', ':']);
        (!link.is_empty()).then_some(link)
    })
}

/// Whether every address a reply would go to is one of the person's own
/// stored addresses: a reply sent by itself never follows a `Reply-To`
/// elsewhere.
fn goes_to_them(db: &Db, person: Option<&str>, to: &[String]) -> Result<bool, DbError> {
    let Some(p) = person.map(|p| db.get_participant(p)).transpose()?.flatten() else {
        return Ok(false);
    };
    Ok(!to.is_empty()
        && to.iter().all(|a| {
            p.identifiers.iter().any(|i| i.kind == IdentifierKind::Email.as_str() && i.value.eq_ignore_ascii_case(a))
        }))
}

/// Whether a check that read the mailbox through began after `since` (the
/// end of a reply's hold): an answer the user sent from another program
/// before then is only known once one has (`ImapSourceConfig::last_read_at`;
/// a check that ended, failed or was stopped after it doesn't count).
fn read_since(db: &Db, config: &ImapSourceConfig, source_id: &str, since: &str) -> Result<bool, DbError> {
    let settled = db.get_source(source_id)?.is_some_and(|s| s.status != "failed" && s.status != "importing");
    let read = config.last_read_at.as_deref().and_then(|t| DateTime::parse_from_rfc3339(t).ok());
    let held = DateTime::parse_from_rfc3339(since).ok();
    Ok(settled && matches!((read, held), (Some(r), Some(h)) if r > h))
}

/// Hold a reply for every draft that may go by itself: one `outbox` row
/// each, `held` until `now` plus the hold. Returns how many.
pub fn hold_replies(db: &Db, now: DateTime<Utc>) -> Result<usize, SendError> {
    let rules = rules(db)?;
    if !is_on(db, &rules)? {
        return Ok(0);
    }
    let waiting = waiting_now(db)?;
    let mut held = 0;
    for draft in db.pending_drafts(SCAN)? {
        if refusal(db, &rules, &draft, &waiting, None, true, now)?.is_some() {
            continue;
        }
        let Some(incoming) = sending::answered(db, &draft)? else { continue };
        // Through the mailbox the message came in to, as the user's own
        // address there, or not at all.
        let Ok((config, account)) = sending::sending_from(db, &incoming.source_id) else { continue };
        let envelope = sending::envelope_for(db, &draft.id)?;
        if envelope.why_not.is_some() || draft.generated_text.trim().is_empty() {
            continue;
        }
        if !goes_to_them(db, draft.participant_id.as_deref(), &envelope.to)? {
            continue;
        }
        // The rules as they are now: turned off, or changed, while this ran.
        let now_rules = self::rules(db)?;
        if !is_on(db, &now_rules)? {
            break;
        }
        if now_rules != rules && refusal(db, &now_rules, &draft, &waiting, None, true, now)?.is_some() {
            continue;
        }
        let queued = db.queue_outgoing(&NewOutgoing {
            draft_id: Some(draft.id.clone()),
            source_id: incoming.source_id.clone(),
            conversation_id: Some(incoming.conversation_id.clone()),
            answers_message_id: Some(incoming.id.clone()),
            participant_id: draft.participant_id.clone(),
            from_address: account.from_address.clone(),
            to: envelope.to.clone(),
            cc: Vec::new(),
            subject: envelope.subject.clone(),
            body: sending::signed(&draft.generated_text, config.signature()),
            message_id: outgoing::new_message_id(&account.from_address),
            how: SendHow::Autonomous,
            // Held for the while the user chose, then until the hours they
            // allow, if those have ended for the day: as they are now.
            send_after: Some(fmt_rfc3339(first_within(
                now_rules.hours,
                now + Duration::minutes(now_rules.hold_minutes),
                &Local,
            ))),
        });
        match queued {
            Ok(_) => held += 1,
            // Decided, or one already on its way, since the draft was read.
            Err(DbError::Invalid(_)) | Err(DbError::NotFound(_)) => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(held)
}

/// A held reply the user said to send now (`Db::send_held_now`): its time
/// and its last change are the same moment. Nothing else gives a held reply
/// that — holding it sets its time a hold ahead, and `Db::hold_until` a
/// window ahead, of the change.
fn released(entry: &crate::db::OutboxEntry) -> bool {
    entry.send_after == entry.updated_at
}

/// Send every held reply whose time has come and which the rules still
/// allow; stop, with the reason, any they no longer do.
pub fn send_due(db: &Db, credentials: &Credentials, now: DateTime<Utc>) -> Result<Run, SendError> {
    let mut run = Run::default();
    let now_text = fmt_rfc3339(now);
    let due: Vec<_> = db.due_outgoing(&now_text)?.into_iter().filter(|e| e.how == SendHow::Autonomous).collect();
    if due.is_empty() {
        return Ok(run);
    }
    let early_waiting = waiting_now(db)?;
    for entry in due {
        let Some(draft) = entry.draft_id.as_deref().map(|id| db.get_draft(id)).transpose()?.flatten() else {
            run.stopped += usize::from(db.stop_held_outgoing(&entry.id, GONE)?);
            continue;
        };
        // The rules as they are now, for each: a send before this one can
        // take a while, and they may have changed meanwhile.
        let rules = rules(db)?;
        if let Some(why) = refusal(db, &rules, &draft, &early_waiting, Some(&entry.id), false, now)? {
            run.stopped += usize::from(db.stop_held_outgoing(&entry.id, why)?);
            continue;
        }
        // Outside the hours allowed — set since it was held, or passed while
        // it waited for a check or the computer slept — it waits for them,
        // unless the user said to send it now.
        if !released(&entry) {
            let start = first_within(rules.hours, now, &Local);
            if start > now {
                db.hold_until(&entry.id, &fmt_rfc3339(start))?;
                continue;
            }
        }
        let (config, account) = match sending::sending_from(db, &entry.source_id) {
            Ok(found) => found,
            Err(SendError::Cannot(why)) => {
                run.stopped += usize::from(db.stop_held_outgoing(&entry.id, &why)?);
                continue;
            }
            Err(e) => return Err(e),
        };
        // From the address it would go from now: one changed, or no longer
        // the user's, since it was held stops it.
        if !entry.from_address.eq_ignore_ascii_case(&account.from_address) {
            run.stopped += usize::from(db.stop_held_outgoing(&entry.id, FROM_CHANGED)?);
            continue;
        }
        // It waits for a check of the mailbox that began after its hold
        // ended, so an answer the user sent meanwhile is known.
        if !read_since(db, &config, &entry.source_id, &entry.send_after)? {
            if !run.waiting_for_check.contains(&entry.source_id) {
                run.waiting_for_check.push(entry.source_id.clone());
            }
            continue;
        }
        // Then checked again on what that check read.
        let waiting = waiting_now(db)?;
        if let Some(why) = refusal(db, &rules, &draft, &waiting, Some(&entry.id), false, now)? {
            run.stopped += usize::from(db.stop_held_outgoing(&entry.id, why)?);
            continue;
        }
        if !goes_to_them(db, draft.participant_id.as_deref(), &entry.to)? {
            run.stopped += usize::from(db.stop_held_outgoing(&entry.id, NOT_THEIRS)?);
            continue;
        }
        // Rehearsing: everything up to here, and no further.
        if rules.rehearse {
            run.rehearsed += usize::from(db.stop_held_outgoing(&entry.id, REHEARSED)?);
            continue;
        }
        // Held → sending, unless it was stopped or taken a moment ago, or its
        // draft was decided.
        if !db.take_held_outgoing(&entry.id, &now_text)? {
            continue;
        }
        let bytes = match sending::written_out(db, &entry, true) {
            Ok(bytes) => bytes,
            Err(e) => {
                db.outgoing_failed(&entry.id, &e.to_string())?;
                run.failed += 1;
                continue;
            }
        };
        match sending::deliver(db, credentials, &config, &account, &entry, &bytes) {
            Ok(()) => {
                run.sent += 1;
                sending::file_a_copy(db, credentials, &config, &account, &entry, &bytes);
                // Marked first: a draft decided as sent but not marked would
                // be learned from as the user's.
                if let Err(e) =
                    // As written: the signature under it is the mailbox's,
                    // not words of the draft.
                    db
                        .mark_sent_on_its_own(&draft.id)
                        .and_then(|_| sending::record_sent_draft(db, &draft, &draft.generated_text))
                {
                    tracing::warn!(target: "autoreply", error = %e, "a reply sent on its own could not be recorded on its draft");
                }
                if let Err(e) = sending::record_in_conversation(db, &entry.source_id, &bytes, true) {
                    tracing::warn!(target: "autoreply", error = %e, "a reply sent on its own could not be put in its conversation");
                }
            }
            // `deliver` recorded why, as failed or unsure.
            Err(e) if e.may_have_gone() => run.unsure += 1,
            Err(_) => run.failed += 1,
        }
    }
    Ok(run)
}

/// One look: hold what may go, then send what is due.
pub fn tick(db: &Db, credentials: &Credentials, now: DateTime<Utc>) -> Result<Run, SendError> {
    let held = hold_replies(db, now)?;
    let mut run = send_due(db, credentials, now)?;
    run.held = held;
    Ok(run)
}

/// What Settings shows: the rules, whether they run, and how many replies
/// went, or are held, by themselves in the last day against the limit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub rules: Rules,
    /// On, and replies are being prepared in advance.
    pub running: bool,
    pub last_day: i64,
}

pub fn status(db: &Db) -> Result<Status, DbError> {
    let rules = rules(db)?;
    Ok(Status { running: is_on(db, &rules)?, last_day: last_day(db)?, rules })
}

/// `tick` at the current time.
pub fn tick_now(db: &Db, credentials: &Credentials) -> Result<Run, SendError> {
    tick(db, credentials, Utc::now())
}

/// Replies sent, or held to send, by themselves in the last day: what the
/// daily limit counts.
pub fn last_day(db: &Db) -> Result<i64, DbError> {
    db.autonomous_since(&fmt_rfc3339(Utc::now() - Duration::hours(24)), None, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{IdentifierKind, NewDraft, NewSource, OutboxEntry, SendStatus};
    use crate::sources::imap::{self, ImapSourceConfig};
    use crate::sources::smtp::test_server::{serve, Behaviour, Transcript};
    use crate::sources::smtp::{SmtpAccount, SmtpSecurity};
    use serde_json::json;
    use std::sync::Arc;

    fn credentials() -> Credentials {
        Credentials::new(
            Arc::new(|_: &str| Some("app-pass".to_string())),
            Arc::new(|_: &str, _: &str| Ok(())),
            crate::sources::oauth::Provider::microsoft(),
            None,
        )
    }

    struct World {
        db: Db,
        mailbox: String,
        ada: String,
        draft: Draft,
        seen: std::sync::mpsc::Receiver<Transcript>,
    }

    const EARLIER: &str = "From: Ada Lovelace <ada@example.com>\r\nTo: C <c@example.com>\r\nSubject: Lunch\r\nDate: Mon, 2 Mar 2026 09:00:00 +0000\r\nMessage-ID: <l1@x.example>\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nlunch on thursday?\r\n";
    const MINE: &str = "From: C <c@example.com>\r\nTo: Ada <ada@example.com>\r\nSubject: Re: Lunch\r\nDate: Mon, 2 Mar 2026 09:30:00 +0000\r\nMessage-ID: <l2@x.example>\r\nIn-Reply-To: <l1@x.example>\r\nReferences: <l1@x.example>\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nyes! where?\r\n";
    /// Ada's question, sent ten minutes before the tests' clock starts.
    fn question() -> String {
        format!(
            "From: Ada Lovelace <ada@example.com>\r\nTo: C <c@example.com>\r\nSubject: Re: Lunch\r\nDate: {}\r\nMessage-ID: <l3@x.example>\r\nIn-Reply-To: <l2@x.example>\r\nReferences: <l1@x.example> <l2@x.example>\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nthe place by the river?\r\n",
            at(-10).to_rfc2822()
        )
    }

    fn read_in(db: &Db, source: &str, raw: &str, sent: bool) {
        let mut mail = crate::sources::mbox::parse_raw(raw.as_bytes()).unwrap();
        mail.sent = sent;
        let mut importer = crate::import::Importer::begin(db, source).unwrap();
        for convo in crate::sources::mbox::thread(vec![mail]) {
            importer.take(convo).unwrap();
        }
        importer.finish().unwrap();
    }

    /// A mailbox that sends to a scripted server, a conversation with Ada in
    /// which the user has written to her, her question waiting, a draft
    /// prepared for it in advance, and replying by itself on for the mailbox.
    fn world(b: Behaviour) -> World {
        let (port, seen) = serve(b);
        let db = Db::open_in_memory().unwrap();
        db.set_setting(crate::db::WITHIN_DAYS_SETTING, &0).unwrap();
        db.set_setting(crate::assist::SETTING, &true).unwrap();
        db.set_user_identity("C Haynes").unwrap();
        db.add_user_identifier(IdentifierKind::Email, "c@example.com").unwrap();
        let config = ImapSourceConfig {
            account: imap::ImapAccount {
                host: "127.0.0.1".into(),
                port: 1,
                username: "c@example.com".into(),
                security: imap::Security::Plain,
                auth: imap::Auth::Password,
            },
            folders: vec![],
            sent_folder: None,
            marks: Default::default(),
            last_checked_at: None,
            last_read_at: None,
            sending: Some(SmtpAccount {
                host: "127.0.0.1".into(),
                port,
                security: SmtpSecurity::Plain,
                username: "c@example.com".into(),
                from_address: "c@example.com".into(),
                copy_to_sent: Some(false),
            }),
            signature: None,
        };
        let mailbox = db
            .create_source(&NewSource {
                connector: imap::CONNECTOR.into(),
                name: "c@example.com".into(),
                channel: "email".into(),
                location: None,
                config: serde_json::to_value(&config).unwrap(),
            })
            .unwrap()
            .id;
        read_in(&db, &mailbox, EARLIER, false);
        read_in(&db, &mailbox, MINE, true);
        read_in(&db, &mailbox, &question(), false);
        // Read five minutes after it was sent, on the tests' clock.
        db.conn()
            .execute("UPDATE messages SET imported_at = ?1 WHERE external_id = 'l3@x.example'", [fmt_rfc3339(at(-5))])
            .unwrap();
        let (message, conversation, ada): (String, String, String) = db
            .conn()
            .query_row(
                "SELECT id, conversation_id, participant_id FROM messages WHERE external_id = 'l3@x.example'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        let draft = db
            .create_draft(&NewDraft {
                participant_id: Some(ada.clone()),
                conversation_id: Some(conversation),
                channel: "email".into(),
                situation_id: None,
                incoming_message: Some("the place by the river?".into()),
                incoming_message_id: Some(message),
                intent: None,
                generated_text: "yes, the river place. 12:30?".into(),
                provider: "mock".into(),
                model: "m".into(),
                context: json!({}),
                prompt_hash: "h".into(),
                evidence: json!({}),
                alternative_to: None,
            })
            .unwrap();
        db.mark_prepared(&draft.id).unwrap();
        set_rules(&db, Rules { enabled: true, mailboxes: vec![mailbox.clone()], ..Rules::default() }).unwrap();
        // Turned on before Ada's question came in, so it isn't a backlog.
        let on = Rules { enabled_at: Some("2026-01-01T00:00:00.000Z".into()), ..rules(&db).unwrap() };
        db.set_setting(SETTING, &on).unwrap();
        World { db, mailbox, ada, draft, seen }
    }

    /// The mailbox has been read since anything was held: by a check that
    /// began after any hold here ends, on the tests' clock or the real one.
    fn checked(w: &World) {
        read_at(w, Utc::now().max(at(0)) + Duration::days(7));
    }

    /// A check that read the mailbox began at `began`.
    fn read_at(w: &World, began: DateTime<Utc>) {
        let mut config: ImapSourceConfig =
            serde_json::from_value(w.db.get_source(&w.mailbox).unwrap().unwrap().config).unwrap();
        config.last_checked_at = Some(fmt_rfc3339(began + Duration::minutes(1)));
        config.last_read_at = Some(fmt_rfc3339(began));
        w.db.set_source_config(&w.mailbox, &serde_json::to_value(&config).unwrap()).unwrap();
        w.db.set_source_status(&w.mailbox, "imported", None).unwrap();
    }

    fn at(minutes: i64) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-10-26T09:00:00Z").unwrap().with_timezone(&Utc) + Duration::minutes(minutes)
    }

    fn only_entry(db: &Db) -> OutboxEntry {
        let all = db.recent_outgoing(10).unwrap();
        assert_eq!(all.len(), 1, "{all:?}");
        all.into_iter().next().unwrap()
    }

    #[test]
    fn a_reply_is_held_then_sent_by_itself_saying_so_and_is_not_the_users_writing() {
        let w = world(Behaviour::default());
        let own_before = w.db.count_self_messages(None, None).unwrap();

        let first = tick(&w.db, &credentials(), at(0)).unwrap();
        assert_eq!((first.held, first.sent), (1, 0), "held, not sent at once");
        let held = only_entry(&w.db);
        assert_eq!(held.status, SendStatus::Held);
        assert_eq!(held.how, SendHow::Autonomous);
        assert_eq!(held.to, vec!["ada@example.com".to_string()]);
        assert_eq!(held.send_after, fmt_rfc3339(at(DEFAULT_HOLD_MINUTES)));
        assert_eq!(w.db.held_outgoing_for_draft(&w.draft.id).unwrap().map(|e| e.id), Some(held.id.clone()));
        assert_eq!(tick(&w.db, &credentials(), at(1)).unwrap(), Run::default(), "nothing more to do while it waits");
        // Its time has come, but the mailbox hasn't been read since: it
        // waits, and asks for it to be.
        let waiting = tick(&w.db, &credentials(), at(DEFAULT_HOLD_MINUTES)).unwrap();
        assert_eq!((waiting.sent, waiting.waiting_for_check.clone()), (0, vec![w.mailbox.clone()]));
        assert!(!waiting.changed_anything());
        checked(&w);

        let later = tick(&w.db, &credentials(), at(DEFAULT_HOLD_MINUTES)).unwrap();
        assert_eq!(later.sent, 1);
        let sent = only_entry(&w.db);
        assert_eq!(sent.status, SendStatus::Sent);
        let data = String::from_utf8(w.seen.recv().unwrap().data).unwrap();
        assert!(data.contains("\r\nAuto-Submitted: auto-replied\r\n"), "{data}");
        assert!(data.contains("\r\nIn-Reply-To: <l3@x.example>\r\n"), "{data}");

        let draft = w.db.get_draft(&w.draft.id).unwrap().unwrap();
        assert_eq!(draft.outcome.as_deref(), Some("sent_unedited"));
        assert!(w.db.sent_drafts_for_learning().unwrap().is_empty(), "nothing is learned from it");
        assert!(w.db.threads_awaiting_reply(10).unwrap().is_empty(), "Ada has her answer");
        assert_eq!(
            w.db.count_self_messages(None, None).unwrap(),
            own_before,
            "and it isn't counted as the user's writing"
        );
        let metadata: String = w
            .db
            .conn()
            .query_row("SELECT metadata_json FROM messages WHERE external_id = ?1", [&sent.message_id], |r| r.get(0))
            .unwrap();
        assert!(metadata.contains("\"sentOnItsOwn\":true") && !metadata.contains("automated"), "{metadata}");

        // The Sent folder's copy, read before the reply was put in its
        // conversation, is marked as sent by itself all the same.
        w.db.conn().execute("DELETE FROM messages WHERE external_id = ?1", [&sent.message_id]).unwrap();
        read_in(&w.db, &w.mailbox, &data, true);
        let metadata: String = w
            .db
            .conn()
            .query_row("SELECT metadata_json FROM messages WHERE external_id = ?1", [&sent.message_id], |r| r.get(0))
            .unwrap();
        assert!(metadata.contains("\"sentOnItsOwn\":true") && !metadata.contains("automated"), "{metadata}");
        assert_eq!(tick(&w.db, &credentials(), at(30)).unwrap(), Run::default(), "and nothing is sent twice");
    }

    #[test]
    fn a_check_that_began_before_the_hold_ended_does_not_count() {
        let w = world(Behaviour::default());
        assert_eq!(tick(&w.db, &credentials(), at(0)).unwrap().held, 1);
        // Read during the hold: an answer the user sent from elsewhere after
        // that check began isn't in what it read.
        read_at(&w, at(1));
        let run = tick(&w.db, &credentials(), at(DEFAULT_HOLD_MINUTES)).unwrap();
        assert_eq!((run.sent, run.waiting_for_check.clone()), (0, vec![w.mailbox.clone()]));
        read_at(&w, at(DEFAULT_HOLD_MINUTES) + Duration::seconds(1));
        assert_eq!(tick(&w.db, &credentials(), at(DEFAULT_HOLD_MINUTES + 1)).unwrap().sent, 1);
    }

    #[test]
    fn mail_through_a_list_or_read_long_after_it_was_sent_is_left_for_the_user() {
        let through_a_list = |w: &World| {
            w.db.conn()
                .execute(
                    "UPDATE messages SET metadata_json = json_set(metadata_json, '$.list', json('true'))
                     WHERE external_id = 'l3@x.example'",
                    [],
                )
                .unwrap();
        };
        let w = world(Behaviour::default());
        through_a_list(&w);
        assert_eq!(tick(&w.db, &credentials(), at(0)).unwrap().held, 0);

        // Read more than a day ago: a backlog, however it came to be allowed.
        let w = world(Behaviour::default());
        w.db.conn()
            .execute(
                "UPDATE messages SET imported_at = ?1 WHERE external_id = 'l3@x.example'",
                [fmt_rfc3339(at(-60 * 25))],
            )
            .unwrap();
        assert_eq!(tick(&w.db, &credentials(), at(0)).unwrap().held, 0);

        // Read just now, but sent days before.
        let w = world(Behaviour::default());
        w.db.conn()
            .execute(
                "UPDATE messages SET sent_at = ?1 WHERE external_id = 'l3@x.example'",
                [fmt_rfc3339(at(-60 * 24 * 4))],
            )
            .unwrap();
        assert_eq!(tick(&w.db, &credentials(), at(0)).unwrap().held, 0);

        // One already held is stopped, saying why.
        let w = world(Behaviour::default());
        assert_eq!(tick(&w.db, &credentials(), at(0)).unwrap().held, 1);
        through_a_list(&w);
        checked(&w);
        let run = tick(&w.db, &credentials(), at(DEFAULT_HOLD_MINUTES)).unwrap();
        assert_eq!((run.sent, run.stopped), (0, 1));
        assert_eq!(only_entry(&w.db).error.as_deref(), Some(LIST));
    }

    #[test]
    fn a_held_reply_whose_sending_address_changed_waits_for_the_user() {
        let w = world(Behaviour::default());
        tick(&w.db, &credentials(), at(0)).unwrap();
        w.db.add_user_identifier(IdentifierKind::Email, "c@work.example").unwrap();
        let mut config: ImapSourceConfig =
            serde_json::from_value(w.db.get_source(&w.mailbox).unwrap().unwrap().config).unwrap();
        config.sending.as_mut().unwrap().from_address = "c@work.example".into();
        w.db.set_source_config(&w.mailbox, &serde_json::to_value(&config).unwrap()).unwrap();
        checked(&w);
        let run = tick(&w.db, &credentials(), at(DEFAULT_HOLD_MINUTES)).unwrap();
        assert_eq!((run.sent, run.stopped), (0, 1));
        let stopped = only_entry(&w.db);
        assert_eq!((stopped.status, stopped.error.as_deref()), (SendStatus::Canceled, Some(FROM_CHANGED)));
    }

    #[test]
    fn pressing_send_on_a_reply_held_to_go_by_itself_says_so_and_leaves_it() {
        let w = world(Behaviour::default());
        tick(&w.db, &credentials(), at(0)).unwrap();
        let req = sending::SendRequest {
            draft_id: w.draft.id.clone(),
            text: "yes, the river place".into(),
            source_id: w.mailbox.clone(),
            to: vec!["ada@example.com".into()],
            cc: vec![],
            subject: "Re: Lunch".into(),
        };
        let e = sending::send_reply(&w.db, &credentials(), &req).unwrap_err();
        assert!(matches!(e, SendError::Db(DbError::Invalid(ref m)) if m == sending::HELD), "{e}");
        assert_eq!(only_entry(&w.db).status, SendStatus::Held, "the held one is untouched");
    }

    #[test]
    fn a_reply_by_itself_that_may_have_gone_is_counted_apart() {
        let w = world(Behaviour { drop_after_data: true, ..Behaviour::default() });
        tick(&w.db, &credentials(), at(0)).unwrap();
        checked(&w);
        let run = tick(&w.db, &credentials(), at(DEFAULT_HOLD_MINUTES)).unwrap();
        assert_eq!((run.sent, run.failed, run.unsure), (0, 0, 1));
        assert!(run.changed_anything());
        assert_eq!(only_entry(&w.db).status, SendStatus::Unsure);
    }

    #[test]
    fn a_reply_held_is_stopped_when_the_user_answers_first_or_turns_it_off() {
        let w = world(Behaviour::default());
        tick(&w.db, &credentials(), at(0)).unwrap();
        // The user used the draft themselves before the hold ended.
        w.db.resolve_draft(&w.draft.id, "sent_unedited", Some("yes, the river place. 12:30?")).unwrap();
        let run = tick(&w.db, &credentials(), at(DEFAULT_HOLD_MINUTES)).unwrap();
        assert_eq!((run.sent, run.stopped), (0, 1));
        let stopped = only_entry(&w.db);
        assert_eq!((stopped.status, stopped.error.as_deref()), (SendStatus::Canceled, Some(DEALT_WITH)));

        let w = world(Behaviour::default());
        tick(&w.db, &credentials(), at(0)).unwrap();
        set_rules(&w.db, Rules { enabled: false, ..rules(&w.db).unwrap() }).unwrap();
        let off = only_entry(&w.db);
        assert_eq!((off.status, off.error.as_deref()), (SendStatus::Canceled, Some(TURNED_OFF)), "stopped at once");
        assert_eq!(tick(&w.db, &credentials(), at(DEFAULT_HOLD_MINUTES)).unwrap(), Run::default());
    }

    #[test]
    fn it_answers_only_whom_and_where_the_user_allowed() {
        // A mailbox not named.
        let w = world(Behaviour::default());
        set_rules(&w.db, Rules { mailboxes: vec![], ..rules(&w.db).unwrap() }).unwrap();
        assert_eq!(tick(&w.db, &credentials(), at(0)).unwrap().held, 0);

        // Only listed people, and Ada isn't listed — then she is.
        set_rules(&w.db, Rules { mailboxes: vec![w.mailbox.clone()], who: Who::Listed, ..rules(&w.db).unwrap() })
            .unwrap();
        assert_eq!(tick(&w.db, &credentials(), at(0)).unwrap().held, 0);
        set_rules(&w.db, Rules { people: vec![w.ada.clone()], ..rules(&w.db).unwrap() }).unwrap();
        assert_eq!(tick(&w.db, &credentials(), at(0)).unwrap().held, 1);

        // Replies in advance off: nothing runs.
        let w = world(Behaviour::default());
        w.db.set_setting(crate::assist::SETTING, &false).unwrap();
        assert_eq!(tick(&w.db, &credentials(), at(0)).unwrap().held, 0);
    }

    #[test]
    fn someone_named_never_is_not_answered_by_itself_whoever_else_is() {
        let w = world(Behaviour::default());
        set_rules(&w.db, Rules { never: vec![w.ada.clone()], ..rules(&w.db).unwrap() }).unwrap();
        assert_eq!(tick(&w.db, &credentials(), at(0)).unwrap().held, 0, "known, and named never");
        set_rules(&w.db, Rules { who: Who::Listed, people: vec![w.ada.clone()], ..rules(&w.db).unwrap() }).unwrap();
        assert_eq!(tick(&w.db, &credentials(), at(0)).unwrap().held, 0, "listed too: never wins");

        // Held, and then named: stopped when it would go, saying why.
        let w = world(Behaviour::default());
        tick(&w.db, &credentials(), at(0)).unwrap();
        set_rules(&w.db, Rules { never: vec![w.ada.clone()], ..rules(&w.db).unwrap() }).unwrap();
        checked(&w);
        assert_eq!(tick(&w.db, &credentials(), at(DEFAULT_HOLD_MINUTES)).unwrap().sent, 0);
        let stopped = only_entry(&w.db);
        assert_eq!((stopped.status, stopped.error.as_deref()), (SendStatus::Canceled, Some(PERSON_OFF)));
    }

    #[test]
    fn someone_the_user_never_wrote_to_is_not_answered_by_itself() {
        let w = world(Behaviour::default());
        // Without the user's own message, Ada is someone they never wrote to.
        w.db.conn().execute("DELETE FROM messages WHERE external_id = 'l2@x.example'", []).unwrap();
        assert_eq!(tick(&w.db, &credentials(), at(0)).unwrap().held, 0);
    }

    #[test]
    fn a_draft_the_user_asked_for_or_is_choosing_between_waits_for_them() {
        let w = world(Behaviour::default());
        w.db.conn().execute("UPDATE drafts SET intent = 'yes' WHERE id = ?1", [&w.draft.id]).unwrap();
        assert_eq!(tick(&w.db, &credentials(), at(0)).unwrap().held, 0);

        let w = world(Behaviour::default());
        w.db.create_draft(&NewDraft {
            alternative_to: Some(w.draft.id.clone()),
            context: json!({"adjustment": "shorter"}),
            generated_text: "river, 12:30?".into(),
            ..NewDraft {
                participant_id: w.draft.participant_id.clone(),
                conversation_id: w.draft.conversation_id.clone(),
                channel: "email".into(),
                situation_id: None,
                incoming_message: w.draft.incoming_message.clone(),
                incoming_message_id: w.draft.incoming_message_id.clone(),
                intent: None,
                generated_text: String::new(),
                provider: "mock".into(),
                model: "m".into(),
                context: json!({}),
                prompt_hash: "h".into(),
                evidence: json!({}),
                alternative_to: None,
            }
        })
        .unwrap();
        assert_eq!(tick(&w.db, &credentials(), at(0)).unwrap().held, 0);
    }

    #[test]
    fn a_message_is_answered_by_itself_once_whatever_became_of_the_reply() {
        let w = world(Behaviour { refuse_rcpt: Some("ada@example.com"), ..Behaviour::default() });
        tick(&w.db, &credentials(), at(0)).unwrap();
        checked(&w);
        let run = tick(&w.db, &credentials(), at(DEFAULT_HOLD_MINUTES)).unwrap();
        assert_eq!(run.failed, 1);
        assert_eq!(only_entry(&w.db).status, SendStatus::Failed);
        assert_eq!(w.db.get_draft(&w.draft.id).unwrap().unwrap().outcome, None, "the draft waits for the user");
        assert_eq!(tick(&w.db, &credentials(), at(60)).unwrap().held, 0, "and isn't tried again by itself");
    }

    #[test]
    fn a_reply_by_itself_goes_only_to_the_writers_own_address_and_only_when_written_to_the_user() {
        // Replies asked to go elsewhere (`Reply-To`) are the user's to send.
        let w = world(Behaviour::default());
        w.db.conn()
            .execute(
                "UPDATE messages SET metadata_json = json_set(metadata_json, '$.replyTo', json('[\"eve@example.net\"]'))
                 WHERE external_id = 'l3@x.example'",
                [],
            )
            .unwrap();
        assert_eq!(tick(&w.db, &credentials(), at(0)).unwrap().held, 0);

        // Not written to the user by name: a list, or a hidden copy.
        let w = world(Behaviour::default());
        w.db.conn()
            .execute(
                "UPDATE messages SET metadata_json = json_set(metadata_json, '$.to', json('[\"team@example.com\"]'))
                 WHERE external_id = 'l3@x.example'",
                [],
            )
            .unwrap();
        assert_eq!(tick(&w.db, &credentials(), at(0)).unwrap().held, 0);

        // Found in the Sent folder: perhaps the user's own.
        let w = world(Behaviour::default());
        w.db.conn()
            .execute(
                "UPDATE messages SET metadata_json = json_set(metadata_json, '$.sentFolder', json('true'))
                 WHERE external_id = 'l3@x.example'",
                [],
            )
            .unwrap();
        assert_eq!(tick(&w.db, &credentials(), at(0)).unwrap().held, 0);
    }

    #[test]
    fn a_backlog_from_before_it_was_turned_on_is_left_for_the_user() {
        let w = world(Behaviour::default());
        // Turned on after Ada's question came in.
        let on = Rules { enabled_at: Some(fmt_rfc3339(at(-1))), ..rules(&w.db).unwrap() };
        w.db.set_setting(SETTING, &on).unwrap();
        assert_eq!(tick(&w.db, &credentials(), at(0)).unwrap().held, 0);
        // Turning it off and on again moves the start to now.
        set_rules(&w.db, Rules { enabled: false, ..rules(&w.db).unwrap() }).unwrap();
        assert_eq!(rules(&w.db).unwrap().enabled_at, None);
        let again = set_rules(&w.db, Rules { enabled: true, ..rules(&w.db).unwrap() }).unwrap();
        assert!(again.enabled_at.is_some_and(|t| Some(t) > on.enabled_at));
    }

    #[test]
    fn a_draft_not_prepared_in_advance_or_not_the_one_on_screen_waits_for_the_user() {
        // Asked for, with nothing typed.
        let w = world(Behaviour::default());
        w.db.conn()
            .execute(
                "UPDATE drafts SET context_json = json_remove(context_json, '$.prepared') WHERE id = ?1",
                [&w.draft.id],
            )
            .unwrap();
        assert_eq!(tick(&w.db, &credentials(), at(0)).unwrap().held, 0);

        // A newer draft for the same message, written by the user, is the one on screen.
        let w = world(Behaviour::default());
        let newer =
            w.db.create_draft(&NewDraft {
                participant_id: w.draft.participant_id.clone(),
                conversation_id: w.draft.conversation_id.clone(),
                channel: "email".into(),
                situation_id: None,
                incoming_message: w.draft.incoming_message.clone(),
                incoming_message_id: w.draft.incoming_message_id.clone(),
                intent: None,
                generated_text: "sure".into(),
                provider: "mock".into(),
                model: "m".into(),
                context: json!({}),
                prompt_hash: "h".into(),
                evidence: json!({}),
                alternative_to: None,
            })
            .unwrap();
        w.db.conn()
            .execute("UPDATE drafts SET created_at = '2999-01-01T00:00:00.000Z' WHERE id = ?1", [&newer.id])
            .unwrap();
        assert_eq!(tick(&w.db, &credentials(), at(0)).unwrap().held, 0);
    }

    /// A reply sent by itself an hour before the tests' clock, in
    /// `conversation` or none of these.
    fn sent_by_itself_earlier(w: &World, conversation: Option<&str>) {
        let t = fmt_rfc3339(at(-60));
        w.db.conn()
            .execute(
                "INSERT INTO outbox(id, source_id, conversation_id, from_address, to_json, subject, body, message_id,
                                    how, status, send_after, created_at, updated_at, sent_at)
                 VALUES (?1, ?2, ?3, 'c@example.com', '[\"ada@example.com\"]', 'Re: Lunch', 'ok', ?4,
                         'autonomous', 'sent', ?5, ?5, ?5, ?5)",
                rusqlite::params![
                    crate::ids::new_id(),
                    w.mailbox,
                    conversation,
                    format!("<{}@x.example>", crate::ids::new_id()),
                    t
                ],
            )
            .unwrap();
    }

    #[test]
    fn the_daily_limit_holds_back_the_rest() {
        let w = world(Behaviour::default());
        set_rules(&w.db, Rules { daily_limit: 0, ..rules(&w.db).unwrap() }).unwrap();
        assert_eq!(tick(&w.db, &credentials(), at(0)).unwrap().held, 0);
        assert!(w.db.recent_outgoing(10).unwrap().is_empty());

        // One a day, and one already went today, elsewhere.
        let w = world(Behaviour::default());
        set_rules(&w.db, Rules { daily_limit: 1, ..rules(&w.db).unwrap() }).unwrap();
        sent_by_itself_earlier(&w, None);
        assert_eq!(tick(&w.db, &credentials(), at(0)).unwrap().held, 0);

        // None yet: held, and when it comes due its own row doesn't count.
        let w = world(Behaviour::default());
        set_rules(&w.db, Rules { daily_limit: 1, ..rules(&w.db).unwrap() }).unwrap();
        assert_eq!(tick(&w.db, &credentials(), at(0)).unwrap().held, 1);
        checked(&w);
        assert_eq!(tick(&w.db, &credentials(), at(DEFAULT_HOLD_MINUTES)).unwrap().sent, 1);
    }

    #[test]
    fn it_replies_by_itself_at_most_twice_in_a_conversation_a_day() {
        let w = world(Behaviour::default());
        let conversation = w.draft.conversation_id.clone().unwrap();
        sent_by_itself_earlier(&w, Some(&conversation));
        sent_by_itself_earlier(&w, Some(&conversation));
        assert_eq!(tick(&w.db, &credentials(), at(0)).unwrap().held, 0, "{LOOPING}");

        let w = world(Behaviour::default());
        let conversation = w.draft.conversation_id.clone().unwrap();
        sent_by_itself_earlier(&w, Some(&conversation));
        assert_eq!(tick(&w.db, &credentials(), at(0)).unwrap().held, 1, "once before is not a loop");
    }

    #[test]
    fn rehearsing_goes_through_everything_and_sends_nothing() {
        let w = world(Behaviour::default());
        set_rules(&w.db, Rules { rehearse: true, ..rules(&w.db).unwrap() }).unwrap();
        assert_eq!(tick(&w.db, &credentials(), at(0)).unwrap().held, 1, "held, as for real");
        let early = tick(&w.db, &credentials(), at(DEFAULT_HOLD_MINUTES)).unwrap();
        assert_eq!((early.rehearsed, early.waiting_for_check.len()), (0, 1), "and waits for a check, as for real");
        checked(&w);
        let run = tick(&w.db, &credentials(), at(DEFAULT_HOLD_MINUTES)).unwrap();
        assert_eq!((run.sent, run.rehearsed), (0, 1));
        assert!(run.changed_anything());
        let entry = only_entry(&w.db);
        assert_eq!((entry.status, entry.error.as_deref()), (SendStatus::Canceled, Some(REHEARSED)));
        assert!(w.seen.try_recv().is_err(), "nothing was said to the server");
        assert_eq!(w.db.get_draft(&w.draft.id).unwrap().unwrap().outcome, None, "its draft waits for the user");
        assert_eq!(tick(&w.db, &credentials(), at(30)).unwrap(), Run::default(), "and it isn't held again");

        // Held while rehearsing, then rehearsing turned off: it doesn't go
        // for real.
        let w = world(Behaviour::default());
        set_rules(&w.db, Rules { rehearse: true, ..rules(&w.db).unwrap() }).unwrap();
        tick(&w.db, &credentials(), at(0)).unwrap();
        set_rules(&w.db, Rules { rehearse: false, ..rules(&w.db).unwrap() }).unwrap();
        let entry = only_entry(&w.db);
        assert_eq!((entry.status, entry.error.as_deref()), (SendStatus::Canceled, Some(REHEARSAL_OVER)));
    }

    #[test]
    fn a_reply_held_a_day_still_goes_when_its_hold_ends() {
        let w = world(Behaviour::default());
        set_rules(&w.db, Rules { hold_minutes: MAX_HOLD_MINUTES, ..rules(&w.db).unwrap() }).unwrap();
        assert_eq!(tick(&w.db, &credentials(), at(0)).unwrap().held, 1);
        checked(&w);
        let run = tick(&w.db, &credentials(), at(MAX_HOLD_MINUTES)).unwrap();
        assert_eq!((run.sent, run.stopped), (1, 0), "read a day before it went, and not a backlog for that");
    }

    #[test]
    fn a_reply_that_looks_unfinished_is_left_for_the_user() {
        let theirs = "lunch at the place by the river? menu is at https://river.example/menu";
        assert_eq!(unfinished("yes, the river place. 12:30?", theirs), None);
        assert_eq!(unfinished("yes! see you at [time] :)", theirs), Some(UNFINISHED));
        assert_eq!(unfinished("thanks {name}", theirs), Some(UNFINISHED));
        assert_eq!(unfinished("send it to <your address>", theirs), Some(UNFINISHED));
        assert_eq!(unfinished("haha [1] and <3", theirs), None, "a number, a heart: no gap");
        assert_eq!(unfinished("As an AI, I can't eat lunch", theirs), Some(NOT_IN_YOUR_VOICE));
        assert_eq!(unfinished("looked at https://river.example/menu, looks good", theirs), None, "their own link");
        assert_eq!(unfinished("book here: www.tables.example.", theirs), Some(NEW_LINK));
        assert_eq!(unfinished(&"a".repeat(MOST_CHARS + 1), theirs), Some(TOO_LONG));
        // Not a gap, not the model, their link however written.
        assert_eq!(unfinished("ask Sam <sam@example.com>", theirs), None);
        assert_eq!(unfinished("menu's here <https://river.example/menu>", theirs), None);
        assert_eq!(unfinished("I’m an aide there, and my thai assistant too", theirs), None);
        assert_eq!(unfinished("I’m an AI", theirs), Some(NOT_IN_YOUR_VOICE), "a curly apostrophe");
        assert_eq!(unfinished("http://river.example/menu/ looks good", theirs), None);
        assert_eq!(unfinished("好的 https://river.example/menu。谢谢", theirs), None);

        let w = world(Behaviour::default());
        w.db.conn()
            .execute("UPDATE drafts SET generated_text = 'yes, see you at [time]' WHERE id = ?1", [&w.draft.id])
            .unwrap();
        assert_eq!(tick(&w.db, &credentials(), at(0)).unwrap().held, 0);
    }

    #[test]
    fn a_held_reply_can_go_now() {
        let w = world(Behaviour::default());
        tick(&w.db, &credentials(), at(0)).unwrap();
        let held = only_entry(&w.db);
        checked(&w);
        w.db.send_held_now(&held.id, &fmt_rfc3339(at(1))).unwrap();
        assert_eq!(tick(&w.db, &credentials(), at(1)).unwrap().sent, 1);
    }

    #[test]
    fn a_reply_by_itself_goes_signed_and_its_draft_is_decided_as_written() {
        let w = world(Behaviour::default());
        let mut config: ImapSourceConfig =
            serde_json::from_value(w.db.get_source(&w.mailbox).unwrap().unwrap().config).unwrap();
        config.signature = Some("C\n".into());
        w.db.set_source_config(&w.mailbox, &serde_json::to_value(&config).unwrap()).unwrap();
        tick(&w.db, &credentials(), at(0)).unwrap();
        let held = only_entry(&w.db);
        assert_eq!(held.body, "yes, the river place. 12:30?\n\n-- \nC", "held as it will go");
        checked(&w);
        w.db.send_held_now(&held.id, &fmt_rfc3339(at(1))).unwrap();
        assert_eq!(tick(&w.db, &credentials(), at(1)).unwrap().sent, 1);
        let draft = w.db.get_draft(&w.draft.id).unwrap().unwrap();
        assert_eq!(draft.outcome.as_deref(), Some("sent_unedited"), "the signature is the mailbox's, not an edit");
        assert_eq!(draft.final_text.as_deref(), Some("yes, the river place. 12:30?"));
    }

    #[test]
    fn the_hours_allowed_are_on_the_computers_clock_and_may_cross_midnight() {
        let utc = chrono::FixedOffset::east_opt(0).unwrap();
        let hst = chrono::FixedOffset::west_opt(10 * 3600).unwrap();
        let t = |s: &str| DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc);
        let day = Some(Hours { from: 8, to: 20 });
        assert_eq!(first_within(None, t("2026-10-26T03:00:00Z"), &utc), t("2026-10-26T03:00:00Z"));
        assert_eq!(first_within(day, t("2026-10-26T07:30:00Z"), &utc), t("2026-10-26T08:00:00Z"));
        assert_eq!(first_within(day, t("2026-10-26T12:10:00Z"), &utc), t("2026-10-26T12:10:00Z"));
        assert_eq!(first_within(day, t("2026-10-26T20:00:00Z"), &utc), t("2026-10-27T08:00:00Z"));
        assert_eq!(first_within(day, t("2026-10-26T17:00:00Z"), &hst), t("2026-10-26T18:00:00Z"), "7am in Hawaii");
        let night = Some(Hours { from: 22, to: 6 });
        assert_eq!(first_within(night, t("2026-10-26T12:00:00Z"), &utc), t("2026-10-26T22:00:00Z"));
        assert_eq!(first_within(night, t("2026-10-26T23:30:00Z"), &utc), t("2026-10-26T23:30:00Z"));
        assert_eq!(first_within(night, t("2026-10-27T05:59:00Z"), &utc), t("2026-10-27T05:59:00Z"));
    }

    #[test]
    fn a_reply_held_outside_the_hours_allowed_waits_for_them() {
        let w = world(Behaviour::default());
        // An hour that the hold's end is not in, on this computer's clock.
        let ends = (at(0) + Duration::minutes(DEFAULT_HOLD_MINUTES)).with_timezone(&Local).hour();
        let from = (ends + 2) % 24;
        set_rules(&w.db, Rules { hours: Some(Hours { from, to: (from + 1) % 24 }), ..rules(&w.db).unwrap() }).unwrap();
        assert_eq!(tick(&w.db, &credentials(), at(0)).unwrap().held, 1, "held as before");
        let held = only_entry(&w.db);
        let goes = DateTime::parse_from_rfc3339(&held.send_after).unwrap().with_timezone(&Utc);
        assert!(goes > at(DEFAULT_HOLD_MINUTES), "{goes}");
        assert_eq!(goes.with_timezone(&Local).hour(), from, "at the start of the hours allowed");
        checked(&w);
        assert_eq!(tick(&w.db, &credentials(), at(DEFAULT_HOLD_MINUTES)).unwrap().sent, 0, "not before then");

        // The same hour twice is no window at all.
        let r = set_rules(&w.db, Rules { hours: Some(Hours { from: 9, to: 9 }), ..rules(&w.db).unwrap() }).unwrap();
        assert_eq!(r.hours, None);
    }

    #[test]
    fn a_reply_already_held_waits_for_hours_set_since_unless_sent_now() {
        let w = world(Behaviour::default());
        tick(&w.db, &credentials(), at(0)).unwrap();
        let ends = at(DEFAULT_HOLD_MINUTES).with_timezone(&Local).hour();
        let from = (ends + 2) % 24;
        let hours = Some(Hours { from, to: (from + 1) % 24 });
        set_rules(&w.db, Rules { hours, ..rules(&w.db).unwrap() }).unwrap();
        checked(&w);
        assert_eq!(tick(&w.db, &credentials(), at(DEFAULT_HOLD_MINUTES)).unwrap().sent, 0);
        let held = only_entry(&w.db);
        assert_eq!(held.status, SendStatus::Held, "still held, not stopped");
        assert_eq!(held.send_after, fmt_rfc3339(first_within(hours, at(DEFAULT_HOLD_MINUTES), &Local)));

        w.db.send_held_now(&held.id, &fmt_rfc3339(at(DEFAULT_HOLD_MINUTES + 1))).unwrap();
        assert_eq!(tick(&w.db, &credentials(), at(DEFAULT_HOLD_MINUTES + 1)).unwrap().sent, 1, "the user said now");
    }

    #[test]
    fn rules_saved_by_hand_are_kept_within_bounds() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(rules(&db).unwrap(), Rules::default());
        assert!(!rules(&db).unwrap().enabled, "off until turned on");
        let saved = set_rules(&db, Rules { hold_minutes: 0, daily_limit: 1000, ..Rules::default() }).unwrap();
        assert_eq!((saved.hold_minutes, saved.daily_limit), (MIN_HOLD_MINUTES, MAX_DAILY_LIMIT));
        db.set_setting(SETTING, &json!({"enabled": true})).unwrap();
        let read = rules(&db).unwrap();
        assert!(read.enabled);
        assert_eq!(read.hold_minutes, DEFAULT_HOLD_MINUTES, "a field left out takes its default");
    }
}
