//! Shared helpers for real-engine end-to-end tests.
#![allow(dead_code)]

use std::path::PathBuf;
use std::time::Duration;

use mimic_core::db::{Db, NewAsset, NewEditSnapshot, VisualFeatures};
use mimic_core::edit_dna;
use mimic_core::engine::{EngineClient, EngineCommand, EngineConfig};
use mimic_core::paths::AppPaths;
use serde_json::{json, Map, Value};

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

/// Synthetic training pairs written through the real normalizer and repositories.
/// Exposure follows luminance, temperature follows colour cast, per-shoot style offsets.
pub fn synthetic_pairs(db: &Db, library_id: &str, n: usize, shoots: usize) -> Vec<String> {
    let mut ids = Vec::new();
    for i in 0..n {
        let shoot = i % shoots;
        let lum = 0.15 + 0.7 * ((i * 37 % 101) as f64 / 100.0);
        let cast = ((i * 13 % 21) as f64 - 10.0) / 200.0;
        let day = format!("2025-03-{:02}T10:{:02}:00", 1 + shoot * 3, i % 60);
        let (asset, _) = db
            .upsert_asset(&NewAsset {
                library_id: Some(library_id.to_string()),
                source_path: format!("/synthetic/s{shoot}/IMG_{i:04}.CR3"),
                file_name: format!("IMG_{i:04}.CR3"),
                extension: "cr3".into(),
                size_bytes: 1000,
                fast_hash: format!("fh1:{i}"),
                camera_make: Some(if shoot % 2 == 0 { "Canon" } else { "SONY" }.into()),
                camera_model: Some(if shoot % 2 == 0 { "EOS R6" } else { "ILCE-7M4" }.into()),
                lens: Some("50mm".into()),
                iso: Some(400),
                aperture: Some(2.8),
                shutter_speed: Some(0.005),
                focal_length: Some(50.0),
                captured_at: Some(day),
                width: Some(6000),
                height: Some(4000),
                ..Default::default()
            })
            .unwrap();
        let style_offset = (shoot % 3) as f64 * 0.15 - 0.15;
        let exposure = (1.2 * (0.5 - lum) + style_offset * 1.0).clamp(-5.0, 5.0);
        let contrast = (10.0 + 40.0 * (0.5 - (0.5 - lum).abs())).round();
        let temp = (5500.0 - 8000.0 * cast).round();
        let mut raw = Map::new();
        raw.insert("ProcessVersion".into(), json!("15.4"));
        raw.insert("Exposure2012".into(), json!(format!("{exposure:+.2}")));
        raw.insert("Contrast2012".into(), json!(format!("{contrast:+}")));
        raw.insert("Temperature".into(), json!(temp));
        raw.insert("Vibrance".into(), json!("+15"));
        raw.insert("Shadows2012".into(), json!(format!("{:+}", (60.0 * (0.5 - lum) + 20.0).round())));
        let normalized = edit_dna::normalize(&raw);
        db.insert_edit_snapshot(&NewEditSnapshot {
            asset_id: asset.id.clone(),
            source: "xmp".into(),
            process_version: Some("15.4".into()),
            normalized_settings: serde_json::to_value(&normalized).unwrap(),
            raw_settings: Value::Object(raw),
            unknown_settings: json!({}),
            mapping_version: normalized.mapping_version.clone(),
            capability_schema_version: None,
            provenance: json!({}),
        })
        .unwrap();
        let hist: Vec<f64> = (0..32).map(|b| (-(b as f64 / 31.0 - lum).powi(2) / 0.02).exp()).collect();
        let sum: f64 = hist.iter().sum();
        let hist: Vec<f64> = hist.iter().map(|h| h / sum).collect();
        let pct: Map<String, Value> = [1, 5, 25, 50, 75, 95, 99]
            .iter()
            .map(|p| (format!("p{p}"), json!((lum + (*p as f64 - 50.0) / 100.0 * 0.6).clamp(0.0, 1.0))))
            .collect();
        db.upsert_visual_features(&VisualFeatures {
            asset_id: asset.id.clone(),
            feature_version: mimic_core::ingest::FEATURE_VERSION.into(),
            histogram: json!({"bins": 32, "luminance": hist}),
            luminance: json!({"mean": lum, "std": 0.2, "percentiles": pct, "dynamicRange": 0.8, "center": lum, "border": lum, "centerBorderDelta": 0.0}),
            color: json!({"channelMeans": [lum + cast, lum, lum - cast], "channelPercentiles": {}, "grayWorldGain": [1.0 + cast, 1.0, 1.0 - cast], "castRedGreen": cast, "castBlueYellow": -cast, "saturationMean": 0.3, "saturationP95": 0.6, "skyLikeFraction": 0.1}),
            sharpness: Some(0.002),
            noise_estimate: Some(0.01),
            clipping: json!({"highlights": 0.0, "shadows": 0.0, "channelHighlights": [0, 0, 0]}),
            scene_labels: json!({}),
            embedding_artifact_id: None,
            preview_path: None,
            computed_at: mimic_core::ids::now_rfc3339(),
        })
        .unwrap();
        ids.push(asset.id);
    }
    ids
}
