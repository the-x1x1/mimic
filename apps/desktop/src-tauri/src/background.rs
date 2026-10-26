//! Mimic in the background (0.10.0-alpha.28): kept running in the
//! notification area when its window is closed, started with Windows when
//! the user asks, and telling the user with a Windows notification when
//! there is something to look at — replies ready, one going or gone by
//! itself, one that didn't go. Replying by itself and reading mail as it
//! arrives only happen while Mimic runs; this is what keeps it running.
//!
//! A notification says how many and where to look, never who wrote or what
//! a message says: Windows keeps notifications where anyone at the computer
//! can read them.

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{AppHandle, Manager};

use crate::SharedState;

/// Closing the window keeps Mimic running in the notification area. On
/// unless the user turns it off.
pub const KEEP_RUNNING: &str = "app.keepRunning";
/// Windows notifications. On unless the user turns them off.
pub const NOTIFY: &str = "notify.enabled";
/// The user has been told once that closing the window leaves Mimic running.
const TOLD: &str = "app.toldAboutBackground";
/// What Windows starts Mimic with at sign-in: no window, only the icon in
/// the notification area.
pub const AT_SIGN_IN: &str = "--background";

/// The icon is in the notification area: without it, closing the window
/// quits, or Mimic would run on with no way back to it.
static HAS_ICON: AtomicBool = AtomicBool::new(false);

fn on(state: &SharedState, key: &str) -> bool {
    state.db.get_setting::<bool>(key).ok().flatten().unwrap_or(true)
}

/// Tell the user something — unless they turned notifications off, or are
/// looking at Mimic now, where the screen already says it.
pub fn tell(app: &AppHandle, title: &str, body: &str) {
    let Some(state) = app.try_state::<SharedState>() else { return };
    if !on(state.inner(), NOTIFY) {
        return;
    }
    if let Some(window) = app.get_webview_window("main") {
        let looking = window.is_visible().unwrap_or(false)
            && window.is_focused().unwrap_or(false)
            && !window.is_minimized().unwrap_or(false);
        if looking {
            return;
        }
    }
    use tauri_plugin_notification::NotificationExt;
    if let Err(e) = app.notification().builder().title(title).body(body).show() {
        tracing::warn!(target: "app", error = %e, "could not show a notification");
    }
}

/// The main window's close button hides it while Mimic keeps running, and
/// the first time says where Mimic went. With keeping running off, it closes
/// and Mimic quits, as before.
pub fn hide_on_close(window: &tauri::WebviewWindow) {
    let w = window.clone();
    window.on_window_event(move |event| {
        let tauri::WindowEvent::CloseRequested { api, .. } = event else { return };
        let app = w.app_handle();
        let Some(state) = app.try_state::<SharedState>() else { return };
        if !HAS_ICON.load(Ordering::SeqCst) || !on(state.inner(), KEEP_RUNNING) {
            return;
        }
        api.prevent_close();
        let _ = w.hide();
        let told = state.db.get_setting::<bool>(TOLD).ok().flatten().unwrap_or(false);
        if !told {
            let _ = state.db.set_setting(TOLD, &true);
            tell(
                app,
                "Mimic is still running",
                "I'm in the notification area, still checking your mail. To quit, right-click my icon there.",
            );
        }
    });
}

/// The icon in the notification area: a click opens Mimic; its menu opens
/// or quits it.
pub fn tray(app: &AppHandle) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
    let open = MenuItem::with_id(app, "open", "Open Mimic", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Mimic", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("Mimic")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => crate::bring_to_front(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                crate::bring_to_front(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    HAS_ICON.store(true, Ordering::SeqCst);
    Ok(())
}

fn replies(n: usize) -> String {
    if n == 1 {
        "a reply".to_string()
    } else {
        format!("{n} replies")
    }
}

/// Replies prepared in advance, ready to look over.
pub fn replies_ready(drafted: usize) -> Option<(String, String)> {
    (drafted > 0).then(|| {
        let where_ = if drafted == 1 { "It's" } else { "They're" };
        (
            "Replies ready".to_string(),
            format!("I've written {}. {where_} on the home screen for you to look over and send.", replies(drafted)),
        )
    })
}

/// What replying by itself just did, as notifications: held, sent, failed.
pub fn replied_by_itself(run: &mimic_core::autoreply::Run, hold_minutes: i64) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if run.held > 0 {
        let (it, them) = if run.held == 1 { ("it", "itself") } else { ("them", "themselves") };
        let mut body = replies(run.held);
        body[..1].make_ascii_uppercase();
        out.push((
            "Going by itself soon".to_string(),
            format!("{body} will go by {them} in {hold_minutes} minutes. You can stop {it} on the home screen."),
        ));
    }
    if run.sent > 0 {
        out.push((
            "Sent by myself".to_string(),
            format!("I sent {} for you. What I've sent is under Your mail.", replies(run.sent)),
        ));
    }
    if run.failed > 0 {
        let what =
            if run.failed == 1 { "A reply I meant to send by myself" } else { "Replies I meant to send by myself" };
        out.push((
            "A reply didn't go".to_string(),
            format!("{what} didn't go. What I've sent, under Your mail, says why."),
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notifications_say_how_many_and_where_to_look() {
        assert_eq!(replies_ready(0), None);
        assert_eq!(
            replies_ready(1).unwrap().1,
            "I've written a reply. It's on the home screen for you to look over and send."
        );
        assert_eq!(
            replies_ready(3).unwrap().1,
            "I've written 3 replies. They're on the home screen for you to look over and send."
        );
        let run = mimic_core::autoreply::Run { held: 1, sent: 2, stopped: 1, failed: 1, ..Default::default() };
        let said = replied_by_itself(&run, 10);
        assert_eq!(
            said.iter().map(|(_, b)| b.as_str()).collect::<Vec<_>>(),
            vec![
                "A reply will go by itself in 10 minutes. You can stop it on the home screen.",
                "I sent 2 replies for you. What I've sent is under Your mail.",
                "A reply I meant to send by myself didn't go. What I've sent, under Your mail, says why.",
            ]
        );
        let held = replied_by_itself(&mimic_core::autoreply::Run { held: 2, ..Default::default() }, 5);
        assert_eq!(held[0].1, "2 replies will go by themselves in 5 minutes. You can stop them on the home screen.");
        assert!(replied_by_itself(&mimic_core::autoreply::Run { stopped: 3, ..Default::default() }, 10).is_empty());
    }
}
