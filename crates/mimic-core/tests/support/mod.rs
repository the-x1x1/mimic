//! Shared helpers for real-engine end-to-end tests.
#![allow(dead_code)]

use std::path::PathBuf;
use std::time::Duration;

use mimic_core::engine::{EngineClient, EngineCommand, EngineConfig};
use mimic_core::paths::AppPaths;
use serde_json::json;

pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

pub fn uv_available() -> bool {
    std::process::Command::new("uv").arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
}

/// Start the real Python engine from the repository and configure it for `paths`.
pub async fn real_engine(paths: &AppPaths) -> EngineClient {
    let engine_dir = repo_root().join("engine");
    let mut cfg = EngineConfig::new(EngineCommand {
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
    cfg.startup_timeout = Duration::from_secs(120);
    let engine = EngineClient::new(cfg);
    let status = engine.start().await.expect("real engine starts");
    assert_eq!(status.state, "ready");
    engine
        .call(
            "engine.configure",
            json!({
                "dbPath": paths.database_file(),
                "previewsDir": paths.previews_cache(),
                "embeddingsDir": paths.embeddings_cache(),
                "encodersDir": paths.encoders_dir(),
                "stylesDir": paths.styles_dir(),
                "manifestsDir": repo_root().join("models/manifests"),
            }),
        )
        .await
        .unwrap();
    engine
}
