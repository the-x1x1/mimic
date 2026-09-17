//! Process-wide state shared by every command.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use mimic_core::bridge::BridgeHandle;
use mimic_core::db::Db;
use mimic_core::engine::EngineClient;
use mimic_core::jobs::JobRunner;
use mimic_core::paths::AppPaths;

pub struct AppState {
    pub paths: AppPaths,
    pub db: Db,
    pub bridge: BridgeHandle,
    pub engine: EngineClient,
    pub jobs: JobRunner,
    pub started_at: String,
    /// Repository root when running from `pnpm tauri dev` (used to find the engine + plugin sources).
    pub repo_root: Option<PathBuf>,
    /// Where the bundled resources live in a packaged build.
    pub resource_dir: Option<PathBuf>,
    pub demo_mode: AtomicBool,
    pub schema_report: mimic_core::db::OpenReport,
    pub log_dir: PathBuf,
}

pub type SharedState = Arc<AppState>;

impl AppState {
    pub fn plugin_source_dir(&self) -> Option<PathBuf> {
        if let Some(res) = &self.resource_dir {
            let p = res.join("plugin").join("Mimic.lrplugin");
            if p.join("Info.lua").is_file() {
                return Some(p);
            }
        }
        if let Some(root) = &self.repo_root {
            let p = root.join("lightroom").join("Mimic.lrplugin");
            if p.join("Info.lua").is_file() {
                return Some(p);
            }
        }
        None
    }

    pub fn manifests_dir(&self) -> Option<PathBuf> {
        if let Some(res) = &self.resource_dir {
            let p = res.join("models").join("manifests");
            if p.is_dir() {
                return Some(p);
            }
        }
        self.repo_root.as_ref().map(|r| r.join("models").join("manifests")).filter(|p| p.is_dir())
    }

    /// True while any job that must not be interrupted by an update is active.
    pub fn is_busy(&self) -> bool {
        self.db.list_jobs(50, true).map(|jobs| !jobs.is_empty()).unwrap_or(false)
    }

    pub fn demo(&self) -> bool {
        self.demo_mode.load(Ordering::Relaxed)
    }
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
