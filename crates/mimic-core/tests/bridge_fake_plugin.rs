//! Fake Lightroom plugin driving the real bridge over loopback HTTP.

use std::time::Duration;

use mimic_core::bridge::{BridgeConfig, BridgeError, BridgeEvent, BridgeServer, CommandType, HandshakeRequest};
use serde_json::{json, Value};

struct FakePlugin {
    client: reqwest::Client,
    base: String,
    token: String,
}

impl FakePlugin {
    fn new(server: &BridgeServer) -> Self {
        Self {
            client: reqwest::Client::new(),
            base: server.handle.base_url().to_string(),
            token: server.handle.token().to_string(),
        }
    }

    fn auth(&self, rb: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        rb.header("Authorization", format!("Bearer {}", self.token))
    }

    async fn handshake(&self) -> (u16, Value) {
        let req: HandshakeRequest =
            serde_json::from_str(include_str!("../../../fixtures/bridge/handshake.request.json")).unwrap();
        let resp =
            self.auth(self.client.post(format!("{}/bridge/v1/handshake", self.base)).json(&req)).send().await.unwrap();
        (resp.status().as_u16(), resp.json().await.unwrap())
    }

    async fn poll(&self, wait_ms: u64) -> Option<Value> {
        let resp = self
            .auth(self.client.get(format!("{}/bridge/v1/commands/next?waitMs={wait_ms}", self.base)))
            .send()
            .await
            .unwrap();
        if resp.status().as_u16() == 204 {
            return None;
        }
        assert_eq!(resp.status().as_u16(), 200, "poll status");
        let body: Value = resp.json().await.unwrap();
        Some(body["command"].clone())
    }

    async fn result(&self, id: &str, body: Value) -> u16 {
        self.auth(self.client.post(format!("{}/bridge/v1/commands/{id}/result", self.base)).json(&body))
            .send()
            .await
            .unwrap()
            .status()
            .as_u16()
    }

    /// Serve exactly one command using a fixture-shaped reply.
    async fn serve_one(&self, wait_ms: u64) -> Option<Value> {
        let cmd = self.poll(wait_ms).await?;
        let id = cmd["commandId"].as_str().unwrap().to_string();
        let reply = match cmd["commandType"].as_str().unwrap() {
            "ping" => json!({"ok": true, "result": {"pong": true}}),
            "get_develop_settings" => {
                let fixture: Value =
                    serde_json::from_str(include_str!("../../../fixtures/bridge/get_develop_settings.result.json"))
                        .unwrap();
                json!({"ok": true, "result": {"items": [fixture["result"].clone()]}})
            }
            "apply_settings_as_plugin_preset" => {
                let fixture: Value = serde_json::from_str(include_str!(
                    "../../../fixtures/bridge/apply_settings_as_plugin_preset.result.partial_failure.json"
                ))
                .unwrap();
                json!({"ok": true, "result": fixture["result"].clone()})
            }
            "create_before_snapshot" => {
                json!({"ok": false, "error": {"code": "catalog_write_denied", "message": "withWriteAccessDo timed out"}})
            }
            other => json!({"ok": false, "error": {"code": "unknown_command", "message": other}}),
        };
        let mut body = reply;
        body["commandId"] = json!(id);
        assert_eq!(self.result(&id, body).await, 200);
        Some(cmd)
    }
}

fn test_config() -> BridgeConfig {
    BridgeConfig {
        liveness_timeout: Duration::from_millis(1500),
        poll_interval: Duration::from_millis(100),
        ..Default::default()
    }
}

#[tokio::test]
async fn rejects_missing_or_wrong_token_and_binds_loopback() {
    let server = BridgeServer::start(test_config()).await.unwrap();
    assert!(server.addr.ip().is_loopback());
    let client = reqwest::Client::new();
    let base = server.handle.base_url().to_string();
    let r = client.get(format!("{base}/bridge/v1/health")).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 401);
    let r = client.get(format!("{base}/bridge/v1/health")).header("Authorization", "Bearer nope").send().await.unwrap();
    assert_eq!(r.status().as_u16(), 401);
    let r = client
        .get(format!("{base}/bridge/v1/health"))
        .header("Authorization", format!("Bearer {}", server.handle.token()))
        .header("Origin", "https://evil.example")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 403);
    let r = client
        .get(format!("{base}/bridge/v1/health"))
        .header("Authorization", format!("Bearer {}", server.handle.token()))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 200);
    let body: Value = r.json().await.unwrap();
    assert_eq!(body["connected"], false);
    server.stop().await;
}

#[tokio::test]
async fn handshake_poll_result_roundtrip() {
    let server = BridgeServer::start(test_config()).await.unwrap();
    let plugin = FakePlugin::new(&server);
    let handle = server.handle.clone();
    let mut events = handle.subscribe();

    // Polling before handshake is refused.
    let r = plugin.auth(plugin.client.get(format!("{}/bridge/v1/commands/next", plugin.base))).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 409);
    assert!(matches!(
        handle.send_command(CommandType::Ping, json!({}), Duration::from_secs(1)).await,
        Err(BridgeError::NotConnected)
    ));

    let (code, body) = plugin.handshake().await;
    assert_eq!(code, 200);
    assert_eq!(body["accepted"], true);
    assert_eq!(body["protocolVersion"], 1);
    let status = handle.status();
    assert!(status.connected);
    let conn = status.connection.unwrap();
    assert_eq!(conn.lightroom_version, "14.3");
    assert!(conn.capabilities.can_apply);
    assert!(matches!(events.try_recv(), Ok(BridgeEvent::Connected { .. })));

    // Desktop sends ping; plugin serves it; desktop gets the result.
    let plugin_task = tokio::spawn(async move { plugin.serve_one(3000).await });
    let result = handle.send_command(CommandType::Ping, json!({}), Duration::from_secs(5)).await.unwrap();
    assert_eq!(result["pong"], true);
    let served = plugin_task.await.unwrap().unwrap();
    assert_eq!(served["commandType"], "ping");
    assert_eq!(handle.status().total_commands_completed, 1);

    server.stop().await;
}

