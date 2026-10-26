//! What Mimic and a mail server said to each other (0.10.0-alpha.38), kept
//! so that a check, a sign-in or a send that fails against a real server can
//! be understood from what happened rather than guessed at.
//!
//! Only while something is `recording` on this thread; otherwise every note
//! is dropped before it is written out. What is kept:
//!
//! * **Mimic's side**: each command as sent, except that anything secret is
//!   replaced by what it was — `LOGIN [the password]`, `AUTH PLAIN [the
//!   password]`, `AUTHENTICATE XOAUTH2 [the sign-in]` — and a message being
//!   handed over or appended is `[a message, N bytes]`.
//! * **The server's side**: each line as received, with a literal (where a
//!   message's text travels) as `{#n}` — never what is in it.
//!
//! So it names servers, folders, addresses and counts, and never a message's
//! words, a password or a token. Each line is one line of at most `WIDTH`
//! characters; the first `HEAD` lines and the last `TAIL` are kept, and how
//! many were left out between them is said.

use std::cell::RefCell;
use std::collections::VecDeque;

const HEAD: usize = 40;
const TAIL: usize = 160;
const WIDTH: usize = 200;

#[derive(Debug, Default)]
struct Transcript {
    head: Vec<String>,
    tail: VecDeque<String>,
    skipped: usize,
}

impl Transcript {
    fn push(&mut self, line: String) {
        if self.head.len() < HEAD {
            self.head.push(line);
            return;
        }
        if self.tail.len() == TAIL {
            self.tail.pop_front();
            self.skipped += 1;
        }
        self.tail.push_back(line);
    }

    fn lines(self) -> Vec<String> {
        let mut out = self.head;
        if self.skipped > 0 {
            out.push(format!("… {} more lines …", self.skipped));
        }
        out.extend(self.tail);
        out
    }
}

thread_local! {
    static CURRENT: RefCell<Option<Transcript>> = const { RefCell::new(None) };
}

/// Run `f`, keeping what is said to mail servers on this thread meanwhile;
/// what it returned, and those lines. A recording inside another is kept
/// apart from it, and the outer one goes on after.
pub fn recording<T>(f: impl FnOnce() -> T) -> (T, Vec<String>) {
    /// Puts the outer recording back however `f` ends, a panic included.
    struct Outer(Option<Option<Transcript>>);
    impl Drop for Outer {
        fn drop(&mut self) {
            if let Some(outer) = self.0.take() {
                CURRENT.with(|c| *c.borrow_mut() = outer);
            }
        }
    }
    let mut outer = Outer(Some(CURRENT.with(|c| c.replace(Some(Transcript::default())))));
    let out = f();
    let kept = CURRENT.with(|c| c.replace(outer.0.take().unwrap_or_default())).unwrap_or_default();
    (out, kept.lines())
}

/// Run `f`; when it fails, keep what was said in the event log as `what`
/// (`warn`, category `mail`), with the server it was said to, where the
/// diagnostics bundle finds it. Nothing is kept when it works.
pub fn kept_if_it_fails<T, E>(
    db: &crate::db::Db,
    what: &str,
    server: &str,
    f: impl FnOnce() -> Result<T, E>,
) -> Result<T, E> {
    let (out, said) = recording(f);
    if out.is_err() {
        let _ = db.log_event(&crate::db::NewEvent::warn(
            "mail",
            what,
            serde_json::json!({ "server": server, "transcript": said }),
        ));
    }
    out
}

fn is_recording() -> bool {
    CURRENT.with(|c| c.borrow().is_some())
}

/// One line said, by `who` (`"imap >"` for Mimic to an IMAP server, `"smtp <"`
/// for an SMTP server to Mimic): made one printable line, cut to `WIDTH`.
pub(crate) fn note(who: &str, said: &str) {
    if !is_recording() {
        return;
    }
    let mut line = format!("{who} ");
    for c in said.chars() {
        if line.chars().count() >= WIDTH {
            line.push('…');
            break;
        }
        line.push(if c.is_control() { ' ' } else { c });
    }
    CURRENT.with(|c| {
        if let Some(t) = c.borrow_mut().as_mut() {
            t.push(line.trim_end().to_string());
        }
    });
}

/// An IMAP command as it may be kept: a sign-in's secret left out.
pub(crate) fn imap_command(cmd: &str) -> String {
    let upper = cmd.trim_start().to_ascii_uppercase();
    if upper.starts_with("LOGIN") {
        "LOGIN [the username and password]".to_string()
    } else if let Some(rest) = upper.strip_prefix("AUTHENTICATE ") {
        let mechanism = rest.split_whitespace().next().unwrap_or_default();
        format!("AUTHENTICATE {mechanism} [the sign-in]")
    } else {
        cmd.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_is_kept_unless_recording_and_a_recording_keeps_its_own() {
        note("imap >", "m1 CAPABILITY");
        let ((), outer) = recording(|| {
            note("imap >", "m1 CAPABILITY");
            let ((), inner) = recording(|| note("smtp >", "EHLO [127.0.0.1]"));
            assert_eq!(inner, vec!["smtp > EHLO [127.0.0.1]"]);
            note("imap <", "m1 OK done");
        });
        assert_eq!(outer, vec!["imap > m1 CAPABILITY", "imap < m1 OK done"]);
        let ((), after) = recording(|| ());
        assert!(after.is_empty());
    }

    #[test]
    fn a_recording_that_panics_leaves_none_running() {
        fn boom() {
            panic!("in the middle");
        }
        let caught = std::panic::catch_unwind(|| recording(boom));
        assert!(caught.is_err());
        assert!(!is_recording());
    }

    #[test]
    fn a_long_talk_keeps_its_start_and_its_end_and_says_how_much_is_left_out() {
        let ((), lines) = recording(|| {
            for i in 0..1000 {
                note("imap <", &format!("* {i} EXISTS"));
            }
        });
        assert_eq!(lines.len(), HEAD + 1 + TAIL);
        assert_eq!(lines[0], "imap < * 0 EXISTS");
        assert_eq!(lines[HEAD], format!("… {} more lines …", 1000 - HEAD - TAIL));
        assert_eq!(lines.last().unwrap(), "imap < * 999 EXISTS");
    }

    #[test]
    fn each_line_is_one_short_printable_line() {
        let ((), lines) = recording(|| note("smtp <", &format!("250 ok\r\n{}\tend", "x".repeat(500))));
        assert_eq!(lines.len(), 1);
        assert!(!lines[0].contains(['\r', '\n', '\t']));
        assert!(lines[0].chars().count() <= WIDTH + 1);
        assert!(lines[0].ends_with('…'));
    }

    #[test]
    fn a_sign_in_is_kept_without_its_secret() {
        assert_eq!(imap_command("LOGIN \"c@example.com\" \"hunter2\""), "LOGIN [the username and password]");
        assert_eq!(imap_command("AUTHENTICATE XOAUTH2 dXNlcj1j"), "AUTHENTICATE XOAUTH2 [the sign-in]");
        assert_eq!(imap_command("EXAMINE \"INBOX\""), "EXAMINE \"INBOX\"");
    }
}
