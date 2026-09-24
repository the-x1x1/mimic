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
//! 3. Otherwise, whether it has gone quiet: its deciding message is older
//!    than the window the user chose (`waiting.withinDays`, 30 days unless
//!    they changed it, 0 for any age), measured against the clock. A message
//!    with no date, or a date that cannot be read, is never quiet — Mimic
//!    does not guess an age.
//! 4. Otherwise it needs a reply.
//!
//! Everything that asks "what is waiting" — the home screen, its counts, and
//! assisted drafting — goes through the one definition below, so the list the
//! user sees and the list Mimic drafts for cannot drift apart.

use rusqlite::{named_params, params, OptionalExtension};
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
    /// The message it is waiting on is older than the waiting window. Only
    /// ever true of a waiting thread when the user said it needs a reply.
    pub quiet: bool,
}

/// How many days back a thread can wait: the setting, what it is unless the
/// user changed it, and the longest it can be.
pub const WITHIN_DAYS_SETTING: &str = "waiting.withinDays";
pub const DEFAULT_WITHIN_DAYS: i64 = 30;
pub const MAX_WITHIN_DAYS: i64 = 36_500;

/// How many unanswered threads were left out of what is waiting, and why.
/// All are counts of rows, never estimates.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LeftOut {
    /// Left out because the last message looks automated, and the user has
    /// not said otherwise.
    pub automated: i64,
    /// Left out because the user said it does not need a reply.
    pub not_needed: i64,
    /// Left out because its last message is older than the waiting window,
    /// and the user has not said otherwise. Not counted here when it also
    /// looks automated: that reason is given first.
    pub quiet: i64,
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
/// `scope` narrows it to one thread (`?1`), for `mark_thread`, or to one
/// person's (`:person`), for `conversations_with`.
fn deciding_cte(scope: Scope) -> String {
    let one = match scope {
        Scope::All => "",
        Scope::Conversation => "AND m.conversation_id = ?1",
        Scope::Person => {
            "AND m.conversation_id IN (SELECT conversation_id FROM conversation_participants WHERE participant_id = :person)"
        }
    };
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
/// Carries the user's mark when it is about that same message, and whether
/// that message is older than `:cutoff` (never, when `:cutoff` is NULL or
/// the message's date cannot be read — `julianday` reads ISO 8601 with `Z`,
/// a `±HH:MM` offset or no zone, and gives NULL for other forms).
fn unanswered_cte() -> String {
    unanswered_cte_in(Scope::All)
}

/// Which threads the deciding message is worked out for.
#[derive(Clone, Copy)]
enum Scope {
    All,
    Conversation,
    Person,
}

fn unanswered_cte_in(scope: Scope) -> String {
    format!(
        "{deciding},
         unanswered AS (
           SELECT d.conversation_id, d.message_id, d.automated, tm.mark AS mark,
                  COALESCE(julianday(dm.sent_at) < julianday(:cutoff), 0) AS quiet
           FROM deciding d
           JOIN messages dm ON dm.id = d.message_id
           LEFT JOIN thread_marks tm
             ON tm.conversation_id = d.conversation_id AND tm.message_id = d.message_id
           WHERE d.direction = 'other'
         )",
        deciding = deciding_cte(scope)
    )
}

/// The columns after the conversation's, for `map_row`.
const ROW_COLS: &str = "u.message_id, m.body, m.sent_at, m.participant_id, u.automated, u.mark, u.quiet";

fn map_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<AwaitingReply> {
    Ok(AwaitingReply {
        conversation: map_conv(r)?,
        last_message_id: r.get(10)?,
        last_message: r.get(11)?,
        last_message_at: r.get(12)?,
        participant_id: r.get(13)?,
        automated: r.get(14)?,
        mark: r.get::<_, Option<String>>(15)?.as_deref().and_then(ThreadMark::parse),
        quiet: r.get(16)?,
    })
}

/// The user's word first, then the headers, then the date.
const NEEDS_REPLY: &str = "(u.mark = 'needs_reply' OR (u.mark IS NULL AND u.automated IS NULL AND NOT u.quiet))";
const LEFT_OUT: &str = "(u.mark = 'no_reply_needed' OR (u.mark IS NULL AND (u.automated IS NOT NULL OR u.quiet)))";

/// The waiting window as of one moment: how many days back a thread can be
/// waiting, and the cutoff that makes. Taken once per read, so the lists and
/// the counts a screen shows are judged against the same moment and setting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaitingWindow {
    /// `None` for any age.
    pub within_days: Option<i64>,
    cutoff: Option<String>,
}

