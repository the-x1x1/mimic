//! Mimic desktop shell. Wires mimic-core (db, bridge, engine, jobs) into a
//! Tauri 2 application and exposes typed commands to the React frontend.

mod app_state;
mod commands;
mod error;
mod logging;
mod plugin_install;
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
            commands::app::enable_demo_mode,
            commands::app::pick_folder,
            commands::settings::get_settings,
            commands::settings::set_setting,
            commands::libraries::list_libraries,
            commands::libraries::create_library,
            commands::libraries::delete_library,
            commands::libraries::start_library_scan,
            commands::libraries::get_data_quality_report,
            commands::libraries::list_library_assets,
            commands::libraries::get_asset_detail,
            commands::styles::list_styles,
            commands::styles::create_style,
            commands::styles::delete_style,
            commands::styles::attach_library_to_style,
            commands::styles::get_style_detail,
            commands::styles::train_style,
            commands::styles::activate_model_version,
            commands::styles::archive_model_version,
            commands::styles::get_model_version,
            commands::sessions::list_sessions,
            commands::sessions::create_session,
            commands::sessions::get_session_detail,
            commands::sessions::list_session_photos,
            commands::sessions::set_session_style,
            commands::sessions::delete_session,
            commands::sessions::group_session,
            commands::sessions::predict_session,
            commands::sessions::set_prediction_review,
            commands::sessions::get_apply_preflight,
            commands::sessions::apply_session,
            commands::sessions::list_applied_edits,
            commands::sessions::restore_apply_batch,
            commands::sessions::get_prediction,
            commands::jobs::list_jobs,
            commands::jobs::get_job,
            commands::jobs::cancel_job,
            commands::lightroom::get_lightroom_status,
            commands::lightroom::get_plugin_setup,
            commands::lightroom::install_lightroom_plugin,
            commands::lightroom::test_lightroom_connection,
            commands::lightroom::start_lightroom_ingest,
            commands::lightroom::get_capability_matrix,
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
