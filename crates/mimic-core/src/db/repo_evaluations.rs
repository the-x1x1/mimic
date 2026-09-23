//! What measuring the drafts reads and writes (`crate::evaluation`).
//!
//! A case names the message answered and the user's reply by id, and is
//! deleted with either of them (migration 0011); their text is read from
//! `messages` when shown. What was written for it is kept with it, and was
//! written from more than those two messages — the conversation before them,
//! examples from other conversations — so deleting any person or mailbox
//! deletes every evaluation (`privacy`). A case only counts while its message
//! is still someone else's and its reply the user's: a person folded into the
//! user since turns the one into the other.

use rusqlite::{params, OptionalExtension};
use serde_json::Value;

use super::{Db, DbResult};
use crate::ids::{new_id, now_rfc3339};

/// Someone else's message, and the user's reply that came straight after it.
#[derive(Debug, Clone, PartialEq)]
pub struct Exchange {
    pub incoming_id: String,
    pub incoming: String,
    pub reply_id: String,
    pub reply: String,
    pub conversation_id: String,
    /// Who wrote the incoming message, when they could be named.
    pub participant_id: Option<String>,
    pub channel: String,
}

/// An evaluation to record, with every answer written for it.
#[derive(Debug, Clone)]
pub struct NewEvaluation {
    pub analysis_version: String,
    pub provider: String,
    pub model: Option<String>,
    pub config: Value,
    pub split: Value,
    pub cases: Vec<NewEvaluationCase>,
}

