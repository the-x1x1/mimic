//! Conversations and messages.
//!
//! Everything here is written to survive a mailbox with a million messages in
//! it: inserts are batched inside one transaction, reads are keyset-paged
//! rather than offset-paged, and no call materializes a whole conversation
//! unless the caller asked for it by id.

use rusqlite::{params, OptionalExtension, Row};
use serde_json::Value;

use super::models::json_obj;
use super::repo_sources::CHANNELS;
use super::{Conversation, Db, DbError, DbResult, Message};
use crate::ids::{new_id, now_rfc3339, sha256_hex};

/// A message as an importer produces it, before it has an id.
#[derive(Debug, Clone)]
pub struct NewMessage {
    pub conversation_id: String,
    pub source_id: String,
    pub participant_id: Option<String>,
    /// Stable within the source. Together with `source_id` this is the import
    /// identity key, so re-importing the same export inserts nothing.
    pub external_id: String,
    pub direction: String,
    pub channel: String,
    pub sent_at: Option<String>,
    pub sequence_index: i64,
    pub body: String,
    pub reply_to_external_id: Option<String>,
    pub metadata: Value,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ImportCounts {
    pub inserted: usize,
    pub duplicates: usize,
    pub empty: usize,
}

/// Words are whitespace-separated runs containing at least one alphanumeric
/// character, so "—" and stray punctuation do not inflate the count.
pub fn word_count(body: &str) -> i64 {
    body.split_whitespace().filter(|w| w.chars().any(char::is_alphanumeric)).count() as i64
}

const COLS: &str = "id, conversation_id, source_id, participant_id, external_id, direction, channel, sent_at, sequence_index, body, word_count, char_count, reply_to_message_id, response_latency_seconds, metadata_json";

fn map(r: &Row<'_>) -> rusqlite::Result<Message> {
    Ok(Message {
        id: r.get(0)?,
        conversation_id: r.get(1)?,
        source_id: r.get(2)?,
        participant_id: r.get(3)?,
        external_id: r.get(4)?,
        direction: r.get(5)?,
        channel: r.get(6)?,
        sent_at: r.get(7)?,
        sequence_index: r.get(8)?,
        body: r.get(9)?,
        word_count: r.get(10)?,
        char_count: r.get(11)?,
        reply_to_message_id: r.get(12)?,
        response_latency_seconds: r.get(13)?,
        metadata: json_obj(r.get(14)?),
    })
}

const CONV_COLS: &str =
    "id, source_id, external_id, channel, subject, is_group, started_at, last_message_at, message_count, created_at";
/// The same columns qualified, for the queries that join
/// `conversation_participants` — which also has a `message_count`.
const CONV_COLS_Q: &str = "c.id, c.source_id, c.external_id, c.channel, c.subject, c.is_group, c.started_at, c.last_message_at, c.message_count, c.created_at";

fn map_conv(r: &Row<'_>) -> rusqlite::Result<Conversation> {
    Ok(Conversation {
        id: r.get(0)?,
        source_id: r.get(1)?,
        external_id: r.get(2)?,
        channel: r.get(3)?,
        subject: r.get(4)?,
        is_group: r.get::<_, i64>(5)? != 0,
        started_at: r.get(6)?,
        last_message_at: r.get(7)?,
        message_count: r.get(8)?,
        created_at: r.get(9)?,
    })
}

impl Db {
    /// Find or create the conversation a message belongs to.
    pub fn upsert_conversation(
        &self,
        source_id: &str,
        external_id: &str,
        channel: &str,
        subject: Option<&str>,
    ) -> DbResult<String> {
        if !CHANNELS.contains(&channel) {
            return Err(DbError::Invalid(format!("unknown channel {channel:?}")));
        }
        let conn = self.conn();
        let existing: Option<String> = conn
            .query_row(
                "SELECT id FROM conversations WHERE source_id = ?1 AND external_id = ?2",
                params![source_id, external_id],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(id) = existing {
            if let Some(s) = subject.filter(|s| !s.trim().is_empty()) {
                conn.execute(
                    "UPDATE conversations SET subject = COALESCE(subject, ?1) WHERE id = ?2",
                    params![s.trim(), id],
                )?;
            }
            return Ok(id);
        }
        let id = new_id();
        conn.execute(
            "INSERT INTO conversations(id, source_id, external_id, channel, subject, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                id,
                source_id,
                external_id,
                channel,
                subject.map(str::trim).filter(|s| !s.is_empty()),
                now_rfc3339()
            ],
        )?;
        Ok(id)
    }

