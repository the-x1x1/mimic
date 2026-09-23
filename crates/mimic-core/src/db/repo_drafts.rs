//! Drafts and the feedback derived from what the user actually sent.
//!
//! The rule the previous product earned and this one inherits: a number shown
//! to the user must come from rows, not from an estimate. `draft_outcomes`
//! returns `None` for a rate it cannot measure yet rather than zero.

use rusqlite::{params, OptionalExtension, Row};
use serde_json::Value;

use super::models::{json_col, json_obj};
use super::{Db, DbError, DbResult, Draft, DraftFeedback};
use crate::ids::{new_id, now_rfc3339};

const COLS: &str = "id, participant_id, conversation_id, channel, situation_id, incoming_message, intent, generated_text, final_text, provider, model, context_json, prompt_hash, evidence_json, created_at, resolved_at, outcome, incoming_message_id";

/// A draft answers message `?2` (text `?3`): by its recorded id, or — only
/// for drafts written before migration 9 recorded ids — by the text it
/// answered. A draft written since with no id answered pasted text, or a
/// message that has been deleted, and matches nothing.
const FOR_MESSAGE: &str = "(d.incoming_message_id = ?2
      OR (d.incoming_message_id IS NULL AND d.incoming_message = ?3
          AND d.created_at < (SELECT applied_at FROM schema_migrations WHERE version = 9)))";
const FOR_MESSAGE_D2: &str = "(d2.incoming_message_id = ?2
      OR (d2.incoming_message_id IS NULL AND d2.incoming_message = ?3
          AND d2.created_at < (SELECT applied_at FROM schema_migrations WHERE version = 9)))";

fn map(r: &Row<'_>) -> rusqlite::Result<Draft> {
    Ok(Draft {
        id: r.get(0)?,
        participant_id: r.get(1)?,
        conversation_id: r.get(2)?,
        channel: r.get(3)?,
        situation_id: r.get(4)?,
        incoming_message: r.get(5)?,
        intent: r.get(6)?,
        generated_text: r.get(7)?,
        final_text: r.get(8)?,
        provider: r.get(9)?,
        model: r.get(10)?,
        context: json_obj(r.get(11)?),
        prompt_hash: r.get(12)?,
        evidence: json_obj(r.get(13)?),
        created_at: r.get(14)?,
        resolved_at: r.get(15)?,
        outcome: r.get(16)?,
        incoming_message_id: r.get(17)?,
    })
}

#[derive(Debug, Clone)]
pub struct NewDraft {
    pub participant_id: Option<String>,
    pub conversation_id: Option<String>,
    pub channel: String,
    pub situation_id: Option<String>,
    pub incoming_message: Option<String>,
    /// The message this draft answers, when it answers a stored one.
    pub incoming_message_id: Option<String>,
    pub intent: Option<String>,
    pub generated_text: String,
    pub provider: String,
    pub model: String,
    pub context: Value,
    pub prompt_hash: String,
    pub evidence: Value,
}

/// Measured draft outcomes. Every field is `None` until there is something to
/// measure; the UI says "not measured yet" rather than showing 0%.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftOutcomes {
    pub total: i64,
    pub resolved: i64,
    pub sent_unedited: i64,
    pub sent_edited: i64,
    pub discarded: i64,
    /// Sent-unedited as a share of sent drafts. `None` when nothing was sent.
    pub unedited_rate: Option<f64>,
    /// Mean absolute change in word count between draft and what was sent.
    /// `None` when no edited draft has been recorded.
    pub mean_length_delta: Option<f64>,
}

impl Db {
    /// Record a draft. Addressed to nobody if the person it was written for
    /// is gone by the time it is saved — folded back into the user while the
    /// model was writing — as drafts already saved to them are kept.
    pub fn create_draft(&self, new: &NewDraft) -> DbResult<Draft> {
        let id = new_id();
        self.conn().execute(
            "INSERT INTO drafts(id, participant_id, conversation_id, channel, situation_id, incoming_message,
                                intent, generated_text, provider, model, context_json, prompt_hash, evidence_json, created_at,
                                incoming_message_id)
             VALUES (?1,(SELECT id FROM participants WHERE id = ?2),?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
            params![
                id,
                new.participant_id,
                new.conversation_id,
                new.channel,
                new.situation_id,
                new.incoming_message,
                new.intent,
                new.generated_text,
                new.provider,
                new.model,
                new.context.to_string(),
                new.prompt_hash,
                new.evidence.to_string(),
                now_rfc3339(),
                new.incoming_message_id
            ],
        )?;
        self.get_draft(&id)?.ok_or(DbError::NotFound(id))
    }

