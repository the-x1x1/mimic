//! People the user talks to, and the user's own identity.

use tauri::State;

use crate::error::{CommandError, CommandResult};
use crate::SharedState;

#[tauri::command]
pub async fn get_user_identity(state: State<'_, SharedState>) -> CommandResult<Option<mimic_core::db::UserIdentity>> {
    Ok(state.db.user_identity()?)
}

#[tauri::command]
pub async fn set_user_identity(
    state: State<'_, SharedState>,
    display_name: String,
) -> CommandResult<mimic_core::db::UserIdentity> {
    Ok(state.db.set_user_identity(&display_name)?)
}

#[tauri::command]
pub async fn add_user_identifier(
    state: State<'_, SharedState>,
    kind: String,
    value: String,
) -> CommandResult<mimic_core::db::UserIdentity> {
    let kind = mimic_core::db::IdentifierKind::parse(&kind)
        .ok_or_else(|| CommandError::new("invalid", format!("{kind:?} is not an address kind Mimic knows")))?;
    state.db.add_user_identifier(kind, &value)?;
    state.db.user_identity()?.ok_or_else(|| CommandError::new("not_found", "identity"))
}

#[tauri::command]
pub async fn remove_user_identifier(
    state: State<'_, SharedState>,
    identifier_id: String,
) -> CommandResult<mimic_core::db::UserIdentity> {
    state.db.remove_user_identifier(&identifier_id)?;
    state.db.user_identity()?.ok_or_else(|| CommandError::new("not_found", "identity"))
}

#[tauri::command]
pub async fn list_people(
    state: State<'_, SharedState>,
    limit: Option<usize>,
) -> CommandResult<Vec<mimic_core::db::ParticipantSummary>> {
    Ok(state.db.list_participants(limit.unwrap_or(200).min(5000))?)
}

#[tauri::command]
pub async fn get_person(
    state: State<'_, SharedState>,
    participant_id: String,
) -> CommandResult<Option<mimic_core::db::Participant>> {
    Ok(state.db.get_participant(&participant_id)?)
}

#[tauri::command]
pub async fn set_person_relationship(
    state: State<'_, SharedState>,
    participant_id: String,
    relationship: Option<String>,
) -> CommandResult<mimic_core::db::Participant> {
    Ok(state.db.set_participant_relationship(&participant_id, relationship.as_deref())?)
}

#[tauri::command]
pub async fn rename_person(
    state: State<'_, SharedState>,
    participant_id: String,
    display_name: String,
) -> CommandResult<mimic_core::db::Participant> {
    Ok(state.db.rename_participant(&participant_id, &display_name)?)
}

/// What deleting this person would remove. Read-only; the UI shows this before
/// asking for confirmation.
#[tauri::command]
pub async fn preview_person_deletion(
    state: State<'_, SharedState>,
    participant_id: String,
) -> CommandResult<mimic_core::privacy::DeletionReport> {
    Ok(state.db.preview_participant_deletion(&participant_id)?)
}

#[tauri::command]
pub async fn delete_person(
    state: State<'_, SharedState>,
    participant_id: String,
) -> CommandResult<mimic_core::privacy::DeletionReport> {
    Ok(state.db.delete_participant(&participant_id)?)
}

#[tauri::command]
pub async fn delete_all_communication_data(
    state: State<'_, SharedState>,
) -> CommandResult<mimic_core::privacy::DeletionReport> {
    Ok(state.db.delete_all_communication_data()?)
}