    pub fn get_conversation(&self, id: &str) -> DbResult<Option<Conversation>> {
        Ok(self
            .conn()
            .query_row(&format!("SELECT {CONV_COLS} FROM conversations WHERE id = ?1"), [id], map_conv)
            .optional()?)
    }

    pub fn link_conversation_participant(&self, conversation_id: &str, participant_id: &str) -> DbResult<()> {
        self.conn().execute(
            "INSERT OR IGNORE INTO conversation_participants(conversation_id, participant_id) VALUES (?1, ?2)",
            params![conversation_id, participant_id],
        )?;
        Ok(())
    }

    /// Insert a batch in one transaction. Duplicates — same `(source_id,
    /// external_id)` — are counted and skipped, so re-running an import is
    /// safe and cheap. Empty bodies are dropped: a message with no text is
    /// not evidence of how anyone writes.
    pub fn insert_messages(&self, batch: &[NewMessage]) -> DbResult<ImportCounts> {
        let mut counts = ImportCounts::default();
        self.transaction(|tx| {
            let mut stmt = tx.prepare_cached(
                "INSERT OR IGNORE INTO messages(
                    id, conversation_id, source_id, participant_id, external_id, direction, channel,
                    sent_at, sequence_index, body, body_hash, word_count, char_count, metadata_json, imported_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
            )?;
            let now = now_rfc3339();
            for m in batch {
                if m.body.trim().is_empty() {
                    counts.empty += 1;
                    continue;
                }
                let metadata =
                    if m.metadata.is_object() { m.metadata.clone() } else { Value::Object(Default::default()) };
                let changed = stmt.execute(params![
                    new_id(),
                    m.conversation_id,
                    m.source_id,
                    m.participant_id,
                    m.external_id,
                    m.direction,
                    m.channel,
                    m.sent_at,
                    m.sequence_index,
                    m.body,
                    sha256_hex(m.body.as_bytes()),
                    word_count(&m.body),
                    m.body.chars().count() as i64,
                    metadata.to_string(),
                    now,
                ])?;
                if changed == 1 {
                    counts.inserted += 1;
                } else {
                    counts.duplicates += 1;
                }
            }
            Ok(())
        })?;
        Ok(counts)
    }