#[tokio::test]
async fn develop_settings_apply_and_plugin_error_paths() {
    let server = BridgeServer::start(test_config()).await.unwrap();
    let plugin = FakePlugin::new(&server);
    let handle = server.handle.clone();
    plugin.handshake().await;

    let serve = |n: usize| {
        let p = FakePlugin::new(&server);
        tokio::spawn(async move {
            for _ in 0..n {
                p.serve_one(3000).await;
            }
        })
    };
    let t = serve(3);
    let settings = handle
        .send_command(CommandType::GetDevelopSettings, json!({"photoIds": [4021]}), Duration::from_secs(5))
        .await
        .unwrap();
    let raw = settings["items"][0]["settings"].as_object().unwrap().clone();
    let normalized = mimic_core::edit_dna::normalize(&raw);
    assert_eq!(normalized.get("tone.exposure").unwrap().raw, json!(0.35));
    assert_eq!(normalized.lightroom.process_version.as_deref(), Some("15.4"));
    assert!(normalized.unknown.contains_key("LookName"));

    let apply = handle
        .send_command(CommandType::ApplySettingsAsPluginPreset, json!({"items": []}), Duration::from_secs(5))
        .await
        .unwrap();
    let parsed: mimic_core::bridge::ApplyBatchResult = serde_json::from_value(apply).unwrap();
    assert_eq!(parsed.items.len(), 2);
    assert_eq!(parsed.items[1].status, "failed");
    assert_eq!(parsed.items[1].error.as_ref().unwrap().code, "photo_not_found");
    let readback = parsed.items[0].read_back.clone().unwrap();
    let intended: serde_json::Map<String, Value> = [("Exposure2012", json!(0.35)), ("Contrast2012", json!(12))]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
    assert!(mimic_core::edit_dna::verify_readback(&intended, &readback).ok);

    let err =
        handle.send_command(CommandType::CreateBeforeSnapshot, json!({}), Duration::from_secs(5)).await.unwrap_err();
    match err {
        BridgeError::Plugin { code, .. } => assert_eq!(code, "catalog_write_denied"),
        other => panic!("{other}"),
    }
    t.await.unwrap();
    server.stop().await;
}

#[tokio::test]
async fn timeout_disconnect_and_reconnect() {
    let server = BridgeServer::start(test_config()).await.unwrap();
    let plugin = FakePlugin::new(&server);
    let handle = server.handle.clone();
    plugin.handshake().await;

    // Nobody polls: command times out, and the late result is rejected.
    let err = handle.send_command(CommandType::Ping, json!({}), Duration::from_millis(300)).await.unwrap_err();
    assert!(matches!(err, BridgeError::Timeout(_)));
    assert_eq!(handle.status().queued_commands, 0);
    assert_eq!(plugin.result("stale-id", json!({"commandId": "stale-id", "ok": true})).await, 404);

    // Plugin stops polling → sweep declares disconnect → in-flight command fails fast.
    let mut events = handle.subscribe();
    tokio::time::sleep(Duration::from_millis(1700)).await;
    assert!(!handle.status().connected);
    handle.sweep();
    assert!(matches!(events.try_recv(), Ok(BridgeEvent::Disconnected { .. })));
    assert!(matches!(
        handle.send_command(CommandType::Ping, json!({}), Duration::from_secs(1)).await,
        Err(BridgeError::NotConnected)
    ));

    // Reconnect works and produces a new session id.
    let (_, first) = plugin.handshake().await;
    let (_, second) = plugin.handshake().await;
    assert_ne!(first["sessionId"], second["sessionId"]);
    assert!(handle.status().connected);
    assert_eq!(handle.status().total_handshakes, 3);

    // Plugin events are forwarded.
    let ev: Value =
        serde_json::from_str(include_str!("../../../fixtures/bridge/event.selection_changed.json")).unwrap();
    let r =
        plugin.auth(plugin.client.post(format!("{}/bridge/v1/events", plugin.base)).json(&ev)).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 200);
    let mut got = false;
    while let Ok(e) = events.try_recv() {
        if let BridgeEvent::PluginEvent { event } = e {
            assert_eq!(event.kind, "selection_changed");
            got = true;
        }
    }
    assert!(got);

    // Discovery file has no world-readable secrets on unix and contains the token.
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("bridge").join("bridge.json");
    handle.write_discovery_file(&path).unwrap();
    let disc: mimic_core::bridge::DiscoveryFile = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(disc.token, handle.token());
    assert!(disc.base_url.starts_with("http://127.0.0.1:"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    }
    server.stop().await;
}

#[tokio::test]
async fn oversized_body_is_rejected() {
    let mut cfg = test_config();
    cfg.max_body_bytes = 16 * 1024;
    let server = BridgeServer::start(cfg).await.unwrap();
    let plugin = FakePlugin::new(&server);
    plugin.handshake().await;
    let big = json!({"events": [{"type": "x", "payload": "y".repeat(50_000)}]});
    let r =
        plugin.auth(plugin.client.post(format!("{}/bridge/v1/events", plugin.base)).json(&big)).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 413);
    server.stop().await;
}
