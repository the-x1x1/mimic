//! Boot sequence: paths → logs → the data folder's lock (one Mimic at a
//! time) → database (migrate) → secrets → providers → engine → jobs. The
//! window opens even if the engine fails to start; its status is shown in the
//! UI rather than blocking launch.

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
    // Before anything reads or writes the data: another Mimic on this folder
    // means this one goes, having touched nothing.
    let instance = take_instance_lock(&paths)?;

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
    log_secrets(&db, &secrets);
    let providers =
        std::sync::Arc::new(RwLock::new(ProviderRegistry::new(crate::providers_config::build(&db, secrets.as_ref()))));

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

    // The assist executor resolves its provider per run rather than holding
    // one, because the user can change provider in Settings between runs.
    let provider_registry = providers.clone();
    let provider_db = db.clone();
    let resolve_provider: std::sync::Arc<
        dyn Fn() -> Option<std::sync::Arc<dyn mimic_core::providers::ModelProvider>> + Send + Sync,
    > = std::sync::Arc::new(move || {
        let registry = provider_registry.read().unwrap_or_else(|p| p.into_inner());
        let chosen = provider_db.get_setting::<String>("generation.provider").ok().flatten();
        let id = chosen.or_else(|| registry.default_id())?;
        registry.get(&id).ok()
    });
    let executor = std::sync::Arc::new(mimic_core::jobs::CompositeExecutor::new(vec![
        mimic_core::import::ImportExecutor::shared(),
        mimic_core::voice::AnalyzeExecutor::shared(),
        mimic_core::assist::AssistExecutor::shared(resolve_provider),
        // A mailbox's password is read from the secret store per run, so a
        // password changed or removed since the check was queued is honoured.
        mimic_core::sources::imap::CheckMailboxExecutor::shared({
            let secrets = secrets.clone();
            std::sync::Arc::new(move |key: &str| {
                use mimic_core::providers::SecretStore;
                secrets.get(key)
            })
        }),
        // The endpoint and model are read per run for the same reason: a
        // download queued before the user changed either should use what is
        // configured when it starts, not when it was asked for.
        mimic_core::localmodel::PullExecutor::shared(std::sync::Arc::new(|db| {
            mimic_core::localmodel::configured(db)
                .unwrap_or_else(|_| ("http://127.0.0.1:11434/v1".to_string(), "llama3.2:3b".to_string()))
        })),
    ]));
    let jobs = JobRunner::new(db.clone(), executor);
    let (interrupted, requeued) = jobs.recover()?;
    // A mailbox left "importing" by a check that never finished is ready
    // again, so it is neither stuck nor shown as busy.
    if let Err(e) = mimic_core::sources::imap::recover(&db) {
        tracing::warn!(target: "mail", error = %e, "could not reset interrupted mailbox checks");
    }
    if interrupted > 0 {
        tracing::warn!(target: "jobs", interrupted, requeued, "recovered interrupted jobs");
    }
    // Mail read before one of the user's addresses was declared, by versions
    // before 0.10.0-alpha.8, may still be filed under someone who is nothing
    // but the user. Nothing is running yet, so nothing holds them.
    crate::commands::people::reconcile_identity(&db, &jobs, "startup");

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
        instance,
    })
}

/// How long a starting Mimic waits for one that is closing to let go. A
/// launch made while Mimic was closing is started by the closing one on its
/// way out (`lib.rs`), so it arrives while the old process is still exiting.
const TAKE_OVER_WITHIN: Duration = Duration::from_secs(10);

/// This process's hold on the data folder. When another Mimic already has it,
/// the error is `instance::AlreadyRunning`, which the shell tells apart from a
/// start-up that failed (`anyhow::Error::is`).
pub fn take_instance_lock(paths: &AppPaths) -> anyhow::Result<mimic_core::instance::InstanceLock> {
    take_instance_lock_within(paths, TAKE_OVER_WITHIN)
}

fn take_instance_lock_within(paths: &AppPaths, wait: Duration) -> anyhow::Result<mimic_core::instance::InstanceLock> {
    use mimic_core::instance::{InstanceLock, LockError};
    match InstanceLock::acquire_within(&paths.instance_lock_file(), wait) {
        Ok(lock) => Ok(lock),
        Err(LockError::AlreadyRunning(held)) => {
            tracing::warn!(target: "app", lock = %held.0.display(), "another Mimic is using this data folder; leaving it alone");
            Err(held.into())
        }
        Err(e) => Err(e.into()),
    }
}

