//! Mimic desktop shell. Wires mimic-core (database, sources, import, voice,
//! generation, providers, jobs) into a Tauri 2 application and exposes typed
//! commands to the React frontend.

mod app_state;
mod commands;
mod error;
mod logging;
mod providers_config;
mod secrets;
mod startup;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tauri::{Emitter, Manager};

pub use app_state::{AppState, SharedState};

/// This Mimic's window has gone and it is finishing closing.
static CLOSING: AtomicBool = AtomicBool::new(false);
/// A launch was handed to this Mimic while it was closing: start Mimic again
/// once this one has gone.
static REOPEN: AtomicBool = AtomicBool::new(false);

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        // First, as the plugin requires: a second launch hands over to the
        // Mimic already running, which comes to the front, and exits before
        // anything else starts.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| second_launch(app)))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let handle = app.handle().clone();
            let resource_dir = app.path().resource_dir().ok();
            let state = match tauri::async_runtime::block_on(startup::boot(resource_dir)) {
                Ok(state) => state,
                Err(e) if e.is::<mimic_core::instance::AlreadyRunning>() => {
                    leave_to_the_running_one(&handle);
                    return Ok(());
                }
                Err(e) => return Err(Box::new(std::io::Error::other(e.to_string())) as Box<dyn std::error::Error>),
            };
            let state: SharedState = Arc::new(state);
            app.manage(state.clone());
            // The window is made here, once this Mimic has the data, rather
            // than from the config at launch: a copy that is refused never
            // shows one.
            let window = app
                .config()
                .app
                .windows
                .iter()
                .find(|w| w.label == "main")
                .cloned()
                .ok_or_else(|| std::io::Error::other("tauri.conf.json has no main window"))?;
            tauri::WebviewWindowBuilder::from_config(&handle, &window)?.build()?;
            startup::spawn_background(handle.clone(), state.clone());
            let _ = handle.emit("app://ready", serde_json::json!({"version": mimic_core::APP_VERSION}));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app::get_app_info,
            commands::app::get_system_status,
            commands::app::get_onboarding_state,
            commands::app::complete_onboarding,
            commands::dashboard::get_dashboard,
            commands::dashboard::mark_thread,
            commands::dashboard::get_conversation_page,
            commands::dashboard::start_assist_drafts,
            commands::localmodel::local_model_status,
            commands::localmodel::start_model_pull,
            commands::settings::get_settings,
            commands::settings::set_setting,
            commands::sources::list_connectors,
            commands::sources::list_sources,
            commands::sources::validate_source_file,
            commands::sources::create_source,
            commands::sources::start_source_import,
            commands::sources::delete_source,
            commands::sources::probe_mailbox,
            commands::sources::connect_mailbox,
            commands::sources::set_mailbox_password,
            commands::sources::pick_source_file,
            commands::people::get_user_identity,
            commands::people::set_user_identity,
            commands::people::preview_user_address,
            commands::people::add_user_identifier,
            commands::people::held_user_addresses,
            commands::people::claim_held_address,
            commands::people::keep_person_apart,
            commands::people::remove_user_identifier,
            commands::people::list_people,
            commands::people::get_person,
            commands::people::set_person_relationship,
            commands::people::rename_person,
            commands::people::preview_person_deletion,
            commands::people::delete_person,
            commands::people::delete_all_communication_data,
            commands::voice::get_voice_overview,
            commands::voice::start_voice_analysis,
            commands::voice::get_voice_profile,
            commands::voice::list_situations,
            commands::voice::get_learning_overview,
            commands::voice::add_voice_note,
            commands::voice::list_voice_examples,
            commands::voice::set_voice_preference,
            commands::voice::list_voice_preferences,
            commands::voice::delete_voice_preference,
            commands::compose::preview_generation_context,
            commands::compose::generate_draft,
            commands::compose::resolve_draft,
            commands::compose::add_draft_preference,
            commands::compose::list_recent_drafts,
            commands::compose::get_draft_outcomes,
            commands::providers::get_provider_state,
            commands::providers::set_active_provider,
            commands::providers::set_provider_secret,
            commands::providers::check_provider,
            commands::jobs::list_jobs,
            commands::jobs::get_job,
            commands::jobs::cancel_job,
            commands::diagnostics::get_diagnostics_bundle,
            commands::diagnostics::get_recent_events,
            commands::diagnostics::restart_engine,
            commands::diagnostics::open_logs_folder,
            commands::updater::get_update_state,
            commands::updater::set_update_preferences,
            commands::updater::record_update_check,
            commands::updater::can_install_update_now,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Mimic");
    let code = app.run_return(on_run_event);
    // By now the hand-over has been released (the plugin does that on exit),
    // so the new process becomes the Mimic, and waits on the data folder's
    // lock until this one has gone.
    if REOPEN.load(Ordering::SeqCst) {
        reopen();
    }
    std::process::exit(code);
}

