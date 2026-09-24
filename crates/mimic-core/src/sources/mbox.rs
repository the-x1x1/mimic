//! The `mbox` connector: RFC 4155 mailboxes, which is what Gmail Takeout,
//! Thunderbird and most mail clients export.
//!
//! Threading uses `References`/`In-Reply-To` where the export provides them
//! and falls back to a normalized subject, because a large share of real mail
//! has neither header intact after passing through a webmail client. The
//! fallback is deliberately narrow: subject matching only joins messages that
//! also share a participant, so two unrelated "Re: lunch" threads with
//! different people stay apart.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use serde_json::json;

use super::normalize::clean_body;
use super::{
    validate_by_dry_run, AuthorRef, CommunicationSource, DiscoveredConversation, LocationKind, RawMessage,
    SourceMetadata, SourceResult, ValidationReport,
};
use crate::db::repo_people::IdentifierInput;
use crate::db::IdentifierKind;
use crate::ids::sha256_hex;

pub struct MboxSource;

/// One parsed mail, before threading. Shared with the IMAP connector, which
/// receives the same RFC 5322 text over the network instead of from a file.
#[derive(Debug, Clone)]
pub(crate) struct Mail {
    pub(crate) message_id: String,
    pub(crate) in_reply_to: Option<String>,
    pub(crate) references: Vec<String>,
    pub(crate) subject: Option<String>,
    pub(crate) from: AuthorRef,
    pub(crate) date: Option<String>,
    pub(crate) body: String,
    /// Why the headers say a machine sent it, if they do. Decided here because
    /// the headers are not kept after parsing.
    pub(crate) automated: Option<super::automated::Automated>,
    /// It was in the user's own Sent folder: read from it over IMAP,
    /// labelled "Sent" in a Gmail export, or in a file whose name says it is
    /// the Sent folder (`sent_folder_file`). What is there is usually what the
    /// user sent, so whoever it is filed under is often the user under an
    /// address Mimic doesn't know yet — and is asked about, never assumed.
    pub(crate) sent: bool,
    /// The headers say it was passed on (`Resent-From`, `Resent-Sender`): the
    /// `From` is whoever wrote it first, so its being in the Sent folder says
    /// nothing about who that is.
    pub(crate) resent: bool,
}

impl CommunicationSource for MboxSource {
    fn metadata(&self) -> SourceMetadata {
        SourceMetadata {
            connector: "mbox",
            display_name: "Email mailbox (mbox)",
            channel: "email",
            description: "A standard .mbox file, as exported by Gmail Takeout, Thunderbird and most mail clients. For Apple Mail, choose the file named mbox inside the exported folder.",
            location_kind: LocationKind::File,
            extensions: &["mbox", "mbx"],
        }
    }

