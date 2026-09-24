//! Process-wide state shared by every command.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use mimic_core::db::Db;
use mimic_core::engine::EngineClient;
use mimic_core::jobs::JobRunner;
use mimic_core::paths::AppPaths;
use mimic_core::providers::{ModelProvider, ProviderRegistry};

pub struct AppState {
    pub paths: AppPaths,
    pub db: Db,
    pub engine: EngineClient,
    pub jobs: JobRunner,
    /// Shared so background jobs can resolve the current provider without
    /// holding a reference to the whole state.
    pub providers: Arc<RwLock<ProviderRegistry>>,
    pub secrets: Arc<crate::secrets::FileSecretStore>,
    pub started_at: String,
    /// Repository root when running from `pnpm tauri dev` (used to find the engine).
    pub repo_root: Option<PathBuf>,
    /// Where the bundled resources live in a packaged build.
    pub resource_dir: Option<PathBuf>,
    pub demo_mode: AtomicBool,
    pub schema_report: mimic_core::db::OpenReport,
    pub log_dir: PathBuf,
    /// This process's hold on the data folder, for as long as the app runs.
    pub instance: mimic_core::instance::InstanceLock,
    /// What mailboxes sign in with, shared with the mailbox check, so a
    /// sign-in made here replaces what the check has kept.
    pub mail_credentials: Arc<mimic_core::sources::imap::Credentials>,
    /// A mailbox sign-in waiting on the browser.
    pub sign_in: MailSignIn,
}

/// A mailbox sign-in: one at a time, stoppable, and — between signing in
/// and saying "connect" — what it brought back, held in memory only.
#[derive(Default)]
pub struct MailSignIn {
    pub active: AtomicBool,
    pub cancel: AtomicBool,
    pub pending: std::sync::Mutex<Option<PendingSignIn>>,
}

/// A mailbox signed in to and looked at, not yet connected.
pub struct PendingSignIn {
    pub email: String,
    pub refresh_token: String,
    pub probe: mimic_core::sources::imap::ImapProbe,
}

pub type SharedState = Arc<AppState>;

impl AppState {
    pub fn manifests_dir(&self) -> Option<PathBuf> {
        manifests_dir(self.resource_dir.as_deref(), self.repo_root.as_deref())
    }

    /// Where the sentence encoder's manifest and downloaded files are.
    pub fn encoder_places(&self) -> mimic_core::encoder::Places {
        mimic_core::encoder::Places { manifests: self.manifests_dir(), encoders: self.paths.encoders_dir() }
    }

    /// True while any job that must not be interrupted by an update is active.
    pub fn is_busy(&self) -> bool {
        self.db.list_jobs(50, true).map(|jobs| !jobs.is_empty()).unwrap_or(false)
    }

    pub fn demo(&self) -> bool {
        self.demo_mode.load(Ordering::Relaxed)
    }

    /// The provider the user selected, or the safest default.
    pub fn active_provider(&self) -> Result<Arc<dyn ModelProvider>, crate::error::CommandError> {
        let registry = self.providers.read().unwrap_or_else(|p| p.into_inner());
        let chosen = self.db.get_setting::<String>("generation.provider").ok().flatten();
        let id = match chosen {
            Some(id) => id,
            None => registry
                .default_id()
                .ok_or_else(|| crate::error::CommandError::new("no_provider", "No model provider is configured."))?,
        };
        Ok(registry.get(&id)?)
    }

    /// The model on this computer to read the user's messages with: the one
    /// they chose, when it is on this computer, or else the first that is.
    /// Never a hosted one.
    pub fn local_provider(&self) -> Option<Arc<dyn ModelProvider>> {
        let registry = self.providers.read().unwrap_or_else(|p| p.into_inner());
        local_provider(&registry, &self.db)
    }

    /// Rebuild the registry from the current settings and secrets. Called at
    /// boot and whenever a provider setting changes.
    pub fn rebuild_providers(&self) {
        let providers = crate::providers_config::build(&self.db, self.secrets.as_ref());
        if let Ok(mut guard) = self.providers.write() {
            *guard = ProviderRegistry::new(providers);
        }
    }
}

/// The encoder manifests bundled with the app, or in the repository when run
/// from it.
pub fn manifests_dir(resource_dir: Option<&std::path::Path>, repo_root: Option<&std::path::Path>) -> Option<PathBuf> {
    if let Some(res) = resource_dir {
        let p = res.join("models").join("manifests");
        if p.is_dir() {
            return Some(p);
        }
    }
    repo_root.map(|r| r.join("models").join("manifests")).filter(|p| p.is_dir())
}

/// `AppState::local_provider`, for a registry held elsewhere (a job's).
pub fn local_provider(registry: &ProviderRegistry, db: &Db) -> Option<Arc<dyn ModelProvider>> {
    let chosen = db.get_setting::<String>("generation.provider").ok().flatten();
    let local = |id: &str| registry.get(id).ok().filter(|p| p.info().local);
    chosen.as_deref().and_then(local).or_else(|| registry.list().iter().find(|p| p.local).and_then(|p| local(&p.id)))
}

/// Locate the repository root from the executable or CWD (dev only).
pub fn detect_repo_root() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.to_path_buf());
        }
    }
    for start in candidates {
        let mut cur = Some(start.as_path());
        while let Some(dir) = cur {
            if dir.join("pnpm-workspace.yaml").is_file() && dir.join("engine").join("pyproject.toml").is_file() {
                return Some(dir.to_path_buf());
            }
            cur = dir.parent();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use mimic_core::providers::mock::MockProvider;

    #[test]
    fn messages_are_read_only_with_a_model_on_this_computer() {
        let db = Db::open_in_memory().unwrap();
        let cloud: Arc<dyn ModelProvider> = Arc::new(MockProvider::named("cloud", false));
        let near: Arc<dyn ModelProvider> = Arc::new(MockProvider::named("near", true));
        let both = ProviderRegistry::new(vec![cloud.clone(), near]);
        db.set_setting("generation.provider", &"cloud".to_string()).unwrap();
        assert_eq!(local_provider(&both, &db).map(|p| p.info().id).as_deref(), Some("near"), "chosen, but hosted");
        let hosted = ProviderRegistry::new(vec![cloud]);
        assert!(local_provider(&hosted, &db).is_none(), "never a hosted one");
        db.set_setting("generation.provider", &"near".to_string()).unwrap();
        assert_eq!(local_provider(&both, &db).map(|p| p.info().id).as_deref(), Some("near"));
    }
}
