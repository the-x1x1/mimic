//! What Mimic sends, before it goes and after (migration 15, `outbox`).
//!
//! A row is written before the mail server is spoken to and moved on by what
//! happened: `sending` → `sent`, `failed` or `unsure`; `held` → `sending`, or
//! `canceled`; `unsure` → `failed` only when the user says it didn't go.
//! Nothing here sends anything; `crate::sending` does, and keeps these rows
//! in step with it.

use rusqlite::{params, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

use super::{Db, DbError, DbResult, ALREADY_DECIDED};
use crate::ids::{new_id, now_rfc3339};

/// Why a reply is not sent again: it is being sent, is waiting to be, or was.
pub const ALREADY_SENDING: &str = "That reply is already on its way, or has gone, so it wasn't sent again.";

/// Said of a reply the user said didn't go, after it may have.
pub const DID_NOT_GO: &str = "You looked in your Sent folder, and it didn't go.";

/// Said of a send that was interrupted while it was being handed over.
pub const INTERRUPTED: &str =
    "Mimic closed while this was being sent, so it may or may not have gone. Look in your Sent folder before sending it again.";

/// Why a reply isn't offered to send again: one may already have gone.
pub const MAY_HAVE_GONE: &str = "A reply to this may already have gone: the connection was lost as it was handed over. Look in your Sent folder. If it isn't there, say so under Your mail → What I've sent, and you can send it again.";

/// Why a reply can't be said not to have gone: it isn't one that may have.
pub const NOT_UNSURE: &str = "I only ask about a reply that may or may not have gone, and that one isn't.";

/// Why a reply can't be stopped: it has already gone, or is going now.
pub const TOO_LATE: &str = "That reply isn't waiting any more, so it couldn't be stopped.";

/// Who decided it should go.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SendHow {
    /// The user pressed Send.
    Approved,
    /// Mimic, under the rules the user set.
    Autonomous,
}

