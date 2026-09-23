//! Participants — the people the user talks to — and the identifier
//! resolution that decides whether two addresses are the same person.

use std::collections::HashSet;

use rusqlite::{params, OptionalExtension, Row};
use serde::Serialize;

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

/// Which participants to list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeopleFilter {
    /// Everyone who is not a sender of automated mail (`classified_cte`).
    People,
    /// Senders all of whose mail looks automated from its headers, whom the
    /// user has neither written to nor said anything about.
    AutomatedSenders,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PeopleCounts {
    pub people: i64,
    pub automated_senders: i64,
}

/// The People screen. `people` may be fewer than `people_total`, and the
/// screen says so; `automated_senders` is empty unless it was asked for, and
/// `showing_automated` says which of those an empty list means.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PeopleView {
    pub people: Vec<ParticipantSummary>,
    pub people_total: i64,
    pub automated_senders_total: i64,
    pub automated_senders: Vec<ParticipantSummary>,
    pub showing_automated: bool,
}

/// Which participants are senders of automated mail, one row per participant
/// other than the user. A sender is automated only when all of these hold:
///
/// * they sent at least one message, and every one reads as automated from
///   its headers — one ordinary message makes them a person;
/// * the user has not said how they know them (`relationship`);
/// * the user has not written in any conversation they are in — an
///   out-of-office reply is often the only message a new contact has sent.
///   Forwarding or answering a sender's mail counts, so an "unsubscribe me"
///   reply keeps a newsletter among people: the error on the side of a person;
/// * the user has not said one of their threads needs a reply.
///
/// The user's word, or the user's own writing, outranks the headers, as it
/// does for threads. Each exception is worked out once as a set, and the
/// classification is materialized, because the People list and its count
/// both read it and it would otherwise be recomputed per reference.
fn classified_cte() -> String {
    let automated = super::repo_waiting::automated_of("x");
    format!(
        "sent AS (
           SELECT x.participant_id AS pid,
                  COUNT(*) AS from_them,
                  SUM({automated} IS NOT NULL) AS automated_from_them
           FROM messages x
           WHERE x.participant_id IS NOT NULL AND x.direction = 'other'
           GROUP BY x.participant_id
         ),
         written_to AS (
           SELECT DISTINCT cp.participant_id AS pid
           FROM messages mine
           CROSS JOIN conversation_participants cp ON cp.conversation_id = mine.conversation_id
           WHERE mine.direction = 'self'
         ),
         kept AS (
           SELECT DISTINCT marked.participant_id AS pid
           FROM thread_marks tm
           JOIN messages marked ON marked.id = tm.message_id
           WHERE tm.mark = 'needs_reply' AND marked.participant_id IS NOT NULL
         ),
         classified AS MATERIALIZED (
           SELECT p.id AS pid,
                  (COALESCE(sent.from_them, 0) > 0
                   AND sent.automated_from_them = sent.from_them
                   AND p.relationship IS NULL
                   AND p.id NOT IN (SELECT pid FROM written_to)
                   AND p.id NOT IN (SELECT pid FROM kept)
                  ) AS automated
           FROM participants p
           LEFT JOIN sent ON sent.pid = p.id
           WHERE p.is_self = 0
         )"
    )
}

