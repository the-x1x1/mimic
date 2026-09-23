//! What is waiting on the user, and what was left out of it.
//!
//! A thread is decided by its **deciding message**, chosen so that nothing
//! from a person is hidden behind a machine and nothing from a machine is
//! lost:
//!
//! * If the last message that does not look automated came from someone
//!   else, it decides — so an out-of-office reply or a read receipt threaded
//!   in after a colleague's question does not hide the question.
//! * Otherwise, the last message decides. After the user's own reply that is
//!   either the reply itself (answered), or something automated that came in
//!   since — which is then left out and counted, never dropped; and a thread
//!   of nothing but automated mail is judged by its newest issue.
//!
//! The thread is *unanswered* when its deciding message came from someone
//! else. Whether an unanswered thread *needs a reply* is decided in this
//! order:
//!
//! 1. What the user said about it (`thread_marks`), while the message they
//!    said it about is still the deciding message.
//! 2. Otherwise, whether the deciding message looks automated from its
//!    headers (`metadata.automated`, written by `sources::automated` at
//!    import) — which it can only when the whole thread does.
//! 3. Otherwise it needs a reply.
//!
//! Everything that asks "what is waiting" — the home screen, its counts, and
//! assisted drafting — goes through the one definition below, so the list the
//! user sees and the list Mimic drafts for cannot drift apart.

use rusqlite::{params, OptionalExtension};
use serde::Serialize;

use super::repo_messages::{map_conv, CONV_COLS_Q};
use super::{Conversation, Db, DbError, DbResult};
use crate::ids::now_rfc3339;

/// An unanswered conversation, with the message it is waiting on.
#[derive(Debug, Clone)]
pub struct AwaitingReply {
    pub conversation: Conversation,
    pub last_message_id: String,
    pub last_message: String,
    pub last_message_at: Option<String>,
    /// Who sent it. `None` when the import could not attribute the message to
    /// a participant; the thread still shows, unattributed.
    pub participant_id: Option<String>,
    /// Why the last message looks automated, if it does (`Automated::as_str`).
    pub automated: Option<String>,
    /// What the user said about this thread, if it still applies.
    pub mark: Option<ThreadMark>,
}

/// How many unanswered threads were left out of what is waiting, and why.
/// Both are counts of rows, never estimates.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LeftOut {
    /// Left out because the last message looks automated, and the user has
    /// not said otherwise.
    pub automated: i64,
    /// Left out because the user said it does not need a reply.
    pub not_needed: i64,
}

/// What the user said about whether a thread needs a reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ThreadMark {
    NoReplyNeeded,
    NeedsReply,
}

impl ThreadMark {
    pub fn as_str(self) -> &'static str {
        match self {
            ThreadMark::NoReplyNeeded => "no_reply_needed",
            ThreadMark::NeedsReply => "needs_reply",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "no_reply_needed" => Some(ThreadMark::NoReplyNeeded),
            "needs_reply" => Some(ThreadMark::NeedsReply),
            _ => None,
        }
    }
}

/// Why a message (the `messages` row aliased `alias`) looks automated, from
/// its `metadata_json`, or NULL. Text only: anything else under that key is
/// not a reading this code wrote. The `instr` keeps the JSON parse off the
/// great majority of rows, which have no reading at all. Shared with the
/// People list, so a sender is automated by the same reading as a thread.
pub(super) fn automated_of(alias: &str) -> String {
    format!(
        "CASE WHEN instr({alias}.metadata_json, '\"automated\"') > 0
                   AND json_type({alias}.metadata_json, '$.automated') = 'text'
              THEN json_extract({alias}.metadata_json, '$.automated') END"
    )
}

