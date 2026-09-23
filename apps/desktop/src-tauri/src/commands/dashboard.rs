//! The home screen's read model, and the assisted-drafting job behind it.

use tauri::State;

use crate::error::{CommandError, CommandResult};
use crate::SharedState;

#[tauri::command]
pub async fn get_dashboard(
    state: State<'_, SharedState>,
    limit: Option<usize>,
    show_left_out: Option<bool>,
) -> CommandResult<mimic_core::dashboard::Dashboard> {
    let auto = mimic_core::assist::is_enabled(&state.db)?;
    Ok(mimic_core::dashboard::dashboard(&state.db, limit.unwrap_or(25).min(200), auto, show_left_out.unwrap_or(false))?)
}

/// Say whether a thread needs a reply: `no_reply_needed` takes it off the
/// list, `needs_reply` keeps one Mimic read as automated on it, and no mark
/// takes back whatever was said. The mark is about `message_id`, the message
/// the user was looking at; the answer is whether it applies, which it does
/// not when someone has written since.
#[tauri::command]
pub async fn mark_thread(
    state: State<'_, SharedState>,
    conversation_id: String,
    message_id: String,
    mark: Option<String>,
) -> CommandResult<bool> {
    let mark = match mark.as_deref() {
        None => None,
        Some(m) => Some(mimic_core::db::ThreadMark::parse(m).ok_or_else(|| {
            CommandError::new("invalid", format!("\"{m}\" is not something a thread can be marked as."))
        })?),
    };
    Ok(state.db.mark_thread(&conversation_id, &message_id, mark)?)
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
