//! Getting a model onto the machine.

use tauri::State;

use crate::error::{CommandError, CommandResult};
use crate::SharedState;

/// What is actually true about the local model right now, plus the one next
/// action that follows from it. The step is computed in the core rather than
/// in the UI so that the screen cannot invent a fourth situation.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalModelView {
    #[serde(flatten)]
    pub status: mimic_core::localmodel::LocalModelStatus,
    pub next_step: mimic_core::localmodel::NextStep,
    /// Where "get the writing engine" sends the person. Mimic opens this in
    /// their browser; it never downloads or runs what is on the other end.
    pub download_page: &'static str,
}

#[tauri::command]
pub async fn local_model_status(state: State<'_, SharedState>) -> CommandResult<LocalModelView> {
    let (endpoint, model) = mimic_core::localmodel::configured(&state.db)?;
    let status = tauri::async_runtime::spawn_blocking(move || mimic_core::localmodel::observe(&endpoint, &model))
        .await
        .map_err(|e| CommandError::new("local_model_check_failed", e.to_string()))?;
    Ok(LocalModelView {
        next_step: mimic_core::localmodel::next_step(&status),
        status,
        download_page: mimic_core::localmodel::DOWNLOAD_PAGE,
    })
}

/// Queue the download. Refuses when nothing is listening rather than starting
/// a job that can only fail: the screen's next action in that state is to get
/// the host, not to pull.
#[tauri::command]
pub async fn start_model_pull(state: State<'_, SharedState>) -> CommandResult<mimic_core::db::Job> {
    let (endpoint, model) = mimic_core::localmodel::configured(&state.db)?;
    let status = tauri::async_runtime::spawn_blocking({
        let endpoint = endpoint.clone();
        let model = model.clone();
        move || mimic_core::localmodel::observe(&endpoint, &model)
    })
    .await
    .map_err(|e| CommandError::new("local_model_check_failed", e.to_string()))?;
    if !status.host_reachable {
        return Err(CommandError::new(
            "no_model_host",
            "Nothing is running on this computer to download the model into yet.",
        ));
    }
    Ok(state.jobs.enqueue(mimic_core::localmodel::JOB_KIND, serde_json::json!({ "model": model }))?)
}
