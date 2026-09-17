//! axum router for the bridge. Kept separate from the state machine so the
//! state can be unit-tested without sockets.

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{header, Request, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use tower_http::limit::RequestBodyLimitLayer;

use super::{BridgeConfig, BridgeHandle, BridgeState, CommandResultBody, EventsBody, HandshakeRequest};
use crate::capability::CapabilityProbe;

pub struct BridgeServer {
    pub handle: BridgeHandle,
    pub addr: SocketAddr,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    task: Option<tokio::task::JoinHandle<()>>,
}

impl BridgeServer {
    /// Bind 127.0.0.1 and start serving. Requires a Tokio runtime.
    pub async fn start(config: BridgeConfig) -> std::io::Result<BridgeServer> {
        let state = Arc::new(BridgeState::new(config.clone()));
        let listener = tokio::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, config.port))).await?;
        let addr = listener.local_addr()?;
        debug_assert!(addr.ip().is_loopback());
        let base_url = format!("http://{}:{}", addr.ip(), addr.port());
        let router = build_router(state.clone(), config.max_body_bytes);
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = rx.await;
                })
                .await;
        });
        Ok(BridgeServer { handle: BridgeHandle { state, base_url }, addr, shutdown: Some(tx), task: Some(task) })
    }

    pub async fn stop(mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(task) = self.task.take() {
            let _ = tokio::time::timeout(Duration::from_secs(2), task).await;
        }
    }
}

impl Drop for BridgeServer {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}

type AppState = Arc<BridgeState>;

fn build_router(state: AppState, max_body: usize) -> Router {
    Router::new()
        .route("/bridge/v1/handshake", post(handshake))
        .route("/bridge/v1/commands/next", get(next_command))
        .route("/bridge/v1/commands/{id}/result", post(command_result))
        .route("/bridge/v1/events", post(events))
        .route("/bridge/v1/capabilities", post(capabilities))
        .route("/bridge/v1/health", get(health))
        .layer(middleware::from_fn_with_state(state.clone(), auth))
        .layer(RequestBodyLimitLayer::new(max_body))
        .with_state(state)
}

async fn auth(State(state): State<AppState>, req: Request<Body>, next: Next) -> Response {
    let ok = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .is_some_and(|t| super::constant_time_eq(t.trim().as_bytes(), state.token.as_bytes()));
    if !ok {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"ok": false, "error": {"code": "unauthorized", "message": "missing or invalid bridge token"}})),
        )
            .into_response();
    }
    // Reject anything that looks like a browser cross-origin call.
    if req.headers().contains_key(header::ORIGIN) {
        return (StatusCode::FORBIDDEN, Json(json!({"ok": false, "error": {"code": "forbidden_origin", "message": "browser origins are not allowed"}}))).into_response();
    }
    next.run(req).await
}

async fn health(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(json!({
        "ok": true,
        "appVersion": crate::APP_VERSION,
        "protocolVersion": crate::BRIDGE_PROTOCOL_VERSION,
        "connected": state.is_connected(),
    }))
}

async fn handshake(State(state): State<AppState>, Json(req): Json<HandshakeRequest>) -> Response {
    let resp = state.handshake(req);
    let code = if resp.accepted { StatusCode::OK } else { StatusCode::UPGRADE_REQUIRED };
    (code, Json(resp)).into_response()
}

#[derive(Deserialize)]
struct NextQuery {
    #[serde(default, rename = "waitMs")]
    wait_ms: Option<u64>,
}

async fn next_command(State(state): State<AppState>, Query(q): Query<NextQuery>) -> Response {
    if !state.is_connected() {
        // Plugin must handshake first (or re-handshake after being declared dead).
        return (StatusCode::CONFLICT, Json(json!({"ok": false, "error": {"code": "handshake_required", "message": "send /bridge/v1/handshake first"}}))).into_response();
    }
    state.touch();
    let wait = Duration::from_millis(q.wait_ms.unwrap_or(0).min(25_000));
    match state.wait_for_command(wait).await {
        Some(cmd) => {
            state.touch();
            (StatusCode::OK, Json(json!({"ok": true, "command": cmd}))).into_response()
        }
        None => {
            state.touch();
            (StatusCode::NO_CONTENT, ()).into_response()
        }
    }
}

async fn command_result(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(mut body): Json<CommandResultBody>,
) -> Response {
    state.touch();
    body.command_id = id;
    if state.complete_command(body) {
        (StatusCode::OK, Json(json!({"ok": true}))).into_response()
    } else {
        (StatusCode::NOT_FOUND, Json(json!({"ok": false, "error": {"code": "unknown_command", "message": "no such in-flight command (timed out or superseded)"}}))).into_response()
    }
}

async fn events(State(state): State<AppState>, Json(body): Json<EventsBody>) -> Json<serde_json::Value> {
    state.touch();
    let n = body.events.len();
    state.publish_plugin_events(body.events);
    Json(json!({"ok": true, "accepted": n}))
}

async fn capabilities(State(state): State<AppState>, Json(probe): Json<CapabilityProbe>) -> Response {
    state.touch();
    match state.update_capabilities(probe) {
        Some(matrix) => (StatusCode::OK, Json(json!({"ok": true, "capabilitySchemaVersion": matrix.schema_version, "supportedControls": matrix.supported_count()}))).into_response(),
        None => (StatusCode::CONFLICT, Json(json!({"ok": false, "error": {"code": "handshake_required", "message": "send /bridge/v1/handshake first"}}))).into_response(),
    }
}