/// Where a conversation stands, by the definition above — the same reading
/// the home screen and its counts make, for any conversation, so a person's
/// conversations and the list can never disagree about one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Standing {
    /// Its deciding message is the user's.
    Answered,
    /// It needs a reply: it is on the home screen.
    Waiting,
    /// Left out: its deciding message looks automated.
    Automated,
    /// Left out: its deciding message is older than the waiting window.
    Quiet,
    /// Left out: the user said it needs no reply.
    NotNeeded,
    /// No message in it could be told to be the user's or someone else's.
    Undecided,
}

/// One conversation someone is in, and where it stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonConversation {
    pub conversation_id: String,
    pub subject: Option<String>,
    pub channel: String,
    pub is_group: bool,
    pub message_count: i64,
    pub last_message_at: Option<String>,
    /// The message that decides where it stands — the one a mark is about.
    /// `None` when it is `Undecided`.
    pub deciding_message_id: Option<String>,
    pub standing: Standing,
    /// What the user said about it, while it still applies.
    pub mark: Option<ThreadMark>,
}

/// A page of someone's conversations, most recently active first, and how
/// many more there are after it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonConversations {
    pub conversations: Vec<PersonConversation>,
    pub more: i64,
    /// The waiting window they were judged against: how many days back a
    /// conversation can be waiting, `None` for any age.
    pub waiting_within_days: Option<i64>,
}

/// A conversation's place in the order `conversations_with` pages by: most
/// recently active first, a conversation with no date last, then by id.
const ACTIVE: &str = "COALESCE(c.last_message_at, '')";

/// Why an unanswered thread was left out, in the order the screen lists the
/// reasons: automated, gone quiet, taken off by the user.
const REASON: &str = "CASE WHEN u.mark = 'no_reply_needed' THEN 2 WHEN u.automated IS NOT NULL THEN 0 ELSE 1 END";

impl Db {
    /// How many days back a thread can be waiting, or `None` for any age.
    /// Read leniently: a value that is not a number is the default, and one
    /// out of range is brought into it, so a bad setting cannot empty or
    /// break the home screen.
    pub fn waiting_within_days(&self) -> DbResult<Option<i64>> {
        let raw = self.get_setting::<serde_json::Value>(WITHIN_DAYS_SETTING)?;
        let days = raw.as_ref().and_then(serde_json::Value::as_f64).map_or(DEFAULT_WITHIN_DAYS, |d| d as i64);
        Ok((days > 0).then(|| days.min(MAX_WITHIN_DAYS)))
    }

    /// The window now: the setting, and the moment before which a thread's
    /// last message has gone quiet.
    pub fn waiting_window(&self) -> DbResult<WaitingWindow> {
        let within_days = self.waiting_within_days()?;
        let cutoff = within_days.map(|days| crate::ids::fmt_rfc3339(chrono::Utc::now() - chrono::Duration::days(days)));
        Ok(WaitingWindow { within_days, cutoff })
    }

    /// Threads that need a reply, newest first.
    pub fn threads_awaiting_reply(&self, limit: usize) -> DbResult<Vec<AwaitingReply>> {
        self.threads_awaiting_reply_in(&self.waiting_window()?, limit)
    }

