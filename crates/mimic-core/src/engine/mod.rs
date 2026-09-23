//! Python engine sidecar client: newline-delimited JSON over stdio
//! (docs/ARCHITECTURE.md §20).
//!
//! * The child is spawned with an argument array — never a shell string.
//! * Every request carries a UUID `requestId`; responses are correlated.
//! * Lines above `max_message_bytes` are rejected and the child is restarted.
//! * Unsolicited `event` lines (`job.progress`, `log`) are broadcast.
//! * Nothing waits on the engine without a limit. A request's timeout covers
//!   writing it as well as the answer, so an engine that stopped reading
//!   can't hold a caller forever; a line that could not be written whole
//!   leaves the stream unreadable, so that engine is killed (and the app
//!   starts another). Stopping the engine never waits on a write.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{broadcast, oneshot};

use crate::ids::new_id;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    #[serde(default)]
    pub env: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub command: EngineCommand,
    pub max_message_bytes: usize,
    pub request_timeout: Duration,
    pub startup_timeout: Duration,
    pub max_restarts: u32,
}

impl EngineConfig {
    pub fn new(command: EngineCommand) -> Self {
        Self {
            command,
            max_message_bytes: 32 * 1024 * 1024,
            request_timeout: Duration::from_secs(120),
            startup_timeout: Duration::from_secs(60),
            max_restarts: 5,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("engine is not running: {0}")]
    NotRunning(String),
    #[error("engine request timed out after {0:?}")]
    Timeout(Duration),
    #[error("engine error {code}: {message}")]
    Remote { code: String, message: String, details: Option<Value> },
    #[error("engine protocol error: {0}")]
    Protocol(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineStatus {
    /// `stopped` | `starting` | `ready` | `failed`
    pub state: String,
    pub pid: Option<u32>,
    pub engine_version: Option<String>,
    pub protocol_version: Option<u32>,
    pub python_version: Option<String>,
    pub accelerator: Option<String>,
    pub restarts: u32,
    pub last_error: Option<String>,
    pub capabilities: Value,
}

impl Default for EngineStatus {
    fn default() -> Self {
        Self {
            state: "stopped".into(),
            pid: None,
            engine_version: None,
            protocol_version: None,
            python_version: None,
            accelerator: None,
            restarts: 0,
            last_error: None,
            capabilities: Value::Null,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum EngineEvent {
    #[serde(rename = "job.progress")]
    JobProgress {
        #[serde(rename = "jobId")]
        job_id: String,
        phase: String,
        current: i64,
        total: i64,
        #[serde(default)]
        message: Option<String>,
    },
    #[serde(rename = "log")]
    Log { level: String, message: String },
    #[serde(rename = "engine.exited")]
    Exited { code: Option<i32> },
}

#[derive(Debug, Deserialize)]
struct WireResponse {
    #[serde(rename = "protocolVersion")]
    protocol_version: u32,
    #[serde(rename = "requestId")]
    request_id: String,
    ok: bool,
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    error: Option<WireError>,
}

#[derive(Debug, Deserialize)]
struct WireError {
    code: String,
    message: String,
    #[serde(default)]
    details: Option<Value>,
}

/// Where requests to one engine process are written. Each process has its
/// own, with its own lock, so a write stuck on an engine that stopped reading
/// holds up only requests to that engine, and starting the next one never
/// waits for it.
#[derive(Clone)]
struct Writer {
    generation: u64,
    stdin: Arc<tokio::sync::Mutex<ChildStdin>>,
}

struct Inner {
    config: EngineConfig,
    /// The live process, with its generation. Locked only to start, reap or
    /// kill it — never while a request is written — so stopping the engine
    /// can't wait on a write.
    child: tokio::sync::Mutex<Option<(u64, Child)>>,
    writer: Mutex<Option<Writer>>,
    pending: Mutex<HashMap<String, oneshot::Sender<Result<Value, EngineError>>>>,
    status: RwLock<EngineStatus>,
    events: broadcast::Sender<EngineEvent>,
    generation: AtomicU64,
}

#[derive(Clone)]
pub struct EngineClient {
    inner: Arc<Inner>,
}

impl EngineClient {
    pub fn new(config: EngineConfig) -> Self {
        let (events, _) = broadcast::channel(1024);
        Self {
            inner: Arc::new(Inner {
                config,
                child: tokio::sync::Mutex::new(None),
                writer: Mutex::new(None),
                pending: Mutex::new(HashMap::new()),
                status: RwLock::new(EngineStatus::default()),
                events,
                generation: AtomicU64::new(0),
            }),
        }
    }

    pub fn status(&self) -> EngineStatus {
        self.inner.status.read().unwrap_or_else(|p| p.into_inner()).clone()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<EngineEvent> {
        self.inner.events.subscribe()
    }

    fn set_status(&self, f: impl FnOnce(&mut EngineStatus)) {
        let mut s = self.inner.status.write().unwrap_or_else(|p| p.into_inner());
        f(&mut s);
    }

    fn writer(&self) -> Option<Writer> {
        self.inner.writer.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// Nothing more is written to the engine of `generation`, if it is still
    /// the live one. Writes already under way fail once it is gone.
    fn close_writer(&self, generation: u64) {
        let mut slot = self.inner.writer.lock().unwrap_or_else(|p| p.into_inner());
        if slot.as_ref().is_some_and(|w| w.generation == generation) {
            *slot = None;
        }
    }

    /// A request to the engine of `generation` could not be written whole:
    /// part of a line may be in the pipe, and nothing after it would be read
    /// correctly. Kill that engine, if it is still the live one; its reader
    /// reports the exit, and the app starts another.
    async fn abandon(&self, generation: u64, why: &str) {
        if self.inner.generation.load(Ordering::SeqCst) != generation {
            return;
        }
        tracing::error!(target: "engine", "{why}; stopping the engine, whose input can no longer be read");
        self.kill_generation(generation).await;
    }

    /// Kill the engine of `generation`, and only that one: one started since
    /// (by the app's restart, say) is left alone.
    async fn kill_generation(&self, generation: u64) {
        self.close_writer(generation);
        let mut guard = self.inner.child.lock().await;
        if guard.as_ref().is_some_and(|(g, _)| *g == generation) {
            if let Some((_, mut child)) = guard.take() {
                let _ = child.start_kill();
                let _ = tokio::time::timeout(Duration::from_secs(3), child.wait()).await;
            }
        }
    }

    /// Spawn the child and wait for a successful `engine.hello`.
    pub async fn start(&self) -> Result<EngineStatus, EngineError> {
        let mut guard = self.inner.child.lock().await;
        if guard.is_some() {
            return Ok(self.status());
        }
        self.set_status(|s| {
            s.state = "starting".into();
            s.last_error = None;
        });
        let cmd = &self.inner.config.command;
        let mut command = Command::new(&cmd.program);
        command.args(&cmd.args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
        if let Some(cwd) = &cmd.cwd {
            command.current_dir(cwd);
        }
        command.env("PYTHONUNBUFFERED", "1").env("PYTHONIOENCODING", "utf-8");
        for (k, v) in &cmd.env {
            command.env(k, v);
        }
        #[cfg(windows)]
        {
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = match command.spawn() {
            Ok(c) => c,
            Err(e) => {
                let msg = format!("failed to start {}: {e}", cmd.program.display());
                self.set_status(|s| {
                    s.state = "failed".into();
                    s.last_error = Some(msg.clone());
                });
                return Err(EngineError::NotRunning(msg));
            }
        };
        let pid = child.id();
        let stdout = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take().expect("piped stderr");
        let stdin = child.stdin.take().expect("piped stdin");
        let generation = self.inner.generation.fetch_add(1, Ordering::SeqCst) + 1;
        *guard = Some((generation, child));
        *self.inner.writer.lock().unwrap_or_else(|p| p.into_inner()) =
            Some(Writer { generation, stdin: Arc::new(tokio::sync::Mutex::new(stdin)) });
        drop(guard);

        // Reader tasks.
        let this = self.clone();
        tokio::spawn(async move { this.read_stdout(stdout, generation).await });
        let events = self.inner.events.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                tracing::warn!(target: "engine.stderr", "{line}");
                let _ = events.send(EngineEvent::Log { level: "stderr".into(), message: line });
            }
        });

        self.set_status(|s| s.pid = pid);
        match tokio::time::timeout(
            self.inner.config.startup_timeout,
            self.call("engine.hello", json!({"appVersion": crate::APP_VERSION})),
        )
        .await
        {
            Ok(Ok(hello)) => {
                let proto = hello.get("protocolVersion").and_then(Value::as_u64).map(|v| v as u32);
                if proto != Some(crate::ENGINE_PROTOCOL_VERSION) {
                    let msg = format!("engine protocol {proto:?} != {}", crate::ENGINE_PROTOCOL_VERSION);
                    self.kill_internal().await;
                    self.set_status(|s| {
                        s.state = "failed".into();
                        s.last_error = Some(msg.clone());
                    });
                    return Err(EngineError::Protocol(msg));
                }
                self.set_status(|s| {
                    s.state = "ready".into();
                    s.engine_version = hello.get("engineVersion").and_then(Value::as_str).map(str::to_string);
                    s.protocol_version = proto;
                    s.python_version = hello.get("pythonVersion").and_then(Value::as_str).map(str::to_string);
                    s.accelerator = hello.get("accelerator").and_then(Value::as_str).map(str::to_string);
                    s.capabilities = hello.get("capabilities").cloned().unwrap_or(Value::Null);
                });
                Ok(self.status())
            }
            Ok(Err(e)) => {
                self.kill_internal().await;
                self.set_status(|s| {
                    s.state = "failed".into();
                    s.last_error = Some(e.to_string());
                });
                Err(e)
            }
            Err(_) => {
                self.kill_internal().await;
                let msg = "engine did not answer hello in time".to_string();
                self.set_status(|s| {
                    s.state = "failed".into();
                    s.last_error = Some(msg.clone());
                });
                Err(EngineError::Timeout(self.inner.config.startup_timeout))
            }
        }
    }

    async fn read_stdout(&self, stdout: tokio::process::ChildStdout, generation: u64) {
        let max = self.inner.config.max_message_bytes;
        let mut reader = BufReader::with_capacity(64 * 1024, stdout);
        let mut buf: Vec<u8> = Vec::new();
        loop {
            buf.clear();
            let n = match reader.read_until(b'\n', &mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            if n > max {
                tracing::error!("engine message of {n} bytes exceeds limit {max}; restarting engine");
                self.fail_all_pending(EngineError::Protocol(format!("message exceeded {max} bytes")));
                self.kill_generation(generation).await;
                break;
            }
            let line = String::from_utf8_lossy(&buf);
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let value: Value = match serde_json::from_str(line) {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!("engine emitted non-JSON line ({e}): {}", truncate(line, 200));
                    continue;
                }
            };
            if value.get("event").is_some() {
                match serde_json::from_value::<EngineEvent>(value) {
                    Ok(ev) => {
                        let _ = self.inner.events.send(ev);
                    }
                    Err(e) => tracing::warn!("unknown engine event: {e}"),
                }
                continue;
            }
            match serde_json::from_value::<WireResponse>(value) {
                Ok(resp) => {
                    if resp.protocol_version != crate::ENGINE_PROTOCOL_VERSION {
                        tracing::warn!("engine response with protocol {}", resp.protocol_version);
                    }
                    let waiter = self.inner.pending.lock().unwrap_or_else(|p| p.into_inner()).remove(&resp.request_id);
                    if let Some(tx) = waiter {
                        let out = if resp.ok {
                            Ok(resp.result.unwrap_or(Value::Null))
                        } else {
                            let e = resp.error.unwrap_or(WireError {
                                code: "unknown".into(),
                                message: "engine failure without details".into(),
                                details: None,
                            });
                            Err(EngineError::Remote { code: e.code, message: e.message, details: e.details })
                        };
                        let _ = tx.send(out);
                    }
                }
                Err(e) => tracing::warn!("unparseable engine response: {e}"),
            }
        }
        // Child ended (or was killed). Only react if this reader belongs to
        // the live generation — checked again under the process lock, and
        // the lock held until the exit is reported, so an engine started
        // meanwhile (a restart) is neither reaped, failed nor reported here.
        if self.inner.generation.load(Ordering::SeqCst) != generation {
            return;
        }
        let mut guard = self.inner.child.lock().await;
        if self.inner.generation.load(Ordering::SeqCst) != generation {
            return;
        }
        self.close_writer(generation);
        // stdout can close before the process is fully reaped (visible on
        // Windows), so wait briefly for the real exit status.
        let code = match guard.as_mut() {
            Some((g, child)) if *g == generation => {
                let code = tokio::time::timeout(Duration::from_secs(5), child.wait())
                    .await
                    .ok()
                    .and_then(|res| res.ok())
                    .and_then(|s| s.code());
                *guard = None;
                code
            }
            _ => None,
        };
        self.fail_all_pending(EngineError::NotRunning("engine process exited".into()));
        self.set_status(|s| {
            if s.state != "failed" {
                s.state = "stopped".into();
            }
            s.pid = None;
            s.last_error.get_or_insert_with(|| format!("engine exited with code {code:?}"));
        });
        let _ = self.inner.events.send(EngineEvent::Exited { code });
        drop(guard);
    }

    fn fail_all_pending(&self, err: EngineError) {
        let pending: Vec<_> = self.inner.pending.lock().unwrap_or_else(|p| p.into_inner()).drain().collect();
        let msg = err.to_string();
        for (_, tx) in pending {
            let _ = tx.send(Err(EngineError::NotRunning(msg.clone())));
        }
    }

    /// Kill the live engine. Never waits on a write: the process lock is not
    /// held while writing, and a write in progress fails once it is gone.
    async fn kill_internal(&self) {
        self.inner.writer.lock().unwrap_or_else(|p| p.into_inner()).take();
        let mut guard = self.inner.child.lock().await;
        if let Some((_, mut child)) = guard.take() {
            let _ = child.start_kill();
            let _ = tokio::time::timeout(Duration::from_secs(3), child.wait()).await;
        }
    }

    /// Graceful stop: ask the engine to exit, then kill.
    pub async fn stop(&self) {
        let _ = self.call_with_timeout("engine.shutdown", json!({}), Duration::from_secs(2)).await;
        self.kill_internal().await;
        self.set_status(|s| {
            s.state = "stopped".into();
            s.pid = None;
        });
    }

    pub async fn restart(&self) -> Result<EngineStatus, EngineError> {
        self.stop().await;
        let restarts = {
            let mut s = self.inner.status.write().unwrap_or_else(|p| p.into_inner());
            s.restarts += 1;
            s.restarts
        };
        if restarts > self.inner.config.max_restarts {
            let msg = format!("engine restarted {restarts} times; giving up until manual restart");
            self.set_status(|s| {
                s.state = "failed".into();
                s.last_error = Some(msg.clone());
            });
            return Err(EngineError::NotRunning(msg));
        }
        self.start().await
    }

    /// Reset the restart budget (manual restart from Settings › Diagnostics).
    pub fn reset_restart_budget(&self) {
        self.set_status(|s| s.restarts = 0);
    }

    pub fn is_ready(&self) -> bool {
        self.status().state == "ready"
    }

    /// Send a request and await its response.
    pub async fn call(&self, method: &str, params: Value) -> Result<Value, EngineError> {
        self.call_with_timeout(method, params, self.inner.config.request_timeout).await
    }

    /// Send a request and await its response, all within `timeout`: waiting
    /// for another request to finish being written, writing this one, and
    /// the answer.
    pub async fn call_with_timeout(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, EngineError> {
        let deadline = tokio::time::Instant::now() + timeout;
        let request_id = new_id();
        let line = serde_json::to_string(&json!({
            "protocolVersion": crate::ENGINE_PROTOCOL_VERSION,
            "requestId": request_id,
            "method": method,
            "params": params,
        }))?;
        if line.len() > self.inner.config.max_message_bytes {
            return Err(EngineError::Protocol("request exceeds max message size".into()));
        }
        let Some(writer) = self.writer() else {
            return Err(EngineError::NotRunning(self.status().last_error.unwrap_or_else(|| "not started".into())));
        };
        let (tx, rx) = oneshot::channel();
        self.inner.pending.lock().unwrap_or_else(|p| p.into_inner()).insert(request_id.clone(), tx);
        let forget = || {
            self.inner.pending.lock().unwrap_or_else(|p| p.into_inner()).remove(&request_id);
        };

        // Written by a task of its own, so a caller that gives up part-way
        // (a timeout around this call, say) can't leave half a line in the
        // pipe: the line is either written whole or its engine is abandoned.
        let mut bytes = line.into_bytes();
        bytes.push(b'\n');
        let generation = writer.generation;
        let this = self.clone();
        let write = tokio::spawn(async move {
            // Nothing is written while waiting for the lock, so running out of
            // time here leaves the stream as it was.
            let Ok(mut stdin) = tokio::time::timeout_at(deadline, writer.stdin.lock()).await else {
                return Err(EngineError::Timeout(timeout));
            };
            let written = tokio::time::timeout_at(deadline, async {
                match stdin.write_all(&bytes).await {
                    Ok(()) => stdin.flush().await,
                    Err(e) => Err(e),
                }
            })
            .await;
            drop(stdin);
            match written {
                Ok(Ok(())) => Ok(()),
                Ok(Err(e)) => {
                    this.abandon(writer.generation, &format!("writing to the engine failed: {e}")).await;
                    Err(EngineError::Io(e))
                }
                Err(_) => {
                    this.abandon(writer.generation, &format!("a request could not be written within {timeout:?}"))
                        .await;
                    Err(EngineError::Timeout(timeout))
                }
            }
        });
        match write.await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                forget();
                return Err(e);
            }
            Err(e) => {
                // The write task itself failed, perhaps part-way through a
                // line: that engine's input can't be trusted either.
                forget();
                self.abandon(generation, &format!("writing a request failed: {e}")).await;
                return Err(EngineError::NotRunning(format!("writing the request failed: {e}")));
            }
        }
        match tokio::time::timeout_at(deadline, rx).await {
            Ok(Ok(res)) => res,
            Ok(Err(_)) => Err(EngineError::NotRunning("engine dropped the request".into())),
            Err(_) => {
                forget();
                Err(EngineError::Timeout(timeout))
            }
        }
    }
}

fn truncate(s: &str, n: usize) -> &str {
    if s.len() <= n {
        s
    } else {
        let mut end = n;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        &s[..end]
    }
}

/// Locate the engine launcher for development and packaged builds.
///
/// Order: `MIMIC_ENGINE_COMMAND` env (program only, for tests) → bundled
/// `mimic-engine(.exe)` beside the executable → `uv run --project engine
/// mimic-engine` from the repository root.
pub fn resolve_engine_command(
    repo_root: Option<&std::path::Path>,
    exe_dir: Option<&std::path::Path>,
) -> Option<EngineCommand> {
    if let Some(cmd) = std::env::var_os("MIMIC_ENGINE_COMMAND") {
        let program = PathBuf::from(cmd);
        return Some(EngineCommand { program, args: vec!["serve".into()], cwd: None, env: vec![] });
    }
    if let Some(dir) = exe_dir {
        for name in ["mimic-engine.exe", "mimic-engine"] {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(EngineCommand {
                    program: candidate,
                    args: vec!["serve".into()],
                    cwd: Some(dir.to_path_buf()),
                    env: vec![],
                });
            }
        }
        // Tauri sidecar naming: engine/mimic-engine-<target-triple>.exe next to the binary.
        if let Ok(entries) = std::fs::read_dir(dir) {
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with("mimic-engine") && e.path().is_file() {
                    return Some(EngineCommand {
                        program: e.path(),
                        args: vec!["serve".into()],
                        cwd: Some(dir.to_path_buf()),
                        env: vec![],
                    });
                }
            }
        }
    }
    if let Some(root) = repo_root {
        let engine_dir = root.join("engine");
        if engine_dir.join("pyproject.toml").is_file() {
            return Some(EngineCommand {
                program: PathBuf::from("uv"),
                args: vec![
                    "run".into(),
                    "--project".into(),
                    engine_dir.to_string_lossy().to_string(),
                    "mimic-engine".into(),
                    "serve".into(),
                ],
                cwd: Some(engine_dir),
                env: vec![],
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The dependency-free fake engine the protocol tests use, if there is a
    /// Python to run it with.
    fn fake_engine() -> Option<EngineClient> {
        let python = ["python3", "python"]
            .into_iter()
            .find(|p| std::process::Command::new(p).arg("--version").output().is_ok_and(|o| o.status.success()))?;
        let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/support/fake_engine.py");
        let mut cfg = EngineConfig::new(EngineCommand {
            program: PathBuf::from(python),
            args: vec![script.to_string_lossy().to_string()],
            cwd: None,
            env: vec![],
        });
        cfg.request_timeout = Duration::from_secs(10);
        cfg.startup_timeout = Duration::from_secs(20);
        Some(EngineClient::new(cfg))
    }

    /// A late failure from an engine that has since been replaced — a write
    /// to it failing after the app restarted it, say — stops nothing: only
    /// the engine it happened to is killed, and that one is gone already.
    #[tokio::test]
    async fn giving_up_on_an_old_engine_leaves_the_new_one_running() {
        let Some(engine) = fake_engine() else { return };
        engine.start().await.unwrap();
        let old = engine.inner.generation.load(Ordering::SeqCst);
        engine.restart().await.unwrap();
        let new = engine.inner.generation.load(Ordering::SeqCst);
        assert_ne!(old, new);

        engine.kill_generation(old).await;
        engine.abandon(old, "a write to the old engine failed").await;
        assert!(engine.is_ready());
        assert_eq!(engine.call("echo", json!({"still": "here"})).await.unwrap()["still"], "here");

        engine.kill_generation(new).await;
        let started = std::time::Instant::now();
        while engine.is_ready() && started.elapsed() < Duration::from_secs(10) {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(!engine.is_ready(), "its own generation is killed");
        engine.stop().await;
    }
}