    fn discover(&self, location: &Path) -> SourceResult<Vec<PathBuf>> {
        if location.is_file() {
            return Ok(vec![location.to_path_buf()]);
        }
        if !location.is_dir() {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for entry in std::fs::read_dir(location)? {
            let path = entry?.path();
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_lowercase();
            if path.is_file() && (ext == "mbox" || ext == "mbx") {
                out.push(path);
            }
        }
        out.sort();
        Ok(out)
    }

    fn validate(&self, location: &Path) -> SourceResult<ValidationReport> {
        let mut report = validate_by_dry_run(self, location)?;
        if self.discover(location)?.iter().any(|p| sent_folder_file(p)) {
            report
                .warnings
                .push("A file here is named as your Sent folder. If mail in it came from an address you haven't given me, I'll ask whether it's you.".into());
        }
        if report.messages > 0 {
            let singletons = report.conversations;
            if singletons == report.messages && report.messages > 5 {
                report.warnings.push(
                    "Every message ended up in a thread of its own. The export may be missing its Message-ID headers, which means replies cannot be paired with what they answer.".into(),
                );
            }
        }
        Ok(report)
    }

    fn import(
        &self,
        location: &Path,
        sink: &mut dyn FnMut(DiscoveredConversation) -> SourceResult<()>,
    ) -> SourceResult<()> {
        // Threading needs the whole file, so mails are read first and only
        // their headers plus bodies are held — not the raw text.
        let mut mails = Vec::new();
        for path in self.discover(location)? {
            let first = mails.len();
            read_mbox(&path, &mut mails)?;
            // A Thunderbird or Apple Mail export is one file per folder, and
            // the file's name is the only thing that says which folder.
            if sent_folder_file(&path) {
                for m in &mut mails[first..] {
                    m.sent = true;
                }
            }
        }
        for convo in thread(mails) {
            sink(convo)?;
        }
        Ok(())
    }
}

/// Whether a file's name says it is the Sent folder: `Sent` (Thunderbird's
/// own file), `Sent.mbox`, `Gesendete Elemente.mbx`, or Apple Mail's
/// `Sent Messages.mbox/mbox`, whose folder carries the name.
fn sent_folder_file(path: &Path) -> bool {
    let name = |p: &Path| p.file_name().and_then(|n| n.to_str()).map(str::to_string);
    let Some(mut file) = name(path) else { return false };
    if file.eq_ignore_ascii_case("mbox") {
        match path.parent().and_then(name) {
            Some(folder) => file = folder,
            None => return false,
        }
    }
    let lower = file.to_lowercase();
    let stem = [".mbox", ".mbx"].iter().find_map(|ext| lower.strip_suffix(ext)).unwrap_or(lower.as_str());
    super::is_sent_folder_name(stem)
}

fn read_mbox(path: &Path, out: &mut Vec<Mail>) -> SourceResult<()> {
    // Bytes, line by line: a mailbox is not UTF-8, it is whatever each
    // message's charset says, and reading it as text would fail the whole
    // import on the first Latin-1 byte.
    let mut reader = BufReader::new(File::open(path)?);
    let mut current: Vec<Vec<u8>> = Vec::new();
    let mut line = Vec::new();
    loop {
        line.clear();
        if reader.read_until(b'\n', &mut line)? == 0 {
            break;
        }
        while matches!(line.last(), Some(b'\n') | Some(b'\r')) {
            line.pop();
        }
        if is_separator(&String::from_utf8_lossy(&line)) {
            if !current.is_empty() {
                if let Some(m) = parse_mail(&current) {
                    out.push(m);
                }
                current.clear();
            }
            continue;
        }
        current.push(line.clone());
    }
    if let Some(m) = parse_mail(&current) {
        out.push(m);
    }
    Ok(())
}

/// A message separator, not a body line that happens to begin with "From ".
///
/// The mbox format says a body line starting with "From " is escaped as
/// ">From ", but plenty of real exports do not do it, so the shape of the
/// line decides: `From <address> <date…>`, with an address that looks like
/// one. "From the look of it, Tuesday works." has no address and is body text.
fn is_separator(line: &str) -> bool {
    let Some(rest) = line.strip_prefix("From ") else { return false };
    let mut parts = rest.split_whitespace();
    let Some(sender) = parts.next() else { return false };
    let looks_like_sender = sender.contains('@') || sender == "MAILER-DAEMON" || sender == "-";
    // A real separator carries a date after the sender: at least three more
    // tokens (weekday, month, day, …).
    looks_like_sender && parts.count() >= 3
}

/// One message from an mbox, as the lines between two separators.
fn parse_mail(lines: &[Vec<u8>]) -> Option<Mail> {
    if lines.is_empty() {
        return None;
    }
    parse_raw(&lines.join(&b'\n'))
}

/// One RFC 5322 message, MIME and all. The body is the text part the person
/// wrote — decoded from base64 or quoted-printable, from its charset, and out
/// of HTML when there is no plain part — with quoted replies and signatures
/// removed. A message with no text in it is `None`.
pub(crate) fn parse_raw(raw: &[u8]) -> Option<Mail> {
    let (headers, raw_body) = super::mime::split_message(raw);
    let get = |name: &str| super::mime::header(&headers, name).map(str::to_string);
    let text = super::mime::body_text(&headers, &raw_body)?;
    let body = clean_body(&text);
    if body.trim().is_empty() {
        return None;
    }
    let from = parse_address(&super::mime::decode_header(get("from").as_deref().unwrap_or_default()));
    let automated = super::automated::detect(&headers, from.identifiers.first().map(|i| i.value.as_str()));
    let message_id = get("message-id")
        .map(|s| s.trim().trim_matches(['<', '>']).to_string())
        .filter(|s| !s.is_empty())
        // No Message-ID: derive one from the content so a re-import of the
        // same file produces the same id rather than a second copy.
        .unwrap_or_else(|| format!("sha-{}", &sha256_hex(format!("{from:?}{body}").as_bytes())[..32]));
    let references: Vec<String> = get("references")
        .unwrap_or_default()
        .split_whitespace()
        .map(|s| s.trim_matches(['<', '>']).to_string())
        .filter(|s| !s.is_empty())
        .collect();
    Some(Mail {
        message_id,
        in_reply_to: get("in-reply-to")
            .and_then(|s| s.split_whitespace().next().map(str::to_string))
            .map(|s| s.trim_matches(['<', '>']).to_string())
            .filter(|s| !s.is_empty()),
        references,
        subject: get("subject").map(|s| super::mime::decode_header(&s)).filter(|s| !s.trim().is_empty()),
        from,
        date: get("date").and_then(|d| parse_date(&d)),
        body,
        automated,
        sent: get("x-gmail-labels").is_some_and(|labels| in_sent(&super::mime::decode_header(&labels))),
        resent: get("resent-from").is_some() || get("resent-sender").is_some(),
    })
}

/// Gmail's labels for a message in an export: comma-separated, "Sent" among
/// them for what the user sent.
fn in_sent(labels: &str) -> bool {
    labels.split(',').any(|l| l.trim().eq_ignore_ascii_case("sent"))
}

/// `Ada Lovelace <ada@example.com>` or a bare address.
fn parse_address(raw: &str) -> AuthorRef {
    let raw = raw.trim();
    let (name, addr) = match (raw.find('<'), raw.rfind('>')) {
        (Some(open), Some(close)) if close > open => (raw[..open].trim(), raw[open + 1..close].trim()),
        _ => ("", raw),
    };
    let name = name.trim_matches('"').trim();
    let mut identifiers = Vec::new();
    if addr.contains('@') && !addr.is_empty() {
        identifiers.push(IdentifierInput::new(IdentifierKind::Email, addr));
    }
    AuthorRef { display_name: name.to_string(), identifiers }
}

/// RFC 5322 dates to RFC 3339. Anything unparseable becomes `None` rather
/// than a guessed timestamp — a wrong time is worse than no time, because
/// response latency is computed from these.
fn parse_date(raw: &str) -> Option<String> {
    let cleaned = raw.split('(').next().unwrap_or(raw).trim();
    chrono::DateTime::parse_from_rfc2822(cleaned).ok().map(|dt| crate::ids::fmt_rfc3339(dt.with_timezone(&chrono::Utc)))
}

/// Subject with any number of reply and forward prefixes removed, folded for
/// comparison.
fn normalized_subject(subject: &str) -> String {
    let mut s = subject.trim();
    loop {
        let lower = s.to_lowercase();
        let stripped =
            ["re:", "re :", "fw:", "fwd:", "aw:", "sv:"].iter().find_map(|p| lower.starts_with(p).then_some(p.len()));
        match stripped {
            Some(n) => s = s[n..].trim_start(),
            None => break,
        }
    }
    s.to_lowercase()
}

/// Group mails into conversations: by reference chain first, then by
/// (normalized subject + a shared participant).
pub(crate) fn thread(mut mails: Vec<Mail>) -> Vec<DiscoveredConversation> {
    // Two copies of one message — the user's post to a list, read back from
    // the inbox with the list as its sender — are one message sent when the
    // Sent copy says, whatever the other copy's date reads (or fails to), so
    // the Sent copy sorts first below and is the one the importer keeps.
    let sent_dates: HashMap<String, Option<String>> =
        mails.iter().filter(|m| m.sent).map(|m| (m.message_id.clone(), m.date.clone())).collect();
    for m in mails.iter_mut().filter(|m| !m.sent) {
        if let Some(date) = sent_dates.get(&m.message_id) {
            m.date = date.clone();
        }
    }
    // Union-find over message ids.
    let mut parent: HashMap<String, String> = HashMap::new();
    fn find(parent: &mut HashMap<String, String>, x: &str) -> String {
        let mut cur = x.to_string();
        while let Some(p) = parent.get(&cur) {
            if p == &cur {
                break;
            }
            cur = p.clone();
        }
        cur
    }
    fn union(parent: &mut HashMap<String, String>, a: &str, b: &str) {
        let ra = find(parent, a);
        let rb = find(parent, b);
        if ra != rb {
            parent.insert(ra, rb);
        }
    }
    for m in &mails {
        parent.entry(m.message_id.clone()).or_insert_with(|| m.message_id.clone());
    }
    let known: std::collections::HashSet<&str> = mails.iter().map(|m| m.message_id.as_str()).collect();
    for m in &mails {
        for r in m.in_reply_to.iter().chain(m.references.iter()) {
            if known.contains(r.as_str()) {
                union(&mut parent, &m.message_id, r);
            }
        }
    }
    // Subject fallback, restricted to mails that share an address.
    let mut by_subject: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, m) in mails.iter().enumerate() {
        if let Some(s) = &m.subject {
            let key = normalized_subject(s);
            if !key.is_empty() {
                by_subject.entry(key).or_default().push(i);
            }
        }
    }
    for group in by_subject.values() {
        for pair in group.windows(2) {
            let (a, b) = (&mails[pair[0]], &mails[pair[1]]);
            let shares_address =
                a.from.identifiers.iter().any(|x| b.from.identifiers.iter().any(|y| x.normalized() == y.normalized()));
            // Different people writing under the same subject are only joined
            // when one of them appears in both, which a real thread guarantees.
            if shares_address || a.from.identifiers.is_empty() || b.from.identifiers.is_empty() {
                union(&mut parent, &a.message_id, &b.message_id);
            }
        }
    }

