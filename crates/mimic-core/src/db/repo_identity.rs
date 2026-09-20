//! The user's own identity and the addresses they write from.
//!
//! This is load-bearing: `direction` on every imported message is decided by
//! matching the author against these identifiers. If the user has not declared
//! who they are, import still runs but every message lands as `unknown`, and
//! no voice profile can be built from it.

use rusqlite::{params, OptionalExtension};

use super::{Db, DbError, DbResult, Identifier, IdentifierKind, UserIdentity};
use crate::ids::{new_id, now_rfc3339};

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

    /// Add an address the user writes from. Idempotent on the normalized value.
    pub fn add_user_identifier(&self, kind: IdentifierKind, value: &str) -> DbResult<Identifier> {
        let value = value.trim();
        if value.is_empty() {
            return Err(DbError::Invalid("identifier cannot be empty".into()));
        }
        let identity = match self.user_identity()? {
            Some(i) => i,
            None => return Err(DbError::Invalid("set the user identity before adding identifiers".into())),
        };
        let normalized = kind.normalize(value);
        if normalized.is_empty() {
            return Err(DbError::Invalid(format!("{value:?} is not a usable {}", kind.as_str())));
        }
        if let Some(existing) = identity.identifiers.iter().find(|i| i.normalized_value == normalized) {
            return Ok(existing.clone());
        }
        let id = new_id();
        self.conn().execute(
            "INSERT INTO user_identifiers(id, user_identity_id, kind, value, normalized_value, added_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, identity.id, kind.as_str(), value, normalized, now_rfc3339()],
        )?;
        Ok(Identifier { id, kind: kind.as_str().into(), value: value.into(), normalized_value: normalized })
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

        let renamed = db.set_user_identity("Ada Lovelace").unwrap();
        assert_eq!(renamed.id, me.id, "renaming keeps the same identity");
        assert_eq!(renamed.identifiers.len(), 2);

        db.remove_user_identifier(&again.id).unwrap();
        assert_eq!(db.user_identity().unwrap().unwrap().identifiers.len(), 1);
        assert!(db.remove_user_identifier("nope").is_err());
    }
}
