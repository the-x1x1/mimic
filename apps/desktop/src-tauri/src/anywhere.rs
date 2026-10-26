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
//! the user sends it there, as they would have — unless they turned on
//! pressing Enter after pasting (0.10.0-alpha.58, `PRESS_ENTER`, off by
//! default), for chat apps where Enter sends, and press **Paste it there and
//! send**: then Enter is pressed too, once, in that window, only if it is
//! still the one in front.

use std::sync::atomic::{AtomicIsize, Ordering};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter};

use crate::SharedState;

/// The setting: one of `CHOICES`' names, empty for off.
pub const SETTING: &str = "anywhere.shortcut";

/// Whether **Paste it there and send** is offered: Enter pressed after the
/// paste, which sends in most chat apps (0.10.0-alpha.58). Off by default.
pub const PRESS_ENTER: &str = "anywhere.pressEnter";

/// Whether Write writes a draft as soon as it opens on a message from
/// anywhere (0.10.0-alpha.63). On by default: pressing the shortcut is asking
/// for a reply.
pub const DRAFT_AT_ONCE: &str = "anywhere.draftAtOnce";

/// Whether the shortcut copies what is selected before reading the
/// clipboard (0.10.0-alpha.71), so selecting the message is enough. Off by
/// default: it presses keys in another app. It presses Ctrl+Insert, which
/// copies in Windows' apps, browsers and terminals alike — never Ctrl+C,
/// which in a terminal stops what is running — and only once the
/// shortcut's own keys are let go, so nothing else is pressed with it.
pub const COPY_FOR_ME: &str = "anywhere.copyForMe";

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

/// That window's title then, whole: before Enter is pressed for the user
/// (0.10.0-alpha.58), the window must still say the same, so a chat app
/// switched to another conversation meanwhile isn't sent to.
static CAME_FROM_TITLE: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

/// The longest window title passed on.
const MOST_TITLE: usize = 200;

/// The shortcut was pressed: bring Mimic forward with Write open on what the
/// clipboard holds, remembering the window it was pressed in, and that
/// window's title (0.10.0-alpha.52), which often names who the message is
/// from. Neither goes anywhere but the screen. With `COPY_FOR_ME` on, what is
/// selected there is copied first, off the shortcut's own thread, since it
/// waits on the keyboard.
pub fn pressed(app: &AppHandle) {
    use tauri::Manager;
    let copy = app
        .try_state::<SharedState>()
        .and_then(|state| state.db.get_setting::<bool>(COPY_FOR_ME).ok().flatten())
        .unwrap_or(false);
    if !copy {
        take(app, false);
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || take(&app, true));
}

fn take(app: &AppHandle, copy: bool) {
    use tauri_plugin_clipboard_manager::ClipboardExt;
    let from = window::front_elsewhere();
    CAME_FROM.store(from, Ordering::SeqCst);
    let whole: String = if from == 0 { String::new() } else { window::title(from) };
    *CAME_FROM_TITLE.lock().unwrap_or_else(|p| p.into_inner()) = whole.clone();
    let title: String = whole.trim().chars().take(MOST_TITLE).collect();
    // Only in another app's window, and only when it is still in front.
    let asked = copy && from != 0;
    let copied = asked && copy_selection(from);
    let mut text = if clipboard_is_the_message(asked, copied) {
        app.clipboard().read_text().unwrap_or_default()
    } else {
        String::new()
    };
    // The app that copied may still be writing to the clipboard: read again,
    // a moment apart, while it is busy or empty (0.10.0-alpha.72).
    if copied {
        let until = Instant::now() + COPIED_WITHIN;
        while text.trim().is_empty() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(50));
            text = app.clipboard().read_text().unwrap_or_default();
        }
    }
    let text: String = text.trim().chars().take(MOST).collect();
    crate::bring_to_front(app);
    let _ = app.emit("write://pasted", serde_json::json!({ "text": text, "title": title }));
}

/// Whether the clipboard holds the message (0.10.0-alpha.72): unless copying
/// it was asked for and nothing came of it — the keys never let go, another
/// window came forward, or nothing was selected — when what is there is from
/// before, perhaps something private, and goes nowhere: Write opens empty.
fn clipboard_is_the_message(asked: bool, copied: bool) -> bool {
    !asked || copied
}

/// Why a reply was copied rather than pasted.
pub const NO_WINDOW: &str = "I don't know which window the message came from, so the reply is copied for you to paste.";
pub const WINDOW_GONE: &str = "The window the message came from has closed, so the reply is copied for you to paste.";
pub const NOT_IN_FRONT: &str =
    "I couldn't bring the window the message came from to the front, and I won't paste into another. The reply is copied for you to paste.";
