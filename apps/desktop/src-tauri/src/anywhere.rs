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
//!
//! The reply can go back the same way (0.10.0-alpha.46, `paste_back`): the
//! window the user was in when they pressed the shortcut is remembered, and
//! **Paste it there** copies the reply, brings that window back and pastes
//! into it — only that window, only once it is in front. Nothing is sent:
//! the user sends it there, as they would have.

use std::sync::atomic::{AtomicIsize, Ordering};
use std::time::{Duration, Instant};

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

/// The window the user was in when they last pressed the shortcut, as
/// Windows names it; zero for none, or for one of Mimic's own.
static CAME_FROM: AtomicIsize = AtomicIsize::new(0);

/// The shortcut was pressed: bring Mimic forward with Write open on what the
/// clipboard holds, remembering the window it was pressed in.
pub fn pressed(app: &AppHandle) {
    use tauri_plugin_clipboard_manager::ClipboardExt;
    CAME_FROM.store(window::front_elsewhere(), Ordering::SeqCst);
    let text = app.clipboard().read_text().unwrap_or_default();
    let text: String = text.trim().chars().take(MOST).collect();
    crate::bring_to_front(app);
    let _ = app.emit("write://pasted", serde_json::json!({ "text": text }));
}

/// Why a reply was copied rather than pasted.
pub const NO_WINDOW: &str = "I don't know which window the message came from, so the reply is copied for you to paste.";
pub const WINDOW_GONE: &str = "The window the message came from has closed, so the reply is copied for you to paste.";
pub const NOT_IN_FRONT: &str =
    "I couldn't bring the window the message came from to the front, and I won't paste into another. The reply is copied for you to paste.";
pub const NOT_PASTED: &str = "Windows didn't let me paste, so the reply is copied for you to paste.";

/// Ctrl+V, as Windows' virtual keys pressed and let go: (key, up).
const VK_CONTROL: u16 = 0x11;
const VK_V: u16 = 0x56;
const PASTE: [(u16, bool); 4] = [(VK_CONTROL, false), (VK_V, false), (VK_V, true), (VK_CONTROL, true)];

/// Only into the window the message came from, once it is the one in front.
fn may_paste(target: isize, front: isize) -> bool {
    target != 0 && target == front
}

/// How long the window the message came from has to come to the front.
const COMING_BACK: Duration = Duration::from_secs(1);

/// Put a reply where the message came from: copied, that window brought
/// back, Mimic out of the way, and Ctrl+V pressed in it. `None` when it was
/// pasted; otherwise why it is only copied. Nothing is sent: the user sends
/// it there. Mimic steps aside either way.
pub fn paste_back(app: &AppHandle, text: &str) -> Result<Option<&'static str>, String> {
    use tauri_plugin_clipboard_manager::ClipboardExt;
    app.clipboard().write_text(text.to_string()).map_err(|e| e.to_string())?;
    let target = CAME_FROM.load(Ordering::SeqCst);
    let unknown = if target == 0 {
        Some(NO_WINDOW)
    } else if !window::exists(target) {
        Some(WINDOW_GONE)
    } else {
        None
    };
    if let Some(why) = unknown {
        crate::background::step_aside(app).map_err(|e| e.to_string())?;
        return Ok(Some(why));
    }
    // Asked while Mimic is in front, which Windows allows; then out of the way.
    window::bring(target);
    crate::background::step_aside(app).map_err(|e| e.to_string())?;
    let until = Instant::now() + COMING_BACK;
    while !may_paste(target, window::front()) {
        if Instant::now() >= until {
            return Ok(Some(NOT_IN_FRONT));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    // A moment for it to take the keyboard back, and a last look.
    std::thread::sleep(Duration::from_millis(120));
    if !may_paste(target, window::front()) {
        return Ok(Some(NOT_IN_FRONT));
    }
    Ok(if window::press(&PASTE) { None } else { Some(NOT_PASTED) })
}

/// Windows' windows, by their handles as numbers.
#[cfg(windows)]
mod window {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowThreadProcessId, IsWindow, SetForegroundWindow,
    };

    /// The window in front.
    pub fn front() -> isize {
        // SAFETY: no arguments; a null handle when there is none.
        unsafe { GetForegroundWindow() as isize }
    }

    /// The window in front, unless it is one of Mimic's own.
    pub fn front_elsewhere() -> isize {
        let w = front();
        let mut pid = 0u32;
        // SAFETY: `pid` outlives the call; a handle that isn't a window
        // leaves it zero.
        unsafe { GetWindowThreadProcessId(w as _, &mut pid) };
        if w == 0 || pid == std::process::id() {
            0
        } else {
            w
        }
    }

    pub fn exists(w: isize) -> bool {
        // SAFETY: any handle may be asked about.
        unsafe { IsWindow(w as _) != 0 }
    }

    pub fn bring(w: isize) {
        // SAFETY: as `exists`; Windows decides whether it may.
        unsafe { SetForegroundWindow(w as _) };
    }

    /// Press and let go of keys, as the keyboard would. True when Windows
    /// took them all.
    pub fn press(keys: &[(u16, bool)]) -> bool {
        let inputs: Vec<INPUT> = keys
            .iter()
            .map(|&(key, up)| INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: key,
                        wScan: 0,
                        dwFlags: if up { KEYEVENTF_KEYUP } else { 0 },
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            })
            .collect();
        // SAFETY: `inputs` is a live array of `INPUT`s of the size given.
        let taken = unsafe { SendInput(inputs.len() as u32, inputs.as_ptr(), std::mem::size_of::<INPUT>() as i32) };
        taken as usize == inputs.len()
    }
}

/// Elsewhere there is no window to go back to: the reply is copied.
#[cfg(not(windows))]
mod window {
    pub fn front() -> isize {
        0
    }
    pub fn front_elsewhere() -> isize {
        0
    }
    pub fn exists(_: isize) -> bool {
        false
    }
    pub fn bring(_: isize) {}
    pub fn press(_: &[(u16, bool)]) -> bool {
        false
    }
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

    #[test]
    fn a_reply_is_pasted_only_into_the_window_it_came_from_once_in_front() {
        assert!(may_paste(42, 42));
        assert!(!may_paste(42, 7), "not into whatever else is in front");
        assert!(!may_paste(0, 0), "not when it came from nowhere known");
        assert_eq!(PASTE, [(0x11, false), (0x56, false), (0x56, true), (0x11, true)], "Ctrl+V, let go in order");
    }

    #[test]
    fn a_window_that_is_not_one_is_not_one() {
        assert!(!window::exists(0));
    }
}
