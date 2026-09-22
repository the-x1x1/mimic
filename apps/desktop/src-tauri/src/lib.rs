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

use std::sync::Arc;

use tauri::{Emitter, Manager};

pub use app_state::{AppState, SharedState};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let handle = app.handle().clone();
            let resource_dir = app.path().resource_dir().ok();
            let state = tauri::async_runtime::block_on(startup::boot(resource_dir))
                .map_err(|e| Box::new(std::io::Error::other(e.to_string())) as Box<dyn std::error::Error>)?;
            let state: SharedState = Arc::new(state);
            app.manage(state.clone());
            startup::spawn_background(handle.clone(), state.clone());
            let _ = handle.emit("app://ready", serde_json::json!({"version": mimic_core::APP_VERSION}));
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Destroyed = event {
                if let Some(state) = window.try_state::<SharedState>() {
                    let engine = state.engine.clone();
                    tauri::async_runtime::block_on(async move { engine.stop().await });
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::app::get_app_info,
            commands::app::get_system_status,
            commands::app::get_onboarding_state,
            commands::app::complete_onboarding,
            commands::dashboard::get_dashboard,
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
            commands::sources::pick_source_file,
            commands::people::get_user_identity,
            commands::people::set_user_identity,
            commands::people::add_user_identifier,
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
        .run(tauri::generate_context!())
        .expect("error while running Mimic");
}
