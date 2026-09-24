//! The user's own identity and the addresses they write from.
//!
//! This is load-bearing: `direction` on every imported message is decided by
//! matching the author against these identifiers. If the user has not declared
//! who they are, import still runs but every message lands as `unknown`, and
//! no voice profile can be built from it.
//!
//! Mail read before an address was declared may be filed under a person who
//! was the user all along. Adding the address can fold that person back into
//! the user (`add_user_address`), but only once the user has seen who it is
//! and what goes with them (`preview_user_address`, the same query), because
//! a mistaken address would otherwise turn a real person into the user.
//! Someone whose every address is already the user's, with no relationship,
//! notes or preferences the user recorded for them, is folded without asking
//! (`claim_user_mail`): there is nothing to show that the user has not
//! already said.
//!
//! None of it can be undone. Which address an old message came from is not
//! kept, so removing an address changes only what is read afterwards.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::repo_messages::{link_replies, refresh_conversation_stats};
use super::{Db, DbError, DbResult, Identifier, IdentifierKind, UserIdentity};
use crate::ids::{new_id, now_rfc3339};

/// What folding people back into the user changed. Counts of rows changed,
/// never estimates.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Claimed {
    /// Messages that were counted as someone else's and are now the user's.
    pub messages: usize,
    /// People who were the user under an address, folded back into the user.
    pub people: usize,
}

/// An address added: the identity as it now stands, and what the mail
/// already read turned out to hold.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddressAdded {
    pub identity: UserIdentity,
    pub claimed: Claimed,
}

/// The person mail from an address is filed under, and everything that
/// would change if the user says the address is theirs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddressOwner {
    pub participant_id: String,
    pub display_name: String,
    /// Messages filed under them. Every one would become the user's.
    pub messages: usize,
    /// Their other addresses, not yet the user's, which would become the
    /// user's too. Names they signed with are not addresses and go with them.
    pub other_addresses: Vec<String>,
    /// What the user said they are to them, which would go.
    pub relationship: Option<String>,
    /// Whether the user wrote notes about them, which would go.
    pub has_notes: bool,
    /// Preferences the user set for writing to them, which would go.
    pub preferences: usize,
}

/// What adding an address would do, before anything is changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddressPreview {
    /// It is already one of the user's addresses.
    pub already_yours: bool,
    /// Who the mail already read from it is filed under, if anyone. Adding
    /// it with this person unconfirmed is refused.
    pub owner: Option<AddressOwner>,
}

/// One of the user's addresses that mail already read is still filed under
/// someone else by — left for the user to decide, with what deciding would
/// change.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeldAddress {
    pub identifier: Identifier,
    pub owner: AddressOwner,
    /// The user already said this person is not them (`Db::keep_apart`).
    pub kept_apart: bool,
}

/// Someone mail is filed under who sent mail found in the user's own Sent
/// folder. What is there is usually the user's, so this is often the user
/// under an address Mimic doesn't know yet — but mail sent for someone, and
/// addresses several people send from, end up there too. Asked about, with
/// how much of their mail was there, and never assumed.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SentFolderPerson {
    /// One of their addresses, to add as the user's. Their others come with
    /// it (`owner.other_addresses`).
    pub kind: String,
    pub address: String,
    /// Their messages that were in the user's Sent folder.
    pub sent: usize,
    /// Everything saying yes would change, as adding `address` would.
    pub owner: AddressOwner,
}

/// What `claim_user_mail` did, and what it left for the user.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reconciled {
    pub claimed: Claimed,
    /// People under one of the user's addresses who were left as they are,
    /// because folding them would take something the user has not seen:
    /// another address, or something the user told Mimic about them.
    pub left: usize,
}

/// Jobs that read mail. An address added while one runs is missing from the
/// set it matches authors against, and what it reads from that address is
/// filed under a person.
const READS_MAIL: [&str; 2] = [crate::import::JOB_KIND, crate::sources::imap::JOB_KIND];

/// Jobs that hold people's ids while they run. Folding someone they hold
/// makes their next write fail, or bring the person back.
const HOLDS_PEOPLE: [&str; 6] = [
    crate::import::JOB_KIND,
    crate::sources::imap::JOB_KIND,
    crate::voice::JOB_KIND,
    crate::voice::CHANGED_JOB_KIND,
    crate::assist::JOB_KIND,
    crate::evaluation::JOB_KIND,
];

impl Db {
    /// The single user identity row, with its identifiers.
    pub fn user_identity(&self) -> DbResult<Option<UserIdentity>> {
        let conn = self.conn();
        let row: Option<(String, String, String, String)> = conn
            .query_row("SELECT id, display_name, created_at, updated_at FROM user_identity LIMIT 1", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })
            .optional()?;
        let Some((id, display_name, created_at, updated_at)) = row else { return Ok(None) };
        let mut stmt = conn.prepare(
            "SELECT id, kind, value, normalized_value FROM user_identifiers
             WHERE user_identity_id = ?1 ORDER BY kind, normalized_value",
        )?;
        let identifiers = stmt
            .query_map([&id], |r| {
                Ok(Identifier { id: r.get(0)?, kind: r.get(1)?, value: r.get(2)?, normalized_value: r.get(3)? })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Some(UserIdentity { id, display_name, identifiers, created_at, updated_at }))
    }

