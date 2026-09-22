//! Which of the user's messages are filed under which situation.
//!
//! Rows written by the rule classifier (`classified_by = 'rule'`) are
//! replaced wholesale on every analysis. Rows the user set (`'user'`) are
//! never touched by it, and a rule never writes over one.

use rusqlite::params;

use super::{Db, DbResult};
use crate::ids::now_rfc3339;

impl Db {
    /// Forget every rule-made classification before a full re-file.
    pub fn clear_rule_situations(&self) -> DbResult<usize> {
        Ok(self.conn().execute("DELETE FROM message_situations WHERE classified_by = 'rule'", [])?)
    }

    /// File messages under situations, by rule. `(message_id, situation_id,
    /// confidence)`. Where the user already filed the same pair, theirs stands.
    pub fn insert_rule_situations(&self, rows: &[(String, String, f64)]) -> DbResult<usize> {
        if rows.is_empty() {
            return Ok(0);
        }
        self.transaction(|tx| {
            let mut stmt = tx.prepare_cached(
                "INSERT OR IGNORE INTO message_situations(message_id, situation_id, confidence, classified_by, classified_at)
                 VALUES (?1, ?2, ?3, 'rule', ?4)",
            )?;
            let now = now_rfc3339();
            let mut n = 0;
            for (message_id, situation_id, confidence) in rows {
                n += stmt.execute(params![message_id, situation_id, confidence, now])?;
            }
            Ok(n)
        })
    }

    /// How many of the user's own messages are filed under each situation.
    pub fn self_situation_counts(&self) -> DbResult<Vec<(String, i64)>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT ms.situation_id, COUNT(*) FROM message_situations ms
             JOIN messages m ON m.id = ms.message_id
             WHERE m.direction = 'self'
             GROUP BY ms.situation_id ORDER BY ms.situation_id",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Remove a situational layer and its examples. Used when a re-analysis
    /// files nothing under a situation that had a layer before, so the screen
    /// does not keep describing messages that are no longer counted there.
    pub fn delete_situational_layer(&self, situation_id: &str) -> DbResult<()> {
        self.transaction(|tx| {
            tx.execute(
                "DELETE FROM representative_examples WHERE layer = 'situational' AND scope_key = ?1",
                [situation_id],
            )?;
            tx.execute("DELETE FROM voice_profiles WHERE layer = 'situational' AND scope_key = ?1", [situation_id])?;
            Ok(())
        })
    }

    /// Whether a situation id names a row in the vocabulary.
    pub fn situation_exists(&self, id: &str) -> DbResult<bool> {
        let n: i64 = self.conn().query_row("SELECT COUNT(*) FROM situations WHERE id = ?1", [id], |r| r.get(0))?;
        Ok(n > 0)
    }

    /// The situations one message is filed under, strongest first:
    /// `(situation_id, confidence, classified_by)`.
    pub fn situations_for_message(&self, message_id: &str) -> DbResult<Vec<(String, f64, String)>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT situation_id, confidence, classified_by FROM message_situations
             WHERE message_id = ?1 ORDER BY confidence DESC, situation_id",
        )?;
        let rows = stmt.query_map([message_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }
}

#[cfg(test)]
mod tests {
    use crate::db::{Db, IdentifierKind, NewMessage, NewSource};

    fn with_messages(bodies: &[(&str, &str)]) -> (Db, Vec<String>) {
        let db = Db::open_in_memory().unwrap();
        db.set_user_identity("C").unwrap();
        db.add_user_identifier(IdentifierKind::Handle, "@c").unwrap();
        let source = db
            .create_source(&NewSource {
                connector: "t".into(),
                name: "T".into(),
                channel: "chat".into(),
                location: None,
                config: serde_json::Value::Null,
            })
            .unwrap();
        let convo = db.upsert_conversation(&source.id, "t", "chat", None).unwrap();
        let batch: Vec<NewMessage> = bodies
            .iter()
            .enumerate()
            .map(|(i, (dir, body))| NewMessage {
                conversation_id: convo.clone(),
                source_id: source.id.clone(),
                participant_id: None,
                external_id: format!("m{i}"),
                direction: (*dir).into(),
                channel: "chat".into(),
                sent_at: Some(format!("2026-02-01T09:{i:02}:00Z")),
                sequence_index: i as i64,
                body: (*body).into(),
                reply_to_external_id: None,
                metadata: serde_json::Value::Null,
            })
            .collect();
        db.insert_messages(&batch).unwrap();
        let ids = db.page_self_messages(None, None, None, 100).unwrap().into_iter().map(|m| m.id).collect();
        (db, ids)
    }

    #[test]
    fn rule_rows_are_replaced_and_user_rows_are_kept() {
        let (db, ids) = with_messages(&[("self", "a"), ("self", "b")]);
        db.conn()
            .execute(
                "INSERT INTO message_situations(message_id, situation_id, confidence, classified_by, classified_at)
                 VALUES (?1, 'thanking', 1.0, 'user', 't')",
                [&ids[0]],
            )
            .unwrap();
        db.insert_rule_situations(&[
            (ids[0].clone(), "thanking".into(), 0.7),
            (ids[1].clone(), "declining".into(), 0.8),
        ])
        .unwrap();
        let first = db.situations_for_message(&ids[0]).unwrap();
        assert_eq!(
            first,
            vec![("thanking".to_string(), 1.0, "user".to_string())],
            "the rule does not overwrite the user"
        );

        db.clear_rule_situations().unwrap();
        assert!(db.situations_for_message(&ids[1]).unwrap().is_empty());
        assert_eq!(db.situations_for_message(&ids[0]).unwrap().len(), 1, "clearing rules leaves the user's call");
    }

    #[test]
    fn counts_only_include_the_users_own_messages() {
        let (db, ids) = with_messages(&[("self", "a"), ("other", "b")]);
        let other: String =
            db.conn().query_row("SELECT id FROM messages WHERE direction = 'other'", [], |r| r.get(0)).unwrap();
        db.insert_rule_situations(&[(ids[0].clone(), "declining".into(), 0.8), (other, "declining".into(), 0.8)])
            .unwrap();
        assert_eq!(db.self_situation_counts().unwrap(), vec![("declining".to_string(), 1)]);
        assert!(db.situation_exists("declining").unwrap());
        assert!(!db.situation_exists("gloating").unwrap());
    }
}