impl SendHow {
    pub fn as_str(self) -> &'static str {
        match self {
            SendHow::Approved => "approved",
            SendHow::Autonomous => "autonomous",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SendStatus {
    /// Waiting for its time, and can still be canceled.
    Held,
    /// Being handed to the mail server now.
    Sending,
    Sent,
    /// It may or may not have gone; the user is asked to look.
    Unsure,
    Failed,
    Canceled,
}

impl SendStatus {
    fn parse(s: &str) -> SendStatus {
        match s {
            "held" => SendStatus::Held,
            "sending" => SendStatus::Sending,
            "sent" => SendStatus::Sent,
            "unsure" => SendStatus::Unsure,
            "canceled" => SendStatus::Canceled,
            _ => SendStatus::Failed,
        }
    }
}

/// One reply Mimic sent, is sending or holds to send.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutboxEntry {
    pub id: String,
    pub draft_id: Option<String>,
    /// The mailbox it goes out through.
    pub source_id: String,
    pub conversation_id: Option<String>,
    /// The message it answers.
    pub answers_message_id: Option<String>,
    pub participant_id: Option<String>,
    pub from_address: String,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub subject: String,
    pub body: String,
    /// The `Message-ID` it carries, without angle brackets.
    pub message_id: String,
    pub how: SendHow,
    pub status: SendStatus,
    /// When it may go: now, for an approved reply.
    pub send_after: String,
    /// Why it failed, in the words the user is shown.
    pub error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub sent_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewOutgoing {
    pub draft_id: Option<String>,
    pub source_id: String,
    pub conversation_id: Option<String>,
    pub answers_message_id: Option<String>,
    pub participant_id: Option<String>,
    pub from_address: String,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub subject: String,
    pub body: String,
    pub message_id: String,
    pub how: SendHow,
    /// When it may go; `None` is now.
    pub send_after: Option<String>,
}

const COLS: &str = "id, draft_id, source_id, conversation_id, answers_message_id, participant_id, from_address, to_json, cc_json, subject, body, message_id, how, status, send_after, error, created_at, updated_at, sent_at";

fn list(raw: String) -> Vec<String> {
    serde_json::from_str(&raw).unwrap_or_default()
}

fn map(r: &Row<'_>) -> rusqlite::Result<OutboxEntry> {
    Ok(OutboxEntry {
        id: r.get(0)?,
        draft_id: r.get(1)?,
        source_id: r.get(2)?,
        conversation_id: r.get(3)?,
        answers_message_id: r.get(4)?,
        participant_id: r.get(5)?,
        from_address: r.get(6)?,
        to: list(r.get(7)?),
        cc: list(r.get(8)?),
        subject: r.get(9)?,
        body: r.get(10)?,
        message_id: r.get(11)?,
        how: if r.get::<_, String>(12)? == "autonomous" { SendHow::Autonomous } else { SendHow::Approved },
        status: SendStatus::parse(&r.get::<_, String>(13)?),
        send_after: r.get(14)?,
        error: r.get(15)?,
        created_at: r.get(16)?,
        updated_at: r.get(17)?,
        sent_at: r.get(18)?,
    })
}

impl Db {
    /// Record a reply before it goes: `sending` when the user approved it,
    /// `held` until `send_after` when Mimic is to send it on its own. In one
    /// transaction, refused when its draft was already used or put aside
    /// (`ALREADY_DECIDED`), when any draft of its set has a reply that may
    /// already have gone (`MAY_HAVE_GONE`), or one held, sending or sent
    /// (`ALREADY_SENDING`) — a second window's Send, or a second way of
    /// saying it, sends nothing.
    pub fn queue_outgoing(&self, new: &NewOutgoing) -> DbResult<OutboxEntry> {
        let id = new_id();
        let now = now_rfc3339();
        let status = match new.how {
            SendHow::Approved => "sending",
            SendHow::Autonomous => "held",
        };
        self.transaction(|tx| {
            if let Some(draft) = &new.draft_id {
                let undecided: Option<bool> = tx
                    .query_row("SELECT outcome IS NULL FROM drafts WHERE id = ?1", [draft], |r| r.get(0))
                    .optional()?;
                match undecided {
                    None => return Err(DbError::NotFound(draft.clone())),
                    Some(false) => return Err(DbError::Invalid(ALREADY_DECIDED.into())),
                    Some(true) => {}
                }
                let live: Option<String> = tx
                    .query_row(
                        "SELECT o.status FROM outbox o JOIN drafts d ON d.id = o.draft_id
                         WHERE o.status IN ('held','sending','sent','unsure')
                           AND COALESCE(d.alternative_to, d.id) =
                               (SELECT COALESCE(alternative_to, id) FROM drafts WHERE id = ?1)
                         ORDER BY o.status = 'unsure' DESC LIMIT 1",
                        [draft],
                        |r| r.get(0),
                    )
                    .optional()?;
                match live.as_deref() {
                    None => {}
                    Some("unsure") => return Err(DbError::Invalid(MAY_HAVE_GONE.into())),
                    Some(_) => return Err(DbError::Invalid(ALREADY_SENDING.into())),
                }
            }
            tx.execute(
                &format!(
                    "INSERT INTO outbox({COLS})
                     VALUES (?1,?2,?3,?4,?5,
                             (SELECT id FROM participants WHERE id = ?6),
                             ?7,?8,?9,?10,?11,?12,?13,?14,?15,NULL,?16,?16,NULL)"
                ),
                params![
                    id,
                    new.draft_id,
                    new.source_id,
                    new.conversation_id,
                    new.answers_message_id,
                    new.participant_id,
                    new.from_address,
                    serde_json::to_string(&new.to)?,
                    serde_json::to_string(&new.cc)?,
                    new.subject,
                    new.body,
                    new.message_id,
                    new.how.as_str(),
                    status,
                    new.send_after.clone().unwrap_or_else(|| now.clone()),
                    now,
                ],
            )?;
            Ok(())
        })?;
        self.outgoing(&id)?.ok_or(DbError::NotFound(id))
    }

    pub fn outgoing(&self, id: &str) -> DbResult<Option<OutboxEntry>> {
        Ok(self.conn().query_row(&format!("SELECT {COLS} FROM outbox WHERE id = ?1"), [id], map).optional()?)
    }

    /// What Mimic sent or holds to send, newest first.
    pub fn recent_outgoing(&self, limit: usize) -> DbResult<Vec<OutboxEntry>> {
        let conn = self.conn();
        let mut stmt =
            conn.prepare(&format!("SELECT {COLS} FROM outbox ORDER BY created_at DESC, id DESC LIMIT ?1"))?;
        let rows = stmt.query_map([limit as i64], map)?;
        rows.map(|r| r.map_err(DbError::from)).collect()
    }

    /// It went.
    pub fn outgoing_sent(&self, id: &str) -> DbResult<()> {
        let now = now_rfc3339();
        self.conn().execute(
            "UPDATE outbox SET status = 'sent', sent_at = ?2, updated_at = ?2, error = NULL
             WHERE id = ?1 AND status = 'sending'",
            params![id, now],
        )?;
        Ok(())
    }

    /// It didn't go, or may not have: `why` is what the user is told.
    pub fn outgoing_failed(&self, id: &str, why: &str) -> DbResult<()> {
        self.conn().execute(
            "UPDATE outbox SET status = 'failed', error = ?2, updated_at = ?3
             WHERE id = ?1 AND status IN ('held','sending')",
            params![id, why, now_rfc3339()],
        )?;
        Ok(())
    }

    /// It may or may not have gone (`why` says how it came to be unknown):
    /// nothing sends its draft again until the user says it didn't go.
    pub fn outgoing_unsure(&self, id: &str, why: &str) -> DbResult<()> {
        self.conn().execute(
            "UPDATE outbox SET status = 'unsure', error = ?2, updated_at = ?3 WHERE id = ?1 AND status = 'sending'",
            params![id, why, now_rfc3339()],
        )?;
        Ok(())
    }

    /// The user looked, and it didn't go: `unsure` → `failed`, so its draft
    /// can be sent again. Refused for a reply that isn't unsure (`NOT_UNSURE`).
    pub fn outgoing_did_not_go(&self, id: &str) -> DbResult<OutboxEntry> {
        let n = self.conn().execute(
            "UPDATE outbox SET status = 'failed', error = ?2, updated_at = ?3 WHERE id = ?1 AND status = 'unsure'",
            params![id, DID_NOT_GO, now_rfc3339()],
        )?;
        let entry = self.outgoing(id)?.ok_or_else(|| DbError::NotFound(id.into()))?;
        if n == 0 {
            return Err(DbError::Invalid(NOT_UNSURE.into()));
        }
        Ok(entry)
    }

    /// A reply to any draft of this draft's set that may already have gone.
    pub fn unsure_outgoing_for_draft(&self, draft_id: &str) -> DbResult<Option<OutboxEntry>> {
        Ok(self
            .conn()
            .query_row(
                &format!(
                    "SELECT {} FROM outbox o JOIN drafts d ON d.id = o.draft_id
                     WHERE o.status = 'unsure'
                       AND COALESCE(d.alternative_to, d.id) =
                           (SELECT COALESCE(alternative_to, id) FROM drafts WHERE id = ?1)
                     ORDER BY o.created_at DESC LIMIT 1",
                    COLS.split(", ").map(|c| format!("o.{c}")).collect::<Vec<_>>().join(", ")
                ),
                [draft_id],
                map,
            )
            .optional()?)
    }

    /// Stop a held reply before it goes. Refused once it is being sent or
    /// has gone (`TOO_LATE`).
    pub fn cancel_outgoing(&self, id: &str) -> DbResult<OutboxEntry> {
        let n = self.conn().execute(
            "UPDATE outbox SET status = 'canceled', updated_at = ?2 WHERE id = ?1 AND status = 'held'",
            params![id, now_rfc3339()],
        )?;
        let entry = self.outgoing(id)?.ok_or_else(|| DbError::NotFound(id.into()))?;
        if n == 0 && entry.status != SendStatus::Canceled {
            return Err(DbError::Invalid(TOO_LATE.into()));
        }
        Ok(entry)
    }

    /// Take a held reply whose time has come to send it: `held` → `sending`,
    /// only if it is still held, so a cancel that lands first wins and two
    /// takers never both send it.
    pub fn take_held_outgoing(&self, id: &str, now: &str) -> DbResult<bool> {
        let n = self.conn().execute(
            "UPDATE outbox SET status = 'sending', updated_at = ?2
             WHERE id = ?1 AND status = 'held' AND send_after <= ?2",
            params![id, now],
        )?;
        Ok(n == 1)
    }

    /// Held replies whose time has come, oldest first.
    pub fn due_outgoing(&self, now: &str) -> DbResult<Vec<OutboxEntry>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLS} FROM outbox WHERE status = 'held' AND send_after <= ?1 ORDER BY send_after, id"
        ))?;
        let rows = stmt.query_map([now], map)?;
        rows.map(|r| r.map_err(DbError::from)).collect()
    }

    /// At start-up: a reply left `sending` by a Mimic that stopped mid-send
    /// may or may not have gone. It is marked `unsure`, saying so, and is
    /// never sent again on its own. Returns how many.
    pub fn recover_outbox(&self) -> DbResult<usize> {
        Ok(self.conn().execute(
            "UPDATE outbox SET status = 'unsure', error = ?1, updated_at = ?2 WHERE status = 'sending'",
            params![INTERRUPTED, now_rfc3339()],
        )?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{NewDraft, NewSource};
    use serde_json::json;

    fn db_with_mailbox() -> (Db, String) {
        let db = Db::open_in_memory().unwrap();
        let source = db
            .create_source(&NewSource {
                connector: "imap".into(),
                name: "c@example.com".into(),
                channel: "email".into(),
                location: None,
                config: json!({}),
            })
            .unwrap();
        (db, source.id)
    }

    fn draft(db: &Db, alternative_to: Option<String>) -> String {
        db.create_draft(&NewDraft {
            participant_id: None,
            conversation_id: None,
            channel: "email".into(),
            situation_id: None,
            incoming_message: Some("can you make friday?".into()),
            incoming_message_id: None,
            intent: None,
            generated_text: "Yes, see you then.".into(),
            provider: "mock".into(),
            model: "m".into(),
            context: json!({}),
            prompt_hash: "h".into(),
            evidence: json!({}),
            alternative_to,
        })
        .unwrap()
        .id
    }

    fn reply(source: &str, draft: Option<String>, how: SendHow) -> NewOutgoing {
        NewOutgoing {
            draft_id: draft,
            source_id: source.into(),
            conversation_id: None,
            answers_message_id: None,
            participant_id: Some("someone-gone".into()),
            from_address: "c@example.com".into(),
            to: vec!["ada@example.com".into()],
            cc: vec![],
            subject: "Re: Friday".into(),
            body: "Yes, see you then.".into(),
            message_id: "m1@example.com".into(),
            how,
            send_after: None,
        }
    }

    #[test]
    fn a_draft_is_sent_once_whichever_way_of_saying_it_is_chosen() {
        let (db, source) = db_with_mailbox();
        let first = draft(&db, None);
        let other = draft(&db, Some(first.clone()));
        let sending = db.queue_outgoing(&reply(&source, Some(first.clone()), SendHow::Approved)).unwrap();
        assert_eq!(sending.status, SendStatus::Sending);
        assert_eq!(sending.participant_id, None, "addressed to someone gone, it is addressed to nobody");
        assert_eq!(sending.to, vec!["ada@example.com".to_string()]);

        let again = db.queue_outgoing(&reply(&source, Some(first.clone()), SendHow::Approved));
        assert!(matches!(again, Err(DbError::Invalid(ref m)) if m == ALREADY_SENDING));
        let beside = db.queue_outgoing(&reply(&source, Some(other.clone()), SendHow::Approved));
        assert!(
            matches!(beside, Err(DbError::Invalid(ref m)) if m == ALREADY_SENDING),
            "another way of the same reply"
        );

        // A send that failed can be tried again.
        db.outgoing_failed(&sending.id, "The mail server said no").unwrap();
        let retry = db.queue_outgoing(&reply(&source, Some(first.clone()), SendHow::Approved)).unwrap();
        db.outgoing_sent(&retry.id).unwrap();
        let sent = db.outgoing(&retry.id).unwrap().unwrap();
        assert_eq!(sent.status, SendStatus::Sent);
        assert!(sent.sent_at.is_some());

        // A draft already decided isn't sent.
        db.resolve_draft(&first, "sent_unedited", Some("Yes, see you then.")).unwrap();
        let decided = db.queue_outgoing(&reply(&source, Some(first), SendHow::Approved));
        assert!(matches!(decided, Err(DbError::Invalid(ref m)) if m == ALREADY_DECIDED));
        assert_eq!(db.recent_outgoing(10).unwrap().len(), 2);
    }

    #[test]
    fn a_held_reply_can_be_stopped_until_it_is_taken_and_only_one_taker_gets_it() {
        let (db, source) = db_with_mailbox();
        let later = NewOutgoing {
            send_after: Some("2026-10-25T22:10:00.000Z".into()),
            ..reply(&source, None, SendHow::Autonomous)
        };
        let held = db.queue_outgoing(&later).unwrap();
        assert_eq!(held.status, SendStatus::Held);
        assert!(db.due_outgoing("2026-10-25T22:05:00.000Z").unwrap().is_empty(), "not yet");
        assert!(!db.take_held_outgoing(&held.id, "2026-10-25T22:05:00.000Z").unwrap(), "not before its time");
        assert_eq!(db.due_outgoing("2026-10-25T22:10:00.000Z").unwrap().len(), 1);
        assert!(db.take_held_outgoing(&held.id, "2026-10-25T22:10:00.000Z").unwrap());
        assert!(!db.take_held_outgoing(&held.id, "2026-10-25T22:10:00.000Z").unwrap(), "taken once");
        let late = db.cancel_outgoing(&held.id);
        assert!(matches!(late, Err(DbError::Invalid(ref m)) if m == TOO_LATE));

        let other = db.queue_outgoing(&later).unwrap();
        assert_eq!(db.cancel_outgoing(&other.id).unwrap().status, SendStatus::Canceled);
        assert!(!db.take_held_outgoing(&other.id, "2026-10-25T23:00:00.000Z").unwrap(), "a canceled reply never goes");
        assert_eq!(db.cancel_outgoing(&other.id).unwrap().status, SendStatus::Canceled, "canceling twice is fine");
    }

    #[test]
    fn a_reply_that_may_have_gone_is_not_sent_again_until_the_user_says_it_didnt() {
        let (db, source) = db_with_mailbox();
        let first = draft(&db, None);
        let other = draft(&db, Some(first.clone()));
        let sending = db.queue_outgoing(&reply(&source, Some(first.clone()), SendHow::Approved)).unwrap();
        // Mimic stopped while it was being handed over.
        assert_eq!(db.recover_outbox().unwrap(), 1);
        let after = db.outgoing(&sending.id).unwrap().unwrap();
        assert_eq!(after.status, SendStatus::Unsure);
        assert_eq!(after.error.as_deref(), Some(INTERRUPTED));
        assert!(db.due_outgoing("9999").unwrap().is_empty());
        assert_eq!(db.unsure_outgoing_for_draft(&other).unwrap().map(|e| e.id), Some(sending.id.clone()));

        for d in [&first, &other] {
            let again = db.queue_outgoing(&reply(&source, Some(d.clone()), SendHow::Approved));
            assert!(matches!(again, Err(DbError::Invalid(ref m)) if m == MAY_HAVE_GONE), "{again:?}");
        }
        let not_unsure = db.queue_outgoing(&reply(&source, None, SendHow::Approved)).unwrap();
        assert!(matches!(db.outgoing_did_not_go(&not_unsure.id), Err(DbError::Invalid(ref m)) if m == NOT_UNSURE));

        // The user looked: it didn't go.
        assert_eq!(db.outgoing_did_not_go(&sending.id).unwrap().status, SendStatus::Failed);
        assert_eq!(db.unsure_outgoing_for_draft(&first).unwrap(), None);
        db.queue_outgoing(&reply(&source, Some(first), SendHow::Approved)).unwrap();
    }

    #[test]
    fn what_was_sent_goes_with_its_mailbox() {
        let (db, source) = db_with_mailbox();
        db.queue_outgoing(&reply(&source, None, SendHow::Approved)).unwrap();
        db.delete_source_and_contents(&source).unwrap();
        assert!(db.recent_outgoing(10).unwrap().is_empty());
    }
}
