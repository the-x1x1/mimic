//! A reply, written out as an email.
//!
//! What goes out is plain text in UTF-8, quoted-printable, with the headers
//! every mail program threads by: `In-Reply-To` naming the message it
//! answers and `References` naming the thread, so it lands under the message
//! it answers in the recipient's mail as it does in the user's. Names and
//! subjects outside ASCII are RFC 2047 encoded words; header lines are folded
//! before 78 characters. Nothing in a header can carry a line break of its
//! own: names and subjects have theirs turned into spaces, and an address or
//! a message id with one is not an address or a message id.
//!
//! `mbox::parse_raw` reads what this writes back to the same subject, body
//! and ids — the tests check — which is how a reply Mimic sent is recorded as
//! the user's own message in the conversation it answers.

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::oauth::base64_standard;

/// Where a header line is folded.
const FOLD_AT: usize = 78;
/// The longest a quoted-printable line is, with its soft line break.
const QP_WIDTH: usize = 76;
/// Bytes of text in one encoded word: short enough that `Subject: ` and one
/// word stay within the 76 characters RFC 2047 allows a line with one.
const WORD_BYTES: usize = 39;
/// References kept on a reply: the thread's first message and the most
/// recent, as RFC 5322 suggests for a long thread.
const MAX_REFERENCES: usize = 20;
/// Addresses read from a `To` or `Cc` line. A list mail sent to hundreds of
/// people is not a conversation with each of them.
const MAX_LISTED: usize = 50;

/// Someone a message is from or to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mailbox {
    pub name: Option<String>,
    pub address: String,
}

impl Mailbox {
    pub fn bare(address: &str) -> Self {
        Mailbox { name: None, address: address.trim().to_string() }
    }
}

/// A message to send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outgoing {
    pub from: Mailbox,
    pub to: Vec<Mailbox>,
    pub cc: Vec<Mailbox>,
    pub subject: String,
    pub body: String,
    /// Its `Message-ID`, without the angle brackets.
    pub message_id: String,
    /// The message it answers.
    pub in_reply_to: Option<String>,
    /// The thread it belongs to, oldest first.
    pub references: Vec<String>,
    pub date: DateTime<Utc>,
    /// A reply Mimic sent without the user approving it, which says so to
    /// other programs (`Auto-Submitted: auto-replied`, RFC 3834), so an
    /// automatic responder on the other side does not answer it in turn.
    pub auto_replied: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OutgoingError {
    #[error("\"{0}\" isn't an email address I can send to.")]
    NotAnAddress(String),
    #[error("There's no one to send it to.")]
    NoRecipient,
    #[error("There's nothing to send: the reply is empty.")]
    Empty,
}

impl Outgoing {
    /// The whole message, headers and body, with CRLF line endings.
    pub fn to_bytes(&self) -> Result<Vec<u8>, OutgoingError> {
        if !is_address(&self.from.address) {
            return Err(OutgoingError::NotAnAddress(self.from.address.clone()));
        }
        if self.to.is_empty() {
            return Err(OutgoingError::NoRecipient);
        }
        if let Some(bad) = self.to.iter().chain(&self.cc).find(|m| !is_address(&m.address)) {
            return Err(OutgoingError::NotAnAddress(bad.address.clone()));
        }
        if self.body.trim().is_empty() {
            return Err(OutgoingError::Empty);
        }
        let mut h = String::new();
        push_header(&mut h, "From", &mailbox(&self.from));
        push_header(&mut h, "To", &mailboxes(&self.to));
        if !self.cc.is_empty() {
            push_header(&mut h, "Cc", &mailboxes(&self.cc));
        }
        push_header(&mut h, "Subject", &unstructured(&self.subject));
        push_header(&mut h, "Date", &self.date.to_rfc2822());
        push_header(&mut h, "Message-ID", &format!("<{}>", self.message_id.trim()));
        if let Some(id) = self.in_reply_to.as_deref().map(str::trim).filter(|id| is_message_id(id)) {
            push_header(&mut h, "In-Reply-To", &format!("<{id}>"));
        }
        let refs = trimmed_references(&self.references);
        if !refs.is_empty() {
            push_header(&mut h, "References", &refs.iter().map(|r| format!("<{r}>")).collect::<Vec<_>>().join(" "));
        }
        if self.auto_replied {
            push_header(&mut h, "Auto-Submitted", "auto-replied");
        }
        push_header(&mut h, "MIME-Version", "1.0");
        push_header(&mut h, "Content-Type", "text/plain; charset=utf-8");
        push_header(&mut h, "Content-Transfer-Encoding", "quoted-printable");
        h.push_str("\r\n");
        let mut out = h.into_bytes();
        out.extend_from_slice(quoted_printable(&self.body).as_bytes());
        Ok(out)
    }
}