/// The longest closing waits for the engine: its shutdown call is allowed two
/// seconds and the kill after it waits on the engine's writer, which a busy
/// engine can hold. Kept under `startup::TAKE_OVER_WITHIN`, so a Mimic started
/// again on the way out still finds the data free in time.
const ENGINE_STOP_LIMIT: std::time::Duration = std::time::Duration::from_secs(5);

/// Closing, kept on a running event loop. When the last window closes, the
/// loop is kept going while the engine stops — a couple of seconds — so a
/// launch in that time still reaches `second_launch` rather than being sent to
/// a process no longer reading its messages; then Mimic exits. An exit asked
/// for in code (`exit`, the refused copy's dialog) goes straight through, and
/// the engine is stopped on the way out if it has not been.
fn on_run_event(app: &tauri::AppHandle, event: tauri::RunEvent) {
    match event {
        tauri::RunEvent::ExitRequested { code: None, api, .. } => {
            let Some(state) = app.try_state::<SharedState>() else { return };
            api.prevent_exit();
            if !CLOSING.swap(true, Ordering::SeqCst) {
                let engine = state.engine.clone();
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    // Bounded, and apart from the exit: an engine that will not
                    // stop (or a stop that panics) still lets Mimic close.
                    let stopping = tauri::async_runtime::spawn(async move { engine.stop().await });
                    let _ = tokio::time::timeout(ENGINE_STOP_LIMIT, stopping).await;
                    app.exit(0);
                });
            }
        }
        tauri::RunEvent::Exit => {
            // Closing has already tried, within its limit; trying again would
            // only double the longest close.
            if CLOSING.load(Ordering::SeqCst) {
                return;
            }
            if let Some(state) = app.try_state::<SharedState>() {
                if state.engine.status().state != "stopped" {
                    let engine = state.engine.clone();
                    tauri::async_runtime::block_on(async move {
                        let _ = tokio::time::timeout(ENGINE_STOP_LIMIT, engine.stop()).await;
                    });
                }
            }
        }
        _ => {}
    }
}

/// A second launch, handed over by the plugin. A Mimic that has the data
/// brings its window to the front; one that is closing remembers to start
/// again once it has gone, rather than swallowing the launch.
fn second_launch(app: &tauri::AppHandle) {
    if app.try_state::<SharedState>().is_none() {
        return;
    }
    if CLOSING.load(Ordering::SeqCst) {
        tracing::info!(target: "app", "opened while closing; starting again once closed");
        REOPEN.store(true, Ordering::SeqCst);
        return;
    }
    bring_to_front(app);
}

fn reopen() {
    match std::env::current_exe().and_then(|exe| std::process::Command::new(exe).spawn()) {
        Ok(_) => tracing::info!(target: "app", "started again, as asked while closing"),
        Err(e) => tracing::warn!(target: "app", error = %e, "could not start again after closing"),
    }
}

/// The window of the Mimic already running, shown and focused for a second
/// launch.
fn bring_to_front(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// A second Mimic that got past the hand-over — a build with another
/// identifier, or the hand-over failing — finds the data folder locked and
/// has touched nothing. It has no
/// window yet; it says one sentence and exits once that has been read. What
/// holds the lock is not known, only that something does, so it says no more.
fn leave_to_the_running_one(app: &tauri::AppHandle) {
    use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
    let handle = app.clone();
    app.dialog()
        .message(
            "Another Mimic is using your data right now, so this one won't start. \
             If you can't see its window, it may still be closing — try again in a moment.",
        )
        .title("Mimic")
        .kind(MessageDialogKind::Info)
        .show(move |_| handle.exit(0));
}
