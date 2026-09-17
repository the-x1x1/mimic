use tauri::State;

use crate::error::{CommandError, CommandResult};
use crate::SharedState;

#[tauri::command]
pub async fn list_jobs(
    state: State<'_, SharedState>,
    limit: Option<usize>,
    active_only: Option<bool>,
) -> CommandResult<Vec<mimic_core::db::Job>> {
    Ok(state.db.list_jobs(limit.unwrap_or(50).min(500), active_only.unwrap_or(false))?)
}

#[tauri::command]
pub async fn get_job(state: State<'_, SharedState>, job_id: String) -> CommandResult<mimic_core::db::Job> {
    state.db.get_job(&job_id)?.ok_or_else(|| CommandError::new("not_found", "job not found"))
}

#[tauri::command]
pub async fn cancel_job(state: State<'_, SharedState>, job_id: String) -> CommandResult<mimic_core::db::Job> {
    Ok(state.jobs.cancel(&job_id)?)
}
