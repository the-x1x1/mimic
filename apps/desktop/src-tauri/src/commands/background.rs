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

/// Put a reply in the window the message came from (0.10.0-alpha.46,
/// `anywhere::paste_back`): none when it was pasted; otherwise why it was
/// only copied. Nothing is sent, unless `send` and the user turned on
/// pressing Enter after pasting (0.10.0-alpha.58); `send` with it off is
/// refused before anything is done.
#[tauri::command]
pub async fn paste_back(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::SharedState>,
    text: String,
    send: Option<bool>,
) -> CommandResult<Option<String>> {
    let send = send.unwrap_or(false);
    if send && state.db.get_setting::<bool>(crate::anywhere::PRESS_ENTER)? != Some(true) {
        return Err(CommandError::new(
            "off",
            "Pressing Enter after pasting is off. Turn it on under Settings, in Answering a message from anywhere.",
        ));
    }
    let pasting = app.clone();
    tauri::async_runtime::spawn_blocking(move || crate::anywhere::paste_back(&pasting, &text, send))
        .await
        .map_err(|e| CommandError::new("window", format!("I couldn't put it there: {e}")))?
        .map(|why| why.map(str::to_string))
        .map_err(|e| CommandError::new("window", format!("I couldn't put it there: {e}")))
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