    pub fn get_draft(&self, id: &str) -> DbResult<Option<Draft>> {
        Ok(self.conn().query_row(&format!("SELECT {COLS} FROM drafts WHERE id = ?1"), [id], map).optional()?)
    }

    pub fn recent_drafts(&self, limit: usize) -> DbResult<Vec<Draft>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!("SELECT {COLS} FROM drafts ORDER BY created_at DESC LIMIT {limit}"))?;
        let rows = stmt.query_map([], map)?;
        rows.map(|r| r.map_err(DbError::from)).collect()
    }

    /// Record what happened to a draft. `final_text` is what the user sent;
    /// pass `None` when it was discarded.
    pub fn resolve_draft(&self, id: &str, outcome: &str, final_text: Option<&str>) -> DbResult<Draft> {
        if !["sent_unedited", "sent_edited", "discarded", "regenerated"].contains(&outcome) {
            return Err(DbError::Invalid(format!("unknown draft outcome {outcome:?}")));
        }
        let n = self.conn().execute(
            "UPDATE drafts SET outcome = ?1, final_text = ?2, resolved_at = ?3 WHERE id = ?4",
            params![outcome, final_text, now_rfc3339(), id],
        )?;
        if n == 0 {
            return Err(DbError::NotFound(id.into()));
        }
        self.get_draft(id)?.ok_or_else(|| DbError::NotFound(id.into()))
    }

    /// Store a feedback row. `kind` is `edit` (derived from the difference
    /// between draft and sent text), `preference` (the user said so) or
    /// `rating`. Weight is how much the learning loop should trust it: an
    /// explicit preference outweighs an inferred edit.
    pub fn add_draft_feedback(
        &self,
        draft_id: &str,
        kind: &str,
        weight: f64,
        diff: &Value,
        note: Option<&str>,
    ) -> DbResult<DraftFeedback> {
        if !["edit", "preference", "rating"].contains(&kind) {
            return Err(DbError::Invalid(format!("unknown feedback kind {kind:?}")));
        }
        let id = new_id();
        self.conn().execute(
            "INSERT INTO draft_feedback(id, draft_id, kind, weight, diff_json, note, created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7)
             ON CONFLICT(draft_id, kind) DO UPDATE SET
               weight = excluded.weight, diff_json = excluded.diff_json,
               note = excluded.note, created_at = excluded.created_at",
            params![id, draft_id, kind, weight, diff.to_string(), note, now_rfc3339()],
        )?;
        self.draft_feedback(draft_id)?
            .into_iter()
            .find(|f| f.kind == kind)
            .ok_or_else(|| DbError::NotFound(draft_id.into()))
    }

    pub fn draft_feedback(&self, draft_id: &str) -> DbResult<Vec<DraftFeedback>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, draft_id, kind, weight, diff_json, note, created_at, applied_to_analysis_version
             FROM draft_feedback WHERE draft_id = ?1 ORDER BY weight DESC, kind",
        )?;
        let rows = stmt.query_map([draft_id], |r| {
            Ok(DraftFeedback {
                id: r.get(0)?,
                draft_id: r.get(1)?,
                kind: r.get(2)?,
                weight: r.get(3)?,
                diff: json_col(r.get(4)?).unwrap_or(Value::Null),
                note: r.get(5)?,
                created_at: r.get(6)?,
                applied_to_analysis_version: r.get(7)?,
            })
        })?;
        rows.map(|r| r.map_err(DbError::from)).collect()
    }

    /// Feedback that no analysis version has consumed yet.
    /// Drafts the user has not resolved yet, newest first. A draft is
    /// "pending" purely because nothing has been recorded about what happened
    /// to it — there is no separate approval state to drift out of step.
    pub fn pending_drafts(&self, limit: usize) -> DbResult<Vec<Draft>> {
        let conn = self.conn();
        let sql =
            format!("SELECT {COLS} FROM drafts WHERE outcome IS NULL ORDER BY created_at DESC, id DESC LIMIT {limit}");
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], map)?;
        rows.map(|r| r.map_err(DbError::from)).collect()
    }

    /// The newest unresolved draft written for this message, unless a draft
    /// for it was used or dropped after this one was written — then the
    /// message has been dealt with, and an older duplicate (one prepared in
    /// the background while the user wrote their own) is not offered again. A
    /// draft the user writes after dropping one is newer than the drop, and is
    /// offered. A draft answers the message it was written for: once someone
    /// writes again, a draft for their earlier message is no longer a reply to
    /// anything on screen.
    pub fn pending_draft_for_message(
        &self,
        conversation_id: &str,
        message_id: &str,
        message_text: &str,
    ) -> DbResult<Option<Draft>> {
        Ok(self
            .conn()
            .query_row(
                &format!(
                    "SELECT {COLS} FROM drafts d
                     WHERE d.conversation_id = ?1 AND d.outcome IS NULL AND {FOR_MESSAGE}
                       AND NOT EXISTS (
                         SELECT 1 FROM drafts d2
                         WHERE d2.conversation_id = ?1
                           AND d2.outcome IN ('sent_unedited','sent_edited','discarded')
                           AND d2.resolved_at > d.created_at
                           AND {FOR_MESSAGE_D2})
                     ORDER BY d.created_at DESC, d.id DESC LIMIT 1"
                ),
                params![conversation_id, message_id, message_text],
                map,
            )
            .optional()?)
    }

    /// Whether any draft, resolved or not, was ever written for this message.
    /// Assisted drafting asks this rather than "is one pending": a draft the
    /// user already used or turned down is an answer too, and writing another
    /// for the same message would spend a model call on a question they dealt
    /// with.
    pub fn any_draft_for_message(&self, conversation_id: &str, message_id: &str, message_text: &str) -> DbResult<bool> {
        Ok(self.conn().query_row(
            &format!("SELECT EXISTS(SELECT 1 FROM drafts d WHERE d.conversation_id = ?1 AND {FOR_MESSAGE})"),
            params![conversation_id, message_id, message_text],
            |r| r.get(0),
        )?)
    }

    /// Every draft the user sent, with who it went to, what Mimic wrote and
    /// what went out: `(participant_id, generated_text, final_text)`, oldest
    /// first. Discarded and unresolved drafts are not evidence of anything.
    pub fn sent_drafts_for_learning(&self) -> DbResult<Vec<(Option<String>, String, String)>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT participant_id, generated_text, final_text FROM drafts
             WHERE outcome IN ('sent_unedited','sent_edited') AND final_text IS NOT NULL
             ORDER BY created_at, id",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        rows.map(|r| r.map_err(DbError::from)).collect()
    }

    pub fn pending_feedback_count(&self) -> DbResult<i64> {
        Ok(self.conn().query_row(
            "SELECT COUNT(*) FROM draft_feedback WHERE applied_to_analysis_version IS NULL",
            [],
            |r| r.get(0),
        )?)
    }

    pub fn measured_draft_outcomes(&self) -> DbResult<DraftOutcomes> {
        let conn = self.conn();
        let (total, resolved, unedited, edited, discarded): (i64, i64, i64, i64, i64) = conn.query_row(
            "SELECT COUNT(*),
                    SUM(outcome IS NOT NULL),
                    SUM(outcome = 'sent_unedited'),
                    SUM(outcome = 'sent_edited'),
                    SUM(outcome = 'discarded')
             FROM drafts",
            [],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get::<_, Option<i64>>(1)?.unwrap_or(0),
                    r.get::<_, Option<i64>>(2)?.unwrap_or(0),
                    r.get::<_, Option<i64>>(3)?.unwrap_or(0),
                    r.get::<_, Option<i64>>(4)?.unwrap_or(0),
                ))
            },
        )?;
        let sent = unedited + edited;
        let unedited_rate = if sent > 0 { Some(unedited as f64 / sent as f64) } else { None };
        let mean_length_delta: Option<f64> = conn.query_row(
            "SELECT AVG(ABS(
                 (LENGTH(final_text) - LENGTH(REPLACE(final_text, ' ', ''))) -
                 (LENGTH(generated_text) - LENGTH(REPLACE(generated_text, ' ', '')))
             )) FROM drafts WHERE outcome = 'sent_edited' AND final_text IS NOT NULL",
            [],
            |r| r.get(0),
        )?;
        Ok(DraftOutcomes {
            total,
            resolved,
            sent_unedited: unedited,
            sent_edited: edited,
            discarded,
            unedited_rate,
            mean_length_delta,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn draft(text: &str) -> NewDraft {
        NewDraft {
            participant_id: None,
            conversation_id: None,
            channel: "email".into(),
            situation_id: None,
            incoming_message: Some("can you send the deck?".into()),
            incoming_message_id: None,
            intent: Some("say yes, tomorrow".into()),
            generated_text: text.into(),
            provider: "mock".into(),
            model: "mock-1".into(),
            context: json!({"layers": ["global"]}),
            prompt_hash: "h".into(),
            evidence: json!({"examples": 0}),
        }
    }

    #[test]
    fn a_draft_for_someone_gone_while_it_was_written_is_kept_addressed_to_nobody() {
        let db = Db::open_in_memory().unwrap();
        let saved = db.create_draft(&NewDraft { participant_id: Some("folded".into()), ..draft("Sure.") }).unwrap();
        assert_eq!(saved.participant_id, None);
        assert_eq!(saved.generated_text, "Sure.");
    }

    #[test]
    fn outcomes_are_unmeasured_until_a_draft_is_resolved() {
        let db = Db::open_in_memory().unwrap();
        let empty = db.measured_draft_outcomes().unwrap();
        assert_eq!(empty.unedited_rate, None, "no drafts means no rate, not 0%");
        assert_eq!(empty.mean_length_delta, None);

        db.create_draft(&draft("Sure — I'll send it tomorrow.")).unwrap();
        let pending = db.measured_draft_outcomes().unwrap();
        assert_eq!((pending.total, pending.resolved), (1, 0));
        assert_eq!(pending.unedited_rate, None, "an unresolved draft is not evidence either");
    }

    #[test]
    fn resolving_drafts_produces_a_real_rate() {
        let db = Db::open_in_memory().unwrap();
        let a = db.create_draft(&draft("Sure, tomorrow.")).unwrap();
        let b = db.create_draft(&draft("I will send the deck tomorrow morning.")).unwrap();
        let c = db.create_draft(&draft("no")).unwrap();
        db.resolve_draft(&a.id, "sent_unedited", Some("Sure, tomorrow.")).unwrap();
        db.resolve_draft(&b.id, "sent_edited", Some("deck tomorrow")).unwrap();
        db.resolve_draft(&c.id, "discarded", None).unwrap();
        assert!(db.resolve_draft(&c.id, "eaten", None).is_err());
        assert!(db.resolve_draft("nope", "discarded", None).is_err());

        let out = db.measured_draft_outcomes().unwrap();
        assert_eq!((out.total, out.resolved, out.sent_unedited, out.sent_edited, out.discarded), (3, 3, 1, 1, 1));
        assert_eq!(out.unedited_rate, Some(0.5), "discarded drafts are not in the denominator");
        // "I will send the deck tomorrow morning." (7 words) -> "deck tomorrow" (2).
        assert_eq!(out.mean_length_delta, Some(5.0));
    }

    #[test]
    fn explicit_preferences_outweigh_inferred_edits_and_replace_in_place() {
        let db = Db::open_in_memory().unwrap();
        let d = db.create_draft(&draft("Hello there,")).unwrap();
        db.add_draft_feedback(&d.id, "edit", 1.0, &json!({"removedGreeting": true}), None).unwrap();
        db.add_draft_feedback(&d.id, "preference", 3.0, &json!({"greeting": "none"}), Some("I never greet")).unwrap();
        assert!(db.add_draft_feedback(&d.id, "vibes", 1.0, &json!({}), None).is_err());

        let fb = db.draft_feedback(&d.id).unwrap();
        assert_eq!(fb.len(), 2);
        assert_eq!(fb[0].kind, "preference", "heavier feedback sorts first");
        assert_eq!(db.pending_feedback_count().unwrap(), 2);

        // Re-recording the same kind updates rather than accumulating.
        db.add_draft_feedback(&d.id, "edit", 2.0, &json!({"removedGreeting": false}), None).unwrap();
        let fb = db.draft_feedback(&d.id).unwrap();
        assert_eq!(fb.len(), 2);
        assert_eq!(fb.iter().find(|f| f.kind == "edit").unwrap().weight, 2.0);
    }
}