/// One row per participant other than the user: what they are in (messages,
/// the user's share, conversations, channels, first and last), whether a
/// relationship profile exists, and whether they are a sender of automated
/// mail (`classified_cte`).
fn summary_cte() -> String {
    format!(
        "WITH seen AS (
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
         ),
         {classified},
         summary AS (
           SELECT p.id, p.display_name, p.is_self, p.relationship, p.notes, p.created_at, p.updated_at,
                  COALESCE(seen.total, 0) AS total, COALESCE(seen.sent_by_user, 0) AS sent_by_user,
                  COALESCE(seen.conversations, 0) AS conversations, COALESCE(seen.channels, '') AS channels,
                  seen.first_at AS first_at, seen.last_at AS last_at,
                  EXISTS(SELECT 1 FROM voice_profiles v WHERE v.layer = 'relationship' AND v.scope_key = p.id) AS has_profile,
                  classified.automated AS automated
           FROM participants p
           JOIN classified ON classified.pid = p.id
           LEFT JOIN seen ON seen.pid = p.id
         )",
        classified = classified_cte()
    )
}

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

    /// Everyone the user's mail has in it, most recent first, with whether
    /// each one has only ever sent automated mail. One grouped query rather
    /// than a per-row count, so this stays flat at a million messages.
    pub fn list_participants(&self, limit: usize) -> DbResult<Vec<ParticipantSummary>> {
        self.participants_where("TRUE", limit)
    }

    /// People, or senders that have only ever sent automated mail, most
    /// recent first. The People screen and every place a person is picked
    /// list the first; the second is what they left out.
    pub fn list_people(&self, which: PeopleFilter, limit: usize) -> DbResult<Vec<ParticipantSummary>> {
        self.participants_where(
            match which {
                PeopleFilter::People => "NOT automated",
                PeopleFilter::AutomatedSenders => "automated",
            },
            limit,
        )
    }

    /// How many people there are, and how many senders have only ever sent
    /// automated mail. Counts of rows, however many the caller asked to see.
    pub fn count_people(&self) -> DbResult<PeopleCounts> {
        Ok(self.conn().query_row(
            &format!(
                "WITH {classified}
                 SELECT COALESCE(SUM(NOT automated), 0), COALESCE(SUM(automated), 0) FROM classified",
                classified = classified_cte()
            ),
            [],
            |r| Ok(PeopleCounts { people: r.get(0)?, automated_senders: r.get(1)? }),
        )?)
    }

    /// The People screen's read model: up to `people_limit` people, how many
    /// there are, and what was left out — the senders themselves only when
    /// `automated_limit` asks for them. A caller that wants only the senders
    /// passes a `people_limit` of 0 rather than paying for a list it ignores.
    pub fn people_view(&self, people_limit: usize, automated_limit: Option<usize>) -> DbResult<PeopleView> {
        let people = if people_limit > 0 { self.list_people(PeopleFilter::People, people_limit)? } else { Vec::new() };
        let automated_senders = match automated_limit {
            Some(limit) => self.list_people(PeopleFilter::AutomatedSenders, limit)?,
            None => Vec::new(),
        };
        // Counted after listing, and never below what was listed: an import
        // landing in between can only add rows, and a total that lagged the
        // list would hide the line saying the list was cut short.
        let counts = self.count_people()?;
        Ok(PeopleView {
            people_total: counts.people.max(people.len() as i64),
            automated_senders_total: counts.automated_senders.max(automated_senders.len() as i64),
            people,
            automated_senders,
            showing_automated: automated_limit.is_some(),
        })
    }

    fn participants_where(&self, filter: &str, limit: usize) -> DbResult<Vec<ParticipantSummary>> {
        let conn = self.conn();
        let sql = format!(
            "{cte}
             SELECT id, display_name, is_self, relationship, notes, created_at, updated_at,
                    total, sent_by_user, conversations, channels, first_at, last_at, has_profile, automated
             FROM summary
             WHERE {filter}
             ORDER BY last_at DESC NULLS LAST, display_name
             LIMIT {limit}",
            cte = summary_cte()
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
                automated: r.get::<_, i64>(14)? != 0,
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

    #[test]
    fn a_sender_of_nothing_but_automated_mail_is_left_out_unless_the_user_says_otherwise() {
        use crate::db::{NewMessage, NewSource};
        use serde_json::{json, Value};
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
        // (who, messages they sent as (body, automated)); nobody sends nothing
        // except Grace, whom the user only wrote to.
        type Sent<'a> = &'a [(&'a str, Option<&'a str>)];
        let cast: [(&str, Sent); 7] = [
            ("Ada", &[("lunch?", None)]),
            ("Brand", &[("sale", Some("newsletter")), ("more sale", Some("newsletter"))]),
            // One message from a person makes them a person.
            ("Shop", &[("order shipped", Some("no_reply_address")), ("sorry, it was lost - refund?", None)]),
            ("Grace", &[]),
            // A new contact whose only message so far is an out-of-office —
            // but the user wrote to them, so they are a person.
            ("Hopper", &[("I'm away until Monday", Some("auto_reply"))]),
            ("Club", &[("meeting saturday", Some("newsletter"))]),
            ("Alerts", &[("your build failed", Some("no_reply_address"))]),
        ];
        let mut seq = 0;
        for (name, sent) in cast {
            let convo = db.upsert_conversation(&source, name, "email", Some(name)).unwrap();
            let who = db.resolve_participant(name, &[email(&format!("{name}@example.com"))], false).unwrap();
            db.link_conversation_participant(&convo, &who).unwrap();
            let mut batch: Vec<NewMessage> = sent
                .iter()
                .map(|(body, automated)| {
                    seq += 1;
                    NewMessage {
                        conversation_id: convo.clone(),
                        source_id: source.clone(),
                        participant_id: Some(who.clone()),
                        external_id: format!("{name}-{seq}"),
                        direction: "other".into(),
                        channel: "email".into(),
                        sent_at: Some(format!("2026-09-{:02}T10:00:00Z", seq)),
                        sequence_index: seq,
                        body: (*body).into(),
                        reply_to_external_id: None,
                        metadata: automated.map(|a| json!({ "automated": a })).unwrap_or(Value::Null),
                    }
                })
                .collect();
            seq += 1;
            batch.push(NewMessage {
                conversation_id: convo.clone(),
                source_id: source.clone(),
                participant_id: None,
                external_id: format!("{name}-mine-{seq}"),
                direction: "self".into(),
                channel: "email".into(),
                sent_at: Some(format!("2026-09-{:02}T10:00:00Z", seq)),
                sequence_index: seq,
                body: "hello".into(),
                reply_to_external_id: None,
                metadata: Value::Null,
            });
            // The user wrote in every conversation except the newsletters'.
            if matches!(name, "Brand" | "Club" | "Alerts") {
                batch.pop();
            }
            db.insert_messages(&batch).unwrap();
        }
        // The user says how they know the club, so it is theirs to call.
        let club =
            db.list_participants(50).unwrap().into_iter().find(|p| p.participant.display_name == "Club").unwrap();
        db.set_participant_relationship(&club.participant.id, Some("book club")).unwrap();
        // And says a thread from the alerts needs a reply.
        let (alerts_convo, alerts_message): (String, String) = db
            .conn()
            .query_row("SELECT conversation_id, id FROM messages WHERE external_id LIKE 'Alerts-%'", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        db.mark_thread(&alerts_convo, &alerts_message, Some(crate::db::ThreadMark::NeedsReply)).unwrap();

        let names = |v: &[ParticipantSummary]| {
            let mut n: Vec<String> = v.iter().map(|p| p.participant.display_name.clone()).collect();
            n.sort();
            n
        };
        assert_eq!(
            names(&db.list_people(PeopleFilter::People, 50).unwrap()),
            ["Ada", "Alerts", "Club", "Grace", "Hopper", "Shop"]
        );
        assert_eq!(names(&db.list_people(PeopleFilter::AutomatedSenders, 50).unwrap()), ["Brand"]);
        assert_eq!(db.count_people().unwrap(), PeopleCounts { people: 6, automated_senders: 1 });
        assert_eq!(db.list_participants(50).unwrap().len(), 7, "everyone is still there for whoever needs everyone");

        let view = db.people_view(2, None).unwrap();
        assert_eq!(view.people.len(), 2, "the list is cut at the limit");
        assert_eq!(view.people_total, 6, "and the total says by how much");
        assert!(view.automated_senders.is_empty() && !view.showing_automated);
        let shown = db.people_view(0, Some(50)).unwrap();
        assert!(shown.people.is_empty(), "a caller after the senders only gets the senders");
        assert_eq!(shown.people_total, 6, "and still the counts");
        assert_eq!(names(&shown.automated_senders), ["Brand"]);
        assert!(shown.automated_senders[0].automated);
    }
}
