//! Boot sequence: paths → logs → database (migrate) → secrets → providers →
//! engine → jobs. The window opens even if the engine fails to start; its
//! status is shown in the UI rather than blocking launch.

use std::path::PathBuf;
use std::sync::RwLock;
use std::time::Duration;

use mimic_core::db::{Db, NewEvent};
use mimic_core::engine::{resolve_engine_command, EngineClient, EngineConfig};
use mimic_core::jobs::JobRunner;
use mimic_core::paths::AppPaths;
use mimic_core::providers::ProviderRegistry;
use serde_json::json;
use tauri::{AppHandle, Emitter};

use crate::app_state::{detect_repo_root, AppState};
use crate::secrets::FileSecretStore;

pub async fn boot(resource_dir: Option<PathBuf>) -> anyhow::Result<AppState> {
    let root = std::env::var_os("MIMIC_DATA_DIR")
        .map(PathBuf::from)
        .or_else(AppPaths::default_root)
        .ok_or_else(|| anyhow::anyhow!("cannot resolve app data directory"))?;
    let paths = AppPaths::new(root);
    paths.ensure()?;
    let log_dir = paths.logs_dir();
    let _guard = crate::logging::init(&log_dir);
    // Keep the appender alive for the process lifetime.
    if let Some(g) = _guard {
        std::mem::forget(g);
    }
    tracing::info!(target: "app", version = mimic_core::APP_VERSION, root = %paths.root.display(), "starting Mimic");

    let (db, schema_report) = Db::open(&paths.database_file(), &paths.backups_dir())?;
    if !schema_report.applied.is_empty() {
        tracing::info!(target: "db", applied = ?schema_report.applied, backup = ?schema_report.backup_path, "migrations applied");
        let _ = db.log_event(&NewEvent::info(
            "db",
            "migrated",
            json!({"applied": schema_report.applied, "backup": schema_report.backup_path}),
        ));
    }

    let secrets = std::sync::Arc::new(FileSecretStore::open(&paths.credentials_dir())?);
    let providers = RwLock::new(ProviderRegistry::new(crate::providers_config::build(&db, secrets.as_ref())));

    let repo_root = if resource_dir.as_ref().is_some_and(|r| {
        r.join("engine").join("mimic-engine.exe").is_file() || r.join("engine").join("mimic-engine").is_file()
    }) {
        None
    } else {
        detect_repo_root()
    };
    let engine_dir = resource_dir.as_ref().map(|r| r.join("engine"));
    let engine = match resolve_engine_command(repo_root.as_deref(), engine_dir.as_deref()) {
        Some(cmd) => {
            tracing::info!(target: "engine", program = %cmd.program.display(), args = ?cmd.args, "engine command resolved");
            EngineClient::new(EngineConfig::new(cmd))
        }
        None => {
            tracing::error!(target: "engine", "no engine found (neither bundled nor repository `engine/`)");
            EngineClient::new(EngineConfig::new(mimic_core::engine::EngineCommand {
                program: PathBuf::from("mimic-engine-not-found"),
                args: vec![],
                cwd: None,
                env: vec![],
            }))
        }
    };

    let executor = std::sync::Arc::new(mimic_core::jobs::CompositeExecutor::new(vec![
        mimic_core::import::ImportExecutor::shared(),
        mimic_core::voice::AnalyzeExecutor::shared(),
    ]));
    let jobs = JobRunner::new(db.clone(), executor);
    let (interrupted, requeued) = jobs.recover()?;
    if interrupted > 0 {
        tracing::warn!(target: "jobs", interrupted, requeued, "recovered interrupted jobs");
    }

    Ok(AppState {
        paths,
        db,
        engine,
        jobs,
        providers,
        secrets,
        started_at: mimic_core::ids::now_rfc3339(),
        repo_root,
        resource_dir,
        demo_mode: std::sync::atomic::AtomicBool::new(false),
        schema_report,
        log_dir,
    })
}

fn engine_config(state: &crate::SharedState) -> serde_json::Value {
    json!({
        "dbPath": state.db.path().map(|p| p.to_string_lossy().to_string()),
        "embeddingsDir": state.paths.embeddings_cache(),
        "encodersDir": state.paths.encoders_dir(),
        "manifestsDir": state.manifests_dir(),
    })
}

/// Long-lived tasks: engine start, job loop, event forwarding.
pub fn spawn_background(app: AppHandle, state: crate::SharedState) {
    // Engine start + configure (non-blocking for the window).
    {
        let st = state.clone();
        let app2 = app.clone();
        tauri::async_runtime::spawn(async move {
            match st.engine.start().await {
                Ok(status) => {
                    tracing::info!(target: "engine", version = ?status.engine_version, "engine ready");
                    if let Err(e) = st.engine.call("engine.configure", engine_config(&st)).await {
                        tracing::error!(target: "engine", error = %e, "engine.configure failed");
                    }
                }
                Err(e) => {
                    tracing::error!(target: "engine", error = %e, "engine failed to start");
                    let _ =
                        st.db.log_event(&NewEvent::error("engine", "start_failed", json!({"error": e.to_string()})));
                }
            }
            let _ = app2.emit("engine://status", st.engine.status());
        });
    }
    // Engine events → UI + auto-restart on crash.
    {
        let st = state.clone();
        let app2 = app.clone();
        tauri::async_runtime::spawn(async move {
            let mut rx = st.engine.subscribe();
            loop {
                match rx.recv().await {
                    Ok(ev) => {
                        let _ = app2.emit("engine://event", &ev);
                        if let mimic_core::engine::EngineEvent::Exited { code } = ev {
                            tracing::warn!(target: "engine", ?code, "engine exited; scheduling restart");
                            let _ = st.db.log_event(&NewEvent::error("engine", "exited", json!({"code": code})));
                            tokio::time::sleep(Duration::from_secs(2)).await;
                            if st.engine.status().state != "ready" {
                                let _ = st.engine.restart().await;
                                if st.engine.is_ready() {
                                    let _ = st.engine.call("engine.configure", engine_config(&st)).await;
                                }
                            }
                            let _ = app2.emit("engine://status", st.engine.status());
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                }
            }
        });
    }
    // Job loop + job events.
    {
        let runner = state.jobs.clone();
        tauri::async_runtime::spawn(async move { runner.run_loop().await });
        let mut rx = state.jobs.subscribe();
        let app2 = app.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(ev) => {
                        let _ = app2.emit("jobs://event", &ev);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                }
            }
        });
    }
}
