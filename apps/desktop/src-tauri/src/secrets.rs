//! Where provider credentials live.
//!
//! **Known limitation, stated plainly rather than papered over:** this is a
//! file with owner-only permissions under the app data directory, not the OS
//! credential store. On Windows that directory is already ACL'd to the user,
//! and on Unix the file is `0600`, so another *user* cannot read it — but any
//! program running as this user can. Moving to DPAPI on Windows and the
//! Keychain/Secret Service elsewhere is Phase 2 work; see
//! `docs/MODEL_PROVIDERS.md` and `docs/ROADMAP.md`.
//!
//! What is already true: credentials are never written to the database, never
//! appear in a diagnostics bundle, and never reach a log.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use mimic_core::providers::SecretStore;

pub struct FileSecretStore {
    path: PathBuf,
    cache: RwLock<BTreeMap<String, String>>,
}

impl FileSecretStore {
    pub fn open(dir: &Path) -> std::io::Result<Self> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join("credentials.json");
        let cache = match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(e) => return Err(e),
        };
        let store = Self { path, cache: RwLock::new(cache) };
        store.restrict();
        Ok(store)
    }

    /// Which keys have a value. Never the values themselves — this is what the
    /// Settings screen shows.
    pub fn keys(&self) -> Vec<String> {
        self.cache.read().map(|c| c.keys().cloned().collect()).unwrap_or_default()
    }

    pub fn set(&self, key: &str, value: &str) -> std::io::Result<()> {
        {
            let mut cache = self.cache.write().unwrap_or_else(|p| p.into_inner());
            if value.trim().is_empty() {
                cache.remove(key);
            } else {
                cache.insert(key.to_string(), value.to_string());
            }
        }
        self.flush()
    }

    pub fn remove(&self, key: &str) -> std::io::Result<()> {
        self.set(key, "")
    }

    fn flush(&self) -> std::io::Result<()> {
        let cache = self.cache.read().unwrap_or_else(|p| p.into_inner());
        let text = serde_json::to_string_pretty(&*cache).unwrap_or_else(|_| "{}".into());
        std::fs::write(&self.path, text)?;
        self.restrict();
        Ok(())
    }

    fn restrict(&self) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&self.path, std::fs::Permissions::from_mode(0o600));
        }
    }
}

impl SecretStore for FileSecretStore {
    fn get(&self, key: &str) -> Option<String> {
        self.cache.read().ok().and_then(|c| c.get(key).cloned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_secret_survives_a_restart_and_can_be_removed() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileSecretStore::open(dir.path()).unwrap();
        assert!(store.keys().is_empty());
        store.set("provider.anthropic.apiKey", "sk-test").unwrap();

        let reopened = FileSecretStore::open(dir.path()).unwrap();
        assert_eq!(reopened.get("provider.anthropic.apiKey").as_deref(), Some("sk-test"));
        assert_eq!(reopened.keys(), vec!["provider.anthropic.apiKey"], "keys are listable, values are not");

        reopened.remove("provider.anthropic.apiKey").unwrap();
        assert!(FileSecretStore::open(dir.path()).unwrap().keys().is_empty());
    }

    #[test]
    fn a_blank_value_clears_rather_than_storing_whitespace() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileSecretStore::open(dir.path()).unwrap();
        store.set("k", "v").unwrap();
        store.set("k", "   ").unwrap();
        assert_eq!(store.get("k"), None);
    }

    #[cfg(unix)]
    #[test]
    fn the_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let store = FileSecretStore::open(dir.path()).unwrap();
        store.set("k", "v").unwrap();
        let mode = std::fs::metadata(dir.path().join("credentials.json")).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
