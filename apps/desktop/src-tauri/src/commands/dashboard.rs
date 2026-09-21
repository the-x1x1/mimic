//! The home screen's read model, and the assisted-drafting job behind it.

use tauri::State;

use crate::error::{CommandError, CommandResult};
use crate::SharedState;

#[tauri::command]
pub async fn get_dashboard(
    state: State<'_, SharedState>,
    limit: Option<usize>,
) -> CommandResult<mimic_core::dashboard::Dashboard> {
    let auto = mimic_core::assist::is_enabled(&state.db)?;
    Ok(mimic_core::dashboard::dashboard(&state.db, limit.unwrap_or(25).min(200), auto)?)
}

/// Queue a run of assisted drafting. Refuses rather than silently doing
/// nothing when the user has not turned it on, so a button can never look like
/// it worked when it did not.
#[tauri::command]
pub async fn start_assist_drafts(state: State<'_, SharedState>) -> CommandResult<mimic_core::db::Job> {
    if !mimic_core::assist::is_enabled(&state.db)? {
        return Err(CommandError::new(
            "assist_disabled",
            "Preparing replies in advance is turned off. Turn it on in Settings first.",
        ));
    }
    Ok(state.jobs.enqueue(mimic_core::assist::JOB_KIND, serde_json::json!({}))?)
}
