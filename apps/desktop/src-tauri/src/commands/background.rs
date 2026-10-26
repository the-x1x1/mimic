//! Starting Mimic when the user signs in to Windows: theirs to turn on, off
//! until then. Keeping it running and notifications are ordinary settings
//! (`background::KEEP_RUNNING`, `background::NOTIFY`).

use tauri_plugin_autostart::ManagerExt;

use crate::error::{CommandError, CommandResult};

fn autostart_error(e: impl std::fmt::Display) -> CommandError {
    CommandError::new("autostart", format!("I couldn't change whether I start with Windows: {e}"))
}

/// Get out of the way after a reply was copied, so the window the user came
/// from is in front to paste it into (`background::step_aside`): true when
/// hidden (the icon or the shortcut brings Mimic back), false when minimised.
#[tauri::command]
pub async fn step_aside(app: tauri::AppHandle) -> CommandResult<bool> {
    crate::background::step_aside(&app)
        .map(|how| how == crate::background::Aside::Hidden)
        .map_err(|e| CommandError::new("window", format!("I couldn't step aside: {e}")))
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
