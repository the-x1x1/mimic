//! Communication sources: where imported messages came from.

use rusqlite::{params, OptionalExtension, Row};
use serde_json::Value;

use super::models::{json_col, json_obj};
use super::{Db, DbError, DbResult, NewSource, Source};
use crate::ids::{new_id, now_rfc3339};

const COLS: &str = "id, connector, name, channel, location, config_json, status, created_at, last_imported_at, message_count, last_error_json";

pub(crate) const CHANNELS: [&str; 5] = ["email", "sms", "chat", "forum", "other"];

/// Channels are a closed set so a connector cannot invent one that the UI and
/// the channel voice layer then have no idea what to do with.
pub fn channel_is_known(channel: &str) -> bool {
    CHANNELS.contains(&channel)
}

fn map(r: &Row<'_>) -> rusqlite::Result<Source> {
    Ok(Source {
        id: r.get(0)?,
        connector: r.get(1)?,
        name: r.get(2)?,
        channel: r.get(3)?,
        location: r.get(4)?,
        config: json_obj(r.get(5)?),
        status: r.get(6)?,
        created_at: r.get(7)?,
        last_imported_at: r.get(8)?,
        message_count: r.get(9)?,
        last_error: json_col(r.get(10)?),
    })
}

impl Db {
    pub fn create_source(&self, new: &NewSource) -> DbResult<Source> {
        if new.name.trim().is_empty() {
            return Err(DbError::Invalid("source name cannot be empty".into()));
        }
        if !CHANNELS.contains(&new.channel.as_str()) {
            return Err(DbError::Invalid(format!("unknown channel {:?}", new.channel)));
        }
        let id = new_id();
        let config = if new.config.is_object() { new.config.clone() } else { Value::Object(Default::default()) };
        self.conn().execute(
            "INSERT INTO sources(id, connector, name, channel, location, config_json, status, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'new', ?7)",
            params![id, new.connector, new.name.trim(), new.channel, new.location, config.to_string(), now_rfc3339()],
        )?;
        self.get_source(&id)?.ok_or(DbError::NotFound(id))
    }

    pub fn get_source(&self, id: &str) -> DbResult<Option<Source>> {
        Ok(self.conn().query_row(&format!("SELECT {COLS} FROM sources WHERE id = ?1"), [id], map).optional()?)
    }

    pub fn list_sources(&self) -> DbResult<Vec<Source>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!("SELECT {COLS} FROM sources ORDER BY created_at DESC"))?;
        let rows = stmt.query_map([], map)?;
        rows.map(|r| r.map_err(DbError::from)).collect()
    }

    pub fn set_source_status(&self, id: &str, status: &str, error: Option<&Value>) -> DbResult<()> {
        self.conn().execute(
            "UPDATE sources SET status = ?1, last_error_json = ?2 WHERE id = ?3",
            params![status, error.map(|e| e.to_string()), id],
        )?;
        Ok(())
    }

    /// Replace a source's configuration. The IMAP connector keeps its sync
    /// position here (never a credential: those live in the secret store).
    pub fn set_source_config(&self, id: &str, config: &Value) -> DbResult<()> {
        let n = self
            .conn()
            .execute("UPDATE sources SET config_json = ?1 WHERE id = ?2", params![config.to_string(), id])?;
        if n == 0 {
            return Err(DbError::NotFound(id.into()));
        }
        Ok(())
    }

    /// Recount from `messages` rather than incrementing, so a re-import or a
    /// deletion can never leave the displayed count lying.
    pub fn refresh_source_counts(&self, id: &str) -> DbResult<i64> {
        let conn = self.conn();
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM messages WHERE source_id = ?1", [id], |r| r.get(0))?;
        conn.execute(
            "UPDATE sources SET message_count = ?1, last_imported_at = ?2 WHERE id = ?3",
            params![n, now_rfc3339(), id],
        )?;
        Ok(n)
    }

    pub fn delete_source(&self, id: &str) -> DbResult<()> {
        let n = self.conn().execute("DELETE FROM sources WHERE id = ?1", [id])?;
        if n == 0 {
            return Err(DbError::NotFound(id.into()));
        }
        Ok(())
    }
    /// When any source last finished an import. `None` before the first one,
    /// which is how the dashboard knows not to describe itself as up to date.
    pub fn last_import_at(&self) -> DbResult<Option<String>> {
        Ok(self.conn().query_row(
            "SELECT MAX(last_imported_at) FROM sources WHERE last_imported_at IS NOT NULL",
            [],
            |r| r.get(0),
        )?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn src(channel: &str) -> NewSource {
        NewSource {
            connector: "mimic_json".into(),
            name: "Export".into(),
            channel: channel.into(),
            location: Some("/tmp/x.json".into()),
            config: Value::Null,
        }
    }

    #[test]
    fn sources_are_created_listed_and_validated() {
        let db = Db::open_in_memory().unwrap();
        assert!(db.create_source(&src("telepathy")).is_err(), "channel is a closed set");
        let mut bad = src("email");
        bad.name = "   ".into();
        assert!(db.create_source(&bad).is_err());

        let s = db.create_source(&src("email")).unwrap();
        assert_eq!(s.status, "new");
        assert_eq!(s.config, serde_json::json!({}), "a null config becomes an empty object");
        assert_eq!(db.list_sources().unwrap().len(), 1);

        db.set_source_status(&s.id, "failed", Some(&serde_json::json!({"message": "no such file"}))).unwrap();
        let s2 = db.get_source(&s.id).unwrap().unwrap();
        assert_eq!(s2.status, "failed");
        assert_eq!(s2.last_error.unwrap()["message"], "no such file");

        assert_eq!(db.refresh_source_counts(&s.id).unwrap(), 0);
        db.delete_source(&s.id).unwrap();
        assert!(db.delete_source(&s.id).is_err());
    }
}
