//! Session pipeline end to end: folder ingest through the real engine, scene
//! grouping, prediction with the active Style Brain, apply through the real
//! bridge against a scripted fake Lightroom plugin (read-back echo, one
//! mismatch, one missing photo), and restore from the recorded before-state.

mod support;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mimic_core::bridge::{BridgeConfig, BridgeServer, HandshakeRequest};
use mimic_core::corrections;
use mimic_core::db::Db;
use mimic_core::jobs::{CompositeExecutor, JobRunner};
use mimic_core::sessions::{self, SessionSource};
use mimic_core::training;
use serde_json::{json, Map, Value};

/// Scripted plugin: photo ids by path, echo read-back except for the photo in
/// `mismatch` (which reads back a different exposure) and `missing` (which is
/// not in the catalog).
struct FakeLightroom {
    client: reqwest::Client,
    base: String,
    token: String,
    photos: Vec<(i64, String)>,
    mismatch: Option<String>,
    missing: Option<String>,
    /// Overrides for `collect_correction_state`: path -> settings the
    /// "photographer" ended with. Photos not listed echo what was applied.
    current: Mutex<HashMap<String, Map<String, Value>>>,
    applied: Mutex<HashMap<i64, Map<String, Value>>>,
    received: Arc<Mutex<Vec<Value>>>,
}

impl FakeLightroom {
    fn new(server: &BridgeServer, photos: Vec<(i64, String)>) -> Self {
        Self {
            client: reqwest::Client::new(),
            base: server.handle.base_url().to_string(),
            token: server.handle.token().to_string(),
            photos,
            mismatch: None,
            missing: None,
            current: Mutex::new(HashMap::new()),
            applied: Mutex::new(HashMap::new()),
            received: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn auth(&self, rb: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        rb.header("Authorization", format!("Bearer {}", self.token))
    }

    async fn handshake(&self) {
        let req: HandshakeRequest =
            serde_json::from_str(include_str!("../../../fixtures/bridge/handshake.request.json")).unwrap();
        let resp =
            self.auth(self.client.post(format!("{}/bridge/v1/handshake", self.base)).json(&req)).send().await.unwrap();
        assert_eq!(resp.status().as_u16(), 200);
    }

    fn respond(&self, cmd: &Value) -> Value {
        let payload = &cmd["payload"];
        self.received.lock().unwrap().push(cmd.clone());
        match cmd["commandType"].as_str().unwrap() {
            "ping" => json!({"ok": true, "result": {"pong": true}}),
            "get_selected_photos" => {
                let photos: Vec<Value> = self
                    .photos
                    .iter()
                    .filter(|(_, p)| self.missing.as_deref() != Some(p.as_str()))
                    .map(|(id, p)| json!({"photoId": id, "path": p, "metadata": {}}))
                    .collect();
                json!({"ok": true, "result": {"scope": payload["scope"], "photos": photos, "total": photos.len(), "truncated": false}})
            }
            "apply_settings_as_plugin_preset" => {
                let items: Vec<Value> = payload["items"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|it| {
                        let pid = it["photoId"].as_i64().unwrap();
                        let path = self.photos.iter().find(|(id, _)| *id == pid).map(|(_, p)| p.clone());
                        let path = match path {
                            Some(p) if self.missing.as_deref() != Some(p.as_str()) => p,
                            _ => {
                                return json!({"photoId": pid, "predictionId": it["predictionId"], "status": "failed",
                                    "error": {"code": "photo_not_found", "message": "no photo"}})
                            }
                        };
                        let settings = it["settings"].as_object().unwrap().clone();
                        self.applied.lock().unwrap().insert(pid, settings.clone());
                        let mut read_back = settings.clone();
                        let is_restore = payload["restore"].as_bool().unwrap_or(false);
                        if self.mismatch.as_deref() == Some(path.as_str()) && !is_restore {
                            read_back.insert("Exposure2012".into(), json!(-4.5));
                        }
                        // "Before" state is a neutral develop table for the same keys.
                        let before: Map<String, Value> = settings
                            .keys()
                            .map(|k| (k.clone(), json!(if k == "Temperature" { 5000 } else { 0 })))
                            .collect();
                        let mut item = json!({"photoId": pid, "predictionId": it["predictionId"], "status": "applied",
                            "before": before, "readBack": read_back});
                        if payload["createSnapshot"].as_bool().unwrap_or(true) {
                            item["snapshotName"] = it["snapshotName"].clone();
                        }
                        item
                    })
                    .collect();
                json!({"ok": true, "result": {"items": items, "canceled": false}})
            }
            "collect_correction_state" => {
                let items: Vec<Value> = payload["photoIds"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter_map(Value::as_i64)
                    .map(|pid| {
                        let path =
                            self.photos.iter().find(|(id, _)| *id == pid).map(|(_, p)| p.clone()).unwrap_or_default();
                        let applied = self.applied.lock().unwrap().get(&pid).cloned().unwrap_or_default();
                        let mut settings = applied;
                        if let Some(over) = self.current.lock().unwrap().get(&path) {
                            for (k, v) in over {
                                settings.insert(k.clone(), v.clone());
                            }
                        }
                        settings.insert("ProcessVersion".into(), json!("15.4"));
                        json!({"photoId": pid, "path": path, "settings": settings})
                    })
                    .collect();
                json!({"ok": true, "result": {"items": items, "collectedAt": "2026-09-17T09:00:00Z"}})
            }
            other => json!({"ok": false, "error": {"code": "unknown_command", "message": other}}),
        }
    }

    /// Serve commands until `stop` flips.
    fn spawn(self: Arc<Self>, stop: Arc<std::sync::atomic::AtomicBool>) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                let resp = self
                    .auth(self.client.get(format!("{}/bridge/v1/commands/next?waitMs=300", self.base)))
                    .send()
                    .await
                    .unwrap();
                if resp.status().as_u16() != 200 {
                    continue;
                }
                let body: Value = resp.json().await.unwrap();
                let cmd = body["command"].clone();
                let id = cmd["commandId"].as_str().unwrap().to_string();
                let mut reply = self.respond(&cmd);
                reply["commandId"] = json!(id);
                let st = self
                    .auth(self.client.post(format!("{}/bridge/v1/commands/{id}/result", self.base)).json(&reply))
                    .send()
                    .await
                    .unwrap()
                    .status()
                    .as_u16();
                assert_eq!(st, 200);
            }
        })
    }
}

