//! Which of the user's messages are filed under which situation, and who
//! decided.
//!
//! Three hands file messages, and a later one never gives way to an earlier:
//!
//! * the rules (`classified_by = 'rule'`), on every analysis, for every one
//!   of the user's messages nobody else has decided about; a row changes only
//!   where the rules now say something else;
//! * a model on this computer (`'model'`), which replaces what the rules
//!   said about a message it read;
//! * the user (`'user'`), whose decision replaces both and stands until they
//!   hand the message back to the rules.
//!
//! A decision by the model or the user is recorded in `situation_readings`,
//! because "doing none of these" leaves no row in `message_situations`.

use std::collections::BTreeSet;

use rusqlite::{params, OptionalExtension, Transaction};

use super::{Db, DbError, DbResult, Message};
use crate::ids::now_rfc3339;

/// Who filed a message, as the screen says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FiledBy {
    Rules,
    Model,
    You,
}

/// What one of the user's messages is filed under, and by whom. Strongest
/// first.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Filing {
    pub by: FiledBy,
    pub situations: Vec<String>,
}

/// How the user's own messages came to be filed: by each hand, and how many
/// no model has read.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilingCounts {
    pub by_rules: i64,
    pub by_model: i64,
    pub by_you: i64,
}

/// Every situation `message_id` is filed under, by any hand.
fn filed_under(tx: &Transaction<'_>, message_id: &str) -> DbResult<BTreeSet<String>> {
    let mut stmt = tx.prepare_cached("SELECT situation_id FROM message_situations WHERE message_id = ?1")?;
    let rows = stmt.query_map([message_id], |r| r.get::<_, String>(0))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// File `message_id` under exactly `situations`, by `by`, whatever it was
/// filed under before and by whom. The situations it joined or left.
fn file_as(
    tx: &Transaction<'_>,
    message_id: &str,
    situations: &[(String, f64)],
    by: &str,
    now: &str,
) -> DbResult<BTreeSet<String>> {
    let before = filed_under(tx, message_id)?;
    tx.execute("DELETE FROM message_situations WHERE message_id = ?1", [message_id])?;
    let mut stmt = tx.prepare_cached(
        "INSERT OR IGNORE INTO message_situations(message_id, situation_id, confidence, classified_by, classified_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;
    let mut after = BTreeSet::new();
    for (situation_id, confidence) in situations {
        stmt.execute(params![message_id, situation_id, confidence, by, now])?;
        after.insert(situation_id.clone());
    }
    Ok(before.symmetric_difference(&after).cloned().collect())
}

/// Mark the layers of situations a message joined or left stale, in the
/// transaction that moved it.
fn mark_stale(tx: &Transaction<'_>, changed: &BTreeSet<String>) -> DbResult<()> {
    if !changed.is_empty() {
        let keys = serde_json::to_string(changed).unwrap_or_else(|_| "[]".into());
        tx.execute(
            "UPDATE voice_profiles SET stale = 1
             WHERE layer = 'situational' AND scope_key IN (SELECT value FROM json_each(?1))",
            [keys],
        )?;
    }
    Ok(())
}

/// The user's own message `message_id`, or why not.
fn own_message(tx: &Transaction<'_>, message_id: &str) -> DbResult<()> {
    let direction: Option<String> =
        tx.query_row("SELECT direction FROM messages WHERE id = ?1", [message_id], |r| r.get(0)).optional()?;
    match direction.as_deref() {
        None => Err(DbError::NotFound(format!("message {message_id}"))),
        Some("self") => Ok(()),
        Some(_) => Err(DbError::Invalid("only your own messages are filed by what they are doing".into())),
    }
}

impl Db {
    /// File a page of the user's messages under what the rules find now:
    /// `(message_id, [(situation_id, confidence)])`, one entry per message
    /// read, found or not. A rule row the rules no longer find is removed, a
    /// new one added, and one found again keeps its place with the new
    /// confidence. Where the user filed the same message under the same
    /// situation, theirs stands.
    ///
    /// Returns the situations a message joined or left, having marked their
    /// layers stale in the same transaction, so a stop between filing and
    /// measuring still leaves them to be measured.
    pub fn refile_by_rule(&self, filed: &[(String, Vec<(String, f64)>)]) -> DbResult<BTreeSet<String>> {
        if filed.is_empty() {
            return Ok(BTreeSet::new());
        }
        self.transaction(|tx| {
            let ids = serde_json::to_string(&filed.iter().map(|(id, _)| id).collect::<Vec<_>>())
                .unwrap_or_else(|_| "[]".into());
            let mut had: std::collections::HashMap<String, BTreeSet<String>> = std::collections::HashMap::new();
            // A message a model read or the user decided about is not the
            // rules' to file.
            let decided: std::collections::HashSet<String> = {
                let mut stmt = tx.prepare_cached(
                    "SELECT message_id FROM situation_readings WHERE message_id IN (SELECT value FROM json_each(?1))",
                )?;
                let rows = stmt.query_map([&ids], |r| r.get::<_, String>(0))?;
                rows.collect::<Result<_, _>>()?
            };
            {
                let mut stmt = tx.prepare_cached(
                    "SELECT message_id, situation_id FROM message_situations
                     WHERE classified_by = 'rule' AND message_id IN (SELECT value FROM json_each(?1))",
                )?;
                let rows = stmt.query_map([&ids], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
                for row in rows {
                    let (message_id, situation_id) = row?;
                    had.entry(message_id).or_default().insert(situation_id);
                }
            }
            let mut forget = tx.prepare_cached(
                "DELETE FROM message_situations WHERE message_id = ?1 AND situation_id = ?2 AND classified_by = 'rule'",
            )?;
            let mut file = tx.prepare_cached(
                "INSERT INTO message_situations(message_id, situation_id, confidence, classified_by, classified_at)
                 VALUES (?1, ?2, ?3, 'rule', ?4)
                 ON CONFLICT(message_id, situation_id) DO UPDATE SET confidence = excluded.confidence
                 WHERE message_situations.classified_by = 'rule'
                   AND message_situations.confidence <> excluded.confidence",
            )?;
            let now = now_rfc3339();
            let mut changed = BTreeSet::new();
            for (message_id, found) in filed.iter().filter(|(id, _)| !decided.contains(id)) {
                let before = had.remove(message_id).unwrap_or_default();
                for gone in before.iter().filter(|s| !found.iter().any(|(f, _)| f == *s)) {
                    forget.execute(params![message_id, gone])?;
                    changed.insert(gone.clone());
                }
                for (situation_id, confidence) in found {
                    let wrote = file.execute(params![message_id, situation_id, confidence, now])?;
                    if wrote > 0 && !before.contains(situation_id) {
                        changed.insert(situation_id.clone());
                    }
                }
            }
            mark_stale(tx, &changed)?;
            Ok(changed)
        })
    }

    /// The user says what their message `message_id` is doing: exactly these
    /// situations, or none of them. It stands over the rules and a model
    /// until they hand it back (`hand_back_to_rules`). The situations it
    /// joined or left, their layers marked stale.
    pub fn decide_situations(&self, message_id: &str, situations: &[String]) -> DbResult<BTreeSet<String>> {
        self.transaction(|tx| {
            own_message(tx, message_id)?;
            let now = now_rfc3339();
            let rows: Vec<(String, f64)> = situations.iter().map(|s| (s.clone(), 1.0)).collect();
            let changed = file_as(tx, message_id, &rows, "user", &now)?;
            tx.execute(
                "INSERT INTO situation_readings(message_id, read_by, version, read_at) VALUES (?1, 'user', 'user', ?2)
                 ON CONFLICT(message_id) DO UPDATE SET read_by = 'user', version = 'user', read_at = excluded.read_at",
                params![message_id, now],
            )?;
            mark_stale(tx, &changed)?;
            Ok(changed)
        })
    }

    /// Forget what the user or a model decided about `message_id`, and file
    /// it as the rules find it now (`found`, from `situations::classify`).
    pub fn hand_back_to_rules(&self, message_id: &str, found: &[(String, f64)]) -> DbResult<BTreeSet<String>> {
        self.transaction(|tx| {
            own_message(tx, message_id)?;
            tx.execute("DELETE FROM situation_readings WHERE message_id = ?1", [message_id])?;
            let changed = file_as(tx, message_id, found, "rule", &now_rfc3339())?;
            mark_stale(tx, &changed)?;
            Ok(changed)
        })
    }

    /// File what a model on this computer read: `(message_id, situations)`,
    /// each replacing what the rules said about that message. A message the
    /// user decided about, or one that is no longer theirs, is left as it
    /// is. `version` names the model and the way it was asked. The
    /// situations a message joined or left, their layers marked stale.
    pub fn read_by_model(
        &self,
        readings: &[(String, Vec<String>)],
        confidence: f64,
        version: &str,
    ) -> DbResult<BTreeSet<String>> {
        self.transaction(|tx| {
            let now = now_rfc3339();
            let mut changed = BTreeSet::new();
            for (message_id, situations) in readings {
                let open: bool = tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM messages WHERE id = ?1 AND direction = 'self')
                        AND NOT EXISTS(SELECT 1 FROM situation_readings WHERE message_id = ?1 AND read_by = 'user')",
                    [message_id],
                    |r| r.get(0),
                )?;
                if !open {
                    continue;
                }
                let rows: Vec<(String, f64)> = situations.iter().map(|s| (s.clone(), confidence)).collect();
                changed.extend(file_as(tx, message_id, &rows, "model", &now)?);
                tx.execute(
                    "INSERT INTO situation_readings(message_id, read_by, version, read_at) VALUES (?1, 'model', ?2, ?3)
                     ON CONFLICT(message_id) DO UPDATE SET read_by = 'model', version = excluded.version,
                                                           read_at = excluded.read_at",
                    params![message_id, version, now],
                )?;
            }
            mark_stale(tx, &changed)?;
            Ok(changed)
        })
    }

    /// The user's own messages no model or person has decided about yet,
    /// most recent first, from after `before` (`(sent_at, id)` of the last
    /// one seen) — the ones a model reading would read next.
    pub fn page_unread_self_messages(&self, before: Option<(&str, &str)>, limit: usize) -> DbResult<Vec<Message>> {
        let conn = self.conn();
        let (at, id) = before.unwrap_or(("", ""));
        let mut stmt = conn.prepare(&format!(
            "SELECT {cols} FROM messages m
             WHERE m.direction = 'self'
               AND NOT EXISTS (SELECT 1 FROM situation_readings sr WHERE sr.message_id = m.id)
               AND (?3 OR COALESCE(m.sent_at, '') < ?1 OR (COALESCE(m.sent_at, '') = ?1 AND m.id < ?2))
             ORDER BY COALESCE(m.sent_at, '') DESC, m.id DESC LIMIT {limit}",
            cols = super::repo_messages::COLS
        ))?;
        let rows = stmt.query_map(params![at, id, before.is_none()], super::repo_messages::map)?;
        rows.map(|r| r.map_err(DbError::from)).collect()
    }

    /// What the user's own message `message_id` is filed under, and by
    /// whom; nothing for a message that is not theirs.
    pub fn filing_of(&self, message_id: &str) -> DbResult<Option<Filing>> {
        let conn = self.conn();
        let direction: Option<String> =
            conn.query_row("SELECT direction FROM messages WHERE id = ?1", [message_id], |r| r.get(0)).optional()?;
        match direction.as_deref() {
            None => return Err(DbError::NotFound(format!("message {message_id}"))),
            Some("self") => {}
            Some(_) => return Ok(None),
        }
        let by = match conn
            .query_row("SELECT read_by FROM situation_readings WHERE message_id = ?1", [message_id], |r| {
                r.get::<_, String>(0)
            })
            .optional()?
            .as_deref()
        {
            Some("user") => FiledBy::You,
            Some(_) => FiledBy::Model,
            None => FiledBy::Rules,
        };
        let mut stmt = conn.prepare(
            "SELECT situation_id FROM message_situations WHERE message_id = ?1 ORDER BY confidence DESC, situation_id",
        )?;
        let situations = stmt.query_map([message_id], |r| r.get::<_, String>(0))?.collect::<Result<_, _>>()?;
        Ok(Some(Filing { by, situations }))
    }

    /// How the user's own messages came to be filed.
    pub fn filing_counts(&self) -> DbResult<FilingCounts> {
        Ok(self.conn().query_row(
            "SELECT COALESCE(SUM(sr.read_by IS NULL), 0), COALESCE(SUM(sr.read_by = 'model'), 0),
                    COALESCE(SUM(sr.read_by = 'user'), 0)
             FROM messages m LEFT JOIN situation_readings sr ON sr.message_id = m.id
             WHERE m.direction = 'self'",
            [],
            |r| Ok(FilingCounts { by_rules: r.get(0)?, by_model: r.get(1)?, by_you: r.get(2)? }),
        )?)
    }

    /// Forget rule rows on messages that are no longer the user's — an
    /// address the user took back, someone kept apart. Only the user's own
    /// messages are filed, and a row left on another's would never be read
    /// again or removed.
    pub fn forget_rule_filing_of_others(&self) -> DbResult<usize> {
        Ok(self.conn().execute(
            "DELETE FROM message_situations WHERE classified_by = 'rule'
               AND EXISTS (SELECT 1 FROM messages m WHERE m.id = message_id AND m.direction <> 'self')",
            [],
        )?)
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
    use super::{FiledBy, Filing, FilingCounts};
    use crate::db::{Db, IdentifierKind, NewMessage, NewSource, VoiceLayer};
    use serde_json::json;

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

    fn rule(situation: &str, confidence: f64) -> Vec<(String, f64)> {
        vec![(situation.to_string(), confidence)]
    }

    #[test]
    fn rule_rows_follow_the_rules_and_user_rows_are_kept() {
        let (db, ids) = with_messages(&[("self", "a"), ("self", "b")]);
        db.conn()
            .execute(
                "INSERT INTO message_situations(message_id, situation_id, confidence, classified_by, classified_at)
                 VALUES (?1, 'thanking', 1.0, 'user', 't')",
                [&ids[0]],
            )
            .unwrap();
        db.put_voice_profile(VoiceLayer::Situational, "declining", None, &json!({}), &json!({}), 1, "v").unwrap();
        db.put_voice_profile(VoiceLayer::Situational, "thanking", None, &json!({}), &json!({}), 1, "v").unwrap();
        let changed = db
            .refile_by_rule(&[(ids[0].clone(), rule("thanking", 0.7)), (ids[1].clone(), rule("declining", 0.8))])
            .unwrap();
        assert_eq!(changed.into_iter().collect::<Vec<_>>(), vec!["declining"], "the user had filed the first already");
        let first = db.situations_for_message(&ids[0]).unwrap();
        assert_eq!(
            first,
            vec![("thanking".to_string(), 1.0, "user".to_string())],
            "the rule does not overwrite the user"
        );
        let stale = |key: &str| db.get_voice_profile(VoiceLayer::Situational, key, "v").unwrap().unwrap().stale;
        assert!(stale("declining"), "a layer a message joined is measured again");
        assert!(!stale("thanking"));

        // Found again, a row keeps its place and takes the new confidence;
        // no longer found, it goes. Neither touches what the user said.
        db.put_voice_profile(VoiceLayer::Situational, "declining", None, &json!({}), &json!({}), 1, "v").unwrap();
        assert!(db.refile_by_rule(&[(ids[1].clone(), rule("declining", 0.9))]).unwrap().is_empty());
        assert_eq!(db.situations_for_message(&ids[1]).unwrap()[0].1, 0.9);
        assert!(!stale("declining"), "nothing joined or left");
        let changed = db.refile_by_rule(&[(ids[0].clone(), vec![]), (ids[1].clone(), vec![])]).unwrap();
        assert_eq!(changed.into_iter().collect::<Vec<_>>(), vec!["declining"]);
        assert!(db.situations_for_message(&ids[1]).unwrap().is_empty());
        assert_eq!(db.situations_for_message(&ids[0]).unwrap().len(), 1, "the rules leave the user's call");
    }

    #[test]
    fn a_conversation_shows_how_each_of_the_users_messages_is_filed() {
        let (db, ids) = with_messages(&[("self", "a"), ("other", "b"), ("self", "c")]);
        db.refile_by_rule(&[(ids[0].clone(), rule("declining", 0.8))]).unwrap();
        db.decide_situations(&ids[1], &["thanking".into(), "apologising".into()]).unwrap();
        let convo: String =
            db.conn().query_row("SELECT conversation_id FROM messages LIMIT 1", [], |r| r.get(0)).unwrap();
        let page = db.conversation_end(&convo, 20).unwrap();
        let filings: Vec<Option<Filing>> = page.messages.iter().map(|m| m.filing.clone()).collect();
        assert_eq!(
            filings,
            vec![
                Some(Filing { by: FiledBy::Rules, situations: vec!["declining".into()] }),
                None,
                Some(Filing { by: FiledBy::You, situations: vec!["apologising".into(), "thanking".into()] }),
            ]
        );
        assert_eq!(db.filing_of(&ids[1]).unwrap(), filings[2]);
        assert_eq!(db.filing_counts().unwrap(), FilingCounts { by_rules: 1, by_model: 0, by_you: 1 });
    }

    #[test]
    fn a_message_that_is_no_longer_the_users_loses_its_rule_rows() {
        let (db, ids) = with_messages(&[("self", "a"), ("self", "b")]);
        db.refile_by_rule(&[(ids[0].clone(), rule("declining", 0.8)), (ids[1].clone(), rule("declining", 0.8))])
            .unwrap();
        db.conn().execute("UPDATE messages SET direction = 'other' WHERE id = ?1", [&ids[0]]).unwrap();
        assert_eq!(db.forget_rule_filing_of_others().unwrap(), 1);
        assert!(db.situations_for_message(&ids[0]).unwrap().is_empty());
        assert_eq!(db.situations_for_message(&ids[1]).unwrap().len(), 1);
    }

    #[test]
    fn counts_only_include_the_users_own_messages() {
        let (db, ids) = with_messages(&[("self", "a"), ("other", "b")]);
        let other: String =
            db.conn().query_row("SELECT id FROM messages WHERE direction = 'other'", [], |r| r.get(0)).unwrap();
        db.refile_by_rule(&[(ids[0].clone(), rule("declining", 0.8)), (other, rule("declining", 0.8))]).unwrap();
        assert_eq!(db.self_situation_counts().unwrap(), vec![("declining".to_string(), 1)]);
        assert!(db.situation_exists("declining").unwrap());
        assert!(!db.situation_exists("gloating").unwrap());
    }
}
