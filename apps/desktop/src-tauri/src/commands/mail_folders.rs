//! Other folders a mailbox is read in (0.10.0-alpha.73): where the user's own
//! rules file mail away from the inbox. Listing them signs in to the mailbox
//! and opens nothing; choosing them is kept, and a check is started — even
//! for a mailbox whose last check failed, since signing in just worked. What
//! comes from them is marked and waits for the user (`autoreply::FILED`).

use mimic_core::sources::imap::{self, ImapSourceConfig};
use mimic_core::sources::transcript;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::State;

use crate::commands::sending::mailbox_config;
use crate::error::{CommandError, CommandResult};
use crate::SharedState;

/// One folder a mailbox may be read in as well.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MailboxFolder {
    /// As the server names it: what is kept and sent back.
    pub name: String,
    /// As people read it (`imap::folder_name`).
    pub shown: String,
    pub chosen: bool,
}

/// The folders offered, and those chosen that the server no longer has.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MailboxFolders {
    pub folders: Vec<MailboxFolder>,
    /// Chosen, and gone from the server, as people read them: not read, and
    /// forgotten when the choice is next saved.
    pub gone: Vec<String>,
    /// Chosen, listed, and not opened by the last check (`passed_over`), as
    /// people read them.
    pub unopened: Vec<String>,
    /// How many may be chosen (`imap::MOST_ALSO_READ`).
    pub most: usize,
}

impl MailboxFolders {
    fn of(config: &ImapSourceConfig, offered: &[String]) -> Self {
        MailboxFolders {
            folders: offered
                .iter()
                .map(|n| MailboxFolder {
                    name: n.clone(),
                    shown: imap::folder_name(n),
                    chosen: config.also_read.contains(n),
                })
                .collect(),
            gone: config.also_read.iter().filter(|n| !offered.contains(n)).map(|n| imap::folder_name(n)).collect(),
            unopened: config
                .passed_over
                .iter()
                .filter(|n| offered.contains(n) && config.also_read.contains(n))
                .map(|n| imap::folder_name(n))
                .collect(),
            most: imap::MOST_ALSO_READ,
        }
    }
}

/// Of `wanted`, those `offered`, in the server's order, each once — or
/// which one isn't, or that there are too many.
fn chosen_from(offered: &[String], wanted: &[String]) -> CommandResult<Vec<String>> {
    if let Some(n) = wanted.iter().find(|n| !offered.contains(n)) {
        return Err(CommandError::new(
            "not_offered",
            format!("{} isn't a folder I can read as well; look again.", imap::folder_name(n)),
        ));
    }
    let chosen: Vec<String> = offered.iter().filter(|n| wanted.contains(n)).cloned().collect();
    if chosen.len() > imap::MOST_ALSO_READ {
        return Err(CommandError::new(
            "invalid",
            format!("{} folders at most, so a check stays quick.", imap::MOST_ALSO_READ),
        ));
    }
    Ok(chosen)
}

/// The mailbox's folders as its server lists them now, with its own
/// credential; what the server said is kept if it fails, as a probe's is.
async fn offered(state: &SharedState, source_id: &str, config: ImapSourceConfig) -> CommandResult<Vec<String>> {
    let (credentials, id, db) = (state.mail_credentials.clone(), source_id.to_string(), state.db.clone());
    tauri::async_runtime::spawn_blocking(move || {
        let credential =
            credentials.credential(config.account.auth, &id).map_err(|why| CommandError::new("mailbox", why))?;
        transcript::kept_if_it_fails(&db, "probe_failed", &config.account.host.clone(), || {
            imap::other_folders(&config.account, &credential)
        })
        .map_err(|e| {
            if matches!(e, imap::ImapError::SignIn(_)) {
                credentials.forget(&id);
            }
            CommandError::new("mailbox", e.to_string())
        })
    })
    .await
    .map_err(|e| CommandError::new("internal", e.to_string()))?
}