    /// The same, judged against a window the caller already took.
    pub fn threads_awaiting_reply_in(&self, window: &WaitingWindow, limit: usize) -> DbResult<Vec<AwaitingReply>> {
        let conn = self.conn();
        let sql = format!(
            "{cte}
             SELECT {CONV_COLS_Q}, {ROW_COLS}
             FROM unanswered u
             JOIN messages m ON m.id = u.message_id
             JOIN conversations c ON c.id = u.conversation_id
             WHERE {NEEDS_REPLY}
             ORDER BY m.sent_at DESC NULLS LAST, c.id
             LIMIT {limit}",
            cte = unanswered_cte()
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(named_params! { ":cutoff": window.cutoff }, map_row)?;
        rows.map(|r| r.map_err(DbError::from)).collect()
    }

    /// Unanswered threads that were left out of what is waiting: up to
    /// `limit` for each reason, newest first within it, in the order the
    /// screen gives the reasons — the ones that look automated, the ones that
    /// have gone quiet, the ones the user took off. Per reason, because the
    /// newest few of all of them together would be this month's newsletters
    /// every time, and a thread that has gone quiet could never be shown.
    pub fn threads_left_out(&self, limit: usize) -> DbResult<Vec<AwaitingReply>> {
        self.threads_left_out_in(&self.waiting_window()?, limit)
    }

    /// The same, judged against a window the caller already took.
    pub fn threads_left_out_in(&self, window: &WaitingWindow, limit: usize) -> DbResult<Vec<AwaitingReply>> {
        let conn = self.conn();
        let sql = format!(
            "{cte},
             left_out AS (
               SELECT u.*, {REASON} AS reason,
                      ROW_NUMBER() OVER (
                        PARTITION BY {REASON}
                        ORDER BY lm.sent_at DESC NULLS LAST, u.conversation_id
                      ) AS rank_in_reason
               FROM unanswered u
               JOIN messages lm ON lm.id = u.message_id
               WHERE {LEFT_OUT}
             )
             SELECT {CONV_COLS_Q}, {ROW_COLS}
             FROM left_out u
             JOIN messages m ON m.id = u.message_id
             JOIN conversations c ON c.id = u.conversation_id
             WHERE u.rank_in_reason <= {limit}
             ORDER BY u.reason, m.sent_at DESC NULLS LAST, c.id",
            cte = unanswered_cte()
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(named_params! { ":cutoff": window.cutoff }, map_row)?;
        rows.map(|r| r.map_err(DbError::from)).collect()
    }

    /// How many threads need a reply, and how many unanswered ones were left
    /// out and why — in one pass, because the home screen asks for all of
    /// them every time it loads.
    pub fn waiting_counts(&self) -> DbResult<(i64, LeftOut)> {
        self.waiting_counts_in(&self.waiting_window()?)
    }

    /// The same, judged against a window the caller already took.
    pub fn waiting_counts_in(&self, window: &WaitingWindow) -> DbResult<(i64, LeftOut)> {
        Ok(self.conn().query_row(
            &format!(
                "{cte}
                 SELECT COALESCE(SUM({NEEDS_REPLY}), 0),
                        COALESCE(SUM(u.mark IS NULL AND u.automated IS NOT NULL), 0),
                        COALESCE(SUM(u.mark = 'no_reply_needed'), 0),
                        COALESCE(SUM(u.mark IS NULL AND u.automated IS NULL AND u.quiet), 0)
                 FROM unanswered u",
                cte = unanswered_cte()
            ),
            named_params! { ":cutoff": window.cutoff },
            |r| Ok((r.get(0)?, LeftOut { automated: r.get(1)?, not_needed: r.get(2)?, quiet: r.get(3)? })),
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

    /// Every conversation someone is in — not only what is waiting — with
    /// where each stands, a page at a time: most recently active first, read
    /// on from the last one shown (`before`: its `last_message_at`, or "" when
    /// it has none, and its id).
    pub fn conversations_with(
        &self,
        participant_id: &str,
        before: Option<(&str, &str)>,
        limit: usize,
    ) -> DbResult<PersonConversations> {
        self.conversations_with_in(&self.waiting_window()?, participant_id, before, limit)
    }

    /// The same, judged against a window the caller already took.
    pub fn conversations_with_in(
        &self,
        window: &WaitingWindow,
        participant_id: &str,
        before: Option<(&str, &str)>,
        limit: usize,
    ) -> DbResult<PersonConversations> {
        let conn = self.conn();
        let after = if before.is_some() {
            format!("AND ({ACTIVE} < :at OR ({ACTIVE} = :at AND c.id < :id))")
        } else {
            String::new()
        };
        let sql = format!(
            "{cte},
             theirs AS (
               SELECT c.* FROM conversations c
               JOIN conversation_participants cp ON cp.conversation_id = c.id
               WHERE cp.participant_id = :person
             )
             SELECT c.id, c.subject, c.channel, c.is_group, c.message_count, c.last_message_at, d.message_id,
                    CASE WHEN d.message_id IS NULL THEN 'undecided'
                         WHEN d.direction = 'self' THEN 'answered'
                         WHEN {NEEDS_REPLY} THEN 'waiting'
                         WHEN u.mark = 'no_reply_needed' THEN 'not_needed'
                         WHEN u.automated IS NOT NULL THEN 'automated'
                         ELSE 'quiet' END,
                    u.mark
             FROM theirs c
             LEFT JOIN deciding d ON d.conversation_id = c.id
             LEFT JOIN unanswered u ON u.conversation_id = c.id
             WHERE 1 = 1 {after}
             ORDER BY {ACTIVE} DESC, c.id DESC
             LIMIT {limit}",
            cte = unanswered_cte_in(Scope::Person)
        );
        let map = |r: &rusqlite::Row<'_>| -> rusqlite::Result<PersonConversation> {
            let standing = match r.get::<_, String>(7)?.as_str() {
                "answered" => Standing::Answered,
                "waiting" => Standing::Waiting,
                "not_needed" => Standing::NotNeeded,
                "automated" => Standing::Automated,
                "quiet" => Standing::Quiet,
                _ => Standing::Undecided,
            };
            Ok(PersonConversation {
                conversation_id: r.get(0)?,
                subject: r.get(1)?,
                channel: r.get(2)?,
                is_group: r.get(3)?,
                message_count: r.get(4)?,
                last_message_at: r.get(5)?,
                deciding_message_id: r.get(6)?,
                standing,
                mark: r.get::<_, Option<String>>(8)?.as_deref().and_then(ThreadMark::parse),
            })
        };
        let mut stmt = conn.prepare(&sql)?;
        let conversations: Vec<PersonConversation> = match before {
            Some((at, id)) => stmt
                .query_map(
                    named_params! { ":cutoff": window.cutoff, ":person": participant_id, ":at": at, ":id": id },
                    map,
                )?
                .collect::<Result<_, _>>()?,
            None => stmt
                .query_map(named_params! { ":cutoff": window.cutoff, ":person": participant_id }, map)?
                .collect::<Result<_, _>>()?,
        };
        let more = match conversations.last() {
            Some(last) if conversations.len() == limit => conn.query_row(
                &format!(
                    "SELECT COUNT(*) FROM conversations c
                     JOIN conversation_participants cp ON cp.conversation_id = c.id
                     WHERE cp.participant_id = ?1 AND ({ACTIVE} < ?2 OR ({ACTIVE} = ?2 AND c.id < ?3))"
                ),
                params![participant_id, last.last_message_at.as_deref().unwrap_or(""), last.conversation_id],
                |r| r.get(0),
            )?,
            _ => 0,
        };
        Ok(PersonConversations { conversations, more, waiting_within_days: window.within_days })
    }

    /// Record what the user said about a thread, against the message they
    /// were looking at; `None` takes back whatever they said. Returns whether
    /// the mark applies now — `false` when someone has written since, in which
    /// case it is kept but changes nothing, and the thread stays where the new
    /// message puts it; and `false`, changing nothing, when the user has
    /// since said something about a later message of it.
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
        // Which message decides the thread now. Marks don't move it.
        let deciding: Option<String> = conn
            .query_row(
                &format!("{cte} SELECT message_id FROM deciding", cte = deciding_cte(Scope::Conversation)),
                [conversation_id],
                |r| r.get(0),
            )
            .optional()?;
        // What the user said about a later message — on another screen,
        // since this one was read — is newer than what this says about an
        // earlier one, and stands while it still applies (that later message
        // is the one deciding the thread). A mark that has lapsed stands in
        // the way of nothing.
        if let Some(now) = deciding.as_deref().filter(|d| *d != message_id) {
            let newer: bool = conn.query_row(
                "SELECT EXISTS(
                   SELECT 1 FROM thread_marks tm
                   JOIN messages kept ON kept.id = tm.message_id
                   JOIN messages this ON this.id = ?2
                   WHERE tm.conversation_id = ?1 AND tm.message_id = ?3
                     AND (kept.sequence_index > this.sequence_index
                          OR (kept.sequence_index = this.sequence_index AND kept.id > this.id)))",
                params![conversation_id, message_id, now],
                |r| r.get(0),
            )?;
            if newer {
                return Ok(false);
            }
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
        // These threads are dated early in September 2026. The window is
        // measured against the clock, so the rules below are tested at any
        // age and the window by itself, with dates relative to now.
        db.set_setting(WITHIN_DAYS_SETTING, &0).unwrap();
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
        assert_eq!(w.db.waiting_counts().unwrap(), (1, LeftOut { automated: 2, not_needed: 0, quiet: 0 }));

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
        assert_eq!(w.db.count_left_out().unwrap(), LeftOut { automated: 1, not_needed: 0, quiet: 0 });
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
        assert_eq!(w.db.waiting_counts().unwrap(), (0, LeftOut { automated: 0, not_needed: 1, quiet: 0 }));
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

    /// A one-message thread from `key`, sent at `sent_at` (None: no date).
    fn dated(w: &World, key: &str, body: &str, sent_at: Option<String>, automated: Option<&str>) -> String {
        let convo = w.db.upsert_conversation(&w.source, key, "email", Some(key)).unwrap();
        w.db.insert_messages(&[NewMessage {
            conversation_id: convo.clone(),
            source_id: w.source.clone(),
            participant_id: None,
            external_id: format!("{key}-0"),
            direction: "other".into(),
            channel: "email".into(),
            sent_at,
            sequence_index: 0,
            body: body.into(),
            reply_to_external_id: None,
            metadata: automated.map(|a| json!({ "automated": a })).unwrap_or(Value::Null),
        }])
        .unwrap();
        w.db.refresh_conversation_stats(&convo).unwrap();
        convo
    }

    fn days_ago(days: i64) -> Option<String> {
        Some(crate::ids::fmt_rfc3339(chrono::Utc::now() - chrono::Duration::days(days)))
    }

    #[test]
    fn a_thread_that_has_gone_quiet_is_left_out_and_counted_and_the_users_word_still_wins() {
        let w = world();
        w.db.set_setting(WITHIN_DAYS_SETTING, &30).unwrap();
        dated(&w, "recent", "are we still on for friday?", days_ago(3), None);
        let old = dated(&w, "old", "any news on the lease?", days_ago(45), None);
        dated(&w, "old-news", "spring sale", days_ago(90), Some("newsletter"));
        // No date, or one that cannot be read: no age is guessed.
        dated(&w, "undated", "hello from nowhere", None, None);
        dated(&w, "garbled", "hello from the past", Some("last tuesday".into()), None);
        // A date with an offset is read as the moment it names.
        let offset = (chrono::Utc::now() - chrono::Duration::days(40))
            .with_timezone(&chrono::FixedOffset::east_opt(2 * 3600).unwrap())
            .to_rfc3339();
        dated(&w, "offset", "sent from Berlin", Some(offset), None);

        let mut waiting = w.waiting();
        waiting.sort();
        assert_eq!(waiting, ["are we still on for friday?", "hello from nowhere", "hello from the past"]);
        assert_eq!(w.db.waiting_counts().unwrap(), (3, LeftOut { automated: 1, not_needed: 0, quiet: 2 }));
        let left = w.db.threads_left_out(50).unwrap();
        let lease = left.iter().find(|t| t.conversation.id == old).unwrap();
        assert!(lease.quiet && lease.automated.is_none() && lease.mark.is_none());
        let sale = left.iter().find(|t| t.last_message == "spring sale").unwrap();
        assert!(sale.automated.is_some() && sale.quiet, "automated first, and counted once");

        // The user's word outranks the date, both ways.
        assert!(w.mark(&old, Some(ThreadMark::NeedsReply)));
        let kept = w.db.threads_awaiting_reply(50).unwrap().into_iter().find(|t| t.conversation.id == old).unwrap();
        assert!(kept.quiet, "still said, so the screen can say it");
        assert_eq!(w.db.count_left_out().unwrap().quiet, 1);
        assert!(w.mark(&old, None));
        assert_eq!(w.db.count_left_out().unwrap().quiet, 2);

        // Any age: nothing goes quiet.
        w.db.set_setting(WITHIN_DAYS_SETTING, &0).unwrap();
        assert_eq!(w.db.waiting_counts().unwrap(), (5, LeftOut { automated: 1, not_needed: 0, quiet: 0 }));
        assert!(w.db.threads_left_out(50).unwrap().iter().all(|t| !t.quiet));
    }

    #[test]
    fn what_was_left_out_is_listed_per_reason_so_an_old_thread_is_never_crowded_out() {
        let w = world();
        w.db.set_setting(WITHIN_DAYS_SETTING, &30).unwrap();
        for i in 0..5 {
            dated(&w, &format!("news{i}"), &format!("sale {i}"), days_ago(i), Some("newsletter"));
        }
        let old = dated(&w, "old", "any news on the lease?", days_ago(60), None);
        let older = dated(&w, "older", "are you coming in june?", days_ago(90), None);
        let off = dated(&w, "off", "fyi, the office is shut monday", days_ago(2), None);
        assert!(w.mark(&off, Some(ThreadMark::NoReplyNeeded)));

        let left = w.db.threads_left_out(2).unwrap();
        let ids: Vec<&str> = left.iter().map(|t| t.conversation.id.as_str()).collect();
        // Up to two of each, in the order the screen gives the reasons, newest
        // first within each: automated, then gone quiet, then taken off.
        assert_eq!(left.len(), 5);
        assert_eq!((left[0].last_message.as_str(), left[1].last_message.as_str()), ("sale 0", "sale 1"));
        assert_eq!(&ids[2..4], [old.as_str(), older.as_str()]);
        assert_eq!(ids[4], off);
        assert_eq!(w.db.count_left_out().unwrap(), LeftOut { automated: 5, not_needed: 1, quiet: 2 });
    }

    #[test]
    fn the_window_is_read_leniently() {
        let w = world();
        let read = |v: Value| {
            w.db.set_setting(WITHIN_DAYS_SETTING, &v).unwrap();
            w.db.waiting_within_days().unwrap()
        };
        assert_eq!(read(json!(14)), Some(14));
        assert_eq!(read(json!(0)), None);
        assert_eq!(read(json!(-5)), None);
        assert_eq!(read(json!(7.0)), Some(7));
        assert_eq!(read(json!("soon")), Some(DEFAULT_WITHIN_DAYS));
        assert_eq!(read(json!(10_000_000)), Some(MAX_WITHIN_DAYS));
        w.db.conn().execute("DELETE FROM app_settings WHERE key = ?1", [WITHIN_DAYS_SETTING]).unwrap();
        assert_eq!(w.db.waiting_within_days().unwrap(), Some(DEFAULT_WITHIN_DAYS), "30 days unless changed");
        // And the largest window still makes a date.
        w.db.set_setting(WITHIN_DAYS_SETTING, &MAX_WITHIN_DAYS).unwrap();
        assert!(w.db.waiting_counts().is_ok());
    }

    #[test]
    fn an_answered_thread_is_neither_waiting_nor_left_out() {
        let w = world();
        w.thread("brand", &[("other", "sale", Some("bulk")), ("self", "stop emailing me", None)]);
        assert!(w.waiting().is_empty());
        assert_eq!(w.db.count_left_out().unwrap(), LeftOut::default());
        assert!(w.db.threads_left_out(10).unwrap().is_empty());
    }

    #[test]
    fn every_conversation_with_someone_says_where_it_stands_by_the_same_reading() {
        let w = world();
        let ada =
            w.db.resolve_participant("Ada", &[IdentifierInput::new(IdentifierKind::Email, "ada@example.com")], false)
                .unwrap();
        let answered = w.thread("answered", &[("other", "shall we?", None), ("self", "yes", None)]);
        let waiting = w.thread("waiting", &[("self", "hi", None), ("other", "and you?", None)]);
        let automated = w.thread("automated", &[("other", "your receipt", Some("no_reply_address"))]);
        let taken_off = w.thread("takenoff", &[("other", "fyi", None)]);
        let kept = w.thread("kept", &[("other", "this week", Some("newsletter"))]);
        let undecided = w.thread("undecided", &[("unknown", "who wrote this?", None)]);
        let elsewhere = w.thread("elsewhere", &[("other", "not with Ada", None)]);
        for c in [&answered, &waiting, &automated, &taken_off, &kept, &undecided] {
            w.db.link_conversation_participant(c, &ada).unwrap();
        }
        assert!(w.mark(&taken_off, Some(ThreadMark::NoReplyNeeded)));
        assert!(w.mark(&kept, Some(ThreadMark::NeedsReply)));

        let all = w.db.conversations_with(&ada, None, 50).unwrap();
        assert_eq!((all.conversations.len(), all.more), (6, 0), "only what Ada is in: {all:?}");
        let standing = |all: &PersonConversations, id: &str| {
            all.conversations.iter().find(|c| c.conversation_id == id).map(|c| (c.standing, c.mark))
        };
        assert_eq!(standing(&all, &answered), Some((Standing::Answered, None)));
        assert_eq!(standing(&all, &waiting), Some((Standing::Waiting, None)));
        assert_eq!(standing(&all, &automated), Some((Standing::Automated, None)));
        assert_eq!(standing(&all, &taken_off), Some((Standing::NotNeeded, Some(ThreadMark::NoReplyNeeded))));
        assert_eq!(standing(&all, &kept), Some((Standing::Waiting, Some(ThreadMark::NeedsReply))));
        assert_eq!(standing(&all, &undecided), Some((Standing::Undecided, None)));
        assert_eq!(standing(&all, &elsewhere), None);

        // The same reading as the home screen: what is said to be waiting is
        // what the list holds, decided by the message the list shows.
        let mut said: Vec<(String, Option<String>)> = all
            .conversations
            .iter()
            .filter(|c| c.standing == Standing::Waiting)
            .map(|c| (c.conversation_id.clone(), c.deciding_message_id.clone()))
            .collect();
        let mut listed: Vec<(String, Option<String>)> =
            w.db.threads_awaiting_reply(50)
                .unwrap()
                .into_iter()
                .filter(|t| t.conversation.id != elsewhere)
                .map(|t| (t.conversation.id, Some(t.last_message_id)))
                .collect();
        said.sort();
        listed.sort();
        assert_eq!(said, listed);
        let undecided_row = all.conversations.iter().find(|c| c.conversation_id == undecided).unwrap();
        assert!(undecided_row.deciding_message_id.is_none());

        // A page at a time, most recently active first; nothing shared or skipped.
        let first = w.db.conversations_with(&ada, None, 4).unwrap();
        assert_eq!((first.conversations.len(), first.more), (4, 2));
        let last = first.conversations.last().unwrap();
        let cursor = (last.last_message_at.as_deref().unwrap_or(""), last.conversation_id.as_str());
        let second = w.db.conversations_with(&ada, Some(cursor), 4).unwrap();
        assert_eq!((second.conversations.len(), second.more), (2, 0));
        let read: Vec<&PersonConversation> = first.conversations.iter().chain(&second.conversations).collect();
        let mut ids: Vec<&str> = read.iter().map(|c| c.conversation_id.as_str()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 6);
        let keys: Vec<(String, String)> =
            read.iter().map(|c| (c.last_message_at.clone().unwrap_or_default(), c.conversation_id.clone())).collect();
        let mut sorted = keys.clone();
        sorted.sort_by(|a, b| b.cmp(a));
        assert_eq!(keys, sorted, "newest first, then by id");

        // A window of a day: these September threads have gone quiet, except
        // the one the user said needs a reply.
        w.db.set_setting(WITHIN_DAYS_SETTING, &1).unwrap();
        let now = w.db.conversations_with(&ada, None, 50).unwrap();
        assert_eq!(standing(&now, &waiting), Some((Standing::Quiet, None)));
        assert_eq!(standing(&now, &kept), Some((Standing::Waiting, Some(ThreadMark::NeedsReply))));
        assert_eq!(standing(&now, &answered), Some((Standing::Answered, None)));
    }

    #[test]
    fn a_whole_conversation_is_read_from_its_end() {
        let w = world();
        let convo = w.thread(
            "long",
            &[("other", "one", None), ("self", "two", None), ("other", "three", None), ("unknown", "four", None)],
        );
        let end = w.db.conversation_end(&convo, 2).unwrap();
        let bodies = |p: &crate::db::ConversationPage| p.messages.iter().map(|m| m.body.clone()).collect::<Vec<_>>();
        assert_eq!(bodies(&end), vec!["three", "four"], "the last two, oldest first, whoever wrote them");
        assert_eq!(end.more, 2);
        let earlier = w.db.conversation_page(&convo, &end.messages[0].id, crate::db::Toward::Earlier, 20).unwrap();
        assert_eq!(bodies(&earlier), vec!["one", "two"]);
        assert_eq!(earlier.more, 0);
        let all = w.db.conversation_end(&convo, 50).unwrap();
        assert_eq!((all.messages.len(), all.more), (4, 0));
        assert!(all.messages[1].author.is_none(), "the user's own message carries no name");
        assert!(matches!(w.db.conversation_end("nope", 5), Err(DbError::NotFound(_))));
    }

    #[test]
    fn a_mark_about_an_earlier_message_never_undoes_one_about_a_later_one() {
        let w = world();
        let convo = w.thread("stale", &[("other", "first", None)]);
        let first = w.shown(&convo);
        w.add(&convo, "stale", 1, "other", "second");
        let second = w.shown(&convo);
        assert_ne!(first, second);
        assert!(w.db.mark_thread(&convo, &second, Some(ThreadMark::NoReplyNeeded)).unwrap());
        assert!(w.waiting().is_empty());
        // A screen read before the second message came in.
        assert!(!w.db.mark_thread(&convo, &first, Some(ThreadMark::NeedsReply)).unwrap());
        assert!(!w.db.mark_thread(&convo, &first, None).unwrap());
        assert!(w.waiting().is_empty(), "what was said about the later message stands");
        // About the same message, or a later one, it is changed as asked.
        assert!(w.db.mark_thread(&convo, &second, None).unwrap());
        assert_eq!(w.waiting(), vec!["second".to_string()]);

        // A mark on a later message that no longer decides the thread — read
        // again as automated, so the earlier one decides — has lapsed, and
        // stands in the way of nothing.
        assert!(w.db.mark_thread(&convo, &second, Some(ThreadMark::NeedsReply)).unwrap());
        w.db.conn()
            .execute(
                "UPDATE messages SET metadata_json = json_set(metadata_json, '$.automated', 'bulk') WHERE external_id = 'stale-1'",
                [],
            )
            .unwrap();
        assert_eq!(w.shown(&convo), first, "the earlier message decides again");
        assert!(w.db.mark_thread(&convo, &first, Some(ThreadMark::NoReplyNeeded)).unwrap());
        assert!(w.waiting().is_empty());
    }
}
