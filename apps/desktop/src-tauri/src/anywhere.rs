//! Answering a message from anywhere (0.10.0-alpha.29). Most places a message
//! arrives — WhatsApp, Discord, Messages on the phone through Phone Link,
//! Instagram — have no approved way for an app to read or send as the user.
//! What every one of them has is copy and paste. So: the user copies the
//! message, presses a shortcut of their choosing, and Mimic comes to the
//! front with Write open on it, ready for a reply in their voice to copy
//! back.
//!
//! The clipboard is read only when the shortcut is pressed, and what it held
//! goes to the screen and nowhere else: not to a log, not to the database
//! until the user asks for a draft. Off until the user picks a shortcut.

use tauri::{AppHandle, Emitter};

use crate::SharedState;

/// The setting: one of `CHOICES`' names, empty for off.
pub const SETTING: &str = "anywhere.shortcut";

/// The shortcuts offered, as the screen names them and as the plugin reads
/// them. Each takes three modifiers, so none is one an app is likely to use
/// already, none is one of Windows' own (most of those take the Windows key),
/// and none types an AltGr character (Ctrl+Alt with a letter does, on many
/// keyboards).
pub const CHOICES: &[(&str, &str)] = &[
    ("Ctrl+Shift+Alt+M", "ctrl+shift+alt+KeyM"),
    ("Ctrl+Shift+Alt+R", "ctrl+shift+alt+KeyR"),
    ("Ctrl+Shift+Alt+Space", "ctrl+shift+alt+Space"),
];

/// The plugin's form of a shortcut the screen names, or none for off. An
/// unknown name is refused.
pub fn accelerator(choice: &str) -> Result<Option<&'static str>, String> {
    if choice.is_empty() {
        return Ok(None);
    }
    CHOICES
        .iter()
        .find(|(name, _)| *name == choice)
        .map(|(_, acc)| Some(*acc))
        .ok_or_else(|| format!("{choice} isn't one of the shortcuts Mimic offers"))
}

/// The longest message taken from the clipboard: past it, the rest is left
/// out rather than a whole document put into Write.
const MOST: usize = 20_000;

/// Listen for `choice` and nothing else; none stops listening. Refused when
/// another app has the shortcut.
pub fn listen(app: &AppHandle, choice: &str) -> Result<(), String> {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;
    let accelerator = accelerator(choice)?;
    let shortcuts = app.global_shortcut();
    shortcuts.unregister_all().map_err(|e| e.to_string())?;
    if let Some(acc) = accelerator {
        shortcuts
            .register(acc)
            .map_err(|_| format!("Another app is already using {choice}. Pick another, or turn it off."))?;
    }
    Ok(())
}

/// The shortcut was pressed: bring Mimic forward with Write open on what the
/// clipboard holds.
pub fn pressed(app: &AppHandle) {
    use tauri_plugin_clipboard_manager::ClipboardExt;
    let text = app.clipboard().read_text().unwrap_or_default();
    let text: String = text.trim().chars().take(MOST).collect();
    crate::bring_to_front(app);
    let _ = app.emit("write://pasted", serde_json::json!({ "text": text }));
}

/// At start-up: listen for the shortcut the user chose, if any. One that
/// can't be had now (another app took it) is said in the log, without the
/// setting being changed, and Settings says so when it is next changed.
pub fn start(app: &AppHandle, state: &SharedState) {
    let choice = state.db.get_setting::<String>(SETTING).ok().flatten().unwrap_or_default();
    if let Err(e) = listen(app, &choice) {
        tracing::warn!(target: "app", error = %e, "could not listen for the shortcut");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_shortcuts_offered_are_listened_for() {
        assert_eq!(accelerator(""), Ok(None));
        assert_eq!(accelerator("Ctrl+Shift+Alt+M"), Ok(Some("ctrl+shift+alt+KeyM")));
        assert_eq!(accelerator("Ctrl+Shift+Alt+Space"), Ok(Some("ctrl+shift+alt+Space")));
        assert!(accelerator("Ctrl+C").is_err());
        for (_, acc) in CHOICES {
            assert!(acc.parse::<tauri_plugin_global_shortcut::Shortcut>().is_ok(), "{acc} parses");
        }
    }
}
