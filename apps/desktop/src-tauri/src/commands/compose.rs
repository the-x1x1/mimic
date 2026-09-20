//! Compose: build a draft, then record what the user did with it.

use tauri::State;

use crate::error::CommandResult;
use crate::SharedState;

/// Everything the Compose screen needs to explain itself before a draft
/// exists: which layers apply, how much evidence there is, which examples
/// would be used.
#[tauri::command]
pub async fn preview_generation_context(
    state: State<'_, SharedState>,
    request: mimic_core::generation::ComposeRequest,
) -> CommandResult<mimic_core::generation::GenerationContext> {
    Ok(mimic_core::generation::build_context(&state.db, &request)?)
}

#[tauri::command]
pub async fn generate_draft(
    state: State<'_, SharedState>,
    request: mimic_core::generation::ComposeRequest,
) -> CommandResult<mimic_core::db::Draft> {
    let provider = state.active_provider()?;
    let db = state.db.clone();
    // Provider calls are blocking HTTP; keep them off the async worker.
    let draft =
        tauri::async_runtime::spawn_blocking(move || mimic_core::generation::compose(&db, provider.as_ref(), &request))
            .await
            .map_err(|e| crate::error::CommandError::new("internal", e.to_string()))??;
    Ok(draft)
}

/// Record the outcome of a draft. `finalText` is what the user actually sent.
/// The difference between the two is the feedback.
#[tauri::command]
pub async fn resolve_draft(
    state: State<'_, SharedState>,
    draft_id: String,
    outcome: String,
    final_text: Option<String>,
) -> CommandResult<mimic_core::db::Draft> {
    let draft = state.db.resolve_draft(&draft_id, &outcome, final_text.as_deref())?;
    if outcome == "sent_edited" {
        if let Some(sent) = final_text.as_deref() {
            let diff = mimic_core::generation::feedback::diff_draft(&draft.generated_text, sent);
            state.db.add_draft_feedback(&draft_id, "edit", 1.0, &serde_json::to_value(&diff)?, None)?;
        }
    }
    Ok(draft)
}

/// An explicit correction the user typed, which outweighs anything inferred
/// from an edit.
#[tauri::command]
pub async fn add_draft_preference(
    state: State<'_, SharedState>,
    draft_id: String,
    note: String,
) -> CommandResult<mimic_core::db::DraftFeedback> {
    Ok(state.db.add_draft_feedback(
        &draft_id,
        "preference",
        mimic_core::generation::feedback::PREFERENCE_WEIGHT,
        &serde_json::json!({ "note": note }),
        Some(&note),
    )?)
}

#[tauri::command]
pub async fn list_recent_drafts(
    state: State<'_, SharedState>,
    limit: Option<usize>,
) -> CommandResult<Vec<mimic_core::db::Draft>> {
    Ok(state.db.recent_drafts(limit.unwrap_or(25).min(200))?)
}

#[tauri::command]
pub async fn get_draft_outcomes(state: State<'_, SharedState>) -> CommandResult<mimic_core::db::DraftOutcomes> {
    Ok(state.db.measured_draft_outcomes()?)
}
