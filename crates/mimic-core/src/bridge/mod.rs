//! Lightroom desktop bridge: a loopback-only HTTP service the Lightroom plugin
//! polls (docs/LIGHTROOM_INTEGRATION.md §10).
//!
//! Security properties (docs/SECURITY_MODEL.md):
//! * binds `127.0.0.1` on an OS-assigned port — never `0.0.0.0`;
//! * every request carries `Authorization: Bearer <256-bit token>` generated
//!   per app launch and written only to the per-user discovery file;
//! * request bodies are capped (`max_body_bytes`);
//! * the plugin initiates every connection; the desktop never connects out.

pub mod protocol;
mod server;

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{broadcast, oneshot, Notify};

use crate::capability::{CapabilityMatrix, CapabilityProbe};
use crate::ids::{new_id, now_rfc3339};
pub use protocol::*;
pub use server::BridgeServer;

#[derive(Debug, Clone)]
pub struct BridgeConfig {
    /// 0 = OS-assigned.
    pub port: u16,
    pub max_body_bytes: usize,
    pub poll_interval: Duration,
    /// Time without a poll after which the plugin is considered disconnected.
    pub liveness_timeout: Duration,
    pub max_batch_size: usize,
    pub max_queue_len: usize,
}

impl Default for BridgeConfig {
    fn default() -> Self {
        Self {
            port: 0,
            max_body_bytes: 8 * 1024 * 1024,
            poll_interval: Duration::from_millis(1000),
            liveness_timeout: Duration::from_secs(6),
            max_batch_size: 25,
            max_queue_len: 256,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BridgeError {
    #[error("Lightroom is not connected")]
    NotConnected,
    #[error("command timed out after {0:?}")]
    Timeout(Duration),
    #[error("command queue is full")]
    QueueFull,
    #[error("plugin error {code}: {message}")]
    Plugin { code: String, message: String, details: Option<Value> },
    #[error("bridge stopped")]
    Stopped,
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionInfo {
    pub session_id: String,
    pub plugin_version: String,
    pub lightroom_version: String,
    pub sdk_version: Option<String>,
    pub catalog_fingerprint: String,
    pub catalog_name: Option<String>,
    pub probe: CapabilityProbe,
    pub capabilities: CapabilityMatrix,
    pub connected_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeStatus {
    pub listening: bool,
    pub base_url: String,
    pub connected: bool,
    pub last_seen_ms_ago: Option<u64>,
    pub connection: Option<ConnectionInfo>,
    pub queued_commands: usize,
    pub in_flight_commands: usize,
    pub total_handshakes: u64,
    pub total_commands_completed: u64,
}

/// Events the shell forwards to the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BridgeEvent {
    Connected { connection: Box<ConnectionInfo> },
    Disconnected { reason: String },
    PluginEvent { event: PluginEvent },
    CommandCompleted { command_id: String, command_type: CommandType, ok: bool },
}

struct QueuedCommand {
    envelope: CommandEnvelope,
    enqueued_at: Instant,
}

struct InFlight {
    command_type: CommandType,
    reply: oneshot::Sender<CommandResultBody>,
}

pub(crate) struct BridgeState {
    pub(crate) config: BridgeConfig,
    pub(crate) token: String,
    connection: RwLock<Option<ConnectionInfo>>,
    last_seen: Mutex<Option<Instant>>,
    queue: Mutex<VecDeque<QueuedCommand>>,
    in_flight: Mutex<HashMap<String, InFlight>>,
    notify: Notify,
    events: broadcast::Sender<BridgeEvent>,
    handshakes: AtomicU64,
    completed: AtomicU64,
}

impl BridgeState {
    fn new(config: BridgeConfig) -> Self {
        let (events, _) = broadcast::channel(256);
        Self {
            config,
            token: generate_token(),
            connection: RwLock::new(None),
            last_seen: Mutex::new(None),
            queue: Mutex::new(VecDeque::new()),
            in_flight: Mutex::new(HashMap::new()),
            notify: Notify::new(),
            events,
            handshakes: AtomicU64::new(0),
            completed: AtomicU64::new(0),
        }
    }

    pub(crate) fn touch(&self) {
        *self.last_seen.lock().unwrap_or_else(|p| p.into_inner()) = Some(Instant::now());
    }

    pub(crate) fn is_connected(&self) -> bool {
        let has_conn = self.connection.read().unwrap_or_else(|p| p.into_inner()).is_some();
        let seen = *self.last_seen.lock().unwrap_or_else(|p| p.into_inner());
        has_conn && seen.is_some_and(|t| t.elapsed() <= self.config.liveness_timeout)
    }

    pub(crate) fn handshake(&self, req: HandshakeRequest) -> HandshakeResponse {
        self.handshakes.fetch_add(1, Ordering::Relaxed);
        let mut reason = None;
        if req.protocol_version != crate::BRIDGE_PROTOCOL_VERSION {
            reason = Some(format!(
                "protocol version {} not supported (desktop speaks {})",
                req.protocol_version,
                crate::BRIDGE_PROTOCOL_VERSION
            ));
        } else if crate::version::compare_core_versions(&req.plugin_version, crate::version::MIN_PLUGIN_VERSION)
            == std::cmp::Ordering::Less
        {
            reason = Some(format!(
                "plugin {} is older than the minimum {} — reinstall the plugin from Settings › Lightroom",
                req.plugin_version,
                crate::version::MIN_PLUGIN_VERSION
            ));
        }
        let accepted = reason.is_none();
        let session_id = new_id();
        if accepted {
            let mut probe = req.capabilities.clone();
            if probe.lightroom_version.is_empty() {
                probe.lightroom_version = req.lightroom_version.clone();
            }
            if probe.plugin_version.is_empty() {
                probe.plugin_version = req.plugin_version.clone();
            }
            let info = ConnectionInfo {
                session_id: session_id.clone(),
                plugin_version: req.plugin_version,
                lightroom_version: req.lightroom_version,
                sdk_version: req.sdk_version,
                catalog_fingerprint: req.catalog_fingerprint,
                catalog_name: req.catalog_name,
                capabilities: CapabilityMatrix::from_probe(&probe),
                probe,
                connected_at: now_rfc3339(),
            };
            // A new handshake invalidates anything queued for the old session.
            self.fail_all_pending("plugin reconnected");
            *self.connection.write().unwrap_or_else(|p| p.into_inner()) = Some(info.clone());
            self.touch();
            let _ = self.events.send(BridgeEvent::Connected { connection: Box::new(info) });
        }
        HandshakeResponse {
            ok: accepted,
            protocol_version: crate::BRIDGE_PROTOCOL_VERSION,
            app_version: crate::APP_VERSION.to_string(),
            session_id,
            poll_interval_ms: self.config.poll_interval.as_millis() as u64,
            max_batch_size: self.config.max_batch_size,
            accepted,
            reason,
        }
    }

    pub(crate) fn update_capabilities(&self, probe: CapabilityProbe) -> Option<CapabilityMatrix> {
        let mut guard = self.connection.write().unwrap_or_else(|p| p.into_inner());
        let conn = guard.as_mut()?;
        conn.capabilities = CapabilityMatrix::from_probe(&probe);
        conn.probe = probe;
        Some(conn.capabilities.clone())
    }

    pub(crate) fn pop_command(&self) -> Option<CommandEnvelope> {
        let mut q = self.queue.lock().unwrap_or_else(|p| p.into_inner());
        q.pop_front().map(|c| c.envelope)
    }

    pub(crate) async fn wait_for_command(&self, max_wait: Duration) -> Option<CommandEnvelope> {
        if let Some(c) = self.pop_command() {
            return Some(c);
        }
        let deadline = tokio::time::Instant::now() + max_wait;
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return None;
            }
            if tokio::time::timeout(remaining, self.notify.notified()).await.is_err() {
                return self.pop_command();
            }
            if let Some(c) = self.pop_command() {
                return Some(c);
            }
        }
    }

    pub(crate) fn complete_command(&self, body: CommandResultBody) -> bool {
        let entry = self.in_flight.lock().unwrap_or_else(|p| p.into_inner()).remove(&body.command_id);
        match entry {
            Some(inflight) => {
                self.completed.fetch_add(1, Ordering::Relaxed);
                let _ = self.events.send(BridgeEvent::CommandCompleted {
                    command_id: body.command_id.clone(),
                    command_type: inflight.command_type,
                    ok: body.ok,
                });
                inflight.reply.send(body).is_ok()
            }
            None => false,
        }
    }

    pub(crate) fn publish_plugin_events(&self, events: Vec<PluginEvent>) {
        for event in events {
            let _ = self.events.send(BridgeEvent::PluginEvent { event });
        }
    }

    fn fail_all_pending(&self, reason: &str) {
        // Queued commands also have an in-flight entry, so draining in_flight
        // answers every waiter exactly once.
        self.queue.lock().unwrap_or_else(|p| p.into_inner()).clear();
        let all: Vec<(String, InFlight)> = self.in_flight.lock().unwrap_or_else(|p| p.into_inner()).drain().collect();
        for (id, inflight) in all {
            let _ = inflight.reply.send(CommandResultBody {
                command_id: id,
                ok: false,
                result: None,
                error: Some(BridgeErrorBody { code: "disconnected".into(), message: reason.into(), details: None }),
            });
        }
    }

    pub(crate) fn mark_disconnected(&self, reason: &str) {
        let had = self.connection.write().unwrap_or_else(|p| p.into_inner()).take().is_some();
        if had {
            self.fail_all_pending(reason);
            let _ = self.events.send(BridgeEvent::Disconnected { reason: reason.into() });
        }
    }

    fn status(&self, base_url: &str, listening: bool) -> BridgeStatus {
        let last_seen = *self.last_seen.lock().unwrap_or_else(|p| p.into_inner());
        BridgeStatus {
            listening,
            base_url: base_url.to_string(),
            connected: self.is_connected(),
            last_seen_ms_ago: last_seen.map(|t| t.elapsed().as_millis() as u64),
            connection: self.connection.read().unwrap_or_else(|p| p.into_inner()).clone(),
            queued_commands: self.queue.lock().unwrap_or_else(|p| p.into_inner()).len(),
            in_flight_commands: self.in_flight.lock().unwrap_or_else(|p| p.into_inner()).len(),
            total_handshakes: self.handshakes.load(Ordering::Relaxed),
            total_commands_completed: self.completed.load(Ordering::Relaxed),
        }
    }
}

/// Cheap, cloneable handle used by jobs and Tauri commands.
#[derive(Clone)]
pub struct BridgeHandle {
    state: Arc<BridgeState>,
    base_url: String,
}

impl BridgeHandle {
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn token(&self) -> &str {
        &self.state.token
    }

    pub fn status(&self) -> BridgeStatus {
        self.state.status(&self.base_url, true)
    }

    pub fn connection(&self) -> Option<ConnectionInfo> {
        if self.state.is_connected() {
            self.state.connection.read().unwrap_or_else(|p| p.into_inner()).clone()
        } else {
            None
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<BridgeEvent> {
        self.state.events.subscribe()
    }

    /// Enqueue a command and wait for the plugin's result.
    pub async fn send_command(
        &self,
        command_type: CommandType,
        payload: Value,
        timeout: Duration,
    ) -> Result<Value, BridgeError> {
        if !self.state.is_connected() {
            return Err(BridgeError::NotConnected);
        }
        let command_id = new_id();
        let (tx, rx) = oneshot::channel();
        {
            let mut q = self.state.queue.lock().unwrap_or_else(|p| p.into_inner());
            if q.len() >= self.state.config.max_queue_len {
                return Err(BridgeError::QueueFull);
            }
            self.state
                .in_flight
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .insert(command_id.clone(), InFlight { command_type, reply: tx });
            q.push_back(QueuedCommand {
                envelope: CommandEnvelope { command_id: command_id.clone(), command_type, payload },
                enqueued_at: Instant::now(),
            });
        }
        self.state.notify.notify_waiters();
        let outcome = tokio::time::timeout(timeout, rx).await;
        match outcome {
            Ok(Ok(body)) if body.ok => Ok(body.result.unwrap_or(Value::Null)),
            Ok(Ok(body)) => {
                let err = body.error.unwrap_or(BridgeErrorBody {
                    code: "unknown".into(),
                    message: "plugin reported failure without details".into(),
                    details: None,
                });
                Err(BridgeError::Plugin { code: err.code, message: err.message, details: err.details })
            }
            Ok(Err(_)) => Err(BridgeError::Stopped),
            Err(_) => {
                // Remove from queue/in-flight so a late result is ignored.
                self.state
                    .queue
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .retain(|c| c.envelope.command_id != command_id);
                self.state.in_flight.lock().unwrap_or_else(|p| p.into_inner()).remove(&command_id);
                Err(BridgeError::Timeout(timeout))
            }
        }
    }

    /// Periodic liveness sweep; call every second or so from the shell.
    pub fn sweep(&self) {
        if !self.state.is_connected() {
            self.state.mark_disconnected("plugin stopped polling");
        }
        // Drop commands that sat in the queue far longer than any timeout.
        let stale = self.state.config.liveness_timeout * 10;
        self.state.queue.lock().unwrap_or_else(|p| p.into_inner()).retain(|c| c.enqueued_at.elapsed() < stale);
    }

    /// Replace the capability probe for the live connection (after `get_capabilities`).
    pub fn update_capabilities_from_probe(&self, probe: CapabilityProbe) -> Option<CapabilityMatrix> {
        self.state.update_capabilities(probe)
    }

    pub fn discovery_file(&self) -> DiscoveryFile {
        DiscoveryFile {
            protocol_version: crate::BRIDGE_PROTOCOL_VERSION,
            app_version: crate::APP_VERSION.to_string(),
            base_url: self.base_url.clone(),
            token: self.state.token.clone(),
            pid: std::process::id(),
            written_at: now_rfc3339(),
        }
    }

    /// Write `bridge.json` with owner-only permissions where the OS supports it.
    pub fn write_discovery_file(&self, path: &std::path::Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(&self.discovery_file()).expect("serializable"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
        }
        std::fs::rename(&tmp, path)
    }
}

fn generate_token() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

pub(crate) fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_256_bit_hex_and_unique() {
        let t = generate_token();
        assert_eq!(t.len(), 64);
        assert!(t.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(t, generate_token());
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
    }

    #[test]
    fn handshake_rejects_wrong_protocol_and_old_plugin() {
        let state = BridgeState::new(BridgeConfig::default());
        let req: HandshakeRequest =
            serde_json::from_str(include_str!("../../../../fixtures/bridge/handshake.request.json")).unwrap();
        let mut bad = req.clone();
        bad.protocol_version = 99;
        let resp = state.handshake(bad);
        assert!(!resp.accepted && resp.reason.unwrap().contains("protocol"));
        let mut old = req.clone();
        old.plugin_version = "0.0.1".into();
        assert!(!state.handshake(old).accepted);
        assert!(!state.is_connected());
        let ok = state.handshake(req);
        assert!(ok.accepted);
        assert!(state.is_connected());
        let conn = state.connection.read().unwrap().clone().unwrap();
        assert_eq!(conn.catalog_fingerprint, "sha256:2d0c8f1c4e0b");
        assert!(conn.capabilities.can_apply);
    }
}
