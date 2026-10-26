//! Starting Mimic when the user signs in to Windows: theirs to turn on, off
//! until then. Keeping it running and notifications are ordinary settings
//! (`background::KEEP_RUNNING`, `background::NOTIFY`).

use tauri_plugin_autostart::ManagerExt;

use crate::error::{CommandError, CommandResult};

fn autostart_error(e: impl std::fmt::Display) -> CommandError {
    CommandError::new("autostart", format!("I couldn't change whether I start with Windows: {e}"))
}

/// Whether Windows starts Mimic when the user signs in.
#[tauri::command]
pub async fn get_start_with_windows(app: tauri::AppHandle) -> CommandResult<bool> {
    app.autolaunch().is_enabled().map_err(autostart_error)
}

/// Start Mimic when the user signs in, or stop doing so; what Windows says
/// afterwards is the answer.
#[tauri::command]
pub async fn set_start_with_windows(app: tauri::AppHandle, on: bool) -> CommandResult<bool> {
    let launcher = app.autolaunch();
    if on {
        launcher.enable().map_err(autostart_error)?;
    } else {
        launcher.disable().map_err(autostart_error)?;
    }
    launcher.is_enabled().map_err(autostart_error)
}