/// One answer to one held-out exchange, and how it compared.
#[derive(Debug, Clone)]
pub struct NewEvaluationCase {
    pub incoming_message_id: String,
    pub reply_message_id: String,
    /// `mimic`, `generic` or `common_reply`.
    pub system: String,
    pub generated_text: String,
    /// For the common reply: the user's message it was taken from.
    pub based_on_message_id: Option<String>,
    pub metrics: Value,
    /// Every message the answer was written from: the conversation before
    /// the message answered, and each example and the message it answered.
    /// A case is recorded only if all of them are still here — someone
    /// deleted while it was being measured takes it with them.
    pub sources: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EvaluationRow {
    pub id: String,
    pub analysis_version: String,
    pub created_at: String,
    pub provider: String,
    pub model: Option<String>,
    pub config: Value,
    pub split: Value,
}

/// A case as it is shown: the messages read from where they are now.
#[derive(Debug, Clone, PartialEq)]
pub struct EvaluationCaseRow {
    pub conversation_id: String,
    pub incoming_message_id: String,
    pub incoming: String,
    /// Who wrote the incoming message, when they could be named.
    pub from: Option<String>,
    pub reply_message_id: String,
    pub reply: String,
    pub system: String,
    pub generated_text: String,
    pub metrics: Value,
}

impl Db {
    /// Every exchange there is to measure on, newest reply first, up to
    /// `limit`: a message from someone else that does not look automated
    /// (nobody answers a receipt in their own voice), and the user's reply
    /// that came straight after it in the same conversation.
    pub fn evaluation_exchanges(&self, limit: usize) -> DbResult<Vec<Exchange>> {
        let automated = super::repo_waiting::automated_of("prev");
        let sql = format!(
            "SELECT prev.id, prev.body, m.id, m.body, m.conversation_id, prev.participant_id, m.channel
             FROM messages m
             JOIN messages prev ON prev.id = m.reply_to_message_id AND prev.conversation_id = m.conversation_id
             WHERE m.direction = 'self' AND prev.direction = 'other' AND ({automated}) IS NULL
             ORDER BY m.sent_at DESC, m.id DESC
             LIMIT {limit}"
        );
        let conn = self.conn();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], |r| {
            Ok(Exchange {
                incoming_id: r.get(0)?,
                incoming: r.get(1)?,
                reply_id: r.get(2)?,
                reply: r.get(3)?,
                conversation_id: r.get(4)?,
                participant_id: r.get(5)?,
                channel: r.get(6)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Record an evaluation and its cases in one transaction. A case with any
    /// message deleted while it was being measured — its own, or one it was
    /// written from — is not recorded: deleting a person deletes every
    /// measurement, and one still running must not bring their words back.
    /// With no case left, nothing is recorded. Returns the evaluation's id
    /// and how many cases were kept.
    pub fn record_evaluation(&self, new: &NewEvaluation) -> DbResult<(String, usize)> {
        let id = new_id();
        let kept = self.transaction(|tx| {
            tx.execute(
                "INSERT INTO evaluations(id, analysis_version, created_at, provider, model, config_json, split_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    id,
                    new.analysis_version,
                    now_rfc3339(),
                    new.provider,
                    new.model,
                    new.config.to_string(),
                    new.split.to_string()
                ],
            )?;
            let mut stmt = tx.prepare(
                "INSERT INTO evaluation_cases(id, evaluation_id, incoming_message_id, reply_message_id, system,
                                              generated_text, based_on_message_id, metrics_json)
                 SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8
                 WHERE EXISTS (SELECT 1 FROM messages WHERE id = ?3)
                   AND EXISTS (SELECT 1 FROM messages WHERE id = ?4)
                   AND (?7 IS NULL OR EXISTS (SELECT 1 FROM messages WHERE id = ?7))
                   AND NOT EXISTS (SELECT 1 FROM json_each(?9) s
                                   WHERE s.value NOT IN (SELECT id FROM messages))",
            )?;
            let mut kept = 0;
            for c in &new.cases {
                kept += stmt.execute(params![
                    new_id(),
                    id,
                    c.incoming_message_id,
                    c.reply_message_id,
                    c.system,
                    c.generated_text,
                    c.based_on_message_id,
                    c.metrics.to_string(),
                    serde_json::to_string(&c.sources).unwrap_or_else(|_| "[]".into())
                ])?;
            }
            // An evaluation with nothing left to show is not recorded: it
            // would become the latest and hide the one before it.
            if kept == 0 {
                tx.execute("DELETE FROM evaluations WHERE id = ?1", [&id])?;
            }
            Ok(kept)
        })?;
        Ok((id, kept))
    }

    /// The most recent evaluation, if any was run.
    pub fn latest_evaluation(&self) -> DbResult<Option<EvaluationRow>> {
        let conn = self.conn();
        let row = conn
            .query_row(
                "SELECT id, analysis_version, created_at, provider, model, config_json, split_json
                 FROM evaluations ORDER BY created_at DESC, id DESC LIMIT 1",
                [],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, Option<String>>(4)?,
                        r.get::<_, String>(5)?,
                        r.get::<_, String>(6)?,
                    ))
                },
            )
            .optional()?;
        Ok(row.map(|(id, analysis_version, created_at, provider, model, config, split)| EvaluationRow {
            id,
            analysis_version,
            created_at,
            provider,
            model,
            config: serde_json::from_str(&config).unwrap_or(Value::Null),
            split: serde_json::from_str(&split).unwrap_or(Value::Null),
        }))
    }

    /// An evaluation's cases that are still here, with the messages they were
    /// measured on read as they are now, newest reply first.
    pub fn evaluation_cases(&self, evaluation_id: &str) -> DbResult<Vec<EvaluationCaseRow>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT c.incoming_message_id, i.body, p.display_name, c.reply_message_id, r.body,
                    c.system, c.generated_text, c.metrics_json, r.conversation_id
             FROM evaluation_cases c
             JOIN messages i ON i.id = c.incoming_message_id AND i.direction = 'other'
             JOIN messages r ON r.id = c.reply_message_id AND r.direction = 'self'
             LEFT JOIN participants p ON p.id = i.participant_id
             WHERE c.evaluation_id = ?1
             ORDER BY r.sent_at DESC, r.id DESC, c.system",
        )?;
        let rows = stmt.query_map([evaluation_id], |r| {
            Ok(EvaluationCaseRow {
                conversation_id: r.get(8)?,
                incoming_message_id: r.get(0)?,
                incoming: r.get(1)?,
                from: r.get(2)?,
                reply_message_id: r.get(3)?,
                reply: r.get(4)?,
                system: r.get(5)?,
                generated_text: r.get(6)?,
                metrics: serde_json::from_str(&r.get::<_, String>(7)?).unwrap_or(Value::Null),
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }
}
