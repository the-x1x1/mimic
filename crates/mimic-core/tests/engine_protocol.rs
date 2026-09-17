//! Engine sidecar transport tests against a dependency-free fake engine.

use std::path::PathBuf;
use std::time::Duration;

use mimic_core::engine::{EngineClient, EngineCommand, EngineConfig, EngineEvent};
use serde_json::json;

fn python() -> Option<PathBuf> {
    for candidate in ["python3", "python"] {
        if let Ok(out) = std::process::Command::new(candidate).arg("--version").output() {
            if out.status.success() {
                return Some(PathBuf::from(candidate));
            }
        }
    }
    None
}

fn fake_engine(max_message: usize) -> Option<EngineClient> {
    let py = python()?;
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/support/fake_engine.py");
    let mut cfg = EngineConfig::new(EngineCommand {
        program: py,
        args: vec![script.to_string_lossy().to_string()],
        cwd: None,
        env: vec![],
    });
    cfg.max_message_bytes = max_message;
    cfg.request_timeout = Duration::from_secs(10);
    cfg.startup_timeout = Duration::from_secs(20);
    Some(EngineClient::new(cfg))
}

#[tokio::test]
async fn hello_echo_error_and_events() {
    let Some(engine) = fake_engine(1 << 20) else {
        eprintln!("python not available; skipping");
        return;
    };
    let status = engine.start().await.expect("engine starts");
    assert_eq!(status.state, "ready");
    assert_eq!(status.engine_version.as_deref(), Some("fake-0.0.0"));
    assert!(engine.is_ready());

    let echoed = engine.call("echo", json!({"a": 1, "b": [true]})).await.unwrap();
    assert_eq!(echoed, json!({"a": 1, "b": [true]}));

    let err = engine.call("fail", json!({})).await.unwrap_err();
    match err {
        mimic_core::engine::EngineError::Remote { code, details, .. } => {
            assert_eq!(code, "boom");
            assert_eq!(details.unwrap()["x"], 1);
        }
        other => panic!("unexpected {other}"),
    }

    let mut events = engine.subscribe();
    let done = engine.call("progress", json!({"n": 3, "jobId": "job-1"})).await.unwrap();
    assert_eq!(done["done"], 3);
    let mut seen = 0;
    while let Ok(ev) = events.try_recv() {
        if let EngineEvent::JobProgress { job_id, current, total, .. } = ev {
            assert_eq!(job_id, "job-1");
            assert_eq!(total, 3);
            assert!((1..=3).contains(&current));
            seen += 1;
        }
    }
    assert_eq!(seen, 3);

    // Non-JSON lines are ignored, the real response still arrives.
    assert_eq!(engine.call("garbage", json!({})).await.unwrap()["ok"], true);

    // Concurrent requests are correlated by requestId.
    let (a, b) = tokio::join!(engine.call("echo", json!({"n": 1})), engine.call("echo", json!({"n": 2})));
    assert_eq!(a.unwrap()["n"], 1);
    assert_eq!(b.unwrap()["n"], 2);

    engine.stop().await;
    assert_eq!(engine.status().state, "stopped");
    assert!(matches!(engine.call("echo", json!({})).await, Err(mimic_core::engine::EngineError::NotRunning(_))));
}

#[tokio::test]
async fn timeout_crash_and_restart() {
    let Some(engine) = fake_engine(1 << 20) else { return };
    engine.start().await.unwrap();
    let err = engine.call_with_timeout("slow", json!({"seconds": 3}), Duration::from_millis(200)).await.unwrap_err();
    assert!(matches!(err, mimic_core::engine::EngineError::Timeout(_)));

    let mut events = engine.subscribe();
    let crash = engine.call_with_timeout("crash", json!({}), Duration::from_secs(5)).await;
    assert!(crash.is_err());
    tokio::time::sleep(Duration::from_millis(300)).await;
    let st = engine.status();
    assert_ne!(st.state, "ready");
    let mut exited = false;
    while let Ok(ev) = events.try_recv() {
        if let EngineEvent::Exited { code } = ev {
            exited = true;
            assert_eq!(code, Some(3));
        }
    }
    assert!(exited, "exit event expected");

    let st = engine.restart().await.unwrap();
    assert_eq!(st.state, "ready");
    assert_eq!(st.restarts, 1);
    assert_eq!(engine.call("echo", json!({"x": 1})).await.unwrap()["x"], 1);
    engine.stop().await;
}

#[tokio::test]
async fn oversized_message_kills_engine() {
    let Some(engine) = fake_engine(4096) else { return };
    engine.start().await.unwrap();
    let res = engine.call_with_timeout("huge", json!({"bytes": 10_000}), Duration::from_secs(5)).await;
    assert!(res.is_err());
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_ne!(engine.status().state, "ready");
    let too_big = engine.call("echo", json!({"blob": "y".repeat(5000)})).await;
    assert!(matches!(
        too_big,
        Err(mimic_core::engine::EngineError::Protocol(_)) | Err(mimic_core::engine::EngineError::NotRunning(_))
    ));
}

#[tokio::test]
async fn missing_program_fails_cleanly() {
    let cfg = EngineConfig::new(EngineCommand {
        program: PathBuf::from("/definitely/not/here/mimic-engine"),
        args: vec![],
        cwd: None,
        env: vec![],
    });
    let engine = EngineClient::new(cfg);
    let err = engine.start().await.unwrap_err();
    assert!(matches!(err, mimic_core::engine::EngineError::NotRunning(_)));
    assert_eq!(engine.status().state, "failed");
    assert!(engine.status().last_error.unwrap().contains("failed to start"));
}
