use rusqlite::{params, OptionalExtension, Row};

use super::{Db, DbError, DbResult, Library};
use crate::ids::{new_id, now_rfc3339};

fn map_library(r: &Row<'_>) -> rusqlite::Result<Library> {
    Ok(Library {
        id: r.get(0)?,
        name: r.get(1)?,
        source_type: r.get(2)?,
        root_path: r.get(3)?,
        lightroom_catalog_fingerprint: r.get(4)?,
        created_at: r.get(5)?,
        last_scanned_at: r.get(6)?,
        status: r.get(7)?,
        purpose: r.get(8)?,
    })
}

const COLS: &str =
    "id, name, source_type, root_path, lightroom_catalog_fingerprint, created_at, last_scanned_at, status, purpose";

impl Db {
    /// Create a training library (the kind shown in the Libraries UI).
    pub fn create_library(
        &self,
        name: &str,
        source_type: &str,
        root_path: Option<&str>,
        catalog_fingerprint: Option<&str>,
    ) -> DbResult<Library> {
        self.create_library_with_purpose(name, source_type, root_path, catalog_fingerprint, "training")
    }

    /// Create a library with an explicit purpose. `session` libraries back a
    /// session's photos and are hidden from `list_libraries`.
    pub fn create_library_with_purpose(
        &self,
        name: &str,
        source_type: &str,
        root_path: Option<&str>,
        catalog_fingerprint: Option<&str>,
        purpose: &str,
    ) -> DbResult<Library> {
        if !matches!(source_type, "lightroom_catalog" | "folder_sidecars" | "demo") {
            return Err(DbError::Invalid(format!("unknown library source_type {source_type}")));
        }
        if !matches!(purpose, "training" | "session") {
            return Err(DbError::Invalid(format!("unknown library purpose {purpose}")));
        }
        let id = new_id();
        let now = now_rfc3339();
        self.conn().execute(
            "INSERT INTO libraries(id, name, source_type, root_path, lightroom_catalog_fingerprint, created_at, status, purpose)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'new', ?7)",
            params![id, name, source_type, root_path, catalog_fingerprint, now, purpose],
        )?;
        self.get_library(&id)?.ok_or_else(|| DbError::NotFound(id))
    }

    pub fn get_library(&self, id: &str) -> DbResult<Option<Library>> {
        Ok(self
            .conn()
            .query_row(&format!("SELECT {COLS} FROM libraries WHERE id = ?1"), [id], map_library)
            .optional()?)
    }

    /// Training libraries only; session-backing libraries are internal.
    pub fn list_libraries(&self) -> DbResult<Vec<Library>> {
        let conn = self.conn();
        let mut stmt =
            conn.prepare(&format!("SELECT {COLS} FROM libraries WHERE purpose = 'training' ORDER BY created_at DESC"))?;
        let rows = stmt.query_map([], map_library)?;
        rows.map(|r| r.map_err(DbError::from)).collect()
    }

    pub fn set_library_status(&self, id: &str, status: &str, scanned: bool) -> DbResult<()> {
        let n = if scanned {
            self.conn().execute(
                "UPDATE libraries SET status = ?2, last_scanned_at = ?3 WHERE id = ?1",
                params![id, status, now_rfc3339()],
            )?
        } else {
            self.conn().execute("UPDATE libraries SET status = ?2 WHERE id = ?1", params![id, status])?
        };
        if n == 0 {
            return Err(DbError::NotFound(id.to_string()));
        }
        Ok(())
    }

    pub fn delete_library(&self, id: &str) -> DbResult<()> {
        // Assets stay (library_id -> NULL) so history is never silently lost.
        let n = self.conn().execute("DELETE FROM libraries WHERE id = ?1", [id])?;
        if n == 0 {
            return Err(DbError::NotFound(id.to_string()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_crud() {
        let db = Db::open_in_memory().unwrap();
        let lib = db.create_library("Weddings 2024", "folder_sidecars", Some("D:/Photos/2024"), None).unwrap();
        assert_eq!(lib.status, "new");
        assert!(db.create_library("x", "bogus", None, None).is_err());
        db.set_library_status(&lib.id, "scanned", true).unwrap();
        let again = db.get_library(&lib.id).unwrap().unwrap();
        assert_eq!(again.status, "scanned");
        assert!(again.last_scanned_at.is_some());
        assert_eq!(db.list_libraries().unwrap().len(), 1);
        let internal =
            db.create_library_with_purpose("Session: x", "folder_sidecars", Some("D:/S"), None, "session").unwrap();
        assert_eq!(internal.purpose, "session");
        assert_eq!(db.list_libraries().unwrap().len(), 1, "session libraries are hidden");
        assert!(db.create_library_with_purpose("x", "demo", None, None, "bogus").is_err());
        db.delete_library(&lib.id).unwrap();
        assert!(db.list_libraries().unwrap().is_empty());
    }
}
