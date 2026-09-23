//! App data layout.
//!
//! ```text
//! <root>/
//!   data/mimic.db  data/backups/
//!   cache/embeddings/
//!   models/encoders/
//!   credentials/   logs/  updates/  tmp/
//! ```
//!
//! On Windows the root is `%LOCALAPPDATA%\Formicaria\Mimic`. The Tauri shell
//! resolves the platform root; this module only knows the layout so tests can
//! point it at a temp directory.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub root: PathBuf,
}

impl AppPaths {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Resolve the default per-user root for this platform without touching disk.
    pub fn default_root() -> Option<PathBuf> {
        #[cfg(target_os = "windows")]
        {
            std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("Formicaria").join("Mimic"))
        }
        #[cfg(target_os = "macos")]
        {
            std::env::var_os("HOME")
                .map(|p| PathBuf::from(p).join("Library").join("Application Support").join("Formicaria").join("Mimic"))
        }
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        {
            let base = std::env::var_os("XDG_DATA_HOME")
                .map(PathBuf::from)
                .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("share")))?;
            Some(base.join("formicaria").join("mimic"))
        }
    }

    pub fn data_dir(&self) -> PathBuf {
        self.root.join("data")
    }
    pub fn database_file(&self) -> PathBuf {
        self.data_dir().join("mimic.db")
    }
    pub fn backups_dir(&self) -> PathBuf {
        self.data_dir().join("backups")
    }
    pub fn embeddings_cache(&self) -> PathBuf {
        self.root.join("cache").join("embeddings")
    }
    pub fn encoders_dir(&self) -> PathBuf {
        self.root.join("models").join("encoders")
    }
    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }
    /// Provider keys and mailbox passwords, sealed to the user's account on
    /// Windows; see `secrets.rs` in the desktop crate for what that does and
    /// does not guarantee.
    pub fn credentials_dir(&self) -> PathBuf {
        self.root.join("credentials")
    }
    pub fn updates_dir(&self) -> PathBuf {
        self.root.join("updates")
    }
    pub fn tmp_dir(&self) -> PathBuf {
        self.root.join("tmp")
    }
    pub fn all_dirs(&self) -> Vec<PathBuf> {
        vec![
            self.data_dir(),
            self.backups_dir(),
            self.embeddings_cache(),
            self.encoders_dir(),
            self.credentials_dir(),
            self.logs_dir(),
            self.updates_dir(),
            self.tmp_dir(),
        ]
    }

    /// Create every directory in the layout. Idempotent.
    pub fn ensure(&self) -> std::io::Result<()> {
        for d in self.all_dirs() {
            std::fs::create_dir_all(&d)?;
        }
        restrict_dir_permissions(&self.credentials_dir());
        Ok(())
    }
}

/// Best-effort: on Unix make the credentials directory owner-only. Windows
/// `%LOCALAPPDATA%` is already per-user ACL'd.
fn restrict_dir_permissions(dir: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensure_creates_layout() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(tmp.path());
        paths.ensure().unwrap();
        for d in paths.all_dirs() {
            assert!(d.is_dir(), "{}", d.display());
        }
        assert!(paths.database_file().starts_with(tmp.path()));
        paths.ensure().unwrap();
    }
}
