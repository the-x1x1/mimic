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

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

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

/// How Mimic gets out of the way after a reply is copied from it
/// (0.10.0-alpha.35): hidden while its icon is in the notification area, to
/// come back from there or by the shortcut; otherwise minimised, since a
/// hidden window with no icon couldn't be found again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Aside {
    Hidden,
    Minimized,
}

pub fn aside(has_icon: bool) -> Aside {
    if has_icon {
        Aside::Hidden
    } else {
        Aside::Minimized
    }
}

/// Step aside, so the window the user came from is in front again and what
/// they copied can be pasted there.
pub fn step_aside(app: &AppHandle) -> tauri::Result<Aside> {
    let how = aside(HAS_ICON.load(Ordering::SeqCst));
    if let Some(window) = app.get_webview_window("main") {
        match how {
            Aside::Hidden => window.hide()?,
            Aside::Minimized => window.minimize()?,
        }
    }
    Ok(how)
}

/// What the icon says of replying by itself (0.10.0-alpha.47): how it
/// stands, and the menu item that stops it, while it runs (on, with
/// replies prepared in advance); nothing otherwise.
pub fn tray_says(status: &mimic_core::autoreply::Status) -> Option<(&'static str, &'static str)> {
    if !status.running {
        None
    } else if status.rules.rehearse {
        Some(("Rehearsing replies by myself", "Turn replying by myself off"))
    } else {
        Some(("Replying by myself", "Turn replying by myself off"))
    }
}

/// The icon's menu for the rules as they are: Open Mimic; while replying by
/// itself is on, how it stands and one item to stop it; Quit Mimic.
fn tray_menu(app: &AppHandle, says: Option<(&str, &str)>) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    use tauri::menu::{IsMenuItem, Menu, MenuItem, PredefinedMenuItem};
    let open = MenuItem::with_id(app, "open", "Open Mimic", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Mimic", true, None::<&str>)?;
    let gap = PredefinedMenuItem::separator(app)?;
    let replying = match says {
        Some((stands, stop)) => Some((
            MenuItem::with_id(app, "replying", stands, false, None::<&str>)?,
            MenuItem::with_id(app, "stop_replying", stop, true, None::<&str>)?,
        )),
        None => None,
    };
    let mut items: Vec<&dyn IsMenuItem<tauri::Wry>> = vec![&open];
    if let Some((stands, stop)) = &replying {
        items.extend([&gap as &dyn IsMenuItem<tauri::Wry>, stands, stop]);
    }
    items.push(&quit);
    Menu::with_items(app, &items)
}

/// How replying by itself stands, as the icon says it.
fn tray_status(app: &AppHandle) -> Option<(&'static str, &'static str)> {
    let state = app.try_state::<SharedState>()?;
    let status = mimic_core::autoreply::status(&state.db).ok()?;
    tray_says(&status)
}

/// Make the icon's menu and its tooltip say how replying by itself stands:
/// after the rules, or preparing replies in advance, change.
pub fn refresh_tray(app: &AppHandle) {
    let Some(tray) = app.tray_by_id("main") else { return };
    let says = tray_status(app);
    let tip = match says {
        Some((stands, _)) => format!("Mimic: {}", stands.to_lowercase()),
        None => "Mimic".to_string(),
    };
    match tray_menu(app, says) {
        Ok(menu) => {
            let _ = tray.set_menu(Some(menu));
            let _ = tray.set_tooltip(Some(tip));
        }
        Err(e) => tracing::warn!(target: "app", error = %e, "could not change the icon's menu"),
    }
}

/// Stop replying by itself from the icon: off, as Settings turns it off,
/// stopping every reply still held.
fn stop_replying(app: &AppHandle) {
    let Some(state) = app.try_state::<SharedState>() else { return };
    let off = mimic_core::autoreply::rules(&state.db)
        .map(|r| mimic_core::autoreply::Rules { enabled: false, ..r })
        .and_then(|r| mimic_core::autoreply::set_rules(&state.db, r));
    if let Err(e) = off {
        tracing::warn!(target: "autoreply", error = %e, "could not turn replying by itself off from the icon");
        return;
    }
    let _ = tauri::Emitter::emit(app, "sending://changed", serde_json::json!({}));
    refresh_tray(app);
}

