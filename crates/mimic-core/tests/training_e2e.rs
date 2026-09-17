//! Train a Style Brain end to end: synthetic pairs written through the real
//! Rust normalizer and repositories, the real Python trainer via the job
//! runner, immutable versions, activation policy and prediction.

mod support;

use mimic_core::db::Db;
use mimic_core::jobs::{CompositeExecutor, JobRunner};
use mimic_core::training;
use serde_json::json;
use std::sync::Arc;
use support::synthetic_pairs;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn train_activate_rollback_and_predict() {
    if !support::uv_available() {
        eprintln!("uv not installed; skipping");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let paths = mimic_core::paths::AppPaths::new(tmp.path());
    paths.ensure().unwrap();
    let (db, _) = Db::open(&paths.database_file(), &paths.backups_dir()).unwrap();
    let lib = db.create_library("Synthetic", "folder_sidecars", Some("/synthetic"), None).unwrap();
    let asset_ids = synthetic_pairs(&db, &lib.id, 150, 8);
    let style = db.create_style_profile("Synthetic Natural", None).unwrap();
    db.link_style_library(&style.id, &lib.id).unwrap();

    let engine = support::real_engine(&paths).await;
    let bridge = mimic_core::bridge::BridgeServer::start(Default::default()).await.unwrap();
    let executor = Arc::new(CompositeExecutor::new(vec![
        mimic_core::ingest::executor(engine.clone(), bridge.handle.clone()),
        training::executor(engine.clone()),
    ]));
    let runner = JobRunner::new(db.clone(), executor);

    // v1.0.0
    let job = runner.enqueue(training::JOB_TRAIN_STYLE, json!({"styleId": style.id})).unwrap();
    assert!(!job.resumable, "training is never blindly resumed");
    runner.run_one(job.clone()).await;
    let job = db.get_job(&job.id).unwrap().unwrap();
    assert_eq!(job.status, "completed", "{:?}", job.error);
    let result = job.result.unwrap();
    assert_eq!(result["semanticVersion"], "1.0.0");
    assert_eq!(result["activated"], true);
    assert!(result["beatsBaselines"]["beatsGlobalMedian"].as_bool().unwrap(), "{}", result["beatsBaselines"]);
    assert!(job.progress_total > 0 || job.phase.is_some(), "engine progress was forwarded");
    let v1 = db.active_model_version(&style.id).unwrap().unwrap();
    assert_eq!(v1.status, "ready");
    assert!(v1.training_set_id.is_some());
    assert_eq!(v1.training_config["seed"], 42);
    assert!(v1.training_config["trainingDataFingerprint"].as_str().unwrap().len() == 64);
    let holdout = training::primary_error(&v1.metrics).unwrap();
    assert!(holdout < 0.1, "holdout nMAE {holdout}");
    let artifacts = v1.artifact_manifest["artifacts"].as_array().unwrap();
    assert_eq!(artifacts.len(), 3);
    for a in artifacts {
        let p = std::path::Path::new(a["path"].as_str().unwrap());
        assert!(p.starts_with(paths.styles_dir()) && p.is_file());
    }
    assert_eq!(db.get_style_profile(&style.id).unwrap().unwrap().status, "ready");

    // v1.1.0 with identical data + seed: same error → activates (<=), v1 stays ready (immutable).
    let job2 = runner.enqueue(training::JOB_TRAIN_STYLE, json!({"styleId": style.id, "config": {"seed": 42}})).unwrap();
    runner.run_one(job2.clone()).await;
    let r2 = db.get_job(&job2.id).unwrap().unwrap().result.unwrap();
    assert_eq!(r2["semanticVersion"], "1.1.0");
    assert_eq!(r2["activated"], true, "{}", r2["activationReason"]);
    let versions = db.list_model_versions(&style.id).unwrap();
    assert_eq!(versions.len(), 2);
    assert_eq!(versions.iter().filter(|v| v.is_active).count(), 1);
    let v1_again = db.get_model_version(&v1.id).unwrap().unwrap();
    assert_eq!(v1_again.metrics, v1.metrics, "old version untouched");
    assert!(std::path::Path::new(artifacts[0]["path"].as_str().unwrap()).is_file(), "old artifacts untouched");

    // Rollback: activate v1 by hand.
    let rolled = training::activate(&db, &v1.id).unwrap();
    assert!(rolled.is_active);
    assert!(!db.get_model_version(&versions[0].id).unwrap().unwrap().is_active || versions[0].id == v1.id);

    // Prediction through the engine for two known assets.
    let model_path = artifacts.iter().find(|a| a["kind"] == "model").unwrap()["path"].as_str().unwrap();
    let pred = engine
        .call("model.predict", json!({"modelPath": model_path, "assetIds": [asset_ids[0], asset_ids[1], "missing-id"]}))
        .await
        .unwrap();
    let results = pred["results"].as_array().unwrap();
    assert_eq!(results.len(), 3);
    assert!(
        results[0]["confidence"].as_f64().unwrap() > 0.5,
        "{} {} {}",
        results[0]["confidence"],
        results[0]["confidenceComponents"],
        results[0]["reasons"]
    );
    assert!(results[0]["global"]["tone"]["exposure"]["raw"].is_number());
    assert_eq!(results[2]["error"]["code"], "not_found");

    // Insufficient data path is a clean failure with a failed (immutable) version row.
    let small_lib = db.create_library("Tiny", "folder_sidecars", Some("/tiny"), None).unwrap();
    let small_style = db.create_style_profile("Tiny", None).unwrap();
    db.link_style_library(&small_style.id, &small_lib.id).unwrap();
    let job3 = runner.enqueue(training::JOB_TRAIN_STYLE, json!({"styleId": small_style.id})).unwrap();
    runner.run_one(job3.clone()).await;
    let j3 = db.get_job(&job3.id).unwrap().unwrap();
    assert_eq!(j3.status, "failed");
    assert!(j3.error.unwrap()["message"].as_str().unwrap().contains("at least 30"));
    assert!(db.list_model_versions(&small_style.id).unwrap().is_empty(), "no version row when the precheck fails");

    engine.stop().await;
    bridge.stop().await;
}