/// The folders a mailbox may be read in as well, and which are chosen.
#[tauri::command]
pub async fn mailbox_folders(state: State<'_, SharedState>, source_id: String) -> CommandResult<MailboxFolders> {
    let config = mailbox_config(&state, &source_id)?;
    let offered = offered(&state, &source_id, config.clone()).await?;
    Ok(MailboxFolders::of(&config, &offered))
}

/// Read these folders as well from now on (none for none), each one the
/// server offers now; a check starts, unless one is already waiting.
#[tauri::command]
pub async fn set_mailbox_folders(
    state: State<'_, SharedState>,
    source_id: String,
    folders: Vec<String>,
) -> CommandResult<MailboxFolders> {
    let config = mailbox_config(&state, &source_id)?;
    let offered = offered(&state, &source_id, config).await?;
    let chosen = chosen_from(&offered, &folders)?;
    // Read again: a check may have written the settings while the server
    // was asked, and it keeps this choice when it writes them next.
    let mut config = mailbox_config(&state, &source_id)?;
    // One no longer chosen is forgotten, mark and all: chosen again, it is
    // read from its newest messages, not from where it was left.
    for dropped in config.also_read.iter().filter(|n| !chosen.contains(n)) {
        config.marks.remove(dropped);
    }
    config.passed_over.retain(|n| chosen.contains(n));
    config.also_read = chosen;
    state.db.set_source_config(&source_id, &serde_json::to_value(&config)?)?;
    check_soon(&state, &source_id)?;
    Ok(MailboxFolders::of(&config, &offered))
}

/// Check the mailbox now, unless a check is already waiting to run — a
/// failing mailbox too: signing in to it just worked.
fn check_soon(state: &crate::AppState, source_id: &str) -> CommandResult<()> {
    let queued =
        state.db.list_jobs(500, true)?.into_iter().any(|j| {
            j.kind == imap::JOB_KIND && j.status == "queued" && j.payload["sourceId"].as_str() == Some(source_id)
        });
    if !queued {
        state.jobs.enqueue(imap::JOB_KIND, json!({ "sourceId": source_id }))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(n: &[&str]) -> Vec<String> {
        n.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn only_folders_offered_are_kept_in_the_servers_order_each_once() {
        let offered = names(&["Receipts", "&AMk-tudes", "Archive"]);
        assert_eq!(
            chosen_from(&offered, &names(&["Archive", "Receipts", "Archive"])).unwrap(),
            names(&["Receipts", "Archive"])
        );
        assert_eq!(chosen_from(&offered, &[]).unwrap(), Vec::<String>::new());
        let err = chosen_from(&offered, &names(&["Drafts"])).unwrap_err();
        assert_eq!(err.code, "not_offered");
        let many: Vec<String> = (0..=imap::MOST_ALSO_READ).map(|i| format!("F{i}")).collect();
        assert_eq!(chosen_from(&many, &many).unwrap_err().code, "invalid");
    }

    #[test]
    fn what_the_folders_card_is_shown_matches_its_fixture() {
        let config: ImapSourceConfig = serde_json::from_value(serde_json::json!({
            "account": { "host": "imap.example.org", "port": 993, "username": "c@example.org", "security": "tls" },
            "alsoRead": ["Receipts", "Old rules", "&AMk-tudes"],
            "passedOver": ["Receipts"],
        }))
        .unwrap();
        let shown = MailboxFolders::of(&config, &names(&["Receipts", "&AMk-tudes"]));
        assert_eq!(shown.folders[1].shown, "\u{c9}tudes");
        assert_eq!((shown.folders[0].chosen, shown.folders[1].chosen), (true, true));
        assert_eq!(shown.gone, names(&["Old rules"]));
        assert_eq!(shown.unopened, names(&["Receipts"]));
        let json = serde_json::to_value(&shown).unwrap();
        crate::fixtures::check_fixture("mailbox_folders.json", &json);
    }
}