    let mut grouped: HashMap<String, Vec<Mail>> = HashMap::new();
    for m in mails {
        let root = find(&mut parent, &m.message_id);
        grouped.entry(root).or_default().push(m);
    }
    let mut out: Vec<DiscoveredConversation> = grouped
        .into_iter()
        .map(|(root, mut group)| {
            // Of two copies of one message, the one from the Sent folder
            // comes first, which is the one the importer keeps: it carries
            // the user's own address, and only it was in the folder.
            group.sort_by(|a, b| a.date.cmp(&b.date).then(a.message_id.cmp(&b.message_id)).then(b.sent.cmp(&a.sent)));
            let subject = group.iter().find_map(|m| m.subject.clone());
            let messages = group
                .into_iter()
                .map(|m| RawMessage {
                    external_id: m.message_id,
                    author: m.from,
                    sent_at: m.date,
                    body: m.body,
                    // What this message answers, by Message-ID. The importer
                    // uses it to join a conversation that already holds one
                    // of them — from an earlier check, or another source.
                    metadata: {
                        let refs: Vec<&String> = m.in_reply_to.iter().chain(m.references.iter()).collect();
                        let mut meta = serde_json::Map::new();
                        if let Some(s) = &m.subject {
                            meta.insert("subject".into(), json!(s));
                        }
                        // Read by `Db::sent_folder_people` to ask whether
                        // whoever this is filed under is the user.
                        if m.sent && !m.resent {
                            meta.insert("sentFolder".into(), json!(true));
                        }
                        if !refs.is_empty() {
                            meta.insert("refs".into(), json!(refs));
                        }
                        // Read by the dashboard to leave the thread out of
                        // what is waiting; absent means a person wrote it.
                        if let Some(a) = m.automated {
                            meta.insert("automated".into(), json!(a.as_str()));
                        }
                        serde_json::Value::Object(meta)
                    },
                })
                .collect();
            DiscoveredConversation { external_id: root, subject, channel: "email".into(), messages }
        })
        .collect();
    // Deterministic order so two runs produce identical output.
    out.sort_by(|a, b| {
        a.messages
            .first()
            .and_then(|m| m.sent_at.clone())
            .cmp(&b.messages.first().and_then(|m| m.sent_at.clone()))
            .then(a.external_id.cmp(&b.external_id))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const MBOX: &str = concat!(
        "From ada@example.com Tue Feb 03 09:14:00 2026\n",
        "From: Ada Lovelace <ada@example.com>\n",
        "To: C <c@example.com>\n",
        "Subject: Numbers for\n",
        " the quarter\n",
        "Date: Tue, 3 Feb 2026 09:14:00 +0000\n",
        "Message-ID: <a1@example.com>\n",
        "\n",
        "Can you send the numbers?\n",
        "\n",
        "From c@example.com Tue Feb 03 09:20:00 2026\n",
        "From: c@example.com\n",
        "Subject: Re: Numbers for the quarter\n",
        "Date: Tue, 3 Feb 2026 09:20:00 +0000\n",
        "Message-ID: <a2@example.com>\n",
        "In-Reply-To: <a1@example.com>\n",
        "\n",
        "yep, sending now\n",
        "\n",
        "On Tue, 3 Feb 2026 at 09:14, Ada <ada@example.com> wrote:\n",
        "> Can you send the numbers?\n",
        "\n",
        "From ada@example.com Wed Feb 04 10:00:00 2026\n",
        "From: Ada Lovelace <ada@example.com>\n",
        "Subject: Lunch\n",
        "Date: Wed, 4 Feb 2026 10:00:00 +0000\n",
        "Message-ID: <b1@example.com>\n",
        "\n",
        "From the look of it, Tuesday works.\n",
    );

    fn write(text: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mail.mbox");
        std::fs::write(&path, text).unwrap();
        (dir, path)
    }

    fn import(path: &Path) -> Vec<DiscoveredConversation> {
        let mut out = Vec::new();
        MboxSource
            .import(path, &mut |c| {
                out.push(c);
                Ok(())
            })
            .unwrap();
        out
    }

    #[test]
    fn replies_are_threaded_and_folded_headers_are_joined() {
        let (_d, path) = write(MBOX);
        let convos = import(&path);
        assert_eq!(convos.len(), 2);
        assert_eq!(convos[0].messages.len(), 2, "the reply joins its parent");
        assert_eq!(convos[0].subject.as_deref(), Some("Numbers for the quarter"), "the folded header is rejoined");
        assert_eq!(convos[0].messages[0].author.display_name, "Ada Lovelace");
        assert_eq!(convos[0].messages[0].author.identifiers[0].value, "ada@example.com");
        assert_eq!(convos[0].messages[1].body, "yep, sending now", "the quoted reply is stripped");
        assert_eq!(convos[0].messages[0].sent_at.as_deref(), Some("2026-02-03T09:14:00.000Z"));
    }

    #[test]
    fn separators_are_told_apart_from_body_text() {
        assert!(is_separator("From ada@example.com Tue Feb 03 09:14:00 2026"));
        assert!(is_separator("From MAILER-DAEMON Tue Feb 03 09:14:00 2026"));
        assert!(!is_separator("From the look of it, Tuesday works."));
        assert!(!is_separator("From: ada@example.com"));
        assert!(!is_separator("Frombidden"));
    }

    #[test]
    fn a_body_line_beginning_with_from_does_not_split_a_message() {
        let (_d, path) = write(MBOX);
        let convos = import(&path);
        let lunch = convos.iter().find(|c| c.subject.as_deref() == Some("Lunch")).unwrap();
        assert_eq!(lunch.messages.len(), 1);
        assert_eq!(lunch.messages[0].body, "From the look of it, Tuesday works.");
    }

    #[test]
    fn the_same_subject_from_different_people_stays_apart() {
        let text = concat!(
            "From a@x.com Tue Feb 03 09:00:00 2026\n",
            "From: a@x.com\nSubject: lunch\nDate: Tue, 3 Feb 2026 09:00:00 +0000\nMessage-ID: <1@x>\n\nlunch?\n",
            "\nFrom b@y.com Tue Feb 03 09:00:00 2026\n",
            "From: b@y.com\nSubject: Re: lunch\nDate: Tue, 3 Feb 2026 09:05:00 +0000\nMessage-ID: <2@y>\n\nlunch?\n",
        );
        let (_d, path) = write(text);
        let convos = import(&path);
        assert_eq!(convos.len(), 2, "no shared address, no thread: {convos:#?}");
    }

    #[test]
    fn mail_with_no_message_id_gets_a_content_derived_one() {
        let text = concat!(
            "From a@x.com Tue Feb 03 09:00:00 2026\n",
            "From: a@x.com\nSubject: hi\nDate: Tue, 3 Feb 2026 09:00:00 +0000\n\nhello there\n",
        );
        let (_d, path) = write(text);
        let first = import(&path);
        let second = import(&path);
        assert_eq!(first.len(), 1);
        assert!(first[0].messages[0].external_id.starts_with("sha-"));
        assert_eq!(
            first[0].messages[0].external_id, second[0].messages[0].external_id,
            "re-importing must not create a second copy"
        );
    }

    #[test]
    fn an_unparseable_date_is_none_rather_than_a_guess() {
        assert_eq!(parse_date("Tue, 3 Feb 2026 09:14:00 +0000"), Some("2026-02-03T09:14:00.000Z".into()));
        assert_eq!(parse_date("Tue, 3 Feb 2026 09:14:00 +0000 (UTC)"), Some("2026-02-03T09:14:00.000Z".into()));
        assert_eq!(parse_date("sometime last tuesday"), None);
    }

    #[test]
    fn subject_prefixes_are_peeled_however_many_there_are() {
        assert_eq!(normalized_subject("Re: Fwd: RE: Numbers"), "numbers");
        assert_eq!(normalized_subject("  Numbers  "), "numbers");
        assert_eq!(normalized_subject("Re:"), "");
    }

    #[test]
    fn addresses_parse_with_and_without_a_display_name() {
        let a = parse_address("\"Ada Lovelace\" <ada@example.com>");
        assert_eq!((a.display_name.as_str(), a.identifiers.len()), ("Ada Lovelace", 1));
        let b = parse_address("bare@example.com");
        assert_eq!((b.display_name.as_str(), b.identifiers[0].value.as_str()), ("", "bare@example.com"));
        assert!(!parse_address("Nobody").is_usable());
    }

    #[test]
    fn a_folder_of_mailboxes_is_discovered_in_a_stable_order() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["b.mbox", "a.mbox", "notes.txt"] {
            std::fs::write(dir.path().join(name), MBOX).unwrap();
        }
        let found = MboxSource.discover(dir.path()).unwrap();
        assert_eq!(found.len(), 2);
        assert!(found[0].ends_with("a.mbox") && found[1].ends_with("b.mbox"));
    }

    #[test]
    fn validation_warns_when_nothing_threaded() {
        let mut text = String::new();
        for i in 0..8 {
            text.push_str(&format!(
                "From a@x.com Tue Feb 03 09:00:00 2026\nFrom: a@x.com\nSubject: note {i}\nDate: Tue, 3 Feb 2026 09:0{i}:00 +0000\n\nbody {i}\n\n"
            ));
        }
        let (_d, path) = write(&text);
        let r = MboxSource.validate(&path).unwrap();
        assert!(r.ok);
        assert_eq!((r.conversations, r.messages), (8, 8));
        assert!(r.warnings.iter().any(|w| w.contains("thread of its own")), "{:?}", r.warnings);
    }

    /// What a Gmail Takeout export actually contains: multipart mail with a
    /// base64 HTML part, a quoted-printable plain part, and an encoded-word
    /// subject. The body stored is what the person typed, not the MIME.
    #[test]
    fn mime_mail_imports_as_the_words_not_the_encoding() {
        let mbox = concat!(
            "From c@example.com Tue Feb 03 09:20:00 2026\n",
            "From: =?UTF-8?Q?C=C3=A9line?= <c@example.com>\n",
            "Subject: =?UTF-8?B?UmU6IE9mZnNpdGU=?=\n",
            "Date: Tue, 3 Feb 2026 09:20:00 +0000\n",
            "Message-ID: <m1@example.com>\n",
            "MIME-Version: 1.0\n",
            "Content-Type: multipart/alternative; boundary=\"0000abc\"\n",
            "\n",
            "--0000abc\n",
            "Content-Type: text/plain; charset=\"UTF-8\"\n",
            "Content-Transfer-Encoding: quoted-printable\n",
            "\n",
            "can=E2=80=99t make it, sorry =E2=80=94 next time\n",
            "\n",
            "On Tue, 3 Feb 2026 at 09:14, Ada <ada@example.com> wrote:\n",
            "> are you coming?\n",
            "--0000abc\n",
            "Content-Type: text/html; charset=\"UTF-8\"\n",
            "Content-Transfer-Encoding: base64\n",
            "\n",
            "PGRpdj5jYW7igJl0IG1ha2UgaXQ8L2Rpdj4=\n",
            "--0000abc--\n",
        );
        let (_dir, path) = write(mbox);
        let convos = import(&path);
        let m = &convos[0].messages[0];
        assert_eq!(m.body, "can’t make it, sorry — next time");
        assert_eq!(m.author.display_name, "Céline");
        assert_eq!(convos[0].subject.as_deref(), Some("Re: Offsite"));
        assert!(!m.body.contains("--0000abc") && !m.body.contains("Content-Type"));
    }

    #[test]
    fn a_latin1_message_does_not_fail_the_import_or_lose_its_accents() {
        let mut raw = b"From c@example.com Tue Feb 03 09:20:00 2026\nFrom: c@example.com\nMessage-ID: <l1@x>\nContent-Type: text/plain; charset=iso-8859-1\n\nJ'arrive ".to_vec();
        raw.push(0xE0);
        raw.extend_from_slice(b" midi\n");
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("latin1.mbox");
        std::fs::write(&path, &raw).unwrap();
        let convos = import(&path);
        assert_eq!(convos[0].messages[0].body, "J'arrive \u{e0} midi");
    }

    #[test]
    fn a_reply_records_what_it_answers() {
        let (_dir, path) = write(MBOX);
        let convos = import(&path);
        let reply = convos.iter().flat_map(|c| &c.messages).find(|m| m.external_id == "a2@example.com").unwrap();
        assert_eq!(reply.metadata["refs"], serde_json::json!(["a1@example.com"]));
    }

    #[test]
    fn a_newsletter_is_marked_as_automated_and_a_person_is_not() {
        let mbox = concat!(
            "From news@brand.example Mon Feb 02 08:00:00 2026\n",
            "From: Brand Weekly <news@brand.example>\n",
            "Subject: This week at Brand\n",
            "Date: Mon, 2 Feb 2026 08:00:00 +0000\n",
            "Message-ID: <n1@brand.example>\n",
            "List-Unsubscribe: <https://brand.example/unsubscribe>\n",
            "\n",
            "Twenty percent off everything this week.\n",
            "\n",
            "From ada@example.com Mon Feb 02 09:00:00 2026\n",
            "From: Ada <ada@example.com>\n",
            "Subject: lunch\n",
            "Date: Mon, 2 Feb 2026 09:00:00 +0000\n",
            "Message-ID: <p1@example.com>\n",
            "\n",
            "lunch on thursday?\n",
        );
        let (_dir, path) = write(mbox);
        let convos = import(&path);
        let all: Vec<_> = convos.iter().flat_map(|c| &c.messages).collect();
        let news = all.iter().find(|m| m.external_id == "n1@brand.example").unwrap();
        let person = all.iter().find(|m| m.external_id == "p1@example.com").unwrap();
        assert_eq!(news.metadata["automated"], serde_json::json!("newsletter"));
        assert!(person.metadata.get("automated").is_none(), "a person's mail carries no marker at all");
    }

    #[test]
    fn mail_gmail_labelled_sent_is_marked_as_from_the_sent_folder() {
        let mbox = concat!(
            "From 1824354102836451234@xxx Mon Feb 02 09:00:00 +0000 2026\n",
            "X-Gmail-Labels: Important,Opened,Sent\n",
            "From: C at work <c@work.example>\n",
            "Subject: numbers\n",
            "Date: Mon, 2 Feb 2026 09:00:00 +0000\n",
            "Message-ID: <s1@work.example>\n",
            "\n",
            "sending them over now\n",
            "\n",
            "From 1824354102836455678@xxx Mon Feb 02 10:00:00 +0000 2026\n",
            "X-Gmail-Labels: Inbox,Category Sent Items\n",
            "From: Ada <ada@example.com>\n",
            "Subject: lunch\n",
            "Date: Mon, 2 Feb 2026 10:00:00 +0000\n",
            "Message-ID: <i1@example.com>\n",
            "\n",
            "lunch on thursday?\n",
        );
        let (_dir, path) = write(mbox);
        let convos = import(&path);
        let all: Vec<_> = convos.iter().flat_map(|c| &c.messages).collect();
        let sent = all.iter().find(|m| m.external_id == "s1@work.example").unwrap();
        let inbox = all.iter().find(|m| m.external_id == "i1@example.com").unwrap();
        assert_eq!(sent.metadata["sentFolder"], serde_json::json!(true));
        assert!(inbox.metadata.get("sentFolder").is_none(), "a label that only contains the word is not Sent");
        assert!(in_sent(" sent "));
        assert!(!in_sent("Unsent"));

        // The user's post to a list, read back from the inbox with the list as
        // its sender, and from the Sent folder.
        // The list's copy has no date it can be read by, so a date alone would put it first.
        let list_copy = parse_raw(
            b"From: 'C' via Team <team@lists.example>\nSubject: hi\nDate: sometime\nMessage-ID: <d1@x>\n\nhello\n",
        )
        .unwrap();
        let mut own = parse_raw(
            b"From: C at work <c@work.example>\nSubject: hi\nDate: Mon, 2 Feb 2026 11:00:00 +0000\nMessage-ID: <d1@x>\n\nhello\n",
        )
        .unwrap();
        own.sent = true;
        let convos = thread(vec![list_copy, own]);
        let copies: Vec<_> = convos.iter().flat_map(|c| &c.messages).collect();
        assert_eq!(copies.len(), 2);
        assert_eq!(copies[0].author.identifiers[0].value, "c@work.example", "the copy that was sent is the one kept");
        assert_eq!(copies[0].metadata["sentFolder"], serde_json::json!(true));
        assert!(copies[1].metadata.get("sentFolder").is_none(), "the list's copy was never in the Sent folder");

        // Passed on by the user, with its first writer's From: not a sign of anything.
        let mut redirected = parse_raw(
            b"Resent-From: c@example.com\nFrom: Ada <ada@example.com>\nSubject: fyi\nMessage-ID: <r1@x>\n\nthe numbers\n",
        )
        .unwrap();
        redirected.sent = true;
        let convos = thread(vec![redirected]);
        assert!(convos[0].messages[0].metadata.get("sentFolder").is_none());
    }

    /// Two messages from the user's work address, one of them passed on.
    const SENT_MBOX: &str = concat!(
        "From c@work.example Mon Feb 02 09:00:00 2026\n",
        "From: C at work <c@work.example>\n",
        "Subject: numbers\n",
        "Date: Mon, 2 Feb 2026 09:00:00 +0000\n",
        "Message-ID: <s1@work.example>\n",
        "\n",
        "sending them over now\n",
        "\n",
        "From c@work.example Mon Feb 02 10:00:00 2026\n",
        "Resent-From: C at work <c@work.example>\n",
        "From: Ada <ada@example.com>\n",
        "Subject: lunch\n",
        "Date: Mon, 2 Feb 2026 10:00:00 +0000\n",
        "Message-ID: <r1@example.com>\n",
        "\n",
        "lunch on thursday?\n",
    );

    #[test]
    fn a_file_named_as_the_sent_folder_is_read_as_one() {
        let dir = tempfile::tempdir().unwrap();
        let apple = dir.path().join("Sent Messages.mbox");
        std::fs::create_dir(&apple).unwrap();
        let named = [
            dir.path().join("Sent"),
            dir.path().join("Sent.mbox"),
            dir.path().join("Gesendete Elemente.MBX"),
            dir.path().join("Éléments envoyés.mbox"),
            apple.join("mbox"),
        ];
        for path in &named {
            std::fs::write(path, SENT_MBOX).unwrap();
            let convos = import(path);
            let all: Vec<_> = convos.iter().flat_map(|c| &c.messages).collect();
            let sent = all.iter().find(|m| m.external_id == "s1@work.example").unwrap();
            let passed_on = all.iter().find(|m| m.external_id == "r1@example.com").unwrap();
            assert_eq!(sent.metadata["sentFolder"], serde_json::json!(true), "{path:?}");
            assert!(passed_on.metadata.get("sentFolder").is_none(), "passed on, so its writer is someone else");
            let report = MboxSource.validate(path).unwrap();
            assert_eq!(report.sent_folder, 1);
            assert!(
                report.warnings.iter().any(|w| w.starts_with("A file here is named as your Sent folder")),
                "{path:?}"
            );
        }

        for other in ["Inbox.mbox", "Sent Archive.mbox", "Unsent.mbox", "mail.mbox"] {
            let path = dir.path().join(other);
            std::fs::write(&path, SENT_MBOX).unwrap();
            let convos = import(&path);
            assert!(
                convos.iter().flat_map(|c| &c.messages).all(|m| m.metadata.get("sentFolder").is_none()),
                "{other} says nothing about the folder"
            );
            let report = MboxSource.validate(&path).unwrap();
            assert_eq!(report.sent_folder, 0);
            assert!(!report.warnings.iter().any(|w| w.contains("Sent folder")));
        }
        // A folder of an export's files: only what came from the Sent file.
        let folder = tempfile::tempdir().unwrap();
        std::fs::write(folder.path().join("Sent.mbox"), SENT_MBOX).unwrap();
        std::fs::write(folder.path().join("Inbox.mbox"), MBOX).unwrap();
        let report = MboxSource.validate(folder.path()).unwrap();
        assert_eq!(report.sent_folder, 1);
    }
}