/// The deciding message of every thread (see the module comment), and why it
/// looks automated. Messages whose direction could not be established
/// (`unknown`) never decide anything, since guessing either way would put a
/// thread in front of the user on no evidence. Only ids and flags go through
/// the window, so its sort does not drag every body along with it.
/// `conversation_filter` narrows it to one thread (`?1`) for `mark_thread`.
fn deciding_cte(conversation_filter: bool) -> String {
    let one = if conversation_filter { "AND m.conversation_id = ?1" } else { "" };
    let automated = automated_of("m");
    format!(
        "WITH msgs AS (
           SELECT m.conversation_id, m.id, m.direction, m.sequence_index, {automated} AS automated
           FROM messages m
           WHERE m.direction IN ('self','other') {one}
         ),
         ranked AS (
           SELECT conversation_id, id AS message_id, direction, automated,
                  ROW_NUMBER() OVER (
                    PARTITION BY conversation_id
                    ORDER BY automated IS NULL DESC, sequence_index DESC, id DESC
                  ) AS by_person,
                  ROW_NUMBER() OVER (
                    PARTITION BY conversation_id
                    ORDER BY sequence_index DESC, id DESC
                  ) AS by_time
           FROM msgs
         ),
         person AS (SELECT * FROM ranked WHERE by_person = 1),
         newest AS (SELECT * FROM ranked WHERE by_time = 1),
         deciding AS (
           SELECT p.conversation_id,
                  CASE WHEN p.automated IS NULL AND p.direction = 'other' THEN p.message_id ELSE n.message_id END AS message_id,
                  CASE WHEN p.automated IS NULL AND p.direction = 'other' THEN p.direction ELSE n.direction END AS direction,
                  CASE WHEN p.automated IS NULL AND p.direction = 'other' THEN NULL ELSE n.automated END AS automated
           FROM person p
           JOIN newest n ON n.conversation_id = p.conversation_id
         )"
    )
}

/// Every unanswered thread: its deciding message came from someone else.
/// Carries the user's mark when it is about that same message.
fn unanswered_cte() -> String {
    format!(
        "{deciding},
         unanswered AS (
           SELECT d.conversation_id, d.message_id, d.automated, tm.mark AS mark
           FROM deciding d
           LEFT JOIN thread_marks tm
             ON tm.conversation_id = d.conversation_id AND tm.message_id = d.message_id
           WHERE d.direction = 'other'
         )",
        deciding = deciding_cte(false)
    )
}

/// The user's word first, then the headers.
const NEEDS_REPLY: &str = "(u.mark = 'needs_reply' OR (u.mark IS NULL AND u.automated IS NULL))";
const LEFT_OUT: &str = "(u.mark = 'no_reply_needed' OR (u.mark IS NULL AND u.automated IS NOT NULL))";

impl Db {
    /// Threads that need a reply, newest first.
    pub fn threads_awaiting_reply(&self, limit: usize) -> DbResult<Vec<AwaitingReply>> {
        self.unanswered_where(NEEDS_REPLY, limit)
    }

    /// Unanswered threads that were left out of what is waiting, newest
    /// first: the ones that look automated and the ones the user took off.
    pub fn threads_left_out(&self, limit: usize) -> DbResult<Vec<AwaitingReply>> {
        self.unanswered_where(LEFT_OUT, limit)
    }

