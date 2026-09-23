//! People the user talks to, and the user's own identity.

use mimic_core::db::{Db, NewEvent};
use mimic_core::jobs::JobRunner;
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

fn identifier_kind(kind: &str) -> CommandResult<mimic_core::db::IdentifierKind> {
    mimic_core::db::IdentifierKind::parse(kind)
        .ok_or_else(|| CommandError::new("invalid", format!("{kind:?} is not an address kind Mimic knows")))
}

/// What adding an address would do: whether it is already the user's, and
/// who the mail already read from it is filed under, with everything saying
/// it is theirs would change. Changes nothing.
#[tauri::command]
pub async fn preview_user_address(
    state: State<'_, SharedState>,
    kind: String,
    value: String,
) -> CommandResult<mimic_core::db::AddressPreview> {
    Ok(state.db.preview_user_address(identifier_kind(&kind)?, &value)?)
}

/// Add an address of the user's. When mail already read from it is filed
/// under a person, `confirmed_owner` must be what the preview showed —
/// exactly, as it still is — or nothing changes and the error's code is
/// `confirm`. Refused with `busy` while work that would miss it, or that
/// holds that person, runs.
#[tauri::command]
pub async fn add_user_identifier(
    state: State<'_, SharedState>,
    kind: String,
    value: String,
    confirmed_owner: Option<mimic_core::db::AddressOwner>,
) -> CommandResult<mimic_core::db::AddressAdded> {
    let added = state.db.add_user_address(identifier_kind(&kind)?, &value, confirmed_owner.as_ref())?;
    // Done and committed: a new analysis that cannot be queued is logged,
    // not reported as a failure to add what was added.
    if let Err(e) = measure_again_if(&state.db, &state.jobs, added.claimed.messages) {
        tracing::warn!(target: "identity", error = %e.message, "could not queue a new analysis");
    }
    Ok(added)
}

/// Fold in the holder of one of the user's addresses that is still held, on
/// the address as stored, when `confirmed_owner` is what the held list shows
/// now. Otherwise nothing changes and the error's code is `confirm`.
#[tauri::command]
pub async fn claim_held_address(
    state: State<'_, SharedState>,
    identifier_id: String,
    confirmed_owner: mimic_core::db::AddressOwner,
) -> CommandResult<mimic_core::db::AddressAdded> {
    let added = state.db.claim_held_address(&identifier_id, &confirmed_owner)?;
    if let Err(e) = measure_again_if(&state.db, &state.jobs, added.claimed.messages) {
        tracing::warn!(target: "identity", error = %e.message, "could not queue a new analysis");
    }
    Ok(added)
}

/// The user said the person under one of their addresses is not them:
/// nothing moves, and they are never folded in without asking.
#[tauri::command]
pub async fn keep_person_apart(state: State<'_, SharedState>, participant_id: String) -> CommandResult<()> {
    Ok(state.db.keep_apart(&participant_id)?)
}

/// The user's addresses that mail already read is still filed under someone
/// else by, for the user to decide.
#[tauri::command]
pub async fn held_user_addresses(state: State<'_, SharedState>) -> CommandResult<Vec<mimic_core::db::HeldAddress>> {
    Ok(state.db.held_user_addresses()?)
}

/// People whose mail was in the user's own Sent folder, most first, for the
/// user to say whether each is them (`add_user_identifier` with the owner
/// shown, or `keep_person_apart`).
#[tauri::command]
pub async fn sent_folder_people(state: State<'_, SharedState>) -> CommandResult<Vec<mimic_core::db::SentFolderPerson>> {
    Ok(state.db.sent_folder_people()?)
}

/// Messages became the user's, so how they write is measured again — once,
/// if a measurement is not already waiting to run.
fn measure_again_if(db: &Db, jobs: &JobRunner, claimed_messages: usize) -> CommandResult<()> {
    if claimed_messages == 0 {
        return Ok(());
    }
    let waiting = db.list_jobs(50, true)?.iter().any(|j| j.kind == mimic_core::voice::JOB_KIND && j.status == "queued");
    if !waiting {
        jobs.enqueue(mimic_core::voice::JOB_KIND, serde_json::json!({}))?;
    }
    Ok(())
}

