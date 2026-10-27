//! Checking a mailbox's servers before it is connected (0.10.0-alpha.89):
//! the reading server, and the sending server Mimic would pair with it, are
//! reached as a check and a send reach them — encrypted the same way — asked
//! what they can do, and left. No password or token is asked for or sent, so
//! a mistyped server name or port shows before anything secret is typed.

use mimic_core::sources::imap::{self, Auth, ImapAccount, Security};
use mimic_core::sources::smtp::{self, SmtpReach};
use serde::{Deserialize, Serialize};

use crate::error::{CommandError, CommandResult};

/// One server, and what reaching it found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerReached {
    pub host: String,
    pub port: u16,
    pub reached: bool,
    /// What it said, or why it couldn't be reached, in a sentence.
    pub said: String,
}

/// The reading server, and the sending server when one is known for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MailServers {
    pub reading: ServerReached,
    pub sending: Option<ServerReached>,
}

/// What a reading server's capabilities say, for a person. Microsoft's says
/// `LOGINDISABLED` and offers only `XOAUTH2`: no password will do there
/// (seen on outlook.office365.com, job 317). iCloud's doesn't offer `IDLE`.
fn reading_said(can: &[String]) -> String {
    let imap = can.iter().any(|c| c.starts_with("IMAP4REV"));
    let idle = can.iter().any(|c| c == "IDLE");
    let no_password =
        can.iter().any(|c| c == "LOGINDISABLED") && !can.iter().any(|c| c == "AUTH=PLAIN" || c == "AUTH=LOGIN");
    if !imap {
        return "It answered, encrypted, but didn't say it speaks IMAP, which I read mail with.".into();
    }
    let arrives = if idle {
        "and tells me as mail arrives (IDLE)"
    } else {
        "but doesn't say when mail arrives, so I'll look on a schedule"
    };
    if no_password {
        format!("It answered, encrypted, {arrives}. It takes no password: sign in with Microsoft or Google instead.")
    } else {
        format!("It answered, encrypted, {arrives}.")
    }
}

/// What a sending server says, for a person.
fn sending_said(r: &SmtpReach) -> String {
    if r.sign_in.is_empty() {
        return "It answered, encrypted, but offered no way to sign in, so I couldn't send through it.".into();
    }
    format!("It answered, encrypted, and takes a sign-in ({}).", r.sign_in.join(", "))
}

fn reached(host: &str, port: u16, outcome: Result<String, String>) -> ServerReached {
    let (reached, said) = match outcome {
        Ok(said) => (true, said),
        Err(why) => (false, why),
    };
    ServerReached { host: host.to_string(), port, reached, said }
}

/// Reach the reading server at `host:port` over TLS, as connecting the
/// mailbox would, and the sending server `smtp::known_for` pairs with it for
/// `address`. Signs in to neither.
#[tauri::command]
pub async fn reach_mail_servers(host: String, port: u16, address: String) -> CommandResult<MailServers> {
    let host = host.trim().to_string();
    if host.is_empty() || port == 0 {
        return Err(CommandError::new("invalid", "Say which server, and its port, to check."));
    }
    tauri::async_runtime::spawn_blocking(move || {
        let account = ImapAccount {
            host: host.clone(),
            port,
            username: address.trim().to_string(),
            security: Security::Tls,
            auth: Auth::Password,
        };
        let reading = reached(&host, port, imap::reach(&account).map(|c| reading_said(&c)).map_err(|e| e.to_string()));
        let sending = smtp::known_for(&host, &account.username)
            .map(|s| reached(&s.host, s.port, smtp::reach(&s).map(|r| sending_said(&r)).map_err(|e| e.to_string())));
        MailServers { reading, sending }
    })
    .await
    .map_err(|e| CommandError::new("internal", e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_reaching_found_is_said_plainly() {
        let can = |c: &[&str]| c.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            reading_said(&can(&["IMAP4REV1", "IDLE", "AUTH=PLAIN"])),
            "It answered, encrypted, and tells me as mail arrives (IDLE)."
        );
        // iCloud's, as it answered (job 317).
        assert!(
            reading_said(&can(&["XAPPLEPUSHSERVICE", "IMAP4", "IMAP4REV1", "AUTH=PLAIN"])).contains("on a schedule")
        );
        assert!(reading_said(&can(&["POP3"])).contains("didn't say it speaks IMAP"));
        // Microsoft's, as it answered (job 317).
        let microsoft = can(&["IMAP4", "IMAP4REV1", "AUTH=XOAUTH2", "LOGINDISABLED", "IDLE"]);
        assert!(reading_said(&microsoft).ends_with("It takes no password: sign in with Microsoft or Google instead."));
        let r = SmtpReach { encrypted: true, sign_in: vec!["PLAIN".into(), "XOAUTH2".into()], largest: None };
        assert_eq!(sending_said(&r), "It answered, encrypted, and takes a sign-in (PLAIN, XOAUTH2).");
        assert!(sending_said(&SmtpReach { sign_in: vec![], ..r }).contains("no way to sign in"));
    }

    #[test]
    fn nothing_is_reached_without_a_server_to_reach() {
        let err =
            tauri::async_runtime::block_on(reach_mail_servers(" ".into(), 993, "c@example.com".into())).unwrap_err();
        assert_eq!(err.code, "invalid");
    }

    #[test]
    fn what_the_check_shows_matches_its_fixture() {
        let shown = MailServers {
            reading: reached("imap.example.org", 993, Ok(reading_said(&["IMAP4REV1".into(), "IDLE".into()]))),
            sending: Some(reached(
                "smtp.example.org",
                465,
                Err("Couldn't reach smtp.example.org:465 to send. Check the server name and port, and that you're online."
                    .into()),
            )),
        };
        assert!(shown.reading.reached && !shown.sending.as_ref().unwrap().reached);
        crate::fixtures::check_fixture("mail_servers.json", &serde_json::to_value(&shown).unwrap());
    }
}