fn build_session_folder(dir: &std::path::Path) -> Vec<String> {
    let fx = support::repo_root().join("fixtures/images");
    std::fs::create_dir_all(dir).unwrap();
    let files = [
        ("A0001.jpg", "demo_landscape_sky.jpg"),
        ("A0002.jpg", "demo_backlit.jpg"),
        ("A0003.jpg", "demo_lowlight_indoor.jpg"),
        ("A0004.tif", "demo_highkey_product.tif"),
        ("A0005.jpg", "demo_landscape_sky.jpg"),
    ];
    files
        .iter()
        .map(|(name, src)| {
            let dest = dir.join(name);
            std::fs::copy(fx.join(src), &dest).unwrap();
            dest.to_string_lossy().to_string()
        })
        .collect()
}

async fn finished(db: &Db, runner: &JobRunner, kind: &str, payload: Value) -> mimic_core::db::Job {
    let job = runner.enqueue(kind, payload).unwrap();
    runner.run_one(job.clone()).await;
    db.get_job(&job.id).unwrap().unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_ingest_group_predict_apply_restore() {
    if !support::uv_available() {
        eprintln!("uv not installed; skipping");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let paths = mimic_core::paths::AppPaths::new(tmp.path().join("appdata"));
    paths.ensure().unwrap();
    let (db, _) = Db::open(&paths.database_file(), &paths.backups_dir()).unwrap();

    // A trained Style.
    let lib = db.create_library("Synthetic", "folder_sidecars", Some("/synthetic"), None).unwrap();
    support::synthetic_pairs(&db, &lib.id, 120, 6);
    let style = db.create_style_profile("Natural", None).unwrap();
    db.link_style_library(&style.id, &lib.id).unwrap();

    let engine = support::real_engine(&paths).await;
    let bridge = BridgeServer::start(BridgeConfig {
        liveness_timeout: Duration::from_secs(5),
        poll_interval: Duration::from_millis(100),
        ..Default::default()
    })
    .await
    .unwrap();
    let executor = Arc::new(CompositeExecutor::new(vec![
        mimic_core::ingest::executor(engine.clone(), bridge.handle.clone()),
        training::executor(engine.clone()),
        sessions::executor(engine.clone(), bridge.handle.clone()),
        corrections::executor(bridge.handle.clone()),
    ]));
    let runner = JobRunner::new(db.clone(), executor);
    let trained = finished(&db, &runner, training::JOB_TRAIN_STYLE, json!({"styleId": style.id})).await;
    assert_eq!(trained.status, "completed", "{:?}", trained.error);

    // Session from a folder, ingested through the real engine.
    let folder = tmp.path().join("shoot");
    let files = build_session_folder(&folder);
    let src = SessionSource::Folder { path: folder.to_string_lossy().to_string() };
    assert!(sessions::create_session(&db, "  ", &src, None, None).is_err(), "blank name refused");
    assert!(
        sessions::create_session(&db, "x", &SessionSource::Lightroom { scope: "selection".into() }, None, None)
            .is_err(),
        "Lightroom source needs a connection"
    );
    let session = sessions::create_session(&db, "Spring shoot", &src, Some(&style.id), None).unwrap();
    assert!(db.list_libraries().unwrap().iter().all(|l| l.purpose == "training"), "session library hidden");
    let ingested = finished(&db, &runner, sessions::JOB_INGEST_SESSION, json!({"sessionId": session.id})).await;
    assert_eq!(ingested.status, "completed", "{:?}", ingested.error);
    assert_eq!(ingested.result.as_ref().unwrap()["photos"], 5);
    assert_eq!(ingested.result.as_ref().unwrap()["withFeatures"], 5);
    let members = db.session_assets(&session.id).unwrap();
    assert_eq!(members.len(), 5);
    assert_eq!(db.get_session(&session.id).unwrap().unwrap().status, "ingested");

    // Predict before grouping is refused only by missing model, not by missing groups.
    let grouped = finished(&db, &runner, sessions::JOB_GROUP_SESSION, json!({"sessionId": session.id})).await;
    assert_eq!(grouped.status, "completed", "{:?}", grouped.error);
    let detail = sessions::session_detail(&db, &session.id).unwrap();
    assert!(detail.grouped);
    assert!(!detail.clusters.is_empty());
    assert_eq!(detail.clusters.iter().map(|c| c.asset_count).sum::<i64>(), 5);
    assert!(db.session_assets(&session.id).unwrap().iter().all(|m| m.cluster_id.is_some()));
    // Grouping again is idempotent for membership and replaces clusters cleanly.
    let regrouped = finished(&db, &runner, sessions::JOB_GROUP_SESSION, json!({"sessionId": session.id})).await;
    assert_eq!(regrouped.status, "completed");
    assert_eq!(sessions::session_detail(&db, &session.id).unwrap().clusters.len(), detail.clusters.len());

    let predicted = finished(&db, &runner, sessions::JOB_PREDICT_SESSION, json!({"sessionId": session.id})).await;
    assert_eq!(predicted.status, "completed", "{:?}", predicted.error);
    let r = predicted.result.clone().unwrap();
    assert_eq!(r["predicted"], 5, "{r}");
    assert_eq!(r["capabilitySchemaVersion"], Value::Null, "no Lightroom at prediction time");
    let preds = db.session_predictions(&session.id, false).unwrap();
    assert_eq!(preds.len(), 5);
    for p in &preds {
        assert_eq!(p.status, "pending");
        assert!(p.cluster_id.is_some());
        assert!(p.predicted_settings["global"]["tone"]["exposure"]["raw"].is_number(), "{}", p.predicted_settings);
        assert!((0.0..=1.0).contains(&p.confidence));
        assert!(p.raw_model_output.get("reasons").is_some());
    }
    let photos = sessions::session_photos(&db, &session.id).unwrap();
    assert_eq!(photos.len(), 5);
    assert!(photos.iter().all(|p| p.prediction.is_some() && p.preview_path.is_some()));
    // Re-predicting supersedes rather than duplicating.
    finished(&db, &runner, sessions::JOB_PREDICT_SESSION, json!({"sessionId": session.id})).await;
    assert_eq!(db.session_predictions(&session.id, false).unwrap().len(), 5);
    assert_eq!(db.session_predictions(&session.id, true).unwrap().len(), 10);
    let preds = db.session_predictions(&session.id, false).unwrap();

    // Review: reject one photo so it is never applied.
    let rejected = sessions::set_review_status(&db, &preds[4].id, "rejected").unwrap();
    assert_eq!(rejected.status, "rejected");

    // Apply without Lightroom is refused with a reason and leaves no batch.
    let refused = finished(&db, &runner, sessions::JOB_APPLY_SESSION, json!({"sessionId": session.id})).await;
    assert_eq!(refused.status, "failed");
    assert!(refused.error.unwrap()["message"].as_str().unwrap().contains("not connected"));
    assert!(db.session_apply_batches(&session.id).unwrap().is_empty());

    // Connect the scripted plugin: files[0..4] are in the catalog as photos 101..;
    // files[1] reads back wrong; files[2] is missing from the catalog.
    let photo_list: Vec<(i64, String)> = files.iter().enumerate().map(|(i, p)| (101 + i as i64, p.clone())).collect();
    let mut fake = FakeLightroom::new(&bridge, photo_list);
    fake.mismatch = Some(files[1].clone());
    fake.missing = Some(files[2].clone());
    let fake = Arc::new(fake);
    fake.handshake().await;
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let serving = fake.clone().spawn(stop.clone());
    tokio::time::sleep(Duration::from_millis(200)).await;
    let conn = bridge.handle.connection().expect("connected");

    let pf = sessions::apply_preflight(&db, &session.id, None, Some(&conn)).unwrap();
    assert!(pf.ok, "{:?}", pf.blockers);
    assert_eq!(pf.candidate_count, 4, "rejected prediction excluded");
    assert!(pf.writable_controls > 0);

    let applied = finished(&db, &runner, sessions::JOB_APPLY_SESSION, json!({"sessionId": session.id})).await;
    assert_eq!(applied.status, "completed", "{:?}", applied.error);
    let r = applied.result.clone().unwrap();
    assert_eq!(r["status"], "completed_with_failures", "{r}");
    assert_eq!(r["applied"], 2, "{r}");
    assert_eq!(r["failed"], 1, "verify_failed counts as failed");
    assert_eq!(r["skippedUnresolved"], 1);
    assert_eq!(r["rollbackAvailable"], true);
    let batches = db.session_apply_batches(&session.id).unwrap();
    assert_eq!(batches.len(), 1);
    let edits = db.applied_edits(&batches[0].id).unwrap();
    assert_eq!(edits.len(), 4);
    let by_asset = |path: &str| {
        let asset = db.find_asset_by_path(path).unwrap().unwrap();
        edits.iter().find(|e| e.asset_id == asset.id).unwrap().clone()
    };
    let ok0 = by_asset(&files[0]);
    assert_eq!(ok0.result, "applied");
    assert!(
        ok0.before_settings.is_some() && ok0.lightroom_snapshot_name.as_deref().unwrap().starts_with("Mimic Before")
    );
    assert!(ok0.applied_settings["Exposure2012"].is_number());
    let bad = by_asset(&files[1]);
    assert_eq!(bad.result, "verify_failed");
    assert_eq!(bad.error.as_ref().unwrap()["code"], "readback_mismatch");
    assert_eq!(bad.error.as_ref().unwrap()["mismatches"][0]["key"], "Exposure2012");
    let miss = by_asset(&files[2]);
    assert_eq!(miss.result, "skipped");
    assert_eq!(miss.error.as_ref().unwrap()["code"], "photo_not_in_catalog");
    // Prediction statuses follow verified outcomes only.
    let statuses: Vec<String> =
        db.session_predictions(&session.id, false).unwrap().iter().map(|p| p.status.clone()).collect();
    assert_eq!(statuses.iter().filter(|s| *s == "applied").count(), 2);
    assert_eq!(statuses.iter().filter(|s| *s == "rejected").count(), 1);
    // Applied read-backs are recorded as `prediction` snapshots for later correction diffs.
    let snaps = db.snapshots_for_asset(&ok0.asset_id).unwrap();
    assert!(snaps.iter().any(|s| s.source == "prediction"));
    // The plugin received the safety flags and only writable keys.
    let received = fake.received.lock().unwrap().clone();
    let apply_cmds: Vec<&Value> =
        received.iter().filter(|c| c["commandType"] == "apply_settings_as_plugin_preset").collect();
    assert_eq!(apply_cmds.len(), 1, "3 photos fit one batch");
    assert_eq!(apply_cmds[0]["payload"]["createSnapshot"], true);
    assert_eq!(apply_cmds[0]["payload"]["readBack"], true);
    let writable = conn.capabilities.writable_keys();
    for it in apply_cmds[0]["payload"]["items"].as_array().unwrap() {
        assert!(it["snapshotName"].as_str().unwrap().starts_with("Mimic Before"));
        for k in it["settings"].as_object().unwrap().keys() {
            assert!(writable.contains(k), "{k} is not writable on this connection");
        }
    }
    assert_eq!(db.get_session(&session.id).unwrap().unwrap().status, "applied");

    // Stale-capability refusal: a prediction made under another capability set.
    let mut stale = mimic_core::db::NewPrediction {
        session_id: session.id.clone(),
        asset_id: preds[3].asset_id.clone(),
        model_version_id: preds[3].model_version_id.clone(),
        predicted_settings: preds[3].predicted_settings.clone(),
        raw_model_output: json!({}),
        confidence: 0.9,
        confidence_components: json!({}),
        nearest_examples: json!([]),
        capability_schema_version: Some("other".into()),
        cluster_id: None,
    };
    let stale_row = db.insert_prediction(&stale).unwrap();
    let pf = sessions::apply_preflight(&db, &session.id, None, Some(&conn)).unwrap();
    assert!(!pf.ok && pf.stale_count == 1, "{:?}", pf);
    db.set_prediction_status(&stale_row.id, "rejected").unwrap();
    stale.capability_schema_version = None;

    // Corrections sync: files[0] was re-edited by the photographer (+0.5 EV and a
    // different Temperature), files[3] left alone. verify_failed/skipped photos are not checked.
    let ok0_applied_exposure = ok0.applied_settings["Exposure2012"].as_f64().unwrap();
    fake.current.lock().unwrap().insert(
        files[0].clone(),
        [("Exposure2012".to_string(), json!(ok0_applied_exposure + 0.5)), ("Temperature".to_string(), json!(7100))]
            .into_iter()
            .collect(),
    );
    let synced = finished(&db, &runner, corrections::JOB_SYNC_CORRECTIONS, json!({"sessionId": session.id})).await;
    assert_eq!(synced.status, "completed", "{:?}", synced.error);
    let r = synced.result.clone().unwrap();
    assert_eq!(
        (r["checked"].as_i64(), r["untouched"].as_i64(), r["corrected"].as_i64()),
        (Some(2), Some(1), Some(1)),
        "{r}"
    );
    assert_eq!(r["noTouchRate"], 0.5);
    let most: Vec<&str> =
        r["mostCorrected"].as_array().unwrap().iter().filter_map(|m| m["canonical"].as_str()).collect();
    assert_eq!(most.len(), 2, "{r}");
    assert!(most.contains(&"tone.exposure") && most.contains(&"whiteBalance.temperature"));
    let corr_rows = db.corrections_for_style(&style.id, 10).unwrap();
    assert_eq!(corr_rows.len(), 1);
    let corr = &corr_rows[0].correction;
    assert_eq!(corr.asset_id, ok0.asset_id);
    assert!(corr.included_in_training_version.is_none());
    let deltas = corr.delta.as_array().unwrap();
    let exp = deltas.iter().find(|d| d["canonical"] == "tone.exposure").unwrap();
    assert!((exp["delta"].as_f64().unwrap() - 0.05).abs() < 1e-6, "{exp}");
    assert!(corr.correction_magnitude > 0.0);
    assert!(db.snapshots_for_asset(&ok0.asset_id).unwrap().iter().any(|s| s.source == "correction"));
    let health = corrections::style_health(&db, &style.id).unwrap();
    assert_eq!(health.active_no_touch_rate, Some(0.5), "{:?}", health.no_touch);
    assert_eq!(health.corrections_pending_training, 1);
    assert_eq!(health.most_corrected.len(), 2);
    assert!(health.most_corrected.iter().any(|c| c.canonical == "tone.exposure" && c.mean_delta > 0.0));
    assert!(health.insights.iter().any(|i| i.contains("No-Touch Rate 50%")), "{:?}", health.insights);
    // Re-sync is idempotent: same counts, still one correction.
    let again = finished(&db, &runner, corrections::JOB_SYNC_CORRECTIONS, json!({"sessionId": session.id})).await;
    assert_eq!(again.status, "completed");
    assert_eq!(db.corrections_for_style(&style.id, 10).unwrap().len(), 1);
    assert_eq!(db.correction_syncs(&session.id).unwrap().len(), 2);

    // Retrain: the correction becomes a training pair and is marked as used.
    let retrained = finished(&db, &runner, training::JOB_TRAIN_STYLE, json!({"styleId": style.id})).await;
    assert_eq!(retrained.status, "completed", "{:?}", retrained.error);
    let rr = retrained.result.clone().unwrap();
    assert_eq!(rr["counts"]["correctionPairs"], 1, "{rr}");
    assert_eq!(rr["correctionsIncluded"], 1);
    let corr_rows = db.corrections_for_style(&style.id, 10).unwrap();
    assert_eq!(corr_rows[0].correction.included_in_training_version.as_deref(), rr["semanticVersion"].as_str());
    assert_eq!(corrections::style_health(&db, &style.id).unwrap().corrections_pending_training, 0);
    // A sync before any apply in a session is a clean failure.
    let fresh = sessions::create_session(&db, "Empty", &src, Some(&style.id), None).unwrap();
    let nothing = finished(&db, &runner, corrections::JOB_SYNC_CORRECTIONS, json!({"sessionId": fresh.id})).await;
    assert_eq!(nothing.status, "failed");

    // Restore the batch: applied + verify_failed items go back to their before values.
    let restored = finished(&db, &runner, sessions::JOB_RESTORE_BATCH, json!({"applyBatchId": batches[0].id})).await;
    assert_eq!(restored.status, "completed", "{:?}", restored.error);
    let r = restored.result.unwrap();
    assert_eq!(r["restored"], 3, "{r}");
    assert_eq!(r["remaining"], 0);
    let edits = db.applied_edits(&batches[0].id).unwrap();
    for e in &edits {
        if e.result == "skipped" {
            assert!(e.restore_result.is_none());
        } else {
            assert_eq!(e.restore_result.as_deref(), Some("restored"), "{:?}", e.restore_error);
            assert_eq!(
                e.result,
                if e.asset_id == bad.asset_id { "verify_failed" } else { "applied" },
                "apply row untouched"
            );
        }
    }
    assert!(!db.get_apply_batch(&batches[0].id).unwrap().unwrap().rollback_available);
    let received = fake.received.lock().unwrap().clone();
    let restore_cmd = received.iter().filter(|c| c["commandType"] == "apply_settings_as_plugin_preset").nth(1).unwrap();
    assert_eq!(restore_cmd["payload"]["createSnapshot"], false);
    assert_eq!(restore_cmd["payload"]["restore"], true);
    let item0 = &restore_cmd["payload"]["items"][0];
    assert_eq!(item0["settings"]["Exposure2012"], 0, "before-value written back");
    assert_eq!(
        db.session_predictions(&session.id, false).unwrap().iter().filter(|p| p.status == "applied").count(),
        0,
        "restored predictions return to the queue"
    );
    let again = finished(&db, &runner, sessions::JOB_RESTORE_BATCH, json!({"applyBatchId": batches[0].id})).await;
    assert_eq!(again.status, "failed", "nothing left to restore");

    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    serving.await.unwrap();
    engine.stop().await;
    bridge.stop().await;
}
