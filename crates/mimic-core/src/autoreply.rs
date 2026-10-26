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

use chrono::{DateTime, Duration, Utc};
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
pub const NOT_TO_YOU: &str =
    "It wasn't sent to you directly — a list, a copy you were hidden on, or I can't tell — so I don't answer it by myself.";
pub const SENT_FOLDER: &str = "It was in your Sent folder, so it may be yours; I don't answer it by myself.";
pub const NOT_THEIRS: &str =
    "A reply would go somewhere other than the address of the person who wrote, so I don't send it by myself.";

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
    pub hold_minutes: i64,
    pub daily_limit: i64,
    /// When it was last turned on: only messages that arrived after are
    /// answered by it, so turning it on doesn't answer a backlog. Set by
    /// `set_rules`, whatever was asked for.
    pub enabled_at: Option<String>,
}

impl Default for Rules {
    fn default() -> Self {
        Rules {
            enabled: false,
            mailboxes: Vec::new(),
            who: Who::Known,
            people: Vec::new(),
            hold_minutes: DEFAULT_HOLD_MINUTES,
            daily_limit: DEFAULT_DAILY_LIMIT,
            enabled_at: None,
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
    db.set_setting(SETTING, &rules)?;
    if !rules.enabled {
        db.stop_all_held_outgoing(TURNED_OFF)?;
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
    /// Held replies that didn't go, or may not have.
    pub failed: usize,
    /// Mailboxes with a reply whose hold has ended, waiting for the mailbox
    /// to be read again: a check of each is asked for, so it goes close to
    /// on time.
    #[serde(skip)]
    pub waiting_for_check: Vec<String>,
}

impl Run {
    pub fn changed_anything(&self) -> bool {
        self.held + self.sent + self.stopped + self.failed > 0
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

/// Whether the mailbox has been read, successfully, since `since`: an answer
/// the user sent from another program in the meantime is only known once it
/// has.
fn read_since(db: &Db, config: &ImapSourceConfig, source_id: &str, since: &str) -> Result<bool, DbError> {
    let settled = db.get_source(source_id)?.is_some_and(|s| s.status != "failed" && s.status != "importing");
    let checked = config.last_checked_at.as_deref().and_then(|t| DateTime::parse_from_rfc3339(t).ok());
    let held = DateTime::parse_from_rfc3339(since).ok();
    Ok(settled && matches!((checked, held), (Some(c), Some(h)) if c > h))
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
        let Ok((_, account)) = sending::sending_from(db, &incoming.source_id) else { continue };
        let envelope = sending::envelope_for(db, &draft.id)?;
        if envelope.why_not.is_some() || draft.generated_text.trim().is_empty() {
            continue;
        }
        if !goes_to_them(db, draft.participant_id.as_deref(), &envelope.to)? {
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
            body: draft.generated_text.clone(),
            message_id: outgoing::new_message_id(&account.from_address),
            how: SendHow::Autonomous,
            send_after: Some(fmt_rfc3339(now + Duration::minutes(rules.hold_minutes))),
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

/// Send every held reply whose time has come and which the rules still
/// allow; stop, with the reason, any they no longer do.
pub fn send_due(db: &Db, credentials: &Credentials, now: DateTime<Utc>) -> Result<Run, SendError> {
    let mut run = Run::default();
    let now_text = fmt_rfc3339(now);
    let due: Vec<_> = db.due_outgoing(&now_text)?.into_iter().filter(|e| e.how == SendHow::Autonomous).collect();
    if due.is_empty() {
        return Ok(run);
    }
    let rules = rules(db)?;
    let waiting = waiting_now(db)?;
    for entry in due {
        let Some(draft) = entry.draft_id.as_deref().map(|id| db.get_draft(id)).transpose()?.flatten() else {
            run.stopped += usize::from(db.stop_held_outgoing(&entry.id, GONE)?);
            continue;
        };
        if let Some(why) = refusal(db, &rules, &draft, &waiting, Some(&entry.id), false, now)? {
            run.stopped += usize::from(db.stop_held_outgoing(&entry.id, why)?);
            continue;
        }
        if !goes_to_them(db, draft.participant_id.as_deref(), &entry.to)? {
            run.stopped += usize::from(db.stop_held_outgoing(&entry.id, NOT_THEIRS)?);
            continue;
        }
        let (config, account) = match sending::sending_from(db, &entry.source_id) {
            Ok(found) => found,
            Err(SendError::Cannot(why)) => {
                run.stopped += usize::from(db.stop_held_outgoing(&entry.id, &why)?);
                continue;
            }
            Err(e) => return Err(e),
        };
        // It waits for the next check of the mailbox after it was held.
        if !read_since(db, &config, &entry.source_id, &entry.created_at)? {
            if !run.waiting_for_check.contains(&entry.source_id) {
                run.waiting_for_check.push(entry.source_id.clone());
            }
            continue;
        }
        // Held → sending, unless it was stopped or taken a moment ago.
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
                    db.mark_sent_on_its_own(&draft.id).and_then(|_| sending::record_sent_draft(db, &draft, &entry.body))
                {
                    tracing::warn!(target: "autoreply", error = %e, "a reply sent on its own could not be recorded on its draft");
                }
                if let Err(e) = sending::record_in_conversation(db, &entry.source_id, &bytes, true) {
                    tracing::warn!(target: "autoreply", error = %e, "a reply sent on its own could not be put in its conversation");
                }
            }
            // `deliver` recorded why, as failed or unsure.
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
    const QUESTION: &str = "From: Ada Lovelace <ada@example.com>\r\nTo: C <c@example.com>\r\nSubject: Re: Lunch\r\nDate: Mon, 2 Mar 2026 10:00:00 +0000\r\nMessage-ID: <l3@x.example>\r\nIn-Reply-To: <l2@x.example>\r\nReferences: <l1@x.example> <l2@x.example>\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nthe place by the river?\r\n";

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
            sending: Some(SmtpAccount {
                host: "127.0.0.1".into(),
                port,
                security: SmtpSecurity::Plain,
                username: "c@example.com".into(),
                from_address: "c@example.com".into(),
                copy_to_sent: Some(false),
            }),
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
        read_in(&db, &mailbox, QUESTION, false);
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

    /// The mailbox has been read since anything was held.
    fn checked(w: &World) {
        let mut config: ImapSourceConfig =
            serde_json::from_value(w.db.get_source(&w.mailbox).unwrap().unwrap().config).unwrap();
        config.last_checked_at = Some(fmt_rfc3339(Utc::now() + Duration::minutes(1)));
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
        let on = Rules { enabled_at: Some("2026-06-01T00:00:00.000Z".into()), ..rules(&w.db).unwrap() };
        w.db.set_setting(SETTING, &on).unwrap();
        assert_eq!(tick(&w.db, &credentials(), at(0)).unwrap().held, 0);
        // Turning it off and on again moves the start to now.
        set_rules(&w.db, Rules { enabled: false, ..rules(&w.db).unwrap() }).unwrap();
        assert_eq!(rules(&w.db).unwrap().enabled_at, None);
        let again = set_rules(&w.db, Rules { enabled: true, ..rules(&w.db).unwrap() }).unwrap();
        assert!(again.enabled_at.is_some_and(|t| t.as_str() > "2026-06-01"));
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

    #[test]
    fn the_daily_limit_holds_back_the_rest() {
        let w = world(Behaviour::default());
        set_rules(&w.db, Rules { daily_limit: 0, ..rules(&w.db).unwrap() }).unwrap();
        assert_eq!(tick(&w.db, &credentials(), at(0)).unwrap().held, 0);
        assert!(w.db.recent_outgoing(10).unwrap().is_empty());
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