    /// Create the identity row or rename it. Identifiers are managed separately.
    pub fn set_user_identity(&self, display_name: &str) -> DbResult<UserIdentity> {
        let name = display_name.trim();
        if name.is_empty() {
            return Err(DbError::Invalid("display name cannot be empty".into()));
        }
        let now = now_rfc3339();
        match self.user_identity()? {
            Some(existing) => {
                self.conn().execute(
                    "UPDATE user_identity SET display_name = ?1, updated_at = ?2 WHERE id = ?3",
                    params![name, now, existing.id],
                )?;
            }
            None => {
                self.conn().execute(
                    "INSERT INTO user_identity(id, display_name, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)",
                    params![new_id(), name, now],
                )?;
            }
        }
        self.user_identity()?.ok_or_else(|| DbError::NotFound("user_identity".into()))
    }

    /// Add an address the user writes from, and nothing else: mail already
    /// read from it stays where it is. Idempotent on kind and normalized
    /// value. What the user adds goes through `add_user_address`.
    pub fn add_user_identifier(&self, kind: IdentifierKind, value: &str) -> DbResult<Identifier> {
        let (value, normalized) = checked(kind, value)?;
        self.transaction(|tx| {
            let identity = identity_id(tx)?;
            insert_identifier(tx, &identity, kind.as_str(), &value, &normalized)
        })
    }

    /// What adding this address would do. Changes nothing; `add_user_address`
    /// runs the same query and refuses if what it finds is not what the
    /// caller confirmed.
    pub fn preview_user_address(&self, kind: IdentifierKind, value: &str) -> DbResult<AddressPreview> {
        let (_, normalized) = checked(kind, value)?;
        let conn = self.conn();
        identity_id(&conn)?;
        Ok(AddressPreview {
            already_yours: is_users(&conn, kind.as_str(), &normalized)?,
            owner: owner_of(&conn, kind.as_str(), &normalized)?,
        })
    }

    /// Add an address the user writes from. If mail already read from it is
    /// filed under a person, that person is folded back into the user — their
    /// messages become the user's, their other addresses become the user's,
    /// and what was derived from or said about them goes — but only when
    /// `confirmed` is exactly what `preview_user_address` finds now: the same
    /// person, with the same messages, addresses and everything else it
    /// named. Something changed since the user was shown it (a check filed
    /// more mail under them, a relationship was set) is refused, so what they
    /// agreed to is what happens. All of it is one transaction.
    ///
    /// Refused while mail is being read (the new address would be missed), or
    /// when there is someone to fold, while anything holding people runs.
    pub fn add_user_address(
        &self,
        kind: IdentifierKind,
        value: &str,
        confirmed: Option<&AddressOwner>,
    ) -> DbResult<AddressAdded> {
        let (value, normalized) = checked(kind, value)?;
        let claimed = self.transaction(|tx| add_address(tx, kind.as_str(), &value, &normalized, confirmed))?;
        let identity = self.user_identity()?.ok_or_else(|| DbError::NotFound("user_identity".into()))?;
        Ok(AddressAdded { identity, claimed })
    }