    fn unanswered_where(&self, filter: &str, limit: usize) -> DbResult<Vec<AwaitingReply>> {
        let conn = self.conn();
        let sql = format!(
            "{cte}
             SELECT {CONV_COLS_Q}, u.message_id, m.body, m.sent_at, m.participant_id, u.automated, u.mark
             FROM unanswered u
             JOIN messages m ON m.id = u.message_id
             JOIN conversations c ON c.id = u.conversation_id
             WHERE {filter}
             ORDER BY m.sent_at DESC NULLS LAST, c.id
             LIMIT {limit}",
            cte = unanswered_cte()
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], |r| {
            Ok(AwaitingReply {
                conversation: map_conv(r)?,
                last_message_id: r.get(10)?,
                last_message: r.get(11)?,
                last_message_at: r.get(12)?,
                participant_id: r.get(13)?,
                automated: r.get(14)?,
                mark: r.get::<_, Option<String>>(15)?.as_deref().and_then(ThreadMark::parse),
            })
        })?;
        rows.map(|r| r.map_err(DbError::from)).collect()
    }

    /// How many threads need a reply, and how many unanswered ones were left
    /// out and why — in one pass, because the home screen asks for all three
    /// every time it loads.
    pub fn waiting_counts(&self) -> DbResult<(i64, LeftOut)> {
        Ok(self.conn().query_row(
            &format!(
                "{cte}
                 SELECT COALESCE(SUM({NEEDS_REPLY}), 0),
                        COALESCE(SUM(u.mark IS NULL AND u.automated IS NOT NULL), 0),
                        COALESCE(SUM(u.mark = 'no_reply_needed'), 0)
                 FROM unanswered u",
                cte = unanswered_cte()
            ),
            [],
            |r| Ok((r.get(0)?, LeftOut { automated: r.get(1)?, not_needed: r.get(2)? })),
        )?)
    }

    /// How many threads need a reply, however many the caller asked to see.
    pub fn count_threads_awaiting_reply(&self) -> DbResult<i64> {
        Ok(self.waiting_counts()?.0)
    }

    /// How many unanswered threads were left out, and why.
    pub fn count_left_out(&self) -> DbResult<LeftOut> {
        Ok(self.waiting_counts()?.1)
    }

    /// Record what the user said about a thread, against the message they
    /// were looking at; `None` takes back whatever they said. Returns whether
    /// the mark applies now — `false` when someone has written since, in which
    /// case it is kept but changes nothing, and the thread stays where the new
    /// message puts it.
    pub fn mark_thread(&self, conversation_id: &str, message_id: &str, mark: Option<ThreadMark>) -> DbResult<bool> {
        let conn = self.conn();
        let belongs: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM messages WHERE id = ?1 AND conversation_id = ?2)",
            params![message_id, conversation_id],
            |r| r.get(0),
        )?;
        if !belongs {
            return Err(DbError::NotFound(format!("message {message_id} in conversation {conversation_id}")));
        }
        let Some(mark) = mark else {
            conn.execute("DELETE FROM thread_marks WHERE conversation_id = ?1", [conversation_id])?;
            return Ok(true);
        };
        conn.execute(
            "INSERT INTO thread_marks(conversation_id, message_id, mark, marked_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(conversation_id) DO UPDATE SET
               message_id = excluded.message_id, mark = excluded.mark, marked_at = excluded.marked_at",
            params![conversation_id, message_id, mark.as_str(), now_rfc3339()],
        )?;
        let deciding: Option<String> = conn
            .query_row(
                &format!("{cte} SELECT message_id FROM deciding", cte = deciding_cte(true)),
                [conversation_id],
                |r| r.get(0),
            )
            .optional()?;
        Ok(deciding.as_deref() == Some(message_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::repo_people::IdentifierInput;
    use crate::db::{IdentifierKind, NewMessage, NewSource};
    use serde_json::{json, Value};

    struct World {
        db: Db,
        source: String,
    }

    fn world() -> World {
        let db = Db::open_in_memory().unwrap();
        let source = db
            .create_source(&NewSource {
                connector: "test".into(),
                name: "T".into(),
                channel: "email".into(),
                location: None,
                config: Value::Null,
            })
            .unwrap()
            .id;
        World { db, source }
    }

    impl World {
        /// A thread whose messages are `(direction, body, automated)` in order.
        fn thread(&self, key: &str, messages: &[(&str, &str, Option<&str>)]) -> String {
            let convo = self.db.upsert_conversation(&self.source, key, "email", Some(key)).unwrap();
            let who = self
                .db
                .resolve_participant(
                    key,
                    &[IdentifierInput::new(IdentifierKind::Email, format!("{key}@example.com"))],
                    false,
                )
                .unwrap();
            let batch: Vec<NewMessage> = messages
                .iter()
                .enumerate()
                .map(|(i, (dir, body, automated))| NewMessage {
                    conversation_id: convo.clone(),
                    source_id: self.source.clone(),
                    participant_id: (*dir == "other").then(|| who.clone()),
                    external_id: format!("{key}-{i}"),
                    direction: (*dir).into(),
                    channel: "email".into(),
                    sent_at: Some(format!("2026-09-{:02}T10:00:00Z", i + 1)),
                    sequence_index: i as i64,
                    body: (*body).into(),
                    reply_to_external_id: None,
                    metadata: automated.map(|a| json!({ "automated": a })).unwrap_or(Value::Null),
                })
                .collect();
            self.db.insert_messages(&batch).unwrap();
            self.db.refresh_conversation_stats(&convo).unwrap();
            convo
        }

        fn add(&self, convo: &str, key: &str, seq: i64, dir: &str, body: &str) {
            self.db
                .insert_messages(&[NewMessage {
                    conversation_id: convo.into(),
                    source_id: self.source.clone(),
                    participant_id: None,
                    external_id: format!("{key}-{seq}"),
                    direction: dir.into(),
                    channel: "email".into(),
                    sent_at: Some(format!("2026-09-{:02}T10:00:00Z", seq + 1)),
                    sequence_index: seq,
                    body: body.into(),
                    reply_to_external_id: None,
                    metadata: Value::Null,
                }])
                .unwrap();
        }

        fn waiting(&self) -> Vec<String> {
            self.db.threads_awaiting_reply(50).unwrap().into_iter().map(|t| t.last_message).collect()
        }

        /// The message a thread is waiting on, as the screen would send it.
        fn shown(&self, convo: &str) -> String {
            self.db
                .threads_awaiting_reply(50)
                .unwrap()
                .into_iter()
                .chain(self.db.threads_left_out(50).unwrap())
                .find(|t| t.conversation.id == convo)
                .map(|t| t.last_message_id)
                .expect("the thread is unanswered")
        }

        fn mark(&self, convo: &str, mark: Option<ThreadMark>) -> bool {
            let shown = self.shown(convo);
            self.db.mark_thread(convo, &shown, mark).unwrap()
        }
    }

    #[test]
    fn automated_mail_is_left_out_and_counted_and_people_are_not() {
        let w = world();
        w.thread("ada", &[("other", "lunch on thursday?", None)]);
        w.thread("brand", &[("other", "twenty percent off", Some("newsletter"))]);
        w.thread("bank", &[("other", "your statement is ready", Some("no_reply_address"))]);

        assert_eq!(w.waiting(), ["lunch on thursday?"]);
        assert_eq!(w.db.waiting_counts().unwrap(), (1, LeftOut { automated: 2, not_needed: 0 }));

        let left: Vec<_> = w.db.threads_left_out(50).unwrap();
        assert_eq!(left.len(), 2);
        assert!(left.iter().all(|t| t.automated.is_some() && t.mark.is_none()));
    }

    #[test]
    fn an_automatic_reply_after_a_question_does_not_hide_the_question() {
        let w = world();
        // Ada asks; a colleague's out-of-office, threaded in after her, is
        // not the last word.
        let convo = w.thread(
            "ada",
            &[
                ("self", "draft attached", None),
                ("other", "can you add the Q3 numbers?", None),
                ("other", "I'm away until Monday", Some("auto_reply")),
            ],
        );
        assert_eq!(w.waiting(), ["can you add the Q3 numbers?"]);
        assert_eq!(w.db.count_left_out().unwrap(), LeftOut::default());
        // And the mark is about her question, which is what the screen showed.
        assert!(w.mark(&convo, Some(ThreadMark::NoReplyNeeded)));
        assert!(w.waiting().is_empty());
    }

    #[test]
    fn a_machine_after_the_users_reply_is_left_out_and_counted_not_lost() {
        let w = world();
        w.thread(
            "shop",
            &[("other", "your order shipped", Some("no_reply_address")), ("other", "did it arrive?", None)],
        );
        // The user answered a notification by email; a colleague's comment
        // came back through the same service, with list headers.
        let issue = w.thread(
            "tracker",
            &[
                ("other", "new issue opened", Some("newsletter")),
                ("self", "on it", None),
                ("other", "@you can you rebase?", Some("newsletter")),
            ],
        );
        assert_eq!(w.waiting(), ["did it arrive?"]);
        assert_eq!(w.db.count_left_out().unwrap(), LeftOut { automated: 1, not_needed: 0 });
        let left = &w.db.threads_left_out(10).unwrap()[0];
        assert_eq!(left.last_message, "@you can you rebase?");
        // And it can be put on the list.
        assert!(w.mark(&issue, Some(ThreadMark::NeedsReply)));
        assert_eq!(w.waiting().len(), 2);
    }

    #[test]
    fn a_reading_that_is_not_text_is_no_reading() {
        let w = world();
        let convo = w.thread("ada", &[("other", "hello?", None)]);
        w.db.conn()
            .execute("UPDATE messages SET metadata_json = '{\"automated\": true}' WHERE conversation_id = ?1", [&convo])
            .unwrap();
        assert_eq!(w.waiting(), ["hello?"]);
        assert!(w.db.threads_left_out(10).unwrap().is_empty());
    }

    #[test]
    fn a_thread_taken_off_the_list_stays_off_until_they_write_again() {
        let w = world();
        let convo = w.thread("ada", &[("other", "fyi, the deck is attached", None)]);
        assert!(w.mark(&convo, Some(ThreadMark::NoReplyNeeded)));
        assert!(w.waiting().is_empty());
        assert_eq!(w.db.waiting_counts().unwrap(), (0, LeftOut { automated: 0, not_needed: 1 }));
        assert_eq!(w.db.threads_left_out(10).unwrap()[0].mark, Some(ThreadMark::NoReplyNeeded));

        // A new message is a new question; the old answer does not cover it.
        w.add(&convo, "ada", 1, "other", "actually, can you check slide 4?");
        assert_eq!(w.waiting(), ["actually, can you check slide 4?"]);
        assert_eq!(w.db.count_left_out().unwrap(), LeftOut::default());
    }

    #[test]
    fn a_mark_made_about_an_older_message_changes_nothing() {
        let w = world();
        let convo = w.thread("ada", &[("other", "fyi", None)]);
        let seen = w.shown(&convo);
        // They write again between the screen loading and the click.
        w.add(&convo, "ada", 1, "other", "actually, urgent: can you sign today?");
        let applied = w.db.mark_thread(&convo, &seen, Some(ThreadMark::NoReplyNeeded)).unwrap();
        assert!(!applied, "the caller is told it did not take");
        assert_eq!(w.waiting(), ["actually, urgent: can you sign today?"]);
    }

    #[test]
    fn the_users_word_outranks_the_headers_in_both_directions() {
        let w = world();
        let news = w.thread("club", &[("other", "are you coming on saturday?", Some("newsletter"))]);
        assert!(w.waiting().is_empty());
        assert!(w.mark(&news, Some(ThreadMark::NeedsReply)));
        assert_eq!(w.waiting(), ["are you coming on saturday?"]);
        let row = &w.db.threads_awaiting_reply(10).unwrap()[0];
        assert_eq!(row.automated.as_deref(), Some("newsletter"), "the reading is still reported");
        assert_eq!(row.mark, Some(ThreadMark::NeedsReply));
        assert_eq!(w.db.count_left_out().unwrap(), LeftOut::default());

        // Taking the mark back returns the thread to what the headers say.
        assert!(w.mark(&news, None));
        assert!(w.waiting().is_empty());
        assert_eq!(w.db.count_left_out().unwrap().automated, 1);
    }

    #[test]
    fn marking_again_replaces_the_mark_and_a_message_from_elsewhere_is_refused() {
        let w = world();
        let convo = w.thread("ada", &[("other", "hello?", None)]);
        let other = w.thread("bob", &[("other", "hi", None)]);
        w.mark(&convo, Some(ThreadMark::NoReplyNeeded));
        w.mark(&convo, Some(ThreadMark::NeedsReply));
        let marks: i64 = w.db.conn().query_row("SELECT COUNT(*) FROM thread_marks", [], |r| r.get(0)).unwrap();
        assert_eq!(marks, 1, "one mark per thread");
        assert_eq!(w.waiting().len(), 2);
        let bobs = w.shown(&other);
        assert!(matches!(w.db.mark_thread(&convo, &bobs, Some(ThreadMark::NoReplyNeeded)), Err(DbError::NotFound(_))));
    }

    #[test]
    fn an_answered_thread_is_neither_waiting_nor_left_out() {
        let w = world();
        w.thread("brand", &[("other", "sale", Some("bulk")), ("self", "stop emailing me", None)]);
        assert!(w.waiting().is_empty());
        assert_eq!(w.db.count_left_out().unwrap(), LeftOut::default());
        assert!(w.db.threads_left_out(10).unwrap().is_empty());
    }
}
