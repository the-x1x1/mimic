//! Boot sequence: paths → logs → database (migrate) → bridge → engine → jobs.
//! The window opens even if the engine or bridge fail; their status is shown
//! in the UI instead of blocking launch (spec §18).

use std::path::PathBuf;
use std::time::Duration;

use mimic_core::bridge::{BridgeConfig, BridgeServer};
use mimic_core::db::{Db, NewEvent};
use mimic_core::engine::{resolve_engine_command, EngineClient, EngineConfig};
use mimic_core::ingest;
use mimic_core::jobs::JobRunner;
use mimic_core::paths::AppPaths;
use serde_json::json;
use tauri::{AppHandle, Emitter};

use crate::app_state::{detect_repo_root, AppState};

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

    // Bridge: loopback only, random port, per-launch token.
    let bridge_server = BridgeServer::start(BridgeConfig::default()).await?;
    let bridge = bridge_server.handle.clone();
    bridge.write_discovery_file(&paths.bridge_discovery_file())?;
    tracing::info!(target: "lightroom", url = bridge.base_url(), "bridge listening");
    // Leak the server so it lives as long as the process (stop on window destroy is not needed: OS reclaims the port).
    std::mem::forget(bridge_server);

    let repo_root = if resource_dir.as_ref().is_some_and(|r| {
        r.join("engine").join("mimic-engine.exe").is_file() || r.join("engine").join("mimic-engine").is_file()
    }) {
        None
    } else {
        detect_repo_root()
    };
    let engine_dir = resource_dir.as_ref().map(|r| r.join("engine"));
    let engine_cmd = resolve_engine_command(repo_root.as_deref(), engine_dir.as_deref());
    let engine = match engine_cmd {
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
        ingest::executor(engine.clone(), bridge.clone()),
        mimic_core::training::executor(engine.clone()),
        mimic_core::sessions::executor(engine.clone(), bridge.clone()),
    ]));
    let jobs = JobRunner::new(db.clone(), executor);
    let (interrupted, requeued) = jobs.recover()?;
    if interrupted > 0 {
        tracing::warn!(target: "jobs", interrupted, requeued, "recovered interrupted jobs");
    }

    let plugin_root = paths.plugin_install_dir();
    let state = AppState {
        paths,
        db,
        bridge,
        engine,
        jobs,
        started_at: mimic_core::ids::now_rfc3339(),
        repo_root,
        resource_dir,
        demo_mode: std::sync::atomic::AtomicBool::new(false),
        schema_report,
        log_dir,
    };
    // Keep the app-managed plugin copy fresh on every launch (idempotent).
    if let Some(src) = state.plugin_source_dir() {
        if let Err(e) = crate::plugin_install::sync_plugin(&src, &plugin_root) {
            tracing::warn!(target: "lightroom", error = %e, "could not sync plugin copy");
        }
    }
    Ok(state)
}

/// Long-lived tasks: engine start, job loop, bridge sweep, event forwarding.
pub fn spawn_background(app: AppHandle, state: crate::SharedState) {
    // Engine start + configure (non-blocking for the window).
    {
        let st = state.clone();
        let app2 = app.clone();
        tauri::async_runtime::spawn(async move {
            match st.engine.start().await {
                Ok(status) => {
                    tracing::info!(target: "engine", version = ?status.engine_version, "engine ready");
                    let cfg = json!({
                        "dbPath": st.db.path().map(|p| p.to_string_lossy().to_string()),
                        "previewsDir": st.paths.previews_cache(),
                        "embeddingsDir": st.paths.embeddings_cache(),
                        "encodersDir": st.paths.encoders_dir(),
                        "stylesDir": st.paths.styles_dir(),
                        "manifestsDir": st.manifests_dir(),
                    });
                    if let Err(e) = st.engine.call("engine.configure", cfg).await {
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
                                    let cfg = json!({
                                        "dbPath": st.db.path().map(|p| p.to_string_lossy().to_string()),
                                        "previewsDir": st.paths.previews_cache(),
                                        "embeddingsDir": st.paths.embeddings_cache(),
                                        "encodersDir": st.paths.encoders_dir(),
                                        "stylesDir": st.paths.styles_dir(),
                                        "manifestsDir": st.manifests_dir(),
                                    });
                                    let _ = st.engine.call("engine.configure", cfg).await;
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
    // Bridge events + liveness sweep + persisted connection record.
    {
        let st = state.clone();
        let app2 = app.clone();
        let mut rx = st.bridge.subscribe();
        tauri::async_runtime::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(ev) => {
                        match &ev {
                            mimic_core::bridge::BridgeEvent::Connected { connection } => {
                                let _ = st.db.record_lightroom_connection(
                                    &connection.catalog_fingerprint,
                                    Some(&connection.lightroom_version),
                                    connection.sdk_version.as_deref(),
                                    Some(&connection.plugin_version),
                                    &serde_json::to_value(&connection.capabilities).unwrap_or_default(),
                                    "connected",
                                );
                                let _ = st.db.log_event(&NewEvent::info("lightroom", "connected", json!({"lightroomVersion": connection.lightroom_version, "catalog": connection.catalog_name})));
                            }
                            mimic_core::bridge::BridgeEvent::Disconnected { reason } => {
                                if let Ok(Some(c)) = st.db.latest_lightroom_connection() {
                                    let _ =
                                        st.db.set_lightroom_connection_status(&c.catalog_fingerprint, "disconnected");
                                }
                                let _ = st.db.log_event(&NewEvent::warn(
                                    "lightroom",
                                    "disconnected",
                                    json!({"reason": reason}),
                                ));
                            }
                            _ => {}
                        }
                        let _ = app2.emit("lightroom://event", &ev);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                }
            }
        });
        let st = state.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(2)).await;
                st.bridge.sweep();
            }
        });
    }
}