    /// Resolve `reply_to_external_id` into real row ids and derive response
    /// latency. Run once per conversation after its messages are in, because
    /// a reply can arrive in the export before the message it answers.
    pub fn link_replies(&self, conversation_id: &str) -> DbResult<usize> {
        let linked = self.conn().execute(
            "UPDATE messages SET reply_to_message_id = (
                 SELECT prev.id FROM messages prev
                 WHERE prev.conversation_id = messages.conversation_id
                   AND prev.sequence_index < messages.sequence_index
                 ORDER BY prev.sequence_index DESC LIMIT 1
             )
             WHERE conversation_id = ?1 AND reply_to_message_id IS NULL",
            [conversation_id],
        )?;
        // Latency is only meaningful when both timestamps exist and the reply
        // is by someone other than the author of the message it answers.
        self.conn().execute(
            "UPDATE messages SET response_latency_seconds = (
                 SELECT CAST((julianday(messages.sent_at) - julianday(prev.sent_at)) * 86400 AS INTEGER)
                 FROM messages prev
                 WHERE prev.id = messages.reply_to_message_id
                   AND prev.sent_at IS NOT NULL AND messages.sent_at IS NOT NULL
                   AND prev.direction <> messages.direction
             )
             WHERE conversation_id = ?1",
            [conversation_id],
        )?;
        Ok(linked)
    }

    /// Recompute the denormalized counters from the rows themselves.
    pub fn refresh_conversation_stats(&self, conversation_id: &str) -> DbResult<()> {
        self.conn().execute(
            "UPDATE conversations SET
                message_count = (SELECT COUNT(*) FROM messages WHERE conversation_id = ?1),
                started_at = (SELECT MIN(sent_at) FROM messages WHERE conversation_id = ?1),
                last_message_at = (SELECT MAX(sent_at) FROM messages WHERE conversation_id = ?1),
                is_group = (SELECT COUNT(*) > 2 FROM conversation_participants WHERE conversation_id = ?1)
             WHERE id = ?1",
            [conversation_id],
        )?;
        self.conn().execute(
            "UPDATE conversation_participants SET message_count = (
                 SELECT COUNT(*) FROM messages m
                 WHERE m.conversation_id = conversation_participants.conversation_id
                   AND m.participant_id = conversation_participants.participant_id
             ) WHERE conversation_id = ?1",
            [conversation_id],
        )?;
        Ok(())
    }

    pub fn conversation_ids_for_source(&self, source_id: &str) -> DbResult<Vec<String>> {
        let conn = self.conn();
        let mut stmt = conn.prepare("SELECT id FROM conversations WHERE source_id = ?1")?;
        let rows = stmt.query_map([source_id], |r| r.get::<_, String>(0))?;
        rows.map(|r| r.map_err(DbError::from)).collect()
    }

    pub fn get_message(&self, id: &str) -> DbResult<Option<Message>> {
        Ok(self.conn().query_row(&format!("SELECT {COLS} FROM messages WHERE id = ?1"), [id], map).optional()?)
    }

    /// Keyset page of the user's own messages, optionally narrowed to one
    /// channel or one conversation partner. `after` is the id returned as
    /// `next` by the previous page; pass `None` for the first page.
    ///
    /// Ordered by `(sent_at, id)` so the cursor is stable even when several
    /// messages share a timestamp, which SMS exports routinely do.
    pub fn page_self_messages(
        &self,
        channel: Option<&str>,
        participant_id: Option<&str>,
        after: Option<(&str, &str)>,
        limit: usize,
    ) -> DbResult<Vec<Message>> {
        let conn = self.conn();
        let mut sql = format!("SELECT {COLS} FROM messages m WHERE direction = 'self'");
        let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(c) = channel {
            sql.push_str(" AND channel = ?");
            args.push(Box::new(c.to_string()));
        }
        if let Some(p) = participant_id {
            sql.push_str(
                " AND conversation_id IN (SELECT conversation_id FROM conversation_participants WHERE participant_id = ?)",
            );
            args.push(Box::new(p.to_string()));
        }
        if let Some((sent_at, id)) = after {
            sql.push_str(" AND (COALESCE(sent_at,'') > ? OR (COALESCE(sent_at,'') = ? AND id > ?))");
            args.push(Box::new(sent_at.to_string()));
            args.push(Box::new(sent_at.to_string()));
            args.push(Box::new(id.to_string()));
        }
        sql.push_str(&format!(" ORDER BY COALESCE(sent_at,''), id LIMIT {limit}"));
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(args.iter().map(|b| b.as_ref())), map)?;
        rows.map(|r| r.map_err(DbError::from)).collect()
    }

    /// The last `limit` messages of a conversation, oldest first — the
    /// transcript the Compose screen shows and the prompt includes.
    pub fn conversation_tail(&self, conversation_id: &str, limit: usize) -> DbResult<Vec<Message>> {
        let conn = self.conn();
        let sql = format!(
            "SELECT * FROM (SELECT {COLS} FROM messages WHERE conversation_id = ?1
             ORDER BY sequence_index DESC LIMIT {limit}) ORDER BY sequence_index"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([conversation_id], map)?;
        rows.map(|r| r.map_err(DbError::from)).collect()
    }

    /// The conversation with this participant that was most recently active.
    pub fn latest_conversation_with(&self, participant_id: &str) -> DbResult<Option<Conversation>> {
        Ok(self
            .conn()
            .query_row(
                &format!(
                    "SELECT {CONV_COLS_Q} FROM conversations c
                     JOIN conversation_participants cp ON cp.conversation_id = c.id
                     WHERE cp.participant_id = ?1
                     ORDER BY c.last_message_at DESC NULLS LAST LIMIT 1"
                ),
                [participant_id],
                map_conv,
            )
            .optional()?)
    }

    pub fn count_self_messages(&self, channel: Option<&str>, participant_id: Option<&str>) -> DbResult<i64> {
        let conn = self.conn();
        Ok(match (channel, participant_id) {
            (None, None) => conn.query_row("SELECT COUNT(*) FROM messages WHERE direction='self'", [], |r| r.get(0))?,
            (Some(c), None) => {
                conn.query_row("SELECT COUNT(*) FROM messages WHERE direction='self' AND channel=?1", [c], |r| {
                    r.get(0)
                })?
            }
            (None, Some(p)) => conn.query_row(
                "SELECT COUNT(*) FROM messages WHERE direction='self' AND conversation_id IN
                 (SELECT conversation_id FROM conversation_participants WHERE participant_id=?1)",
                [p],
                |r| r.get(0),
            )?,
            (Some(c), Some(p)) => conn.query_row(
                "SELECT COUNT(*) FROM messages WHERE direction='self' AND channel=?1 AND conversation_id IN
                 (SELECT conversation_id FROM conversation_participants WHERE participant_id=?2)",
                params![c, p],
                |r| r.get(0),
            )?,
        })
    }

    /// Channels the user has actually written in, with counts.
    pub fn self_message_channels(&self) -> DbResult<Vec<(String, i64)>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT channel, COUNT(*) FROM messages WHERE direction='self' GROUP BY channel ORDER BY COUNT(*) DESC",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.map(|r| r.map_err(DbError::from)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::repo_people::IdentifierInput;
    use crate::db::{IdentifierKind, NewSource};

    fn setup() -> (Db, String, String, String) {
        let db = Db::open_in_memory().unwrap();
        let source = db
            .create_source(&NewSource {
                connector: "test".into(),
                name: "T".into(),
                channel: "chat".into(),
                location: None,
                config: Value::Null,
            })
            .unwrap();
        let convo = db.upsert_conversation(&source.id, "thread-1", "chat", Some("Lunch")).unwrap();
        let ada =
            db.resolve_participant("Ada", &[IdentifierInput::new(IdentifierKind::Handle, "@ada")], false).unwrap();
        db.link_conversation_participant(&convo, &ada).unwrap();
        (db, source.id, convo, ada)
    }

    fn msg(source: &str, convo: &str, ext: &str, dir: &str, seq: i64, at: &str, body: &str) -> NewMessage {
        NewMessage {
            conversation_id: convo.into(),
            source_id: source.into(),
            participant_id: None,
            external_id: ext.into(),
            direction: dir.into(),
            channel: "chat".into(),
            sent_at: Some(at.into()),
            sequence_index: seq,
            body: body.into(),
            reply_to_external_id: None,
            metadata: Value::Null,
        }
    }

    #[test]
    fn re_importing_the_same_export_inserts_nothing() {
        let (db, source, convo, _) = setup();
        let batch = vec![
            msg(&source, &convo, "m1", "other", 0, "2026-01-01T10:00:00Z", "lunch?"),
            msg(&source, &convo, "m2", "self", 1, "2026-01-01T10:02:00Z", "yeah sounds good"),
        ];
        let first = db.insert_messages(&batch).unwrap();
        assert_eq!(first, ImportCounts { inserted: 2, duplicates: 0, empty: 0 });
        let second = db.insert_messages(&batch).unwrap();
        assert_eq!(second, ImportCounts { inserted: 0, duplicates: 2, empty: 0 });
        assert_eq!(db.count_self_messages(None, None).unwrap(), 1);
    }

    #[test]
    fn empty_bodies_are_dropped_not_stored() {
        let (db, source, convo, _) = setup();
        let counts = db
            .insert_messages(&[
                msg(&source, &convo, "m1", "self", 0, "2026-01-01T10:00:00Z", "   \n "),
                msg(&source, &convo, "m2", "self", 1, "2026-01-01T10:01:00Z", "real"),
            ])
            .unwrap();
        assert_eq!(counts, ImportCounts { inserted: 1, duplicates: 0, empty: 1 });
    }

    #[test]
    fn replies_and_latency_are_derived_after_the_batch() {
        let (db, source, convo, _) = setup();
        // Deliberately inserted out of order, as an mbox often is.
        db.insert_messages(&[
            msg(&source, &convo, "m2", "self", 1, "2026-01-01T10:05:00Z", "on my way"),
            msg(&source, &convo, "m1", "other", 0, "2026-01-01T10:00:00Z", "where are you"),
            msg(&source, &convo, "m3", "self", 2, "2026-01-01T10:06:00Z", "two minutes"),
        ])
        .unwrap();
        db.link_replies(&convo).unwrap();
        let tail = db.conversation_tail(&convo, 10).unwrap();
        assert_eq!(tail.len(), 3);
        assert_eq!(tail[0].reply_to_message_id, None, "the first message answers nothing");
        assert_eq!(tail[1].reply_to_message_id.as_deref(), Some(tail[0].id.as_str()));
        assert_eq!(tail[1].response_latency_seconds, Some(300));
        // Two of the user's own messages in a row are not a response time.
        assert_eq!(tail[2].reply_to_message_id.as_deref(), Some(tail[1].id.as_str()));
        assert_eq!(tail[2].response_latency_seconds, None);
    }

    #[test]
    fn conversation_stats_are_recomputed_from_rows() {
        let (db, source, convo, ada) = setup();
        db.insert_messages(&[
            msg(&source, &convo, "m1", "other", 0, "2026-01-01T10:00:00Z", "hi"),
            msg(&source, &convo, "m2", "self", 1, "2026-01-02T10:00:00Z", "hello"),
        ])
        .unwrap();
        db.refresh_conversation_stats(&convo).unwrap();
        let c = db.get_conversation(&convo).unwrap().unwrap();
        assert_eq!(c.message_count, 2);
        assert_eq!(c.started_at.as_deref(), Some("2026-01-01T10:00:00Z"));
        assert_eq!(c.last_message_at.as_deref(), Some("2026-01-02T10:00:00Z"));
        assert!(!c.is_group, "two people is not a group");
        assert_eq!(db.latest_conversation_with(&ada).unwrap().unwrap().id, convo);
    }

    #[test]
    fn self_messages_page_by_keyset_even_with_duplicate_timestamps() {
        let (db, source, convo, _) = setup();
        let same = "2026-01-01T10:00:00Z";
        let batch: Vec<NewMessage> =
            (0..5).map(|i| msg(&source, &convo, &format!("m{i}"), "self", i, same, "text")).collect();
        db.insert_messages(&batch).unwrap();
        let mut seen: Vec<String> = Vec::new();
        let mut cursor: Option<(String, String)> = None;
        loop {
            let page =
                db.page_self_messages(None, None, cursor.as_ref().map(|(a, b)| (a.as_str(), b.as_str())), 2).unwrap();
            if page.is_empty() {
                break;
            }
            let last = page.last().unwrap();
            cursor = Some((last.sent_at.clone().unwrap_or_default(), last.id.clone()));
            seen.extend(page.iter().map(|m| m.id.clone()));
        }
        assert_eq!(seen.len(), 5, "every message is visited exactly once");
        let unique: std::collections::HashSet<_> = seen.iter().collect();
        assert_eq!(unique.len(), 5, "and none twice");
    }

    #[test]
    fn counts_filter_by_channel_and_partner() {
        let (db, source, convo, ada) = setup();
        db.insert_messages(&[msg(&source, &convo, "m1", "self", 0, "2026-01-01T10:00:00Z", "hi")]).unwrap();
        assert_eq!(db.count_self_messages(Some("chat"), None).unwrap(), 1);
        assert_eq!(db.count_self_messages(Some("email"), None).unwrap(), 0);
        assert_eq!(db.count_self_messages(None, Some(&ada)).unwrap(), 1);
        assert_eq!(db.count_self_messages(None, Some("nobody")).unwrap(), 0);
        assert_eq!(db.self_message_channels().unwrap(), vec![("chat".to_string(), 1)]);
    }
}
