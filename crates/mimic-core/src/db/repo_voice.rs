//! Voice profiles, manual overrides and representative examples.

use rusqlite::{params, OptionalExtension, Row};
use serde_json::Value;

use super::models::json_obj;
use super::{Db, DbError, DbResult, RepresentativeExample, VoiceLayer, VoicePreference, VoiceProfileRow};
use crate::ids::{new_id, now_rfc3339};

const COLS: &str = "id, layer, scope_key, participant_id, metrics_json, qualitative_json, sample_size, analysis_version, computed_at, stale";

fn map(r: &Row<'_>) -> rusqlite::Result<VoiceProfileRow> {
    Ok(VoiceProfileRow {
        id: r.get(0)?,
        layer: r.get(1)?,
        scope_key: r.get(2)?,
        participant_id: r.get(3)?,
        metrics: json_obj(r.get(4)?),
        qualitative: json_obj(r.get(5)?),
        sample_size: r.get(6)?,
        analysis_version: r.get(7)?,
        computed_at: r.get(8)?,
        stale: r.get::<_, i64>(9)? != 0,
    })
}

impl Db {
    /// Write (or replace) the profile for one scope at one analysis version.
    #[allow(clippy::too_many_arguments)]
    pub fn put_voice_profile(
        &self,
        layer: VoiceLayer,
        scope_key: &str,
        participant_id: Option<&str>,
        metrics: &Value,
        qualitative: &Value,
        sample_size: i64,
        analysis_version: &str,
    ) -> DbResult<VoiceProfileRow> {
        self.conn().execute(
            "INSERT INTO voice_profiles(id, layer, scope_key, participant_id, metrics_json, qualitative_json,
                                        sample_size, analysis_version, computed_at, stale)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,0)
             ON CONFLICT(layer, scope_key, analysis_version) DO UPDATE SET
               participant_id = excluded.participant_id,
               metrics_json = excluded.metrics_json,
               -- A reading of the numbers in words is kept: it says which
               -- numbers it was written from, so one of older numbers is
               -- shown as that rather than lost.
               qualitative_json = CASE
                 WHEN json_extract(voice_profiles.qualitative_json, '$.description') IS NULL
                   THEN excluded.qualitative_json
                 ELSE json_set(excluded.qualitative_json, '$.description',
                               json(json_extract(voice_profiles.qualitative_json, '$.description')))
               END,
               sample_size = excluded.sample_size,
               computed_at = excluded.computed_at,
               stale = 0",
            params![
                new_id(),
                layer.as_str(),
                scope_key,
                participant_id,
                metrics.to_string(),
                qualitative.to_string(),
                sample_size,
                analysis_version,
                now_rfc3339()
            ],
        )?;
        self.get_voice_profile(layer, scope_key, analysis_version)?
            .ok_or_else(|| DbError::NotFound(format!("{}:{scope_key}", layer.as_str())))
    }

    pub fn get_voice_profile(
        &self,
        layer: VoiceLayer,
        scope_key: &str,
        analysis_version: &str,
    ) -> DbResult<Option<VoiceProfileRow>> {
        Ok(self
            .conn()
            .query_row(
                &format!("SELECT {COLS} FROM voice_profiles WHERE layer=?1 AND scope_key=?2 AND analysis_version=?3"),
                params![layer.as_str(), scope_key, analysis_version],
                map,
            )
            .optional()?)
    }

    /// Whether a relationship-layer profile exists for this person. The
    /// dashboard uses it to say whether a draft would be shaped by how the
    /// user writes *to them*, rather than only by how they write in general.
    pub fn has_relationship_profile(&self, participant_id: &str) -> DbResult<bool> {
        let n: i64 = self.conn().query_row(
            "SELECT COUNT(*) FROM voice_profiles WHERE layer='relationship' AND participant_id=?1",
            [participant_id],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    }

    pub fn list_voice_profiles(&self, analysis_version: &str) -> DbResult<Vec<VoiceProfileRow>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLS} FROM voice_profiles WHERE analysis_version = ?1 ORDER BY layer, sample_size DESC"
        ))?;
        let rows = stmt.query_map([analysis_version], map)?;
        rows.map(|r| r.map_err(DbError::from)).collect()
    }

    /// Mark profiles as needing recomputation. Called when messages change
    /// underneath them — an import, or a deletion.
    pub fn mark_profiles_stale(&self, participant_id: Option<&str>) -> DbResult<usize> {
        let conn = self.conn();
        Ok(match participant_id {
            // A change to one person invalidates their relationship profile
            // and every aggregate computed over material that included them.
            Some(p) => conn.execute(
                "UPDATE voice_profiles SET stale = 1 WHERE participant_id = ?1 OR layer IN ('global','channel','situational')",
                [p],
            )?,
            None => conn.execute("UPDATE voice_profiles SET stale = 1", [])?,
        })
    }

    /// Mark stale the profiles a change to these conversations could move:
    /// where one of them holds a message of the user's, the layer over
    /// everything, the channels those messages went over, the people in
    /// those conversations, and the situations those messages are filed
    /// under. New messages change the replies around them too — how long the
    /// user took to answer — so a conversation counts when anything in it
    /// changed, not only when the user wrote the new message.
    ///
    /// A conversation holding nothing of the user's moves no profile, and a
    /// scope with no profile yet is measured by the next analysis anyway.
    pub fn mark_conversations_changed(&self, conversation_ids: &[String]) -> DbResult<usize> {
        if conversation_ids.is_empty() {
            return Ok(0);
        }
        let ids = serde_json::to_string(conversation_ids).unwrap_or_else(|_| "[]".into());
        Ok(self.conn().execute(
            "WITH touched AS (
               SELECT DISTINCT conversation_id FROM messages
               WHERE direction = 'self' AND conversation_id IN (SELECT value FROM json_each(?1))
             )
             UPDATE voice_profiles SET stale = 1
             WHERE stale = 0 AND (
               (layer = 'global' AND EXISTS (SELECT 1 FROM touched))
               OR (layer = 'channel' AND scope_key IN (
                     SELECT m.channel FROM messages m JOIN touched t ON t.conversation_id = m.conversation_id
                     WHERE m.direction = 'self'))
               OR (layer = 'relationship' AND scope_key IN (
                     SELECT cp.participant_id FROM conversation_participants cp
                     JOIN touched t ON t.conversation_id = cp.conversation_id))
               OR (layer = 'situational' AND scope_key IN (
                     SELECT ms.situation_id FROM message_situations ms
                     JOIN messages m ON m.id = ms.message_id
                     JOIN touched t ON t.conversation_id = m.conversation_id
                     WHERE m.direction = 'self'))
             )",
            [ids],
        )?)
    }

    /// Everyone the user has written to, with how many of the user's own
    /// messages are in conversations they are in — the count a relationship
    /// layer is measured over. However many people there are.
    pub fn people_written_to(&self) -> DbResult<Vec<(String, String, i64)>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT p.id, p.display_name, COUNT(m.id) FROM participants p
             JOIN conversation_participants cp ON cp.participant_id = p.id
             JOIN messages m ON m.conversation_id = cp.conversation_id AND m.direction = 'self'
             GROUP BY p.id ORDER BY p.id",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Remove one scope's profile, at every analysis version, and its
    /// examples: what it described is no longer in the messages.
    pub fn delete_voice_scope(&self, layer: VoiceLayer, scope_key: &str) -> DbResult<()> {
        self.transaction(|tx| {
            tx.execute(
                "DELETE FROM representative_examples WHERE layer = ?1 AND scope_key = ?2",
                params![layer.as_str(), scope_key],
            )?;
            tx.execute(
                "DELETE FROM voice_profiles WHERE layer = ?1 AND scope_key = ?2",
                params![layer.as_str(), scope_key],
            )?;
            Ok(())
        })
    }

    // ----- manual overrides ----------------------------------------------

    /// A preference the user set by hand. These beat the statistics; the
    /// prompt assembler applies them last.
    pub fn set_voice_preference(
        &self,
        layer: VoiceLayer,
        scope_key: &str,
        key: &str,
        value: &Value,
        note: Option<&str>,
    ) -> DbResult<VoicePreference> {
        if key.trim().is_empty() {
            return Err(DbError::Invalid("preference key cannot be empty".into()));
        }
        let now = now_rfc3339();
        self.conn().execute(
            "INSERT INTO voice_preferences(id, layer, scope_key, key, value_json, note, created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?7)
             ON CONFLICT(layer, scope_key, key) DO UPDATE SET
               value_json = excluded.value_json, note = excluded.note, updated_at = excluded.updated_at",
            params![new_id(), layer.as_str(), scope_key, key.trim(), value.to_string(), note, now],
        )?;
        self.get_voice_preference(layer, scope_key, key.trim())?.ok_or_else(|| DbError::NotFound(key.to_string()))
    }

    pub fn get_voice_preference(
        &self,
        layer: VoiceLayer,
        scope_key: &str,
        key: &str,
    ) -> DbResult<Option<VoicePreference>> {
        Ok(self
            .conn()
            .query_row(
                "SELECT id, layer, scope_key, key, value_json, note, updated_at
                 FROM voice_preferences WHERE layer=?1 AND scope_key=?2 AND key=?3",
                params![layer.as_str(), scope_key, key],
                |r| {
                    Ok(VoicePreference {
                        id: r.get(0)?,
                        layer: r.get(1)?,
                        scope_key: r.get(2)?,
                        key: r.get(3)?,
                        value: serde_json::from_str(&r.get::<_, String>(4)?).unwrap_or(Value::Null),
                        note: r.get(5)?,
                        updated_at: r.get(6)?,
                    })
                },
            )
            .optional()?)
    }

    /// Every preference the user has set, anywhere, newest first — for the
    /// screen that lists them so any of them can be taken back.
    pub fn all_voice_preferences(&self) -> DbResult<Vec<VoicePreference>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, layer, scope_key, key, value_json, note, updated_at
             FROM voice_preferences ORDER BY updated_at DESC, id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(VoicePreference {
                id: r.get(0)?,
                layer: r.get(1)?,
                scope_key: r.get(2)?,
                key: r.get(3)?,
                value: serde_json::from_str(&r.get::<_, String>(4)?).unwrap_or(Value::Null),
                note: r.get(5)?,
                updated_at: r.get(6)?,
            })
        })?;
        rows.map(|r| r.map_err(DbError::from)).collect()
    }

    /// Every override that applies to a scope, innermost last so a caller can
    /// fold them in order.
    pub fn voice_preferences_for(&self, scopes: &[(VoiceLayer, String)]) -> DbResult<Vec<VoicePreference>> {
        let mut out = Vec::new();
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, layer, scope_key, key, value_json, note, updated_at
             FROM voice_preferences WHERE layer=?1 AND scope_key=?2 ORDER BY key",
        )?;
        for (layer, scope) in scopes {
            let rows = stmt.query_map(params![layer.as_str(), scope], |r| {
                Ok(VoicePreference {
                    id: r.get(0)?,
                    layer: r.get(1)?,
                    scope_key: r.get(2)?,
                    key: r.get(3)?,
                    value: serde_json::from_str(&r.get::<_, String>(4)?).unwrap_or(Value::Null),
                    note: r.get(5)?,
                    updated_at: r.get(6)?,
                })
            })?;
            for row in rows {
                out.push(row?);
            }
        }
        Ok(out)
    }

    pub fn delete_voice_preference(&self, id: &str) -> DbResult<()> {
        let n = self.conn().execute("DELETE FROM voice_preferences WHERE id = ?1", [id])?;
        if n == 0 {
            return Err(DbError::NotFound(id.into()));
        }
        Ok(())
    }

    // ----- representative examples ---------------------------------------

    /// Replace the example set for one scope.
    pub fn put_representative_examples(
        &self,
        layer: VoiceLayer,
        scope_key: &str,
        participant_id: Option<&str>,
        examples: &[(String, String, f64)],
    ) -> DbResult<usize> {
        self.transaction(|tx| {
            tx.execute(
                "DELETE FROM representative_examples WHERE layer = ?1 AND scope_key = ?2",
                params![layer.as_str(), scope_key],
            )?;
            let mut stmt = tx.prepare_cached(
                "INSERT OR IGNORE INTO representative_examples(id, message_id, layer, scope_key, participant_id, reason, score, selected_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            )?;
            let now = now_rfc3339();
            for (message_id, reason, score) in examples {
                stmt.execute(params![
                    new_id(),
                    message_id,
                    layer.as_str(),
                    scope_key,
                    participant_id,
                    reason,
                    score,
                    now
                ])?;
            }
            Ok(())
        })?;
        Ok(examples.len())
    }

    pub fn representative_examples(
        &self,
        layer: VoiceLayer,
        scope_key: &str,
        limit: usize,
    ) -> DbResult<Vec<RepresentativeExample>> {
        let conn = self.conn();
        let sql = format!(
            "SELECT e.id, e.message_id, e.layer, e.scope_key, e.participant_id, e.reason, e.score, m.body, m.sent_at
             FROM representative_examples e JOIN messages m ON m.id = e.message_id
             WHERE e.layer = ?1 AND e.scope_key = ?2 ORDER BY e.score DESC LIMIT {limit}"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![layer.as_str(), scope_key], |r| {
            Ok(RepresentativeExample {
                id: r.get(0)?,
                message_id: r.get(1)?,
                layer: r.get(2)?,
                scope_key: r.get(3)?,
                participant_id: r.get(4)?,
                reason: r.get(5)?,
                score: r.get(6)?,
                body: r.get(7)?,
                sent_at: r.get(8)?,
            })
        })?;
        rows.map(|r| r.map_err(DbError::from)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn profiles_are_replaced_in_place_per_analysis_version() {
        let db = Db::open_in_memory().unwrap();
        let a = db
            .put_voice_profile(
                VoiceLayer::Global,
                "",
                None,
                &json!({"avgWordsPerMessage": 12.0}),
                &json!({}),
                100,
                "voice_v1",
            )
            .unwrap();
        let b = db
            .put_voice_profile(
                VoiceLayer::Global,
                "",
                None,
                &json!({"avgWordsPerMessage": 14.0}),
                &json!({}),
                120,
                "voice_v1",
            )
            .unwrap();
        assert_eq!(a.id, b.id, "same scope and version is one row");
        assert_eq!(b.metrics["avgWordsPerMessage"], 14.0);
        assert_eq!(b.sample_size, 120);
        // A different analysis version is a different row, so old numbers stay
        // readable while a new version is computed.
        db.put_voice_profile(VoiceLayer::Global, "", None, &json!({}), &json!({}), 1, "voice_v2").unwrap();
        assert_eq!(db.list_voice_profiles("voice_v1").unwrap().len(), 1);
        assert_eq!(db.list_voice_profiles("voice_v2").unwrap().len(), 1);
    }

    #[test]
    fn staleness_spreads_from_a_person_to_the_aggregates() {
        let db = Db::open_in_memory().unwrap();
        db.put_voice_profile(VoiceLayer::Global, "", None, &json!({}), &json!({}), 1, "v").unwrap();
        db.put_voice_profile(VoiceLayer::Channel, "email", None, &json!({}), &json!({}), 1, "v").unwrap();
        let p1 = db
            .resolve_participant(
                "Ada",
                &[crate::db::IdentifierInput::new(crate::db::IdentifierKind::Email, "a@b.c")],
                false,
            )
            .unwrap();
        db.put_voice_profile(VoiceLayer::Relationship, &p1, Some(&p1), &json!({}), &json!({}), 1, "v").unwrap();
        db.mark_profiles_stale(Some(&p1)).unwrap();
        let all = db.list_voice_profiles("v").unwrap();
        assert!(all.iter().all(|p| p.stale), "{all:?}");
        // Recomputing one clears only that one.
        db.put_voice_profile(VoiceLayer::Global, "", None, &json!({}), &json!({}), 2, "v").unwrap();
        let all = db.list_voice_profiles("v").unwrap();
        assert!(!all.iter().find(|p| p.layer == "global").unwrap().stale);
        assert!(all.iter().find(|p| p.layer == "channel").unwrap().stale);
    }

    #[test]
    fn manual_preferences_are_upserted_and_scoped() {
        let db = Db::open_in_memory().unwrap();
        db.set_voice_preference(VoiceLayer::Global, "", "signOff", &json!("— C"), None).unwrap();
        db.set_voice_preference(VoiceLayer::Relationship, "p1", "signOff", &json!("c"), Some("she knows me")).unwrap();
        let updated = db.set_voice_preference(VoiceLayer::Global, "", "signOff", &json!("Thanks, C"), None).unwrap();
        assert_eq!(updated.value, json!("Thanks, C"));
        assert!(db.set_voice_preference(VoiceLayer::Global, "", "  ", &json!(1), None).is_err());

        let folded = db
            .voice_preferences_for(&[(VoiceLayer::Global, String::new()), (VoiceLayer::Relationship, "p1".into())])
            .unwrap();
        assert_eq!(folded.len(), 2);
        assert_eq!(folded[0].value, json!("Thanks, C"));
        assert_eq!(folded[1].value, json!("c"), "the innermost scope comes last");

        db.delete_voice_preference(&folded[1].id).unwrap();
        assert!(db.delete_voice_preference(&folded[1].id).is_err());
    }
}