/// The icon in the notification area: a click opens Mimic; its menu opens
/// or quits it, and stops replying by itself while that is on.
pub fn tray(app: &AppHandle) -> tauri::Result<()> {
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
    let menu = tray_menu(app, tray_status(app))?;
    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("Mimic")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => crate::bring_to_front(app),
            "stop_replying" => stop_replying(app),
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
    refresh_tray(app);
    Ok(())
}

/// Mailboxes whose last check failed and the user has been told so, this
/// run. Told once when one starts failing, and again only after it has
/// worked in between.
static FAILING: Mutex<Option<HashSet<String>>> = Mutex::new(None);

/// A mailbox check ended: if it failed and hadn't been failing, say so —
/// replying by itself, reading mail as it arrives and everything else wait
/// on it working, and nobody may be looking at the screen that says it.
pub fn mailbox_checked(app: &AppHandle, job_id: &str, failed: bool) {
    let Some(state) = app.try_state::<SharedState>() else { return };
    let Some(job) = state.db.get_job(job_id).ok().flatten() else { return };
    let Some(source_id) = job.payload["sourceId"].as_str().map(str::to_string) else { return };
    let newly = {
        let mut failing = FAILING.lock().unwrap_or_else(|p| p.into_inner());
        let failing = failing.get_or_insert_with(HashSet::new);
        if failed {
            failing.insert(source_id.clone())
        } else {
            failing.remove(&source_id);
            false
        }
    };
    if !newly {
        return;
    }
    let Some(source) = state.db.get_source(&source_id).ok().flatten() else { return };
    let why = source.last_error.as_ref().and_then(|e| e["message"].as_str()).unwrap_or_default();
    let (title, body) = mailbox_failing(&source.name, why);
    tell(app, &title, &body);
}