pub const NOT_PASTED: &str = "Windows didn't let me paste, so the reply is copied for you to paste.";
pub const MOVED_ON: &str =
    "That window shows something else now than when you copied the message, so I didn't paste or send. The reply is copied for you to paste.";
pub const NOT_SENT: &str =
    "It's pasted there, but I didn't press Enter: that window wasn't in front any more, or Windows didn't let me. Send it there yourself.";

/// Ctrl+V, as Windows' virtual keys pressed and let go: (key, up).
const VK_CONTROL: u16 = 0x11;
const VK_V: u16 = 0x56;
const PASTE: [(u16, bool); 4] = [(VK_CONTROL, false), (VK_V, false), (VK_V, true), (VK_CONTROL, true)];
/// Enter, alone: no Shift, which most chat apps read as a new line.
const VK_RETURN: u16 = 0x0D;
const ENTER: [(u16, bool); 2] = [(VK_RETURN, false), (VK_RETURN, true)];
/// How long a paste has to land before Enter.
const ENTER_AFTER: Duration = Duration::from_millis(250);

/// Ctrl+Insert: copy, in every Windows app, and never an interrupt.
const VK_INSERT: u16 = 0x2D;
const COPY: [(u16, bool); 4] = [(VK_CONTROL, false), (VK_INSERT, false), (VK_INSERT, true), (VK_CONTROL, true)];
/// The shortcut's own keys, which must be up before anything is pressed.
const VK_SHIFT: u16 = 0x10;
const VK_MENU: u16 = 0x12;
const HELD: [u16; 3] = [VK_SHIFT, VK_CONTROL, VK_MENU];
/// How long to wait for them to be let go, and then for the copy to land.
const LET_GO: Duration = Duration::from_millis(1000);
const COPIED_WITHIN: Duration = Duration::from_millis(500);