/// What opening the credential store did, as counts and reasons: never a
/// value.
fn log_secrets(db: &Db, store: &FileSecretStore) {
    let report = store.report();
    let protection = store.protection();
    if report.moved > 0 {
        tracing::info!(target: "secrets", moved = report.moved, ?protection, "old credentials file moved in");
        let _ = db.log_event(&NewEvent::info(
            "secrets",
            "moved",
            json!({ "moved": report.moved, "protection": protection }),
        ));
    }
    if report.removed_damaged_old_file {
        tracing::warn!(target: "secrets", "the old credentials file was damaged and has been deleted");
        let _ = db.log_event(&NewEvent::warn("secrets", "damaged_old_file_deleted", json!({})));
    }
    if let Some(why) = &report.old_file_kept {
        tracing::warn!(target: "secrets", reason = %why, "the old unsealed credentials file is still there");
        let _ = db.log_event(&NewEvent::warn("secrets", "old_file_kept", json!({ "reason": why })));
    }
    if let Some(why) = &report.left_alone {
        tracing::warn!(target: "secrets", reason = %why, "the credentials file is left alone and nothing is saved over it");
        let _ = db.log_event(&NewEvent::warn("secrets", "left_alone", json!({ "reason": why })));
    }
    if report.set_aside.is_some() {
        tracing::warn!(target: "secrets", "the credentials file could not be read and was set aside");
        let _ = db.log_event(&NewEvent::warn("secrets", "set_aside", json!({})));
    }
    if report.locked > 0 {
        tracing::warn!(target: "secrets", locked = report.locked, "saved credentials that do not open on this account");
        let _ = db.log_event(&NewEvent::warn("secrets", "locked", json!({ "count": report.locked })));
    }
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
        let st = state.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(ev) => {
                        // Mail just read may be filed under someone whose
                        // every address is the user's (read while one was
                        // being added). They are folded back before the
                        // screen is told. After any job, not only a read: one
                        // that holds people defers it, and the next job to
                        // finish is the next chance.
                        if matches!(ev.status.as_str(), "completed" | "failed" | "canceled") {
                            crate::commands::people::reconcile_identity(&st.db, &st.jobs, "after_job");
                        }
                        let _ = app2.emit("jobs://event", &ev);
                        // New messages or a new profile change what a prepared
                        // reply would say, so a finished import or analysis is
                        // the moment to prepare them — but only if the user
                        // asked for that, and never off the back of an assist
                        // run itself.
                        let follows_work = ev.status == "completed"
                            && (ev.kind == mimic_core::import::JOB_KIND
                                || ev.kind == mimic_core::voice::JOB_KIND
                                || ev.kind == mimic_core::sources::imap::JOB_KIND);
                        if follows_work && mimic_core::assist::is_enabled(&st.db).unwrap_or(false) {
                            match st.jobs.enqueue(mimic_core::assist::JOB_KIND, json!({})) {
                                Ok(job) => tracing::info!(target: "assist", job = %job.id, "queued assisted drafting"),
                                Err(e) => {
                                    tracing::warn!(target: "assist", error = %e, "could not queue assisted drafting")
                                }
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                }
            }
        });
    }
    // Connected mailboxes: once a minute, queue a check for any that are due.
    // The interval is read each time, so turning checking off in Settings
    // takes effect without a restart, and a mailbox already being checked is
    // never queued twice.
    {
        let st = state.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(60)).await;
                match mimic_core::sources::imap::due_now(&st.db) {
                    Ok(ids) => {
                        for id in ids {
                            if let Err(e) =
                                st.jobs.enqueue(mimic_core::sources::imap::JOB_KIND, json!({ "sourceId": id }))
                            {
                                tracing::warn!(target: "mail", error = %e, "could not queue a mailbox check");
                            }
                        }
                    }
                    Err(e) => tracing::warn!(target: "mail", error = %e, "could not work out which mailboxes are due"),
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mimic_core::instance::AlreadyRunning;

    #[test]
    fn a_second_mimic_on_the_same_data_is_told_apart_from_a_failed_start() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(dir.path());
        paths.ensure().unwrap();
        let wait = Duration::from_millis(200);
        let first = take_instance_lock_within(&paths, wait).unwrap();
        let second = take_instance_lock_within(&paths, wait).unwrap_err();
        assert!(second.is::<AlreadyRunning>(), "{second}");
        drop(first);
        take_instance_lock(&paths).expect("the data is free once the first has gone");

        // A folder that cannot be locked is a failure, not "already running".
        let missing = AppPaths::new(dir.path().join("never-made"));
        assert!(!take_instance_lock_within(&missing, wait).unwrap_err().is::<AlreadyRunning>());
    }
}