/// What a mailbox that stopped working is said as: its own address (the
/// user's, not anyone else's) and why, on one short line.
pub fn mailbox_failing(address: &str, why: &str) -> (String, String) {
    let why: String = why.lines().next().unwrap_or_default().trim().chars().take(200).collect();
    let why = if why.is_empty() { "the check didn't work.".to_string() } else { why };
    (
        "I can't check your mail".to_string(),
        format!("{address}: {why} Your mail, in Mimic, says what to do. Nothing goes out from it until it works."),
    )
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

/// What replying by itself just did, as notifications: held, sent, failed,
/// and sent but perhaps not received.
pub fn replied_by_itself(
    run: &mimic_core::autoreply::Run,
    rules: &mimic_core::autoreply::Rules,
) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if run.held > 0 {
        let (it, them) = if run.held == 1 { ("it", "itself") } else { ("them", "themselves") };
        let mut body = replies(run.held);
        body[..1].make_ascii_uppercase();
        let hold = rules.hold_minutes;
        // With hours set, the hold may end outside them and the reply wait
        // for them to begin (`autoreply::first_within`).
        let when = match rules.hours {
            Some(_) => format!("after at least {hold} minutes, within the hours you chose"),
            None => format!("in {hold} minutes"),
        };
        if rules.rehearse {
            let (is, place) = if run.held == 1 { ("is", "It's") } else { ("are", "They're") };
            out.push((
                "Rehearsing".to_string(),
                format!(
                    "{body} {is} held as if to go by {them} {when}. Nothing will be sent. {place} on the home screen."
                ),
            ));
        } else {
            out.push((
                "Going by itself soon".to_string(),
                format!("{body} will go by {them} {when}. You can stop {it} on the home screen."),
            ));
        }
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
    if run.rehearsed > 0 {
        let wait = if run.rehearsed == 1 { "It waits" } else { "They wait" };
        out.push((
            "Rehearsing".to_string(),
            format!(
                "I'd have sent {} by myself just now; rehearsing, I sent nothing. {wait} for you on the home screen.",
                replies(run.rehearsed)
            ),
        ));
    }
    if run.unsure > 0 {
        let what = if run.unsure == 1 { "A reply I sent by myself" } else { "Replies I sent by myself" };
        out.push((
            "A reply may have gone".to_string(),
            format!(
                "{what} may or may not have gone: the connection was lost as it went. Look in your Sent folder; What I've sent, under Your mail, asks what you found."
            ),
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_icon_says_replying_by_itself_only_while_it_runs() {
        use mimic_core::autoreply::{Rules, Status};
        let on = Rules { enabled: true, ..Rules::default() };
        let status = |rules: &Rules, running: bool| Status { rules: rules.clone(), running, last_day: 0 };
        assert_eq!(tray_says(&status(&Rules::default(), false)), None);
        assert_eq!(tray_says(&status(&on, false)), None, "on, but nothing prepared to send");
        assert_eq!(tray_says(&status(&on, true)), Some(("Replying by myself", "Turn replying by myself off")));
        assert_eq!(
            tray_says(&status(&Rules { rehearse: true, ..on.clone() }, true)),
            Some(("Rehearsing replies by myself", "Turn replying by myself off"))
        );
    }

    #[test]
    fn stepping_aside_hides_only_where_the_icon_can_bring_it_back() {
        assert_eq!(aside(true), Aside::Hidden);
        assert_eq!(aside(false), Aside::Minimized);
    }

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
        use mimic_core::autoreply::{Hours, Rules, Run};
        let ten = Rules { hold_minutes: 10, ..Rules::default() };
        let run = Run { held: 1, sent: 2, stopped: 1, failed: 1, unsure: 1, ..Default::default() };
        let said = replied_by_itself(&run, &ten);
        assert_eq!(
            said.iter().map(|(_, b)| b.as_str()).collect::<Vec<_>>(),
            vec![
                "A reply will go by itself in 10 minutes. You can stop it on the home screen.",
                "I sent 2 replies for you. What I've sent is under Your mail.",
                "A reply I meant to send by myself didn't go. What I've sent, under Your mail, says why.",
                "A reply I sent by myself may or may not have gone: the connection was lost as it went. Look in your Sent folder; What I've sent, under Your mail, asks what you found.",
            ]
        );
        let five = Rules { hold_minutes: 5, ..Rules::default() };
        let held = replied_by_itself(&Run { held: 2, ..Default::default() }, &five);
        assert_eq!(held[0].1, "2 replies will go by themselves in 5 minutes. You can stop them on the home screen.");
        let in_hours = Rules { hours: Some(Hours { from: 9, to: 17 }), ..five };
        assert_eq!(
            replied_by_itself(&Run { held: 1, ..Default::default() }, &in_hours)[0].1,
            "A reply will go by itself after at least 5 minutes, within the hours you chose. You can stop it on the home screen."
        );
        assert!(replied_by_itself(&Run { stopped: 3, ..Default::default() }, &ten).is_empty());
        let rehearsing = Rules { rehearse: true, ..Rules { hold_minutes: 10, ..Rules::default() } };
        assert_eq!(
            replied_by_itself(&Run { held: 1, ..Default::default() }, &rehearsing)[0].1,
            "A reply is held as if to go by itself in 10 minutes. Nothing will be sent. It's on the home screen."
        );
        assert_eq!(
            replied_by_itself(&Run { rehearsed: 2, ..Default::default() }, &ten)[0].1,
            "I'd have sent 2 replies by myself just now; rehearsing, I sent nothing. They wait for you on the home screen."
        );
        assert_eq!(
            replied_by_itself(&Run { rehearsed: 1, ..Default::default() }, &ten)[0].1,
            "I'd have sent a reply by myself just now; rehearsing, I sent nothing. It waits for you on the home screen."
        );
    }

    #[test]
    fn a_mailbox_that_stops_working_is_named_with_why_on_one_line() {
        let (title, body) = mailbox_failing("c@example.com", "Microsoft wants you to sign in again.\nTrace: 123");
        assert_eq!(title, "I can't check your mail");
        assert_eq!(
            body,
            "c@example.com: Microsoft wants you to sign in again. Your mail, in Mimic, says what to do. Nothing goes out from it until it works."
        );
        assert!(mailbox_failing("c@example.com", "").1.contains("the check didn't work."));
        assert!(mailbox_failing("c@example.com", &"x".repeat(500)).1.len() < 320);
    }
}
