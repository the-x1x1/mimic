//! App data layout.
//!
//! ```text
//! <root>/
//!   data/mimic.db
//!   cache/previews/  cache/embeddings/
//!   models/encoders/ models/styles/
//!   logs/  bridge/  updates/  tmp/  plugin/
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
    pub fn previews_cache(&self) -> PathBuf {
        self.root.join("cache").join("previews")
    }
    pub fn embeddings_cache(&self) -> PathBuf {
        self.root.join("cache").join("embeddings")
    }
    pub fn encoders_dir(&self) -> PathBuf {
        self.root.join("models").join("encoders")
    }
    pub fn styles_dir(&self) -> PathBuf {
        self.root.join("models").join("styles")
    }
    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }
    pub fn bridge_dir(&self) -> PathBuf {
        self.root.join("bridge")
    }
    /// Discovery file the Lightroom plugin reads to find the bridge port + token.
    pub fn bridge_discovery_file(&self) -> PathBuf {
        self.bridge_dir().join("bridge.json")
    }
    pub fn updates_dir(&self) -> PathBuf {
        self.root.join("updates")
    }
    pub fn tmp_dir(&self) -> PathBuf {
        self.root.join("tmp")
    }
    /// App-managed copy of `Mimic.lrplugin` that Lightroom's Plugin Manager points at.
    pub fn plugin_install_dir(&self) -> PathBuf {
        self.root.join("plugin").join("Mimic.lrplugin")
    }

    pub fn all_dirs(&self) -> Vec<PathBuf> {
        vec![
            self.data_dir(),
            self.backups_dir(),
            self.previews_cache(),
            self.embeddings_cache(),
            self.encoders_dir(),
            self.styles_dir(),
            self.logs_dir(),
            self.bridge_dir(),
            self.updates_dir(),
            self.tmp_dir(),
            self.root.join("plugin"),
        ]
    }

    /// Create every directory in the layout. Idempotent.
    pub fn ensure(&self) -> std::io::Result<()> {
        for d in self.all_dirs() {
            std::fs::create_dir_all(&d)?;
        }
        restrict_dir_permissions(&self.bridge_dir());
        Ok(())
    }
}

/// Best-effort: on Unix make the bridge directory owner-only. Windows
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
