//! End-to-end: real Python engine (via `uv run`) + real job runner + real
//! database. Scans a temp library built from repository fixtures and checks
//! that assets, sidecars, normalized snapshots and visual features land in
//! SQLite and that the data-quality report reflects them.
//!
//! Skips (with a message) when `uv` is not installed.

use std::path::PathBuf;
use std::time::Duration;

use mimic_core::bridge::{BridgeConfig, BridgeServer};
use mimic_core::db::Db;
use mimic_core::engine::{EngineClient, EngineCommand, EngineConfig};
use mimic_core::ingest;
use mimic_core::jobs::JobRunner;
use serde_json::json;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

fn uv_available() -> bool {
    std::process::Command::new("uv").arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
}

fn build_library(dir: &std::path::Path) {
    let fx = repo_root().join("fixtures");
    let a = dir.join("2024-05-11 Wedding");
    let b = dir.join("2025-06-21 Portraits");
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    let img = fx.join("images/demo_landscape_sky.jpg");
    std::fs::copy(&img, a.join("IMG_1024.jpg")).unwrap();
    std::fs::copy(fx.join("xmp/simple_pv2012.xmp"), a.join("IMG_1024.xmp")).unwrap();
    std::fs::copy(fx.join("images/demo_backlit.jpg"), a.join("IMG_1025.jpg")).unwrap();
    std::fs::copy(fx.join("images/demo_lowlight_indoor.jpg"), b.join("DSC00001.jpg")).unwrap();
    std::fs::copy(fx.join("xmp/modern_masks_unknown.xmp"), b.join("DSC00001.xmp")).unwrap();
    std::fs::write(b.join("DSC00001.acr"), b"opaque").unwrap();
    std::fs::copy(fx.join("images/demo_highkey_product.tif"), b.join("DSC00002.tif")).unwrap();
    std::fs::copy(fx.join("xmp/legacy_pv2010.xmp"), b.join("DSC00002.xmp")).unwrap();
    std::fs::copy(fx.join("xmp/malformed_truncated.xmp"), b.join("DSC00003.xmp")).unwrap();
    std::fs::copy(&img, b.join("DSC00003.jpg")).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scan_library_job_end_to_end_with_real_engine() {
    if !uv_available() {
        eprintln!("uv not installed; skipping real-engine e2e");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let lib_dir = tmp.path().join("library");
    build_library(&lib_dir);
    let data = mimic_core::paths::AppPaths::new(tmp.path().join("appdata"));
    data.ensure().unwrap();
    let (db, _) = Db::open(&data.database_file(), &data.backups_dir()).unwrap();

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
    assert_eq!(status.protocol_version, Some(mimic_core::ENGINE_PROTOCOL_VERSION));
    engine
        .call(
            "engine.configure",
            json!({
                "dbPath": data.database_file(),
                "previewsDir": data.previews_cache(),
                "embeddingsDir": data.embeddings_cache(),
                "encodersDir": data.encoders_dir(),
                "stylesDir": data.styles_dir(),
                "manifestsDir": repo_root().join("models/manifests"),
            }),
        )
        .await
        .unwrap();

    let bridge = BridgeServer::start(BridgeConfig::default()).await.unwrap();
    let runner = JobRunner::new(db.clone(), ingest::executor(engine.clone(), bridge.handle.clone()));
    let library = db.create_library("E2E", "folder_sidecars", Some(&lib_dir.to_string_lossy()), None).unwrap();
    let job = runner.enqueue(ingest::JOB_SCAN_LIBRARY, json!({"libraryId": library.id})).unwrap();
    runner.run_one(job.clone()).await;

    let job = db.get_job(&job.id).unwrap().unwrap();
    assert_eq!(job.status, "completed", "job error: {:?}", job.error);
    let result = job.result.unwrap();
    assert_eq!(result["assetsFound"], 5);
    assert_eq!(result["xmpParsed"], 3);
    assert_eq!(result["xmpFailed"], 1);
    assert_eq!(result["acrSidecars"], 1);
    assert_eq!(result["analyzed"], 5);

    // Assets, sidecars, snapshots.
    assert_eq!(db.count_assets(Some(&library.id)).unwrap(), 5);
    assert_eq!(db.count_assets_with_edits(&library.id).unwrap(), 3);
    let a =
        db.find_asset_by_path(&lib_dir.join("2025-06-21 Portraits/DSC00001.jpg").to_string_lossy()).unwrap().unwrap();
    let sidecars = db.sidecars_for_asset(&a.id).unwrap();
    assert_eq!(sidecars.len(), 2);
    assert!(sidecars.iter().any(|s| s.kind == "acr" && s.parse_status == "opaque"));
    let snap = db.latest_observed_snapshot(&a.id).unwrap().unwrap();
    assert_eq!(snap.source, "xmp");
    assert_eq!(snap.process_version.as_deref(), Some("15.4"));
    assert_eq!(snap.mapping_version, "edit_mapping_v1");
    let normalized: mimic_core::edit_dna::Normalized =
        serde_json::from_value(snap.normalized_settings.clone()).unwrap();
    assert_eq!(normalized.get("tone.exposure").unwrap().raw, json!(-0.25));
    assert_eq!(normalized.local.status, "observed");
    assert!(normalized.unknown.contains_key("SomeNewSlider2027"));
    assert_eq!(a.camera_make.as_deref(), None, "synthetic JPEG has no EXIF make");
    assert_eq!(a.width, Some(720));

    // Features + previews + embeddings on disk, not in SQLite.
    let f = db.visual_features(&a.id, ingest::FEATURE_VERSION).unwrap().unwrap();
    assert!(f.preview_path.as_ref().is_some_and(|p| std::path::Path::new(p).is_file()));
    assert!(f.embedding_artifact_id.as_ref().is_some_and(|id| data.embeddings_cache().join(id).is_file()));
    assert!(f.luminance["mean"].as_f64().unwrap() < 0.35, "low-light fixture");

    // Data quality report.
    let report = ingest::data_quality_report(&db, &library.id).unwrap();
    assert_eq!(report.assets_found, 5);
    assert_eq!(report.valid_pairs, 3);
    assert_eq!(report.features_computed, 5);
    assert_eq!(report.acr_heavy_edit_count, 1);
    assert_eq!(report.local_edit_count, 1);
    assert_eq!(report.failed_sidecars, 1);
    assert_eq!(report.recommendation.level, "insufficient");
    assert!(report.warnings.iter().any(|w| w.contains("ACR")));

    // Rescan is idempotent: unchanged XMPs are not re-parsed, counts stay stable.
    let job2 = runner.enqueue(ingest::JOB_SCAN_LIBRARY, json!({"libraryId": library.id})).unwrap();
    runner.run_one(job2.clone()).await;
    let job2 = db.get_job(&job2.id).unwrap().unwrap();
    assert_eq!(job2.status, "completed");
    let r2 = job2.result.unwrap();
    assert_eq!(r2["inserted"], 0);
    assert_eq!(r2["updated"], 5);
    assert_eq!(r2["xmpUnchanged"], 3);
    assert_eq!(r2["analyzed"], 0);
    assert_eq!(db.count_assets(Some(&library.id)).unwrap(), 5);
    assert_eq!(db.snapshots_for_asset(&a.id).unwrap().len(), 1, "no duplicate snapshots on rescan");

    // Source media and sidecars untouched.
    assert_eq!(
        std::fs::read(lib_dir.join("2025-06-21 Portraits/DSC00001.xmp")).unwrap(),
        std::fs::read(repo_root().join("fixtures/xmp/modern_masks_unknown.xmp")).unwrap()
    );

    engine.stop().await;
    bridge.stop().await;
}
