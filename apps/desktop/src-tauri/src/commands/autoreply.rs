//! Replies Mimic sends by itself — Trusted mode, 0.10.0-alpha.24 — and the
//! rules the user sets for them (`mimic_core::autoreply`).
//!
//! A look (`look`) holds a reply for each draft the rules allow and sends
//! those whose hold has ended, every half minute from `startup.rs` and at once
//! after a change here; the screen is told when it held, sent, stopped or
//! failed anything (`sending://changed`).

use std::sync::atomic::Ordering;

use mimic_core::autoreply::{self, Rules};
use mimic_core::db::OutboxEntry;
use tauri::{AppHandle, Emitter, State};

use crate::error::{CommandError, CommandResult};
use crate::SharedState;

fn read(state: &crate::AppState) -> CommandResult<autoreply::Status> {
    Ok(autoreply::status(&state.db)?)
}

#[tauri::command]
pub async fn get_autoreply(state: State<'_, SharedState>) -> CommandResult<autoreply::Status> {
    read(&state)
}

/// Save the rules. Turning it on turns on preparing replies in advance too,
/// since those are the drafts it sends; turning it off stops every reply
/// still held, at once.
#[tauri::command]
pub async fn set_autoreply(
    app: AppHandle,
    state: State<'_, SharedState>,
    rules: Rules,
) -> CommandResult<autoreply::Status> {
    if rules.enabled {
        state.db.set_setting(mimic_core::assist::SETTING, &true)?;
    }
    autoreply::set_rules(&state.db, rules)?;
    let _ = app.emit("sending://changed", serde_json::json!({}));
    look(&app, &state).await;
    read(&state)
}

/// Stop a held reply. Its draft waits for the user, as any draft does.
#[tauri::command]
pub async fn stop_held_reply(
    app: AppHandle,
    state: State<'_, SharedState>,
    outbox_id: String,
) -> CommandResult<OutboxEntry> {
    let entry = state.db.cancel_outgoing(&outbox_id)?;
    let _ = app.emit("sending://changed", serde_json::json!({}));
    Ok(entry)
}

/// Send a held reply now rather than when its hold ends, still under every
/// rule it was held under.
#[tauri::command]
pub async fn send_held_reply_now(
    app: AppHandle,
    state: State<'_, SharedState>,
    outbox_id: String,
) -> CommandResult<OutboxEntry> {
    let now = mimic_core::ids::now_rfc3339();
    state.db.send_held_now(&outbox_id, &now)?;
    look(&app, &state).await;
    state.db.outgoing(&outbox_id)?.ok_or_else(|| CommandError::new("not_found", "That reply isn't here any more."))
}

/// Hold what may go and send what is due, once; a look already running is
/// left to finish. Never logs what was sent: only counts.
pub async fn look(app: &AppHandle, state: &SharedState) {
    if state.replying.swap(true, Ordering::SeqCst) {
        return;
    }
    let (db, credentials) = (state.db.clone(), state.mail_credentials.clone());
    let ran = tauri::async_runtime::spawn_blocking(move || autoreply::tick_now(&db, &credentials)).await;
    state.replying.store(false, Ordering::SeqCst);
    match ran {
        Ok(Ok(run)) => {
            // A reply whose hold has ended waits for its mailbox to be read
            // again: ask for that now rather than at the next scheduled check
            // — unless a check is already queued, or the last one failed
            // (the schedule backs off from a failing mailbox; this doesn't).
            for source_id in &run.waiting_for_check {
                queue_check(state, source_id);
            }
            if run.changed_anything() {
                tracing::info!(target: "autoreply", held = run.held, sent = run.sent, stopped = run.stopped, failed = run.failed, unsure = run.unsure, rehearsed = run.rehearsed, "replied by itself");
                let _ = app.emit("sending://changed", &run);
                let rules = autoreply::rules(&state.db).unwrap_or_default();
                for (title, body) in crate::background::replied_by_itself(&run, &rules) {
                    crate::background::tell(app, &title, &body);
                }
            }
        }
        Ok(Err(e)) => tracing::warn!(target: "autoreply", error = %e, "a look at replies sent by themselves failed"),
        Err(e) => tracing::warn!(target: "autoreply", error = %e, "a look at replies sent by themselves stopped"),
    }
}

/// Ask for a mailbox to be checked, unless one is already queued or the
/// mailbox is failing (that waits for the user). One running doesn't stop
/// it: mail that arrived after that check began isn't in what it reads.
pub(crate) fn queue_check(state: &SharedState, source_id: &str) {
    use mimic_core::sources::imap;
    let failing = state.db.get_source(source_id).ok().flatten().is_none_or(|s| s.status == "failed");
    let queued = state.db.list_jobs(500, true).map(|jobs| {
        jobs.iter().any(|j| {
            j.kind == imap::JOB_KIND && j.status == "queued" && j.payload["sourceId"].as_str() == Some(source_id)
        })
    });
    if failing || !matches!(queued, Ok(false)) {
        return;
    }
    if let Err(e) = state.jobs.enqueue(imap::JOB_KIND, serde_json::json!({ "sourceId": source_id })) {
        tracing::warn!(target: "autoreply", error = %e, "could not ask for a mailbox check");
    }
}
