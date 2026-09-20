//! Participants — the people the user talks to — and the identifier
//! resolution that decides whether two addresses are the same person.

use std::collections::HashSet;

use rusqlite::{params, OptionalExtension, Row};

use super::{Db, DbError, DbResult, Identifier, IdentifierKind, Participant, ParticipantSummary};
use crate::ids::{new_id, now_rfc3339};

/// An address seen on an imported message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentifierInput {
    pub kind: IdentifierKind,
    pub value: String,
}

impl IdentifierInput {
    pub fn new(kind: IdentifierKind, value: impl Into<String>) -> Self {
        Self { kind, value: value.into() }
    }
    pub fn normalized(&self) -> String {
        self.kind.normalize(&self.value)
    }
    /// The globally comparable key: kind and normalized value together, so a
    /// handle `5550109999` is never confused with a phone number.
    pub fn key(&self) -> String {
        format!("{}:{}", self.kind.as_str(), self.normalized())
    }
}

fn map(r: &Row<'_>) -> rusqlite::Result<Participant> {
    Ok(Participant {
        id: r.get(0)?,
        display_name: r.get(1)?,
        is_self: r.get::<_, i64>(2)? != 0,
        relationship: r.get(3)?,
        notes: r.get(4)?,
        identifiers: Vec::new(),
        created_at: r.get(5)?,
        updated_at: r.get(6)?,
    })
}

const COLS: &str = "id, display_name, is_self, relationship, notes, created_at, updated_at";