    /// Settle one of the user's addresses that is still held
    /// (`held_user_addresses`) by folding its holder in — as
    /// `add_user_address` would, on the address exactly as stored, and only
    /// when `confirmed` is what the held list shows now.
    pub fn claim_held_address(&self, identifier_id: &str, confirmed: &AddressOwner) -> DbResult<AddressAdded> {
        let claimed = self.transaction(|tx| {
            let (kind, value, normalized): (String, String, String) = tx
                .query_row(
                    "SELECT kind, value, normalized_value FROM user_identifiers WHERE id = ?1",
                    [identifier_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?
                .ok_or_else(|| DbError::NotFound(identifier_id.into()))?;
            add_address(tx, &kind, &value, &normalized, Some(confirmed))
        })?;
        let identity = self.user_identity()?.ok_or_else(|| DbError::NotFound("user_identity".into()))?;
        Ok(AddressAdded { identity, claimed })
    }

    /// The user said this person, under one of their addresses, is not them.
    /// Nothing moves, and `claim_user_mail` never folds them without asking,
    /// even if what made them worth asking about (a relationship, notes) is
    /// later cleared. Saying yes after all still folds them.
    pub fn keep_apart(&self, participant_id: &str) -> DbResult<()> {
        let n = self.conn().execute(
            "UPDATE participants SET kept_apart = 1, updated_at = ?2 WHERE id = ?1 AND is_self = 0",
            params![participant_id, now_rfc3339()],
        )?;
        if n == 0 {
            return Err(DbError::NotFound(participant_id.into()));
        }
        Ok(())
    }

    /// The user's addresses that mail already read is still filed under
    /// someone else by, each with what folding that person would change.
    pub fn held_user_addresses(&self) -> DbResult<Vec<HeldAddress>> {
        let conn = self.conn();
        let identifiers: Vec<Identifier> = {
            let mut stmt = conn.prepare(
                "SELECT id, kind, value, normalized_value FROM user_identifiers ORDER BY kind, normalized_value",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok(Identifier { id: r.get(0)?, kind: r.get(1)?, value: r.get(2)?, normalized_value: r.get(3)? })
            })?;
            rows.collect::<Result<_, _>>()?
        };
        let mut held = Vec::new();
        for identifier in identifiers {
            if let Some(owner) = owner_of(&conn, &identifier.kind, &identifier.normalized_value)? {
                let kept_apart = is_kept_apart(&conn, &owner.participant_id)?;
                held.push(HeldAddress { identifier, owner, kept_apart });
            }
        }
        Ok(held)
    }

    /// People mail is filed under whose messages were in the user's own Sent
    /// folder — often the user under an address Mimic doesn't have yet, and
    /// asked about (`add_user_address` with the owner shown, or `keep_apart`),
    /// never assumed: a Sent folder can also hold mail sent for someone, or
    /// from an address several people share. Likeliest first: the largest
    /// share of their mail in the Sent folder, then the most. Not someone the
    /// user said isn't them, not someone with no address to add, and not
    /// someone holding an address that is already the user's:
    /// `held_user_addresses` asks about them.
    pub fn sent_folder_people(&self) -> DbResult<Vec<SentFolderPerson>> {
        let conn = self.conn();
        let counted: Vec<(String, i64)> = {
            let mut stmt = conn.prepare(
                "SELECT participant_id, sent FROM (
                   SELECT m.participant_id AS participant_id, COUNT(*) AS sent,
                          (SELECT COUNT(*) FROM messages a WHERE a.participant_id = m.participant_id) AS total
                   FROM messages m
                   JOIN participants p ON p.id = m.participant_id
                   WHERE m.direction = 'other' AND p.is_self = 0 AND p.kept_apart = 0
                     AND CASE WHEN instr(m.metadata_json, '\"sentFolder\"') > 0 AND json_valid(m.metadata_json)
                              THEN json_type(m.metadata_json, '$.sentFolder') END = 'true'
                     AND NOT EXISTS (SELECT 1 FROM participant_identifiers pi
                                     JOIN user_identifiers u ON u.kind = pi.kind AND u.normalized_value = pi.normalized_value
                                     WHERE pi.participant_id = p.id)
                   GROUP BY m.participant_id)
                 ORDER BY CAST(sent AS REAL) / total DESC, sent DESC, participant_id",
            )?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
            rows.collect::<Result<_, _>>()?
        };
        let mut out = Vec::new();
        for (participant_id, sent) in counted {
            // An email address first: that is what a Sent folder shows. A
            // name they signed with is not an address to add.
            let address: Option<(String, String, String)> = conn
                .query_row(
                    "SELECT kind, value, normalized_value FROM participant_identifiers
                     WHERE participant_id = ?1 AND kind <> 'display_name'
                     ORDER BY kind = 'email' DESC, normalized_value LIMIT 1",
                    [&participant_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?;
            let Some((kind, address, normalized)) = address else { continue };
            if let Some(owner) = person(&conn, &participant_id, Some((kind.as_str(), normalized.as_str())))? {
                out.push(SentFolderPerson { kind, address, sent: sent as usize, owner });
            }
        }
        Ok(out)
    }

    /// Fold back into the user everyone whose every address is already one of
    /// the user's and about whom the user recorded nothing that would go
    /// (`is_only_the_user`): the user has already said everything a preview
    /// would ask. Anyone else under one of the user's addresses is left for
    /// the user to decide (`held_user_addresses`), and counted.
    ///
    /// For mail read before an address was declared — by versions before
    /// 0.10.0-alpha.8, or by a read that was running when a mailbox's address
    /// was added. The desktop runs it at start-up, on connecting a mailbox and
    /// after every import or check. Refused, changing nothing, while anything
    /// holding people runs.
    pub fn claim_user_mail(&self) -> DbResult<Reconciled> {
        self.transaction(|tx| {
            let holders: Vec<String> = {
                let mut stmt = tx.prepare(
                    "SELECT DISTINCT pi.participant_id FROM participant_identifiers pi
                     JOIN user_identifiers ui ON ui.kind = pi.kind AND ui.normalized_value = pi.normalized_value
                     ORDER BY pi.participant_id",
                )?;
                let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
                rows.collect::<Result<_, _>>()?
            };
            let mut only_the_user = Vec::new();
            let mut left = 0;
            for id in &holders {
                match person(tx, id, None)? {
                    // Said not to be the user: never folded without asking.
                    Some(_) if is_kept_apart(tx, id)? => left += 1,
                    Some(owner) if is_only_the_user(&owner) => only_the_user.push(owner.participant_id),
                    Some(_) => left += 1,
                    None => {}
                }
            }
            if only_the_user.is_empty() {
                return Ok(Reconciled { claimed: Claimed::default(), left });
            }
            refuse_while(tx, &HOLDS_PEOPLE)?;
            Ok(Reconciled { claimed: fold(tx, &only_the_user)?, left })
        })
    }

    pub fn remove_user_identifier(&self, id: &str) -> DbResult<()> {
        let n = self.conn().execute("DELETE FROM user_identifiers WHERE id = ?1", [id])?;
        if n == 0 {
            return Err(DbError::NotFound(id.into()));
        }
        Ok(())
    }

    /// Every normalized identifier the user writes from, for direction matching.
    pub fn user_identifier_set(&self) -> DbResult<std::collections::HashSet<String>> {
        let conn = self.conn();
        let mut stmt = conn.prepare("SELECT kind || ':' || normalized_value FROM user_identifiers")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        rows.map(|r| r.map_err(DbError::from)).collect()
    }
}

/// The value as given (trimmed) and its comparison form, or why neither is
/// usable.
fn checked(kind: IdentifierKind, value: &str) -> DbResult<(String, String)> {
    let value = value.trim();
    if value.is_empty() {
        return Err(DbError::Invalid("identifier cannot be empty".into()));
    }
    let normalized = kind.normalize(value);
    if normalized.is_empty() {
        return Err(DbError::Invalid(format!("{value:?} is not a usable {}", kind.as_str())));
    }
    Ok((value.to_string(), normalized))
}

fn identity_id(conn: &Connection) -> DbResult<String> {
    conn.query_row("SELECT id FROM user_identity LIMIT 1", [], |r| r.get(0))
        .optional()?
        .ok_or_else(|| DbError::Invalid("set the user identity before adding identifiers".into()))
}

fn is_users(conn: &Connection, kind: &str, normalized: &str) -> DbResult<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM user_identifiers WHERE kind = ?1 AND normalized_value = ?2)",
        params![kind, normalized],
        |r| r.get(0),
    )?)
}

