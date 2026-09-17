//! Sessions, prediction, review and the apply/restore path. Every mutation of
//! a catalog is a job (`apply_session`, `restore_batch`) so it is recorded,
//! cancellable between batches and never re-run blindly after a crash.

use serde_json::{json, Value};
use tauri::State;

use mimic_core::sessions::{self, GroupEdit, SessionSource};

use crate::error::{CommandError, CommandResult};
use crate::SharedState;

fn require_engine(state: &SharedState) -> CommandResult<()> {
    if !state.engine.is_ready() {
        return Err(CommandError::new("engine_not_running", "The analysis engine is not running."));
    }
    Ok(())
}

fn session_job_running(state: &SharedState, session_id: &str, kinds: &[&str]) -> bool {
    state
        .db
        .list_jobs(50, true)
        .map(|jobs| jobs.into_iter().any(|j| kinds.contains(&j.kind.as_str()) && j.payload["sessionId"] == session_id))
        .unwrap_or(false)
}

#[tauri::command]
pub async fn list_sessions(state: State<'_, SharedState>) -> CommandResult<Vec<mimic_core::db::Session>> {
    Ok(state.db.list_sessions(200)?)
}

#[tauri::command]
pub async fn create_session(
    state: State<'_, SharedState>,
    name: String,
    source: SessionSource,
    style_id: Option<String>,
) -> CommandResult<mimic_core::db::Session> {
    require_engine(&state)?;
    let conn = state.bridge.connection();
    let session = sessions::create_session(&state.db, &name, &source, style_id.as_deref(), conn.as_ref())?;
    state.jobs.enqueue(sessions::JOB_INGEST_SESSION, json!({"sessionId": session.id}))?;
    Ok(session)
}

#[tauri::command]
pub async fn get_session_detail(
    state: State<'_, SharedState>,
    session_id: String,
) -> CommandResult<sessions::SessionDetail> {
    Ok(sessions::session_detail(&state.db, &session_id)?)
}

#[tauri::command]
pub async fn list_session_photos(
    state: State<'_, SharedState>,
    session_id: String,
) -> CommandResult<Vec<sessions::SessionPhoto>> {
    Ok(sessions::session_photos(&state.db, &session_id)?)
}

#[tauri::command]
pub async fn set_session_style(
    state: State<'_, SharedState>,
    session_id: String,
    style_id: Option<String>,
) -> CommandResult<mimic_core::db::Session> {
    if let Some(id) = &style_id {
        state.db.get_style_profile(id)?.ok_or_else(|| CommandError::new("not_found", "style not found"))?;
    }
    state.db.set_session_style(&session_id, style_id.as_deref())?;
    state.db.get_session(&session_id)?.ok_or_else(|| CommandError::new("not_found", "session not found"))
}

#[tauri::command]
pub async fn delete_session(state: State<'_, SharedState>, session_id: String) -> CommandResult<()> {
    if session_job_running(&state, &session_id, sessions::SessionExecutor::ALL_KINDS) {
        return Err(CommandError::new("busy", "a job is still running for this session"));
    }
    Ok(state.db.delete_session(&session_id)?)
}

#[tauri::command]
pub async fn group_session(state: State<'_, SharedState>, session_id: String) -> CommandResult<mimic_core::db::Job> {
    require_engine(&state)?;
    if session_job_running(&state, &session_id, &[sessions::JOB_GROUP_SESSION, sessions::JOB_INGEST_SESSION]) {
        return Err(CommandError::new("busy", "this session is still being ingested or grouped"));
    }
    Ok(state.jobs.enqueue(sessions::JOB_GROUP_SESSION, json!({"sessionId": session_id}))?)
}

#[tauri::command]
pub async fn predict_session(
    state: State<'_, SharedState>,
    session_id: String,
    style_id: Option<String>,
    consistency: Option<bool>,
) -> CommandResult<mimic_core::db::Job> {
    require_engine(&state)?;
    let session =
        state.db.get_session(&session_id)?.ok_or_else(|| CommandError::new("not_found", "session not found"))?;
    let style = style_id
        .or(session.active_style_profile_id)
        .ok_or_else(|| CommandError::new("invalid", "Choose a Style for this session first."))?;
    if state.db.active_model_version(&style)?.is_none() {
        return Err(CommandError::new("invalid", "This Style has no active trained version. Train it first."));
    }
    if session_job_running(&state, &session_id, &[sessions::JOB_PREDICT_SESSION, sessions::JOB_APPLY_SESSION]) {
        return Err(CommandError::new("busy", "a prediction or apply is already running for this session"));
    }
    Ok(state.jobs.enqueue(
        sessions::JOB_PREDICT_SESSION,
        json!({"sessionId": session_id, "styleId": style, "consistency": consistency.unwrap_or(true)}),
    )?)
}

#[tauri::command]
pub async fn set_prediction_review(
    state: State<'_, SharedState>,
    prediction_id: String,
    status: String,
) -> CommandResult<mimic_core::db::Prediction> {
    Ok(sessions::set_review_status(&state.db, &prediction_id, &status)?)
}

