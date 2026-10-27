//! The real mail servers of the common providers, reached as Mimic reaches
//! them and signed in to none (0.10.0-alpha.88). Ignored unless asked for,
//! since it goes out over the internet:
//! `cargo test -p mimic-core --test live_servers -- --ignored --nocapture`.
//!
//! Each reading server is connected to over TLS, greeted, asked what it can
//! do and left (`imap::reach`); each sending server, the one `smtp::known_for`
//! pairs with it, is connected to, encrypted as Mimic would (TLS, or
//! STARTTLS), greeted and left (`smtp::reach`). No username, password or
//! token is sent, and nothing is read or sent.

use mimic_core::sources::imap::{self, Auth, ImapAccount, Security};
use mimic_core::sources::smtp;

/// Reading servers, each with an address that picks its sending server.
const PROVIDERS: [(&str, &str); 9] = [
    ("imap.gmail.com", "someone@gmail.com"),
    ("outlook.office365.com", "someone@outlook.com"),
    ("outlook.office365.com", "someone@contoso.example"),
    ("imap.mail.yahoo.com", "someone@yahoo.com"),
    ("imap.mail.me.com", "someone@icloud.com"),
    ("imap.aol.com", "someone@aol.com"),
    ("imap.fastmail.com", "someone@fastmail.com"),
    ("imap.zoho.com", "someone@zoho.com"),
    ("imap.gmx.com", "someone@gmx.com"),
];

#[test]
#[ignore = "reaches the providers' real servers over the internet"]
fn the_common_providers_servers_are_reached_as_mimic_reaches_them() {
    let mut failed = Vec::new();
    for (host, username) in PROVIDERS {
        let account = ImapAccount {
            host: host.into(),
            port: 993,
            username: username.into(),
            security: Security::Tls,
            auth: Auth::Password,
        };
        match imap::reach(&account) {
            Ok(can) => {
                println!("{host} (reading): {}", can.join(" "));
                if !can.iter().any(|c| c.starts_with("IMAP4REV")) {
                    failed.push(format!("{host} didn't say it speaks IMAP4: {can:?}"));
                }
            }
            Err(e) => failed.push(format!("{host} (reading): {e}")),
        }
        let Some(sending) = smtp::known_for(host, username) else {
            failed.push(format!("no sending server known for {host}"));
            continue;
        };
        let label = format!("{}:{} {:?}", sending.host, sending.port, sending.security);
        match smtp::reach(&sending) {
            Ok(r) => {
                println!("  {label} (sending, for {username}): sign in with {}", r.sign_in.join(" "));
                if !r.encrypted || r.sign_in.is_empty() {
                    failed.push(format!("{label}: {r:?}"));
                }
            }
            Err(e) => failed.push(format!("{label} (sending): {e}")),
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}