impl Db {
    fn load_identifiers(&self, participant_id: &str) -> DbResult<Vec<Identifier>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, kind, value, normalized_value FROM participant_identifiers
             WHERE participant_id = ?1 ORDER BY kind, normalized_value",
        )?;
        let rows = stmt.query_map([participant_id], |r| {
            Ok(Identifier { id: r.get(0)?, kind: r.get(1)?, value: r.get(2)?, normalized_value: r.get(3)? })
        })?;
        rows.map(|r| r.map_err(DbError::from)).collect()
    }

    pub fn get_participant(&self, id: &str) -> DbResult<Option<Participant>> {
        let base =
            self.conn().query_row(&format!("SELECT {COLS} FROM participants WHERE id = ?1"), [id], map).optional()?;
        match base {
            Some(mut p) => {
                p.identifiers = self.load_identifiers(&p.id)?;
                Ok(Some(p))
            }
            None => Ok(None),
        }
    }

    /// Find the participant who owns any of these identifiers.
    pub fn find_participant_by_identifiers(&self, ids: &[IdentifierInput]) -> DbResult<Option<String>> {
        if ids.is_empty() {
            return Ok(None);
        }
        let conn = self.conn();
        for input in ids {
            let found: Option<String> = conn
                .query_row(
                    "SELECT participant_id FROM participant_identifiers WHERE kind = ?1 AND normalized_value = ?2",
                    params![input.kind.as_str(), input.normalized()],
                    |r| r.get(0),
                )
                .optional()?;
            if found.is_some() {
                return Ok(found);
            }
        }
        Ok(None)
    }

    /// Resolve an author to a participant, creating one if this is the first
    /// time the address has been seen, and attaching any identifier that is
    /// new. Returns the participant id.
    ///
    /// Identifiers with an empty normalized form (a blank display name, a
    /// phone field with no digits) are ignored rather than creating a
    /// participant that would swallow every other unnamed author.
    pub fn resolve_participant(&self, display_name: &str, ids: &[IdentifierInput], is_self: bool) -> DbResult<String> {
        let usable: Vec<&IdentifierInput> = ids.iter().filter(|i| !i.normalized().is_empty()).collect();
        if usable.is_empty() {
            return Err(DbError::Invalid("cannot resolve a participant with no usable identifier".into()));
        }
        let owned: Vec<IdentifierInput> = usable.iter().map(|i| (*i).clone()).collect();
        let existing = self.find_participant_by_identifiers(&owned)?;
        let now = now_rfc3339();
        let participant_id = match existing {
            Some(id) => {
                // A better display name replaces a placeholder one (an address
                // used as a name), but never overwrites a real name.
                if !display_name.trim().is_empty() {
                    self.conn().execute(
                        "UPDATE participants SET display_name = ?1, updated_at = ?2
                         WHERE id = ?3 AND (display_name = '' OR display_name LIKE '%@%' OR display_name GLOB '*[0-9]*' AND display_name NOT GLOB '*[A-Za-z]*')",
                        params![display_name.trim(), now, id],
                    )?;
                }
                id
            }
            None => {
                let id = new_id();
                let name = if display_name.trim().is_empty() {
                    usable[0].value.clone()
                } else {
                    display_name.trim().to_string()
                };
                self.conn().execute(
                    "INSERT INTO participants(id, display_name, is_self, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?4)",
                    params![id, name, is_self as i64, now],
                )?;
                id
            }
        };
        let known: HashSet<String> = self
            .load_identifiers(&participant_id)?
            .into_iter()
            .map(|i| format!("{}:{}", i.kind, i.normalized_value))
            .collect();
        for input in usable {
            if known.contains(&input.key()) {
                continue;
            }
            // Another participant may already own this identifier (two people
            // sharing a family address). Leave it where it is rather than
            // silently merging them.
            self.conn().execute(
                "INSERT OR IGNORE INTO participant_identifiers(id, participant_id, kind, value, normalized_value, first_seen_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![new_id(), participant_id, input.kind.as_str(), input.value, input.normalized(), now],
            )?;
        }
        Ok(participant_id)
    }

    pub fn count_participants(&self) -> DbResult<i64> {
        Ok(self.conn().query_row("SELECT COUNT(*) FROM participants", [], |r| r.get(0))?)
    }

    pub fn set_participant_relationship(&self, id: &str, relationship: Option<&str>) -> DbResult<Participant> {
        let n = self.conn().execute(
            "UPDATE participants SET relationship = ?1, updated_at = ?2 WHERE id = ?3",
            params![relationship.map(str::trim).filter(|s| !s.is_empty()), now_rfc3339(), id],
        )?;
        if n == 0 {
            return Err(DbError::NotFound(id.into()));
        }
        self.get_participant(id)?.ok_or_else(|| DbError::NotFound(id.into()))
    }

    pub fn rename_participant(&self, id: &str, display_name: &str) -> DbResult<Participant> {
        let name = display_name.trim();
        if name.is_empty() {
            return Err(DbError::Invalid("display name cannot be empty".into()));
        }
        let n = self.conn().execute(
            "UPDATE participants SET display_name = ?1, updated_at = ?2 WHERE id = ?3",
            params![name, now_rfc3339(), id],
        )?;
        if n == 0 {
            return Err(DbError::NotFound(id.into()));
        }
        self.get_participant(id)?.ok_or_else(|| DbError::NotFound(id.into()))
    }

    /// People the user actually talks to, most recent first. One grouped query
    /// rather than a per-row count, so this stays flat at a million messages.
    pub fn list_participants(&self, limit: usize) -> DbResult<Vec<ParticipantSummary>> {
        let conn = self.conn();
        let sql = format!(
            "SELECT p.id, p.display_name, p.is_self, p.relationship, p.notes, p.created_at, p.updated_at,
                    COALESCE(m.total, 0), COALESCE(m.sent_by_user, 0), COALESCE(m.conversations, 0),
                    COALESCE(m.channels, ''), m.first_at, m.last_at,
                    EXISTS(SELECT 1 FROM voice_profiles v WHERE v.layer = 'relationship' AND v.scope_key = p.id)
             FROM participants p
             LEFT JOIN (
               SELECT cp.participant_id AS pid,
                      COUNT(msg.id) AS total,
                      SUM(CASE WHEN msg.direction = 'self' THEN 1 ELSE 0 END) AS sent_by_user,
                      COUNT(DISTINCT msg.conversation_id) AS conversations,
                      GROUP_CONCAT(DISTINCT msg.channel) AS channels,
                      MIN(msg.sent_at) AS first_at,
                      MAX(msg.sent_at) AS last_at
               FROM conversation_participants cp
               JOIN messages msg ON msg.conversation_id = cp.conversation_id
               GROUP BY cp.participant_id
             ) m ON m.pid = p.id
             WHERE p.is_self = 0
             ORDER BY m.last_at DESC NULLS LAST, p.display_name
             LIMIT {limit}"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], |r| {
            let channels: String = r.get(10)?;
            Ok(ParticipantSummary {
                participant: Participant {
                    id: r.get(0)?,
                    display_name: r.get(1)?,
                    is_self: r.get::<_, i64>(2)? != 0,
                    relationship: r.get(3)?,
                    notes: r.get(4)?,
                    identifiers: Vec::new(),
                    created_at: r.get(5)?,
                    updated_at: r.get(6)?,
                },
                message_count: r.get(7)?,
                sent_by_user: r.get(8)?,
                conversation_count: r.get(9)?,
                channels: channels.split(',').filter(|s| !s.is_empty()).map(str::to_string).collect(),
                first_message_at: r.get(11)?,
                last_message_at: r.get(12)?,
                has_relationship_profile: r.get::<_, i64>(13)? != 0,
            })
        })?;
        let mut out: Vec<ParticipantSummary> = rows.collect::<Result<_, _>>()?;
        drop(stmt);
        drop(conn);
        for s in &mut out {
            s.participant.identifiers = self.load_identifiers(&s.participant.id)?;
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn email(v: &str) -> IdentifierInput {
        IdentifierInput::new(IdentifierKind::Email, v)
    }

    #[test]
    fn the_same_address_in_any_case_is_the_same_person() {
        let db = Db::open_in_memory().unwrap();
        let a = db.resolve_participant("Ada", &[email("Ada@Example.com")], false).unwrap();
        let b = db.resolve_participant("", &[email("ada@example.com")], false).unwrap();
        assert_eq!(a, b);
        assert_eq!(db.get_participant(&a).unwrap().unwrap().identifiers.len(), 1);
    }

    #[test]
    fn a_second_address_joins_the_person_it_arrived_with() {
        let db = Db::open_in_memory().unwrap();
        let a = db.resolve_participant("Ada", &[email("ada@work.com")], false).unwrap();
        let again = db.resolve_participant("Ada", &[email("ada@work.com"), email("ada@home.com")], false).unwrap();
        assert_eq!(a, again);
        let p = db.get_participant(&a).unwrap().unwrap();
        assert_eq!(p.identifiers.len(), 2);
        // And the new address now resolves on its own.
        assert_eq!(db.resolve_participant("", &[email("ada@home.com")], false).unwrap(), a);
    }

    #[test]
    fn a_shared_address_is_not_silently_merged_into_a_second_person() {
        let db = Db::open_in_memory().unwrap();
        let house = db.resolve_participant("The Lovelaces", &[email("house@example.com")], false).unwrap();
        // A message signed by Ada but sent from the shared address resolves to
        // the existing owner; it does not move the address.
        let who = db.resolve_participant("Ada", &[email("house@example.com")], false).unwrap();
        assert_eq!(who, house);
        assert_eq!(db.get_participant(&house).unwrap().unwrap().identifiers.len(), 1);
    }

    #[test]
    fn an_address_shaped_name_is_upgraded_but_a_real_name_is_kept() {
        let db = Db::open_in_memory().unwrap();
        let id = db.resolve_participant("", &[email("ada@example.com")], false).unwrap();
        assert_eq!(db.get_participant(&id).unwrap().unwrap().display_name, "ada@example.com");
        db.resolve_participant("Ada Lovelace", &[email("ada@example.com")], false).unwrap();
        assert_eq!(db.get_participant(&id).unwrap().unwrap().display_name, "Ada Lovelace");
        // A later, worse name does not overwrite the good one.
        db.resolve_participant("ada@example.com", &[email("ada@example.com")], false).unwrap();
        assert_eq!(db.get_participant(&id).unwrap().unwrap().display_name, "Ada Lovelace");
    }

    #[test]
    fn unusable_identifiers_are_refused_rather_than_pooled() {
        let db = Db::open_in_memory().unwrap();
        let blank = IdentifierInput::new(IdentifierKind::Phone, "no digits here");
        assert!(db.resolve_participant("Someone", &[blank], false).is_err());
        assert!(db.resolve_participant("Someone", &[], false).is_err());
    }

    #[test]
    fn relationship_and_name_can_be_corrected_by_hand() {
        let db = Db::open_in_memory().unwrap();
        let id = db.resolve_participant("Ada", &[email("a@b.c")], false).unwrap();
        let p = db.set_participant_relationship(&id, Some("  colleague  ")).unwrap();
        assert_eq!(p.relationship.as_deref(), Some("colleague"));
        let p = db.set_participant_relationship(&id, Some("  ")).unwrap();
        assert_eq!(p.relationship, None, "blank clears rather than storing whitespace");
        assert_eq!(db.rename_participant(&id, "Ada L").unwrap().display_name, "Ada L");
        assert!(db.rename_participant(&id, " ").is_err());
        assert!(db.rename_participant("nope", "X").is_err());
    }
}