/// Idempotent on kind and normalized value: the same address in another case
/// is the same address, and a phone number that happens to read like a
/// handle is not.
fn insert_identifier(
    conn: &Connection,
    identity_id: &str,
    kind: &str,
    value: &str,
    normalized: &str,
) -> DbResult<Identifier> {
    let existing = conn
        .query_row(
            "SELECT id, kind, value, normalized_value FROM user_identifiers WHERE kind = ?1 AND normalized_value = ?2",
            params![kind, normalized],
            |r| Ok(Identifier { id: r.get(0)?, kind: r.get(1)?, value: r.get(2)?, normalized_value: r.get(3)? }),
        )
        .optional()?;
    if let Some(existing) = existing {
        return Ok(existing);
    }
    let id = new_id();
    conn.execute(
        "INSERT INTO user_identifiers(id, user_identity_id, kind, value, normalized_value, added_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, identity_id, kind, value, normalized, now_rfc3339()],
    )?;
    Ok(Identifier { id, kind: kind.into(), value: value.into(), normalized_value: normalized.into() })
}

/// A person's addresses that are not the user's yet — what folding them
/// would make the user's. Names are not addresses and are not carried.
fn carried_addresses(conn: &Connection, participant_id: &str) -> DbResult<Vec<(String, String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT pi.kind, pi.value, pi.normalized_value FROM participant_identifiers pi
         WHERE pi.participant_id = ?1 AND pi.kind <> 'display_name'
           AND NOT EXISTS (SELECT 1 FROM user_identifiers ui
                           WHERE ui.kind = pi.kind AND ui.normalized_value = pi.normalized_value)
         ORDER BY pi.kind, pi.normalized_value",
    )?;
    let rows = stmt.query_map([participant_id], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// `add_user_address` on the caller's transaction, for an address already
/// checked and normalized.
fn add_address(
    tx: &Connection,
    kind: &str,
    value: &str,
    normalized: &str,
    confirmed: Option<&AddressOwner>,
) -> DbResult<Claimed> {
    let identity = identity_id(tx)?;
    let owner = owner_of(tx, kind, normalized)?;
    match &owner {
        Some(o) if confirmed.is_none() => {
            return Err(DbError::Unconfirmed(format!(
                "Mail from {value} is filed under {}. See what saying it's yours would change first.",
                o.display_name
            )));
        }
        Some(o) if confirmed != Some(o) => {
            return Err(DbError::Unconfirmed(format!(
                "What saying {value} is yours would change is different now from what you were shown. Look again first."
            )));
        }
        Some(_) => refuse_while(tx, &HOLDS_PEOPLE)?,
        None if is_users(tx, kind, normalized)? => return Ok(Claimed::default()),
        None => refuse_while(tx, &READS_MAIL)?,
    }
    insert_identifier(tx, &identity, kind, value, normalized)?;
    match owner {
        Some(o) => fold(tx, &[o.participant_id]),
        None => Ok(Claimed::default()),
    }
}

/// The user said this person is not them (`Db::keep_apart`).
fn is_kept_apart(conn: &Connection, participant_id: &str) -> DbResult<bool> {
    Ok(conn
        .query_row("SELECT kept_apart <> 0 FROM participants WHERE id = ?1", [participant_id], |r| r.get(0))
        .optional()?
        .unwrap_or(false))
}

/// Who mail from this address is filed under, with everything folding them
/// would change. The one query behind the preview, the confirmation check
/// and the list of addresses still held.
fn owner_of(conn: &Connection, kind: &str, normalized: &str) -> DbResult<Option<AddressOwner>> {
    let id: Option<String> = conn
        .query_row(
            "SELECT p.id FROM participants p
             JOIN participant_identifiers pi ON pi.participant_id = p.id
             WHERE p.is_self = 0 AND pi.kind = ?1 AND pi.normalized_value = ?2",
            params![kind, normalized],
            |r| r.get(0),
        )
        .optional()?;
    match id {
        Some(id) => person(conn, &id, Some((kind, normalized))),
        None => Ok(None),
    }
}

/// Everything folding this person would change, as `AddressOwner`. Their
/// other addresses leave out `except`, the address being asked about.
fn person(conn: &Connection, participant_id: &str, except: Option<(&str, &str)>) -> DbResult<Option<AddressOwner>> {
    let row: Option<(String, Option<String>, Option<String>)> = conn
        .query_row(
            "SELECT display_name, relationship, notes FROM participants WHERE id = ?1 AND is_self = 0",
            [participant_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let Some((display_name, relationship, notes)) = row else { return Ok(None) };
    let messages: i64 =
        conn.query_row("SELECT COUNT(*) FROM messages WHERE participant_id = ?1", [participant_id], |r| r.get(0))?;
    let preferences: i64 = conn.query_row(
        "SELECT COUNT(*) FROM voice_preferences WHERE layer = 'relationship' AND scope_key = ?1",
        [participant_id],
        |r| r.get(0),
    )?;
    let other_addresses = carried_addresses(conn, participant_id)?
        .into_iter()
        .filter(|(k, _, n)| except != Some((k.as_str(), n.as_str())))
        .map(|(_, value, _)| value)
        .collect();
    Ok(Some(AddressOwner {
        participant_id: participant_id.to_string(),
        display_name,
        messages: messages as usize,
        other_addresses,
        relationship: relationship.map(|r| r.trim().to_string()).filter(|r| !r.is_empty()),
        has_notes: notes.is_some_and(|n| !n.trim().is_empty()),
        preferences: preferences as usize,
    }))
}

/// Folding them needs no question: every address they hold is already the
/// user's, and nothing the user recorded about them — relationship, notes,
/// preferences for writing to them — would go. A name the user gave them
/// is not recorded as theirs, so it is not counted, and it goes with them.
fn is_only_the_user(owner: &AddressOwner) -> bool {
    owner.other_addresses.is_empty() && owner.relationship.is_none() && !owner.has_notes && owner.preferences == 0
}

/// Refuse, naming what is running, while any of these jobs is.
fn refuse_while(conn: &Connection, kinds: &[&str]) -> DbResult<()> {
    let running: Vec<String> = {
        let mut stmt = conn.prepare("SELECT type FROM jobs WHERE status = 'running' ORDER BY created_at")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        rows.collect::<Result<_, _>>()?
    };
    let Some(kind) = running.iter().find(|k| kinds.contains(&k.as_str())) else { return Ok(()) };
    let doing = match kind.as_str() {
        crate::import::JOB_KIND => "reading your mail",
        crate::sources::imap::JOB_KIND => "checking your mail",
        crate::voice::JOB_KIND | crate::voice::CHANGED_JOB_KIND => "working out how you write",
        crate::evaluation::JOB_KIND => "measuring how close my drafts come",
        _ => "writing replies",
    };
    Err(DbError::Busy(format!("I'm {doing} right now. Try again when that's finished.")))
}

/// Fold these people back into the user, on the caller's transaction: their
/// addresses become the user's, their messages become the user's own, and
/// what was derived from or said about them as a person goes. Drafts written
/// to them are the record of what the user sent and stay, addressed to
/// nobody. Every conversation they were in is recounted and its reply
/// latencies recomputed, since those depend on who wrote what, and every
/// profile is marked stale, because the user's own writing changed.
fn fold(tx: &Connection, people: &[String]) -> DbResult<Claimed> {
    let identity = identity_id(tx)?;
    let mut claimed = Claimed::default();
    let mut conversations: Vec<String> = Vec::new();
    for id in people {
        for (kind, value, normalized) in carried_addresses(tx, id)? {
            insert_identifier(tx, &identity, &kind, &value, &normalized)?;
        }
        {
            let mut stmt = tx.prepare(
                "SELECT conversation_id FROM conversation_participants WHERE participant_id = ?1
                 UNION SELECT conversation_id FROM messages WHERE participant_id = ?1",
            )?;
            let rows = stmt.query_map([id], |r| r.get::<_, String>(0))?;
            for c in rows {
                conversations.push(c?);
            }
        }
        // Before the person goes: their messages and drafts would cascade.
        claimed.messages += tx
            .execute("UPDATE messages SET direction = 'self', participant_id = NULL WHERE participant_id = ?1", [id])?;
        tx.execute("UPDATE drafts SET participant_id = NULL WHERE participant_id = ?1", [id])?;
        tx.execute("DELETE FROM representative_examples WHERE participant_id = ?1", [id])?;
        tx.execute(
            "DELETE FROM voice_profiles WHERE participant_id = ?1 OR (layer = 'relationship' AND scope_key = ?1)",
            [id],
        )?;
        tx.execute("DELETE FROM voice_preferences WHERE layer = 'relationship' AND scope_key = ?1", [id])?;
        // Their identifiers and conversation links go by cascade.
        claimed.people += tx.execute("DELETE FROM participants WHERE id = ?1", [id])?;
    }
    conversations.sort();
    conversations.dedup();
    for c in &conversations {
        link_replies(tx, c)?;
        refresh_conversation_stats(tx, c)?;
    }
    if claimed.people > 0 {
        tx.execute("UPDATE voice_profiles SET stale = 1", [])?;
    }
    Ok(claimed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_is_created_renamed_and_deduplicated() {
        let db = Db::open_in_memory().unwrap();
        assert!(db.user_identity().unwrap().is_none());
        assert!(db.add_user_identifier(IdentifierKind::Email, "a@b.c").is_err(), "identifiers need an identity");

        let me = db.set_user_identity("  Ada  ").unwrap();
        assert_eq!(me.display_name, "Ada");
        assert!(db.set_user_identity("  ").is_err());

        db.add_user_identifier(IdentifierKind::Email, "Ada@Example.com").unwrap();
        // Same address in a different case is the same identifier.
        let again = db.add_user_identifier(IdentifierKind::Email, "ada@example.com").unwrap();
        db.add_user_identifier(IdentifierKind::Phone, "+1 (555) 010-9999").unwrap();
        let me = db.user_identity().unwrap().unwrap();
        assert_eq!(me.identifiers.len(), 2);
        assert_eq!(db.user_identifier_set().unwrap().len(), 2);

        // The same text as another kind is another address, not a duplicate.
        let handle = db.add_user_identifier(IdentifierKind::Handle, "5550109999").unwrap();
        assert_eq!(handle.kind, "handle");
        assert_eq!(db.user_identity().unwrap().unwrap().identifiers.len(), 3);
        db.remove_user_identifier(&handle.id).unwrap();

        let renamed = db.set_user_identity("Ada Lovelace").unwrap();
        assert_eq!(renamed.id, me.id, "renaming keeps the same identity");
        assert_eq!(renamed.identifiers.len(), 2);

        db.remove_user_identifier(&again.id).unwrap();
        assert_eq!(db.user_identity().unwrap().unwrap().identifiers.len(), 1);
        assert!(db.remove_user_identifier("nope").is_err());
    }

    /// Mail read before an address was declared: a thread with Ada, where the
    /// user replied from an alias a day later, and a note the user sent
    /// themselves.
    fn inbox_with_an_alias() -> (Db, String, String) {
        use crate::db::repo_people::IdentifierInput;
        use crate::db::{NewMessage, NewSource};
        let db = Db::open_in_memory().unwrap();
        db.set_user_identity("C").unwrap();
        db.add_user_identifier(IdentifierKind::Email, "c@example.com").unwrap();
        db.set_setting(crate::db::WITHIN_DAYS_SETTING, &0).unwrap();
        let source = db
            .create_source(&NewSource {
                connector: "mbox".into(),
                name: "export".into(),
                channel: "email".into(),
                location: None,
                config: serde_json::Value::Null,
            })
            .unwrap()
            .id;
        let ada =
            db.resolve_participant("Ada", &[IdentifierInput::new(IdentifierKind::Email, "ada@x.com")], false).unwrap();
        let alias = db
            .resolve_participant("C (work)", &[IdentifierInput::new(IdentifierKind::Email, "C@Work.example")], false)
            .unwrap();
        let lunch = db.upsert_conversation(&source, "lunch", "email", Some("Lunch")).unwrap();
        db.link_conversation_participant(&lunch, &ada).unwrap();
        db.link_conversation_participant(&lunch, &alias).unwrap();
        let msg = |convo: &str, who: &str, ext: &str, seq: i64, body: &str| NewMessage {
            conversation_id: convo.into(),
            source_id: source.clone(),
            participant_id: Some(who.into()),
            external_id: ext.into(),
            direction: "other".into(),
            channel: "email".into(),
            sent_at: Some(format!("2026-09-0{}T10:00:00Z", seq + 1)),
            sequence_index: seq,
            body: body.into(),
            reply_to_external_id: None,
            metadata: serde_json::Value::Null,
        };
        let note = db.upsert_conversation(&source, "note", "email", Some("Note to self")).unwrap();
        db.link_conversation_participant(&note, &alias).unwrap();
        db.insert_messages(&[
            msg(&lunch, &ada, "l1", 0, "lunch thursday?"),
            msg(&lunch, &alias, "l2", 1, "yes, 12:30 works"),
            msg(&note, &alias, "n1", 0, "remember the keys"),
        ])
        .unwrap();
        for c in [&lunch, &note] {
            db.link_replies(c).unwrap();
            db.refresh_conversation_stats(c).unwrap();
        }
        (db, lunch, alias)
    }

    fn latency_of(db: &Db, external_id: &str) -> Option<i64> {
        db.conn()
            .query_row("SELECT response_latency_seconds FROM messages WHERE external_id = ?1", [external_id], |r| {
                r.get(0)
            })
            .unwrap()
    }

    #[test]
    fn an_address_filed_under_someone_is_folded_in_only_once_the_user_has_seen_who() {
        let (db, lunch, alias) = inbox_with_an_alias();
        assert_eq!(db.count_self_messages(None, None).unwrap(), 0);
        assert_eq!(
            db.threads_awaiting_reply(10).unwrap().len(),
            2,
            "the user's own reply and note look like someone waiting"
        );
        assert_eq!(latency_of(&db, "l2"), None, "two people's messages, as far as it knew");
        // A draft once written to the alias is the user's record; it stays.
        db.conn()
            .execute(
                "INSERT INTO drafts(id, participant_id, conversation_id, channel, generated_text, provider, model, context_json, prompt_hash, evidence_json, created_at)
                 VALUES ('d1', ?1, ?2, 'email', 'hi', 'local', 'm', '{}', 'h', '{}', '2026-09-01T00:00:00Z')",
                params![alias, lunch],
            )
            .unwrap();

        let preview = db.preview_user_address(IdentifierKind::Email, " c@WORK.example ").unwrap();
        assert!(!preview.already_yours);
        let owner = preview.owner.expect("mail from it is filed under the alias");
        assert_eq!(owner.participant_id, alias);
        assert_eq!(owner.display_name, "C (work)");
        assert_eq!(owner.messages, 2);
        assert!(owner.other_addresses.is_empty() && owner.relationship.is_none() && !owner.has_notes);
        assert_eq!(owner.preferences, 0);

        // Not without saying who: nothing changes, not even the address.
        let refused = db.add_user_address(IdentifierKind::Email, "c@work.example", None);
        assert!(matches!(refused, Err(DbError::Unconfirmed(_))), "{refused:?}");
        let someone_else = AddressOwner { participant_id: "someone else".into(), ..owner.clone() };
        let refused = db.add_user_address(IdentifierKind::Email, "c@work.example", Some(&someone_else));
        assert!(matches!(refused, Err(DbError::Unconfirmed(_))), "{refused:?}");
        assert_eq!(db.user_identity().unwrap().unwrap().identifiers.len(), 1);
        assert!(db.get_participant(&alias).unwrap().is_some());

        let added = db.add_user_address(IdentifierKind::Email, "c@work.example", Some(&owner)).unwrap();
        assert_eq!(added.claimed, Claimed { messages: 2, people: 1 });
        assert_eq!(added.identity.identifiers.len(), 2);
        assert_eq!(db.count_self_messages(None, None).unwrap(), 2);
        assert!(db.get_participant(&alias).unwrap().is_none(), "the alias was the user, not a person");
        assert!(db.threads_awaiting_reply(10).unwrap().is_empty(), "the reply answered Ada; the note was the user's");
        assert_eq!(latency_of(&db, "l2"), Some(86_400), "the user answered Ada a day later");
        let lunch_row = db.get_conversation(&lunch).unwrap().unwrap();
        assert_eq!(lunch_row.message_count, 2, "no message went with the person");
        let draft: Option<String> =
            db.conn().query_row("SELECT participant_id FROM drafts WHERE id = 'd1'", [], |r| r.get(0)).unwrap();
        assert_eq!(draft, None, "kept, addressed to nobody");

        // Nothing more to fold: adding it again changes nothing.
        let preview = db.preview_user_address(IdentifierKind::Email, "c@work.example").unwrap();
        assert_eq!(preview, AddressPreview { already_yours: true, owner: None });
        assert_eq!(
            db.add_user_address(IdentifierKind::Email, "c@work.example", None).unwrap().claimed,
            Claimed::default()
        );
        assert_eq!(db.claim_user_mail().unwrap(), Reconciled::default());
    }

    #[test]
    fn a_yes_to_what_has_since_changed_is_refused_and_asked_again() {
        let (db, _lunch, alias) = inbox_with_an_alias();
        let shown = db.preview_user_address(IdentifierKind::Email, "c@work.example").unwrap().owner.unwrap();
        // Between the question and the answer, the user wrote down who this
        // is. The question never said that would go.
        db.set_participant_relationship(&alias, Some("me at work")).unwrap();
        let refused = db.add_user_address(IdentifierKind::Email, "c@work.example", Some(&shown));
        assert!(matches!(&refused, Err(DbError::Unconfirmed(m)) if m.contains("Look again")), "{refused:?}");
        assert!(db.get_participant(&alias).unwrap().is_some(), "refused means nothing changed");
        assert_eq!(db.count_self_messages(None, None).unwrap(), 0);

        let now = db.preview_user_address(IdentifierKind::Email, "c@work.example").unwrap().owner.unwrap();
        assert_eq!(now.relationship.as_deref(), Some("me at work"));
        assert_eq!(
            db.add_user_address(IdentifierKind::Email, "c@work.example", Some(&now)).unwrap().claimed,
            Claimed { messages: 2, people: 1 }
        );
    }

    #[test]
    fn folding_someone_makes_their_other_addresses_the_users_and_takes_what_was_said_about_them() {
        use crate::db::repo_people::IdentifierInput;
        let (db, _lunch, alias) = inbox_with_an_alias();
        db.resolve_participant(
            "C (work)",
            &[
                IdentifierInput::new(IdentifierKind::Email, "c@work.example"),
                IdentifierInput::new(IdentifierKind::Phone, "555 010 2222"),
            ],
            false,
        )
        .unwrap();
        db.set_participant_relationship(&alias, Some("colleague")).unwrap();
        db.conn()
            .execute(
                "INSERT INTO voice_preferences(id, layer, scope_key, key, value_json, created_at, updated_at)
                 VALUES ('p1', 'relationship', ?1, 'greeting', '\"Hi\"', 'x', 'x')",
                [&alias],
            )
            .unwrap();

        let owner = db.preview_user_address(IdentifierKind::Email, "c@work.example").unwrap().owner.unwrap();
        assert_eq!(owner.other_addresses, vec!["555 010 2222".to_string()]);
        assert_eq!(owner.relationship.as_deref(), Some("colleague"));
        assert_eq!(owner.preferences, 1);

        db.add_user_address(IdentifierKind::Email, "c@work.example", Some(&owner)).unwrap();
        let mine = db.user_identifier_set().unwrap();
        assert!(mine.contains("phone:5550102222"), "{mine:?}");
        let prefs: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM voice_preferences WHERE scope_key = ?1", [&alias], |r| r.get(0))
            .unwrap();
        assert_eq!(prefs, 0);
    }

    #[test]
    fn someone_who_is_only_the_users_addresses_is_folded_without_asking_and_anyone_else_is_left() {
        let (db, _lunch, alias) = inbox_with_an_alias();
        // Declared the old way, or by connecting the mailbox: the address is
        // the user's and the mail read from it is still the alias's.
        db.add_user_identifier(IdentifierKind::Email, "c@work.example").unwrap();
        db.set_participant_relationship(&alias, Some("me at work")).unwrap();
        assert_eq!(
            db.claim_user_mail().unwrap(),
            Reconciled { claimed: Claimed::default(), left: 1 },
            "the user said something about them, so it is theirs to decide"
        );
        let held = db.held_user_addresses().unwrap();
        assert_eq!(held.len(), 1);
        assert_eq!(held[0].identifier.normalized_value, "c@work.example");
        assert_eq!(held[0].owner.participant_id, alias);

        db.set_participant_relationship(&alias, None).unwrap();
        assert_eq!(db.claim_user_mail().unwrap(), Reconciled { claimed: Claimed { messages: 2, people: 1 }, left: 0 });
        assert!(db.get_participant(&alias).unwrap().is_none());
        assert!(db.held_user_addresses().unwrap().is_empty());
        assert_eq!(latency_of(&db, "l2"), Some(86_400));
    }

    #[test]
    fn someone_the_user_said_is_not_them_is_never_folded_without_asking() {
        let (db, _lunch, alias) = inbox_with_an_alias();
        db.add_user_identifier(IdentifierKind::Email, "c@work.example").unwrap();
        db.set_participant_relationship(&alias, Some("me at work")).unwrap();
        let held = db.held_user_addresses().unwrap();
        assert_eq!(held.len(), 1);
        assert!(!held[0].kept_apart);

        db.keep_apart(&alias).unwrap();
        db.keep_apart(&alias).unwrap();
        assert!(matches!(db.keep_apart("nobody"), Err(DbError::NotFound(_))));
        let held = db.held_user_addresses().unwrap();
        assert!(held[0].kept_apart);
        // What made them worth asking about goes; the answer stands.
        db.set_participant_relationship(&alias, None).unwrap();
        assert_eq!(db.claim_user_mail().unwrap(), Reconciled { claimed: Claimed::default(), left: 1 });
        assert!(db.get_participant(&alias).unwrap().is_some());

        // Yes after all, to what is there now — not to what was shown before.
        let refused = db.claim_held_address(&held[0].identifier.id, &held[0].owner);
        assert!(matches!(refused, Err(DbError::Unconfirmed(_))), "{refused:?}");
        let now = db.held_user_addresses().unwrap().remove(0);
        let added = db.claim_held_address(&now.identifier.id, &now.owner).unwrap();
        assert_eq!(added.claimed, Claimed { messages: 2, people: 1 });
        assert_eq!(added.identity.identifiers.len(), 2, "the address as stored, not a second copy");
        assert!(matches!(db.claim_held_address("gone", &now.owner), Err(DbError::NotFound(_))));
    }

    #[test]
    fn someone_whose_mail_was_in_the_sent_folder_is_asked_about_until_answered() {
        let mark = |db: &Db| {
            db.conn()
                .execute(
                    "UPDATE messages SET metadata_json = '{\"sentFolder\": true}' WHERE external_id IN ('l2', 'n1')",
                    [],
                )
                .unwrap();
            // Not a reading this code wrote.
            db.conn()
                .execute("UPDATE messages SET metadata_json = '{\"sentFolder\": \"yes\"}' WHERE external_id = 'l1'", [])
                .unwrap();
        };
        let (db, _lunch, alias) = inbox_with_an_alias();
        assert!(db.sent_folder_people().unwrap().is_empty(), "nothing was read from a Sent folder");
        mark(&db);
        let asked = db.sent_folder_people().unwrap();
        assert_eq!(asked.len(), 1, "{asked:?}");
        assert_eq!((asked[0].kind.as_str(), asked[0].address.as_str(), asked[0].sent), ("email", "C@Work.example", 2));
        assert_eq!(asked[0].owner.participant_id, alias);
        assert_eq!(
            Some(&asked[0].owner),
            db.preview_user_address(IdentifierKind::Email, "c@work.example").unwrap().owner.as_ref(),
            "the same question adding the address asks, so a yes to it is accepted"
        );
        db.keep_apart(&alias).unwrap();
        assert!(db.sent_folder_people().unwrap().is_empty(), "a no is kept");

        let (db, _lunch, _alias) = inbox_with_an_alias();
        mark(&db);
        db.add_user_identifier(IdentifierKind::Email, "c@work.example").unwrap();
        assert!(db.sent_folder_people().unwrap().is_empty(), "an address already the user's is asked about as held");
        assert_eq!(db.held_user_addresses().unwrap().len(), 1);
    }

    #[test]
    fn a_person_under_an_address_that_is_not_the_users_is_left_alone() {
        let (db, _lunch, alias) = inbox_with_an_alias();
        assert_eq!(db.claim_user_mail().unwrap(), Reconciled::default());
        assert!(db.get_participant(&alias).unwrap().is_some());
        let added = db.add_user_address(IdentifierKind::Email, "someone@else.example", None).unwrap();
        assert_eq!(added.claimed, Claimed::default());
        assert!(db.get_participant(&alias).unwrap().is_some());
        assert_eq!(db.count_self_messages(None, None).unwrap(), 0);
    }

    #[test]
    fn nothing_is_folded_or_added_under_work_that_would_miss_it() {
        let (db, _lunch, alias) = inbox_with_an_alias();
        let job = |kind: &str| {
            let j = db.create_job(kind, &serde_json::json!({}), false).unwrap();
            db.mark_job_running(&j.id).unwrap();
            j.id
        };

        // Mail being read would miss a new address.
        let reading = job(crate::import::JOB_KIND);
        let refused = db.add_user_address(IdentifierKind::Email, "new@example.com", None);
        assert!(matches!(&refused, Err(DbError::Busy(m)) if m.contains("reading your mail")), "{refused:?}");
        db.complete_job(&reading, &serde_json::json!({})).unwrap();

        // An analysis holds people, so nobody is folded under it; a plain
        // address is fine.
        let analysing = job(crate::voice::JOB_KIND);
        db.add_user_address(IdentifierKind::Email, "new@example.com", None).unwrap();
        let owner = db.preview_user_address(IdentifierKind::Email, "c@work.example").unwrap().owner.unwrap();
        let refused = db.add_user_address(IdentifierKind::Email, "c@work.example", Some(&owner));
        assert!(matches!(&refused, Err(DbError::Busy(m)) if m.contains("how you write")), "{refused:?}");
        db.add_user_identifier(IdentifierKind::Email, "c@work.example").unwrap();
        assert!(matches!(db.claim_user_mail(), Err(DbError::Busy(_))));
        assert!(db.get_participant(&alias).unwrap().is_some(), "refused means nothing changed");

        db.complete_job(&analysing, &serde_json::json!({})).unwrap();
        assert_eq!(db.claim_user_mail().unwrap().claimed, Claimed { messages: 2, people: 1 });
    }
}