/// Fold back into the user anyone whose every address is the user's and
/// about whom the user recorded nothing (`Db::claim_user_mail`), and record
/// it when anyone was. Never fails the caller: with work holding people
/// running it is left for the next occasion — the next job to finish, or the
/// next start.
pub(crate) fn reconcile_identity(db: &Db, jobs: &JobRunner, occasion: &str) {
    match db.claim_user_mail() {
        // Only when something moved: people left for the user are listed in
        // Settings, and this runs after every job.
        Ok(r) if r.claimed.people > 0 => {
            tracing::info!(target: "identity", occasion, messages = r.claimed.messages, people = r.claimed.people, left = r.left, "folded people back into the user");
            let _ = db.log_event(&NewEvent::info(
                "identity",
                "reconciled",
                serde_json::json!({
                    "occasion": occasion,
                    "messages": r.claimed.messages,
                    "people": r.claimed.people,
                    "left": r.left,
                }),
            ));
            if let Err(e) = measure_again_if(db, jobs, r.claimed.messages) {
                tracing::warn!(target: "identity", error = %e.message, "could not queue a new analysis");
            }
        }
        Ok(_) => {}
        Err(mimic_core::db::DbError::Busy(_)) => {
            tracing::debug!(target: "identity", occasion, "left reconciling addresses for later: work holding people is running");
        }
        Err(e) => tracing::warn!(target: "identity", occasion, error = %e, "could not reconcile the user's addresses"),
    }
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
    automated_limit: Option<usize>,
) -> CommandResult<mimic_core::db::PeopleView> {
    Ok(state.db.people_view(limit.unwrap_or(200).min(5000), automated_limit.map(|l| l.min(5000)))?)
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
    crate::commands::voice::stop_measuring(&state);
    Ok(state.db.delete_participant(&participant_id)?)
}

#[tauri::command]
pub async fn delete_all_communication_data(
    state: State<'_, SharedState>,
) -> CommandResult<mimic_core::privacy::DeletionReport> {
    crate::commands::voice::stop_measuring(&state);
    Ok(state.db.delete_all_communication_data()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mimic_core::db::{IdentifierInput, IdentifierKind, NewMessage, NewSource};

    /// The user's address was added by connecting its mailbox, after mail
    /// from it was read and filed under "C (work)", who holds nothing else.
    fn read_before_the_address_was_added() -> (Db, JobRunner, String) {
        let db = Db::open_in_memory().unwrap();
        db.set_user_identity("C").unwrap();
        db.add_user_identifier(IdentifierKind::Email, "c@example.com").unwrap();
        let source = db
            .create_source(&NewSource {
                connector: "mbox".into(),
                name: "export".into(),
                channel: "email".into(),
                location: None,
                config: serde_json::Value::Null,
            })
            .unwrap()
            .id;
        let alias = db
            .resolve_participant("C (work)", &[IdentifierInput::new(IdentifierKind::Email, "c@work.example")], false)
            .unwrap();
        let note = db.upsert_conversation(&source, "note", "email", Some("Note to self")).unwrap();
        db.link_conversation_participant(&note, &alias).unwrap();
        db.insert_messages(&[NewMessage {
            conversation_id: note.clone(),
            source_id: source,
            participant_id: Some(alias.clone()),
            external_id: "n1".into(),
            direction: "other".into(),
            channel: "email".into(),
            sent_at: Some("2026-09-01T10:00:00Z".into()),
            sequence_index: 0,
            body: "remember the keys".into(),
            reply_to_external_id: None,
            metadata: serde_json::Value::Null,
        }])
        .unwrap();
        db.add_user_identifier(IdentifierKind::Email, "c@work.example").unwrap();
        let jobs = JobRunner::new(
            db.clone(),
            std::sync::Arc::new(mimic_core::jobs::CompositeExecutor::new(vec![
                mimic_core::import::ImportExecutor::shared(),
                mimic_core::voice::AnalyzeExecutor::shared(),
            ])),
        );
        (db, jobs, alias)
    }

    #[test]
    fn reconciling_folds_back_someone_who_is_only_the_user_says_so_and_measures_again() {
        let (db, jobs, alias) = read_before_the_address_was_added();
        reconcile_identity(&db, &jobs, "startup");
        assert!(db.get_participant(&alias).unwrap().is_none());
        assert_eq!(db.count_self_messages(None, None).unwrap(), 1);
        let queued = db.list_jobs(10, true).unwrap();
        assert_eq!(queued.iter().filter(|j| j.kind == mimic_core::voice::JOB_KIND).count(), 1);
        let events = db.recent_events(10, None).unwrap();
        assert!(events.iter().any(|e| e.category == "identity" && e.event_type == "reconciled"));

        // Nothing left to do: no second analysis, no second record.
        reconcile_identity(&db, &jobs, "after_job");
        assert_eq!(db.list_jobs(10, true).unwrap().len(), queued.len());
        assert_eq!(db.recent_events(10, None).unwrap().len(), events.len());
    }

    #[test]
    fn reconciling_while_mail_is_read_changes_nothing_and_does_not_fail() {
        let (db, jobs, alias) = read_before_the_address_was_added();
        let reading = db.create_job(mimic_core::import::JOB_KIND, &serde_json::json!({}), false).unwrap();
        db.mark_job_running(&reading.id).unwrap();
        reconcile_identity(&db, &jobs, "after_job");
        assert!(db.get_participant(&alias).unwrap().is_some());
        assert!(db.recent_events(10, None).unwrap().iter().all(|e| e.category != "identity"));

        db.complete_job(&reading.id, &serde_json::json!({})).unwrap();
        reconcile_identity(&db, &jobs, "after_job");
        assert!(db.get_participant(&alias).unwrap().is_none(), "the next job to finish is the next chance");
    }
}