/// An address Mimic will put a message to: `local@domain`, ASCII, nothing
/// that needs quoting, a domain with a dot in it. Stricter than RFC 5321
/// allows, on purpose — an address this refuses is one to check by eye.
pub fn is_address(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty() || s.len() > 254 || !s.is_ascii() {
        return false;
    }
    if s.chars().any(|c| c.is_ascii_control() || c.is_whitespace() || "<>()[],;:\\\"".contains(c)) {
        return false;
    }
    let Some((local, domain)) = s.rsplit_once('@') else { return false };
    !local.is_empty()
        && local.len() <= 64
        && !local.contains('@')
        && !local.starts_with('.')
        && !local.ends_with('.')
        && !local.contains("..")
        && domain.contains('.')
        && !domain.starts_with(['.', '-'])
        && !domain.ends_with(['.', '-'])
        && !domain.contains("..")
        && domain.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
}

/// A message id that can go in `In-Reply-To` or `References`: one that a
/// mail program wrote (they have an `@`), not one Mimic made up for a
/// message that had none (`sha-…`), and nothing that could end the header.
pub fn is_message_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 250
        && s.contains('@')
        && !s.chars().any(|c| c.is_control() || c.is_whitespace() || c == '<' || c == '>')
}

/// A new `Message-ID` for a message from `from`: random, at the sender's
/// domain.
pub fn new_message_id(from: &str) -> String {
    let domain = from
        .trim()
        .rsplit_once('@')
        .map(|(_, d)| d.trim().to_ascii_lowercase())
        .filter(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.'))
        .unwrap_or_else(|| "localhost".to_string());
    format!("{}@{domain}", uuid::Uuid::new_v4().simple())
}

/// What marks a reply Mimic sent by itself, in its Message-ID and nowhere
/// else, so Mimic on another of the user's computers knows it when it reads
/// it back from the Sent folder (0.10.0-alpha.56): the random part, a dot,
/// and the first 12 hex digits of a digest of this and the random part. It
/// names nothing; a vacation responder's reply, which also says it was sent
/// automatically, doesn't carry it.
const OWN_MARK: &str = "mimic: sent on its own\n";

fn own_tag(random: &str) -> String {
    crate::ids::sha256_hex(format!("{OWN_MARK}{random}").as_bytes())[..12].to_string()
}

/// The Message-ID of a reply sent by itself: `new_message_id`'s, with the
/// mark `is_sent_on_its_own` reads.
pub fn new_own_message_id(from: &str) -> String {
    let id = new_message_id(from);
    match id.split_once('@') {
        Some((random, domain)) => format!("{random}.{}@{domain}", own_tag(random)),
        None => id,
    }
}

/// Whether a Message-ID (with or without its angle brackets) is one
/// `new_own_message_id` made.
pub fn is_sent_on_its_own(message_id: &str) -> bool {
    let id = message_id.trim().trim_matches(['<', '>']);
    let Some((local, _)) = id.split_once('@') else { return false };
    let Some((random, tag)) = local.split_once('.') else { return false };
    random.len() == 32 && random.bytes().all(|b| b.is_ascii_hexdigit()) && tag == own_tag(random)
}

/// The subject of a reply: the original's, with `Re: ` before it unless it
/// already says it is a reply (in English, German, the Nordic languages or
/// Dutch).
pub fn reply_subject(original: Option<&str>) -> String {
    let s = original.map(clean_line).unwrap_or_default();
    if s.is_empty() {
        return "Re:".to_string();
    }
    let lower = s.to_lowercase();
    let already = ["re:", "re :", "aw:", "sv:", "vs:", "antw:"].iter().any(|p| lower.starts_with(p));
    if already {
        s
    } else {
        format!("Re: {s}")
    }
}

/// The `References` of a reply to a message: the thread it was part of, then
/// the message itself. `refs` is what the importer stored for the message
/// (`metadata.refs`): its `In-Reply-To` first, then its `References`. When
/// `in_reply_to` says which one led (mail read since 0.10.0-alpha.23 stores
/// it), the rest are the thread in order; for mail read before, a lead that
/// appears again further on is taken to be that, and otherwise the order is
/// kept as stored.
pub fn reply_references(refs: &[String], in_reply_to: Option<&str>, answered: &str) -> Vec<String> {
    let mut thread: Vec<String> = match (refs.split_first(), in_reply_to) {
        (Some((first, rest)), Some(irt)) if first == irt => {
            let mut t = rest.to_vec();
            if !t.iter().any(|r| r == irt) {
                t.push(irt.to_string());
            }
            t
        }
        (Some((first, rest)), None) if rest.contains(first) => rest.to_vec(),
        _ => refs.to_vec(),
    };
    thread.push(answered.to_string());
    let mut seen = HashSet::new();
    thread.retain(|r| is_message_id(r.trim()) && seen.insert(r.trim().to_string()));
    thread
}

/// Every address in a `To`, `Cc` or `Reply-To` line, in order, without names:
/// commas inside quotes or angle brackets don't separate. A group's name and
/// anything that isn't an address are left out; at most `MAX_LISTED`.
pub fn parse_address_list(raw: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let (mut quoted, mut angled, mut escaped) = (false, false, false);
    for c in raw.chars() {
        if escaped {
            current.push(c);
            escaped = false;
            continue;
        }
        match c {
            '\\' if quoted => escaped = true,
            '"' => quoted = !quoted,
            '<' if !quoted => angled = true,
            '>' if !quoted => angled = false,
            ',' | ';' if !quoted && !angled => {
                parts.push(std::mem::take(&mut current));
                continue;
            }
            _ => {}
        }
        current.push(c);
    }
    parts.push(current);
    let mut out: Vec<String> = Vec::new();
    for part in parts {
        let part = part.trim();
        let address = match (part.rfind('<'), part.rfind('>')) {
            (Some(open), Some(close)) if close > open => part[open + 1..close].trim(),
            // "undisclosed-recipients:" and other group names end in a colon.
            _ => part.rsplit(':').next().unwrap_or(part).trim(),
        };
        if is_address(address) && !out.iter().any(|a| a.eq_ignore_ascii_case(address)) {
            out.push(address.to_string());
        }
        if out.len() >= MAX_LISTED {
            break;
        }
    }
    out
}

// ------------------------------------------------------------------ writing

/// A name or a subject as one line: line breaks and other control
/// characters become spaces, and the ends are trimmed.
fn clean_line(s: &str) -> String {
    s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect::<String>().trim().to_string()
}

/// Text as RFC 2047 encoded words (UTF-8, base64), split on character
/// boundaries, separated by spaces where the header may be folded. Spaces
/// between two encoded words are not part of the text, so none is lost.
fn encoded_words(text: &str) -> String {
    let mut words = Vec::new();
    let mut chunk = String::new();
    for c in text.chars() {
        if !chunk.is_empty() && chunk.len() + c.len_utf8() > WORD_BYTES {
            words.push(std::mem::take(&mut chunk));
        }
        chunk.push(c);
    }
    if !chunk.is_empty() {
        words.push(chunk);
    }
    words.iter().map(|w| format!("=?UTF-8?B?{}?=", base64_standard(w.as_bytes()))).collect::<Vec<_>>().join(" ")
}

/// Whether text can go in a header as it is: printable ASCII, and nothing a
/// reader would take for the start of an encoded word.
fn plain_ascii(s: &str) -> bool {
    s.chars().all(|c| (' '..='~').contains(&c)) && !s.contains("=?")
}

fn unstructured(text: &str) -> String {
    let line = clean_line(text);
    if plain_ascii(&line) {
        line
    } else {
        encoded_words(&line)
    }
}

/// `Name <address>`, or the bare address when there is no name. A name with
/// characters outside RFC 5322's atoms is quoted; one outside ASCII is
/// encoded.
fn mailbox(m: &Mailbox) -> String {
    let address = m.address.trim();
    let Some(name) = m.name.as_deref().map(clean_line).filter(|n| !n.is_empty()) else {
        return address.to_string();
    };
    let shown = if !plain_ascii(&name) {
        encoded_words(&name)
    } else if name.chars().all(|c| c.is_ascii_alphanumeric() || " !#$%&'*+-/=?^_`{|}~".contains(c)) {
        name
    } else {
        format!("\"{}\"", name.replace('\\', "\\\\").replace('"', "\\\""))
    };
    format!("{shown} <{address}>")
}

fn mailboxes(list: &[Mailbox]) -> String {
    list.iter().map(mailbox).collect::<Vec<_>>().join(", ")
}

/// `Name: value`, folded at spaces so no line passes `FOLD_AT` where a space
/// allows it.
fn push_header(out: &mut String, name: &str, value: &str) {
    out.push_str(name);
    out.push(':');
    let mut width = name.len() + 1;
    for word in value.split(' ') {
        if width + 1 + word.len() > FOLD_AT && width > name.len() + 1 {
            out.push_str("\r\n");
            width = 0;
        }
        out.push(' ');
        out.push_str(word);
        width += 1 + word.len();
    }
    out.push_str("\r\n");
}

fn trimmed_references(refs: &[String]) -> Vec<&str> {
    let mut seen = HashSet::new();
    let ok: Vec<&str> = refs.iter().map(|r| r.trim()).filter(|r| is_message_id(r) && seen.insert(*r)).collect();
    if ok.len() <= MAX_REFERENCES {
        return ok;
    }
    let mut out = vec![ok[0]];
    out.extend_from_slice(&ok[ok.len() - (MAX_REFERENCES - 1)..]);
    out
}

/// Quoted-printable (RFC 2045 §6.7): printable ASCII as itself, `=` and
/// everything else as `=XX`, a space or tab at the end of a line encoded so
/// nothing on the way trims it, and lines kept to 76 characters with soft
/// breaks. Every line ends CRLF.
pub fn quoted_printable(text: &str) -> String {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut out = String::with_capacity(text.len() + text.len() / 8 + 2);
    for line in text.trim_end_matches('\n').split('\n') {
        let bytes = line.as_bytes();
        let mut width = 0;
        for (i, &b) in bytes.iter().enumerate() {
            let last = i + 1 == bytes.len();
            let token = match b {
                b'=' => format!("={b:02X}"),
                b' ' | b'\t' if last => format!("={b:02X}"),
                b' ' | b'\t' | 33..=126 => (b as char).to_string(),
                _ => format!("={b:02X}"),
            };
            // The soft break's `=` has to fit on the line too.
            if width + token.len() > QP_WIDTH - 1 {
                out.push_str("=\r\n");
                width = 0;
            }
            out.push_str(&token);
            width += token.len();
        }
        out.push_str("\r\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::mbox::parse_raw;

    fn reply() -> Outgoing {
        Outgoing {
            from: Mailbox { name: Some("C Haynes".into()), address: "c@example.com".into() },
            to: vec![Mailbox { name: Some("Ada Lovelace".into()), address: "ada@example.com".into() }],
            cc: vec![],
            subject: reply_subject(Some("Offsite in März — who's coming?")),
            body: "Yes — I'll be there, and I'll bring the Füße.\n\nC".into(),
            message_id: "abc123@example.com".into(),
            in_reply_to: Some("q1@x.example".into()),
            references: vec!["q0@x.example".into(), "q1@x.example".into()],
            date: DateTime::parse_from_rfc3339("2026-10-25T21:00:00Z").unwrap().with_timezone(&Utc),
            auto_replied: false,
        }
    }

    #[test]
    fn what_is_written_reads_back_as_the_same_reply() {
        let bytes = reply().to_bytes().unwrap();
        let text = String::from_utf8(bytes.clone()).unwrap();
        assert!(text.is_ascii(), "everything outside ASCII is encoded");
        assert!(text.split("\r\n").all(|l| l.len() <= 78), "{text}");
        assert!(!text.replace("\r\n", "").contains('\n'), "every line ends CRLF");

        let mail = parse_raw(&bytes).expect("a reply with words in it");
        assert_eq!(mail.message_id, "abc123@example.com");
        assert_eq!(mail.subject.as_deref(), Some("Re: Offsite in März — who's coming?"));
        assert_eq!(mail.in_reply_to.as_deref(), Some("q1@x.example"));
        assert_eq!(mail.references, vec!["q0@x.example".to_string(), "q1@x.example".to_string()]);
        assert_eq!(mail.body.trim(), "Yes — I'll be there, and I'll bring the Füße.\n\nC");
        assert_eq!(mail.from.identifiers[0].value, "c@example.com");
        assert_eq!(mail.from.display_name, "C Haynes");
        assert!(mail.automated.is_none(), "a reply the user approved doesn't look automated");
    }

    #[test]
    fn a_reply_sent_on_its_own_says_so_and_reads_as_automated() {
        let bytes = Outgoing { auto_replied: true, ..reply() }.to_bytes().unwrap();
        assert!(String::from_utf8_lossy(&bytes).contains("\r\nAuto-Submitted: auto-replied\r\n"));
        assert!(parse_raw(&bytes).unwrap().automated.is_some(), "another Mimic won't answer it");
    }

    #[test]
    fn long_subjects_and_names_in_any_script_fold_and_read_back() {
        let subject = "Встреча в пятницу: кто придёт, что принести и во сколько начинаем — ответьте до среды";
        let r = Outgoing {
            subject: subject.into(),
            to: vec![Mailbox {
                name: Some("Ада \"Ада\" Лавлейс".into()), address: "ada@example.com".into()
            }],
            ..reply()
        };
        let bytes = r.to_bytes().unwrap();
        let text = String::from_utf8(bytes.clone()).unwrap();
        assert!(text.split("\r\n").all(|l| l.len() <= 76), "{text}");
        assert_eq!(parse_raw(&bytes).unwrap().subject.as_deref(), Some(subject));
    }

    #[test]
    fn a_line_break_in_a_name_or_subject_cannot_start_a_header() {
        let r = Outgoing {
            subject: "hello\r\nBcc: eve@example.com".into(),
            from: Mailbox { name: Some("C\nBcc: eve@example.com".into()), address: "c@example.com".into() },
            ..reply()
        };
        let text = String::from_utf8(r.to_bytes().unwrap()).unwrap();
        assert!(!text.contains("\r\nBcc:"), "{text}");
        let quoted =
            Outgoing { from: Mailbox { name: Some("Haynes, C.".into()), address: "c@example.com".into() }, ..reply() };
        let text = String::from_utf8(quoted.to_bytes().unwrap()).unwrap();
        assert!(text.contains("From: \"Haynes, C.\" <c@example.com>"), "{text}");
    }

    #[test]
    fn nothing_is_written_to_an_address_that_is_not_one() {
        let bad = |a: &str| Outgoing { to: vec![Mailbox::bare(a)], ..reply() }.to_bytes();
        for a in [
            "",
            "ada",
            "ada@",
            "@example.com",
            "ada@example",
            "a b@example.com",
            "ada@example.com>\r\nBcc: e@x.io",
            "ада@пример.рф",
        ] {
            assert!(matches!(bad(a), Err(OutgoingError::NotAnAddress(_))), "{a:?}");
        }
        assert_eq!(Outgoing { to: vec![], ..reply() }.to_bytes(), Err(OutgoingError::NoRecipient));
        assert_eq!(Outgoing { body: " \n ".into(), ..reply() }.to_bytes(), Err(OutgoingError::Empty));
    }

    #[test]
    fn quoted_printable_keeps_lines_short_and_ends_safe() {
        let long = "word ".repeat(40);
        let qp = quoted_printable(&format!("{long}\nend with space \n=sign\n.dot"));
        assert!(qp.split("\r\n").all(|l| l.len() <= 76), "{qp}");
        assert!(qp.contains("space=20\r\n"), "a trailing space survives being trimmed on the way");
        assert!(qp.contains("=3Dsign"));
        let back = crate::sources::mime::decode_quoted_printable(&qp);
        assert_eq!(String::from_utf8(back).unwrap(), format!("{long}\r\nend with space \r\n=sign\r\n.dot\r\n"));
    }

    #[test]
    fn a_reply_threads_under_what_it_answers() {
        assert_eq!(reply_subject(Some("Offsite")), "Re: Offsite");
        assert_eq!(reply_subject(Some("RE: Offsite")), "RE: Offsite");
        assert_eq!(reply_subject(Some("AW: Termin")), "AW: Termin");
        assert_eq!(reply_subject(None), "Re:");

        let stored = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        // In-Reply-To first, then References (mail read since this version).
        assert_eq!(
            reply_references(&stored(&["b@x", "a@x", "b@x"]), Some("b@x"), "c@x"),
            stored(&["a@x", "b@x", "c@x"])
        );
        // Mail read before: a lead seen again further on is the In-Reply-To.
        assert_eq!(reply_references(&stored(&["b@x", "a@x", "b@x"]), None, "c@x"), stored(&["a@x", "b@x", "c@x"]));
        // Nothing to go on: the message itself.
        assert_eq!(reply_references(&[], None, "c@x"), stored(&["c@x"]));
        // A made-up id isn't one to thread by.
        assert_eq!(reply_references(&stored(&["sha-123"]), None, "sha-456"), Vec::<String>::new());
    }

    #[test]
    fn a_long_thread_keeps_its_first_message_and_its_latest() {
        let refs: Vec<String> = (0..30).map(|i| format!("m{i}@x")).collect();
        let kept = trimmed_references(&refs);
        assert_eq!(kept.len(), MAX_REFERENCES);
        assert_eq!(kept[0], "m0@x");
        assert_eq!(kept[1], "m11@x");
        assert_eq!(*kept.last().unwrap(), "m29@x");
    }

    #[test]
    fn address_lists_are_read_without_names_or_groups() {
        assert_eq!(
            parse_address_list("\"Lovelace, Ada\" <ada@example.com>, bob@example.com; undisclosed-recipients:;"),
            vec!["ada@example.com".to_string(), "bob@example.com".to_string()]
        );
        assert_eq!(parse_address_list("Ada <ADA@example.com>, ada@example.com"), vec!["ADA@example.com".to_string()]);
        assert!(parse_address_list("noreply").is_empty());
    }

    #[test]
    fn a_message_id_is_random_at_the_senders_domain() {
        let a = new_message_id("c@Example.com");
        assert!(a.ends_with("@example.com") && is_message_id(&a), "{a}");
        assert_ne!(a, new_message_id("c@example.com"));
        assert!(new_message_id("not an address").ends_with("@localhost"));
    }

    #[test]
    fn a_reply_sent_by_itself_is_known_by_its_message_id_alone() {
        let own = new_own_message_id("c@Example.com");
        assert!(own.ends_with("@example.com") && is_message_id(&own), "{own}");
        assert!(is_sent_on_its_own(&own));
        assert!(is_sent_on_its_own(&format!("<{own}>")), "as a header writes it");
        assert_ne!(own, new_own_message_id("c@example.com"));
        // One the user sent, and one whose mark doesn't match what it marks.
        assert!(!is_sent_on_its_own(&new_message_id("c@example.com")));
        let (random, rest) = own.split_once('.').unwrap();
        let other = format!("{}{}.{rest}", &random[..31], if random.ends_with('0') { '1' } else { '0' });
        assert!(!is_sent_on_its_own(&other), "{other}");
        assert!(!is_sent_on_its_own("CAFxyz.abc@mail.gmail.com"));
        assert!(!is_sent_on_its_own("no at sign"));
    }
}