#[tauri::command]
pub async fn get_apply_preflight(
    state: State<'_, SharedState>,
    session_id: String,
    prediction_ids: Option<Vec<String>>,
) -> CommandResult<sessions::ApplyPreflight> {
    let only = prediction_ids.map(|v| v.into_iter().collect());
    let conn = state.bridge.connection();
    Ok(sessions::apply_preflight(&state.db, &session_id, only.as_ref(), conn.as_ref())?)
}

#[tauri::command]
pub async fn apply_session(
    state: State<'_, SharedState>,
    session_id: String,
    prediction_ids: Option<Vec<String>>,
    min_confidence: Option<f64>,
) -> CommandResult<mimic_core::db::Job> {
    let only: Option<std::collections::HashSet<String>> = prediction_ids.clone().map(|v| v.into_iter().collect());
    let conn = state.bridge.connection();
    let pf = sessions::apply_preflight(&state.db, &session_id, only.as_ref(), conn.as_ref())?;
    if !pf.ok {
        return Err(CommandError::new("apply_refused", pf.blockers.join(" ")));
    }
    if session_job_running(&state, &session_id, &[sessions::JOB_APPLY_SESSION, sessions::JOB_PREDICT_SESSION]) {
        return Err(CommandError::new("busy", "an apply or prediction is already running for this session"));
    }
    let mut payload = json!({"sessionId": session_id, "minConfidence": min_confidence.unwrap_or(0.0)});
    if let Some(ids) = prediction_ids {
        payload["predictionIds"] = json!(ids);
    }
    Ok(state.jobs.enqueue(sessions::JOB_APPLY_SESSION, payload)?)
}

#[tauri::command]
pub async fn list_applied_edits(
    state: State<'_, SharedState>,
    apply_batch_id: String,
) -> CommandResult<Vec<mimic_core::db::AppliedEdit>> {
    Ok(state.db.applied_edits(&apply_batch_id)?)
}

#[tauri::command]
pub async fn restore_apply_batch(
    state: State<'_, SharedState>,
    apply_batch_id: String,
) -> CommandResult<mimic_core::db::Job> {
    let batch = state
        .db
        .get_apply_batch(&apply_batch_id)?
        .ok_or_else(|| CommandError::new("not_found", "apply batch not found"))?;
    if !batch.rollback_available {
        return Err(CommandError::new("invalid", "nothing in this batch can be restored (no recorded before-state)"));
    }
    let conn = state
        .bridge
        .connection()
        .ok_or_else(|| CommandError::new("lightroom_not_connected", "Lightroom is not connected."))?;
    if let Some(fp) = &batch.lightroom_catalog_fingerprint {
        if fp != &conn.catalog_fingerprint {
            return Err(CommandError::new(
                "invalid",
                "Lightroom has a different catalog open than the one this batch was applied to.",
            ));
        }
    }
    Ok(state.jobs.enqueue(sessions::JOB_RESTORE_BATCH, json!({"applyBatchId": apply_batch_id}))?)
}

#[tauri::command]
pub async fn get_prediction(state: State<'_, SharedState>, prediction_id: String) -> CommandResult<Value> {
    let p = state
        .db
        .get_prediction(&prediction_id)?
        .ok_or_else(|| CommandError::new("not_found", "prediction not found"))?;
    let asset = state.db.get_asset(&p.asset_id)?;
    let before = state.db.latest_observed_snapshot(&p.asset_id)?;
    let mv = state.db.get_model_version(&p.model_version_id)?;
    Ok(json!({
        "prediction": p,
        "asset": asset,
        "beforeSnapshot": before,
        "modelVersion": mv,
    }))
}

#[tauri::command]
pub async fn sync_corrections(state: State<'_, SharedState>, session_id: String) -> CommandResult<mimic_core::db::Job> {
    state.db.get_session(&session_id)?.ok_or_else(|| CommandError::new("not_found", "session not found"))?;
    state
        .bridge
        .connection()
        .ok_or_else(|| CommandError::new("lightroom_not_connected", "Lightroom is not connected."))?;
    if session_job_running(
        &state,
        &session_id,
        &[mimic_core::corrections::JOB_SYNC_CORRECTIONS, sessions::JOB_APPLY_SESSION, sessions::JOB_RESTORE_BATCH],
    ) {
        return Err(CommandError::new("busy", "an apply, restore or sync is already running for this session"));
    }
    Ok(state.jobs.enqueue(mimic_core::corrections::JOB_SYNC_CORRECTIONS, json!({"sessionId": session_id}))?)
}

#[tauri::command]
pub async fn edit_session_groups(
    state: State<'_, SharedState>,
    session_id: String,
    edit: GroupEdit,
) -> CommandResult<Vec<mimic_core::db::SceneCluster>> {
    if session_job_running(&state, &session_id, &[sessions::JOB_GROUP_SESSION, sessions::JOB_PREDICT_SESSION]) {
        return Err(CommandError::new("busy", "wait for grouping or prediction to finish before editing groups"));
    }
    Ok(sessions::edit_groups(&state.db, &session_id, &edit)?)
}
