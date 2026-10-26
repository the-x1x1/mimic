//! Watching connected mailboxes for mail as it arrives (IMAP `IDLE`), so a
//! message is read within moments rather than at the next scheduled check —
//! and a reply that holds for a check (`autoreply`) is not kept waiting.
//!
//! One thread per mailbox, each holding one connection that only waits:
//! `imap::watch` opens the inbox with `EXAMINE` and reads nothing; when the
//! server says mail came, the thread queues a check, which does the reading.
//! A mailbox whose server can't say (no `IDLE`) is left to the schedule, and
//! one whose watch fails is tried again after a pause that grows. Nothing
//! watches while checking is off in Settings, or a mailbox whose check
//! failed: that one waits for the user, as its checks do.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mimic_core::sources::imap::{self, ImapSourceConfig, Watched, Watching};

use crate::SharedState;

/// The first pause after a watch fails; it doubles to `LONGEST_PAUSE`.
const FIRST_PAUSE: Duration = Duration::from_secs(30);
const LONGEST_PAUSE: Duration = Duration::from_secs(10 * 60);

/// The mailboxes being watched, those the server is listening for now, and
/// those whose server can't be.
#[derive(Default)]
pub struct Watchers {
    running: Mutex<HashMap<String, Arc<AtomicBool>>>,
    listening: Mutex<HashSet<String>>,
    no_idle: Mutex<HashSet<String>>,
}

impl Watchers {
    /// How many mailboxes the server is listening for now: what the home
    /// screen may say arrives as it comes.
    pub fn listening(&self) -> usize {
        self.listening.lock().unwrap_or_else(|p| p.into_inner()).len()
    }

    fn heard(&self, source_id: &str, on: bool) {
        let mut listening = self.listening.lock().unwrap_or_else(|p| p.into_inner());
        if on {
            listening.insert(source_id.to_string());
        } else {
            listening.remove(source_id);
        }
    }
}

/// Which mailboxes should be watched: connected, readable, not failing,
/// and only while checking is on.
fn wanted(state: &SharedState) -> HashSet<String> {
    let on = imap::interval_minutes(&state.db).map(|m| m > 0).unwrap_or(false);
    if !on {
        return HashSet::new();
    }
    state
        .db
        .list_sources()
        .unwrap_or_default()
        .into_iter()
        .filter(|s| s.connector == imap::CONNECTOR && s.status != "failed")
        .filter(|s| serde_json::from_value::<ImapSourceConfig>(s.config.clone()).is_ok())
        .map(|s| s.id)
        .collect()
}

/// Start a watch for each mailbox that should have one, and stop those that
/// shouldn't. Called every half minute from start-up.
pub fn look(state: &SharedState) {
    let wanted = wanted(state);
    let no_idle = state.watchers.no_idle.lock().unwrap_or_else(|p| p.into_inner()).clone();
    let mut running = state.watchers.running.lock().unwrap_or_else(|p| p.into_inner());
    running.retain(|id, stop| {
        let keep = wanted.contains(id);
        if !keep {
            stop.store(true, Ordering::SeqCst);
        }
        keep
    });
    for id in wanted {
        if running.contains_key(&id) || no_idle.contains(&id) {
            continue;
        }
        let stop = Arc::new(AtomicBool::new(false));
        running.insert(id.clone(), stop.clone());
        let (st, source_id) = (state.clone(), id.clone());
        let spawned = std::thread::Builder::new().name(format!("watch-{id}")).spawn(move || run(st, source_id, stop));
        if let Err(e) = spawned {
            tracing::warn!(target: "mail", error = %e, "could not start watching a mailbox");
            running.remove(&id);
        }
    }
}

/// Wait `pause`, or less if told to stop. True when stopped.
fn rest(pause: Duration, stop: &AtomicBool) -> bool {
    let mut left = pause;
    while !left.is_zero() {
        if stop.load(Ordering::SeqCst) {
            return true;
        }
        let step = left.min(Duration::from_secs(1));
        std::thread::sleep(step);
        left -= step;
    }
    stop.load(Ordering::SeqCst)
}

fn run(state: SharedState, source_id: String, stop: Arc<AtomicBool>) {
    let mut pause = FIRST_PAUSE;
    while !stop.load(Ordering::SeqCst) {
        let Some(source) = state.db.get_source(&source_id).ok().flatten() else { break };
        let Ok(config) = serde_json::from_value::<ImapSourceConfig>(source.config) else { break };
        let credential = match state.mail_credentials.credential(config.account.auth, &source_id) {
            Ok(c) => c,
            // The check says why on the mailbox's row; this only waits.
            Err(_) => {
                if rest(pause, &stop) {
                    break;
                }
                pause = (pause * 2).min(LONGEST_PAUSE);
                continue;
            }
        };
        let folder = config.folders.first().cloned().unwrap_or_else(|| "INBOX".to_string());
        let should_stop = || stop.load(Ordering::SeqCst);
        let listening = |on: bool| state.watchers.heard(&source_id, on);
        let mut arrived = || crate::commands::autoreply::queue_check(&state, &source_id);
        let watched =
            imap::watch(&config.account, &folder, &credential, Watching::USUAL, &should_stop, &listening, &mut arrived);
        match watched {
            Ok(Watched::Stopped) => break,
            Ok(Watched::Renew) => pause = FIRST_PAUSE,
            Ok(Watched::NoIdle) => {
                tracing::info!(target: "mail", "a mailbox's server can't say when mail arrives; it is checked on the schedule");
                state.watchers.no_idle.lock().unwrap_or_else(|p| p.into_inner()).insert(source_id.clone());
                break;
            }
            Err(e) => {
                if matches!(e, imap::ImapError::SignIn(_)) {
                    // A token the server turned down: the next try gets a new one.
                    state.mail_credentials.forget(&source_id);
                }
                tracing::warn!(target: "mail", error = %e, "watching a mailbox stopped; trying again later");
                if rest(pause, &stop) {
                    break;
                }
                pause = (pause * 2).min(LONGEST_PAUSE);
            }
        }
    }
    // Gone from the list, unless a newer watch for this mailbox took its place.
    let mut running = state.watchers.running.lock().unwrap_or_else(|p| p.into_inner());
    if running.get(&source_id).is_some_and(|s| Arc::ptr_eq(s, &stop)) {
        running.remove(&source_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rest_ends_at_once_when_stopped_and_otherwise_lasts() {
        let stop = AtomicBool::new(true);
        let started = std::time::Instant::now();
        assert!(rest(Duration::from_secs(30), &stop));
        assert!(started.elapsed() < Duration::from_millis(100));
        let go_on = AtomicBool::new(false);
        assert!(!rest(Duration::from_millis(20), &go_on));
    }
}