/// Copy what is selected in `from`: once the shortcut's keys are up, and
/// only while `from` is in front — else nothing is pressed, and the
/// clipboard is read as it is. Waits for the clipboard to change, briefly,
/// then a moment more for the app to finish writing it. Whether it changed.
fn copy_selection(from: isize) -> bool {
    let until = Instant::now() + LET_GO;
    while window::any_down(&HELD) {
        if Instant::now() >= until {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    if !may_paste(from, window::front()) {
        return false;
    }
    let before = window::clipboard_count();
    if !window::press(&COPY) {
        return false;
    }
    let until = Instant::now() + COPIED_WITHIN;
    while window::clipboard_count() == before {
        if Instant::now() >= until {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    std::thread::sleep(Duration::from_millis(80));
    true
}

/// Only into the window the message came from, once it is the one in front.
fn may_paste(target: isize, front: isize) -> bool {
    target != 0 && target == front
}

/// Whether a window's title still says what it did: the same, but for an
/// unread count — "(2) …", "[3]" — or a marker that comes and goes ("•",
/// "*"), which change as messages arrive.
fn same_place(then: &str, now: &str) -> bool {
    fn calm(title: &str) -> String {
        let mut out = String::new();
        let mut chars = title.chars();
        while let Some(c) = chars.next() {
            let close = match c {
                '(' => ')',
                '[' => ']',
                '•' | '*' => continue,
                _ => {
                    out.push(c);
                    continue;
                }
            };
            // A bracket holding only a number is a count: left out.
            let rest: String = chars.clone().take_while(|&d| d != close).collect();
            let n = rest.chars().count();
            if n > 0 && rest.chars().all(|d| d.is_ascii_digit() || d == '+') && chars.clone().nth(n) == Some(close) {
                // The count and its closing bracket.
                chars.nth(n);
            } else {
                out.push(c);
            }
        }
        out.split_whitespace().collect::<Vec<_>>().join(" ")
    }
    calm(then) == calm(now)
}

fn came_from_title() -> String {
    CAME_FROM_TITLE.lock().unwrap_or_else(|p| p.into_inner()).clone()
}

/// How long the window the message came from has to come to the front.
const COMING_BACK: Duration = Duration::from_secs(1);

/// Put a reply where the message came from: copied, that window brought
/// back, Mimic out of the way, and Ctrl+V pressed in it — and, when `send`
/// (which the command allows only with `PRESS_ENTER` on), Enter after it, if
/// that window is still in front. `None` when it was all done; otherwise why
/// it is only copied, or only pasted (`NOT_SENT`). Mimic steps aside either
/// way.
pub fn paste_back(app: &AppHandle, text: &str, send: bool) -> Result<Option<&'static str>, String> {
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
    // To be sent: only to what that window showed when the message was
    // copied — not, say, another conversation in the same chat app.
    if send && !same_place(&came_from_title(), &window::title(target)) {
        crate::background::step_aside(app).map_err(|e| e.to_string())?;
        return Ok(Some(MOVED_ON));
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
    if !window::press(&PASTE) {
        return Ok(Some(NOT_PASTED));
    }
    if !send {
        return Ok(None);
    }
    // The paste a moment to land, and a last look: Enter goes only to the
    // window the message came from.
    std::thread::sleep(ENTER_AFTER);
    if !may_paste(target, window::front()) || !same_place(&came_from_title(), &window::title(target)) {
        return Ok(Some(NOT_SENT));
    }
    Ok(if window::press(&ENTER) { None } else { Some(NOT_SENT) })
}

/// Windows' windows, by their handles as numbers.
#[cfg(windows)]
mod window {
    use windows_sys::Win32::System::DataExchange::GetClipboardSequenceNumber;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsWindow,
        SetForegroundWindow,
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

    /// A window's title; empty for none, or for a handle that isn't one.
    pub fn title(w: isize) -> String {
        // SAFETY: any handle may be asked about; the buffer is as long as
        // the length asked for, and one more for the terminating zero.
        let len = unsafe { GetWindowTextLengthW(w as _) };
        if len <= 0 {
            return String::new();
        }
        let mut buf = vec![0u16; len as usize + 1];
        let got = unsafe { GetWindowTextW(w as _, buf.as_mut_ptr(), buf.len() as i32) };
        String::from_utf16_lossy(&buf[..got.max(0) as usize])
    }

    pub fn bring(w: isize) {
        // SAFETY: as `exists`; Windows decides whether it may.
        unsafe { SetForegroundWindow(w as _) };
    }

    /// Whether any of these keys is held down now.
    pub fn any_down(keys: &[u16]) -> bool {
        // SAFETY: any virtual-key code may be asked about.
        keys.iter().any(|&k| (unsafe { GetAsyncKeyState(i32::from(k)) } as u16) & 0x8000 != 0)
    }

    /// The clipboard's count of changes: a copy moves it on.
    pub fn clipboard_count() -> u32 {
        // SAFETY: no arguments.
        unsafe { GetClipboardSequenceNumber() }
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
                        // Insert is on the extended keys, not the keypad's.
                        dwFlags: (if up { KEYEVENTF_KEYUP } else { 0 })
                            | (if key == super::VK_INSERT { KEYEVENTF_EXTENDEDKEY } else { 0 }),
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
    pub fn title(_: isize) -> String {
        String::new()
    }
    pub fn bring(_: isize) {}
    pub fn any_down(_: &[u16]) -> bool {
        false
    }
    pub fn clipboard_count() -> u32 {
        0
    }
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
    fn copying_for_the_user_presses_ctrl_insert_and_lets_every_key_go() {
        // Ctrl down, Insert down and up, Ctrl up: never C, an interrupt in a
        // terminal, and nothing left held.
        assert_eq!(COPY, [(0x11, false), (0x2D, false), (0x2D, true), (0x11, true)]);
        assert!(HELD.contains(&VK_SHIFT) && HELD.contains(&VK_CONTROL) && HELD.contains(&VK_MENU));
    }

    #[test]
    fn a_copy_that_brought_nothing_never_sends_what_was_there_before() {
        assert!(clipboard_is_the_message(false, false), "not asked: the clipboard, as before");
        assert!(clipboard_is_the_message(true, true));
        assert!(!clipboard_is_the_message(true, false), "asked, and nothing came: empty");
    }

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
        assert_eq!(ENTER, [(0x0D, false), (0x0D, true)], "Enter alone, without Shift");
    }

    #[test]
    fn a_window_still_shows_the_same_place_but_for_a_count_that_changed() {
        assert!(same_place("Ada Lovelace | Microsoft Teams", "Ada Lovelace | Microsoft Teams"));
        assert!(same_place("(2) WhatsApp", "WhatsApp"));
        assert!(same_place("Slack | #general | Acme", "Slack | #general | Acme [3]"));
        assert!(same_place("• Discord | @ada", "Discord | @ada"));
        assert!(!same_place("#general - Acme - Slack", "#random - Acme - Slack"));
        assert!(!same_place("Ada Lovelace | Messenger", "Charles Babbage | Messenger"));
        assert!(same_place("Notes (draft)", "Notes (draft)"), "words in brackets are kept");
        assert!(!same_place("Notes (draft)", "Notes"));
    }

    #[test]
    fn a_window_that_is_not_one_is_not_one() {
        assert!(!window::exists(0));
        assert_eq!(window::title(0), "", "and has no title");
    }
}
