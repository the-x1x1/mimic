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

/// Why another way of a draft is not written or kept: the draft was used or
/// put aside first.
pub const ALREADY_CHOSEN: &str =
    "That draft was used or put aside, so there's nothing to write another way of. Nothing else was kept.";

/// Why another way of a draft is not written or kept: the draft went with
/// its person or its mail.
pub const DRAFT_GONE: &str =
    "That draft isn't here any more — its person or its mail was removed — so there's nothing to write another way of.";

/// Why a draft is not decided again: something was already recorded about
/// it, perhaps from another window, or with the rest of its set.
pub const ALREADY_DECIDED: &str = "That draft was already used or put aside, so nothing was changed.";

const COLS: &str = "id, participant_id, conversation_id, channel, situation_id, incoming_message, intent, generated_text, final_text, provider, model, context_json, prompt_hash, evidence_json, created_at, resolved_at, outcome, incoming_message_id, alternative_to";

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
        alternative_to: r.get(18)?,
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
    /// The first draft this one is another way of saying, for drafts written
    /// to be shown beside it. Always a first draft, never an alternative.
    pub alternative_to: Option<String>,
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
    /// model was writing — as drafts already saved to them are kept. Another
    /// way of a first draft is kept only while that draft is still there and
    /// not yet used or put aside: written for a choice already made, it would
    /// be offered beside nothing.
    pub fn create_draft(&self, new: &NewDraft) -> DbResult<Draft> {
        let id = new_id();
        let n = self.conn().execute(
            "INSERT INTO drafts(id, participant_id, conversation_id, channel, situation_id, incoming_message,
                                intent, generated_text, provider, model, context_json, prompt_hash, evidence_json, created_at,
                                incoming_message_id, alternative_to)
             SELECT ?1,(SELECT id FROM participants WHERE id = ?2),?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16
             WHERE ?16 IS NULL
                OR EXISTS (SELECT 1 FROM drafts WHERE id = ?16 AND outcome IS NULL AND alternative_to IS NULL)",
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
                new.incoming_message_id,
                new.alternative_to
            ],
        )?;
        if n == 0 {
            let there = new.alternative_to.as_deref().map(|first| self.get_draft(first)).transpose()?.flatten();
            return Err(DbError::Invalid(if there.is_some() { ALREADY_CHOSEN } else { DRAFT_GONE }.into()));
        }
        self.get_draft(&id)?.ok_or(DbError::NotFound(id))
    }

    /// The other ways of saying a first draft that are still on offer beside
    /// it, oldest first.
    pub fn alternatives_of(&self, first: &str) -> DbResult<Vec<Draft>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLS} FROM drafts WHERE alternative_to = ?1 AND outcome IS NULL ORDER BY created_at, id"
        ))?;
        let rows = stmt.query_map([first], map)?;
        rows.map(|r| r.map_err(DbError::from)).collect()
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
    ///
    /// A draft shown beside other ways of saying it is one of a set, and what
    /// happens to one decides the rest, in the same transaction: using any of
    /// them puts every other still on offer aside as `regenerated` — passed
    /// over for another way, not turned down — and putting the first aside
    /// puts its alternatives aside with it. Putting an alternative aside
    /// leaves the others as they are. A draft already decided is not decided
    /// again: a second window's Use this on a draft passed over would
    /// otherwise make two drafts of one set sent.
    pub fn resolve_draft(&self, id: &str, outcome: &str, final_text: Option<&str>) -> DbResult<Draft> {
        if !["sent_unedited", "sent_edited", "discarded", "regenerated"].contains(&outcome) {
            return Err(DbError::Invalid(format!("unknown draft outcome {outcome:?}")));
        }
        self.transaction(|tx| {
            let now = now_rfc3339();
            let n = tx.execute(
                "UPDATE drafts SET outcome = ?1, final_text = ?2, resolved_at = ?3 WHERE id = ?4 AND outcome IS NULL",
                params![outcome, final_text, now, id],
            )?;
            if n == 0 {
                let there: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM drafts WHERE id = ?1)", [id], |r| r.get(0))?;
                return Err(if there { DbError::Invalid(ALREADY_DECIDED.into()) } else { DbError::NotFound(id.into()) });
            }
            let (first, is_first): (String, bool) = tx.query_row(
                "SELECT COALESCE(alternative_to, id), alternative_to IS NULL FROM drafts WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            match outcome {
                "sent_unedited" | "sent_edited" => {
                    tx.execute(
                        "UPDATE drafts SET outcome = 'regenerated', resolved_at = ?2
                         WHERE outcome IS NULL AND id <> ?3 AND (id = ?1 OR alternative_to = ?1)",
                        params![first, now, id],
                    )?;
                }
                "discarded" | "regenerated" if is_first => {
                    tx.execute(
                        "UPDATE drafts SET outcome = ?3, resolved_at = ?2 WHERE outcome IS NULL AND alternative_to = ?1",
                        params![first, now, outcome],
                    )?;
                }
                _ => {}
            }
            Ok(())
        })?;
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
    /// Other ways of saying a draft are offered with it, not counted apart.
    pub fn pending_drafts(&self, limit: usize) -> DbResult<Vec<Draft>> {
        let conn = self.conn();
        let sql =
            format!("SELECT {COLS} FROM drafts WHERE outcome IS NULL AND alternative_to IS NULL ORDER BY created_at DESC, id DESC LIMIT {limit}");
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
    /// anything on screen. Only a first draft is returned — its other ways
    /// come with it (`alternatives_of`) — and one of those other ways put
    /// aside has not dealt with the message; one used has.
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
                     WHERE d.conversation_id = ?1 AND d.outcome IS NULL AND d.alternative_to IS NULL
                       AND {FOR_MESSAGE}
                       AND NOT EXISTS (
                         SELECT 1 FROM drafts d2
                         WHERE d2.conversation_id = ?1
                           AND (d2.outcome IN ('sent_unedited','sent_edited')
                                OR (d2.outcome = 'discarded' AND d2.alternative_to IS NULL))
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
    /// first. Discarded and unresolved drafts are not evidence of anything,
    /// and nor is a draft written with an adjustment — another way the user
    /// asked for, or Compose's Shorter and the rest: what was changed in it
    /// was changed from what the user asked for, not from how Mimic writes,
    /// so trimming a Longer draft back down says nothing about length.
    pub fn sent_drafts_for_learning(&self) -> DbResult<Vec<(Option<String>, String, String)>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT participant_id, generated_text, final_text FROM drafts
             WHERE outcome IN ('sent_unedited','sent_edited') AND final_text IS NOT NULL
               AND json_extract(context_json, '$.adjustment') IS NULL
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
            alternative_to: None,
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

    fn beside(db: &Db, first: &Draft, text: &str) -> Draft {
        db.create_draft(&NewDraft { alternative_to: Some(first.id.clone()), ..draft(text) }).unwrap()
    }

    fn outcome(db: &Db, d: &Draft) -> Option<String> {
        db.get_draft(&d.id).unwrap().unwrap().outcome
    }

    #[test]
    fn using_any_way_of_saying_it_passes_the_others_over_rather_than_turning_them_down() {
        let db = Db::open_in_memory().unwrap();
        let first = db.create_draft(&draft("Sure, I'll send the deck tomorrow.")).unwrap();
        let shorter = beside(&db, &first, "Tomorrow!");
        let longer = beside(&db, &first, "Sure — I'll send the deck first thing tomorrow, with the notes.");
        assert_eq!(db.pending_drafts(10).unwrap().len(), 1, "a set is one draft waiting");
        let offered: Vec<String> = db.alternatives_of(&first.id).unwrap().into_iter().map(|d| d.id).collect();
        assert_eq!(offered, [shorter.id.clone(), longer.id.clone()], "oldest first");

        db.resolve_draft(&shorter.id, "sent_unedited", Some("Tomorrow!")).unwrap();
        assert_eq!(outcome(&db, &shorter).as_deref(), Some("sent_unedited"));
        assert_eq!(outcome(&db, &first).as_deref(), Some("regenerated"));
        assert_eq!(outcome(&db, &longer).as_deref(), Some("regenerated"));
        // A second window still showing the set can't make a second one sent.
        let again = db.resolve_draft(&longer.id, "sent_unedited", Some(&longer.generated_text));
        assert!(matches!(again, Err(DbError::Invalid(ref m)) if m == ALREADY_DECIDED));
        assert_eq!(outcome(&db, &longer).as_deref(), Some("regenerated"));
        let out = db.measured_draft_outcomes().unwrap();
        assert_eq!((out.resolved, out.sent_unedited, out.discarded), (3, 1, 0), "nothing was turned down");
    }

    #[test]
    fn putting_the_first_aside_puts_its_other_ways_aside_and_one_other_way_only_itself() {
        let db = Db::open_in_memory().unwrap();
        let first = db.create_draft(&draft("Sure, tomorrow.")).unwrap();
        let shorter = beside(&db, &first, "Tomorrow.");
        let casual = beside(&db, &first, "yep tmrw");

        db.resolve_draft(&shorter.id, "discarded", None).unwrap();
        assert_eq!((outcome(&db, &first), outcome(&db, &casual)), (None, None), "the rest stay on offer");
        let offered: Vec<String> = db.alternatives_of(&first.id).unwrap().into_iter().map(|d| d.id).collect();
        assert_eq!(offered, std::slice::from_ref(&casual.id));

        db.resolve_draft(&first.id, "discarded", None).unwrap();
        assert_eq!(outcome(&db, &casual).as_deref(), Some("discarded"));
        assert!(db.pending_drafts(10).unwrap().is_empty());
    }

    #[test]
    fn another_way_is_kept_only_beside_a_first_draft_still_on_offer() {
        let db = Db::open_in_memory().unwrap();
        let first = db.create_draft(&draft("Sure, tomorrow.")).unwrap();
        let other = beside(&db, &first, "Tomorrow.");
        let refused = |to: &str| {
            let r = db.create_draft(&NewDraft { alternative_to: Some(to.into()), ..draft("x") });
            matches!(r, Err(DbError::Invalid(ref m)) if m == ALREADY_CHOSEN)
        };
        assert!(refused(&other.id), "another way of another way is not a set of its own");
        let gone = db.create_draft(&NewDraft { alternative_to: Some("gone".into()), ..draft("x") });
        assert!(matches!(gone, Err(DbError::Invalid(ref m)) if m == DRAFT_GONE));
        db.resolve_draft(&first.id, "sent_edited", Some("Sure, tomorrow at 9.")).unwrap();
        assert!(refused(&first.id), "a choice already made");
        assert_eq!(db.recent_drafts(10).unwrap().len(), 2, "nothing refused was kept");
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
