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

/// One parsed mail, before threading.
#[derive(Debug, Clone)]
struct Mail {
    message_id: String,
    in_reply_to: Option<String>,
    references: Vec<String>,
    subject: Option<String>,
    from: AuthorRef,
    date: Option<String>,
    body: String,
}

impl CommunicationSource for MboxSource {
    fn metadata(&self) -> SourceMetadata {
        SourceMetadata {
            connector: "mbox",
            display_name: "Email mailbox (mbox)",
            channel: "email",
            description: "A standard .mbox file, as exported by Gmail Takeout, Thunderbird and most mail clients.",
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
            read_mbox(&path, &mut mails)?;
        }
        for convo in thread(mails) {
            sink(convo)?;
        }
        Ok(())
    }
}

fn read_mbox(path: &Path, out: &mut Vec<Mail>) -> SourceResult<()> {
    let reader = BufReader::new(File::open(path)?);
    let mut current: Vec<String> = Vec::new();
    for line in reader.lines() {
        let line = line?;
        if is_separator(&line) {
            if !current.is_empty() {
                if let Some(m) = parse_mail(&current) {
                    out.push(m);
                }
                current.clear();
            }
            continue;
        }
        current.push(line);
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

fn parse_mail(lines: &[String]) -> Option<Mail> {
    if lines.is_empty() {
        return None;
    }
    let mut headers: Vec<(String, String)> = Vec::new();
    let mut body_start = lines.len();
    for (i, line) in lines.iter().enumerate() {
        if line.trim().is_empty() {
            body_start = i + 1;
            break;
        }
        // A continuation line begins with whitespace and extends the previous
        // header. Long Subject and References headers are routinely folded.
        if line.starts_with(' ') || line.starts_with('\t') {
            if let Some(last) = headers.last_mut() {
                last.1.push(' ');
                last.1.push_str(line.trim());
            }
            continue;
        }
        match line.split_once(':') {
            Some((k, v)) => headers.push((k.trim().to_lowercase(), v.trim().to_string())),
            None => continue,
        }
    }
    let get = |name: &str| headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone());
    let raw_body = lines.get(body_start..).map(|l| l.join("\n")).unwrap_or_default();
    let body = clean_body(&raw_body);
    if body.trim().is_empty() {
        return None;
    }
    let from = parse_address(get("from").as_deref().unwrap_or_default());
    let message_id = get("message-id")
        .map(|s| s.trim_matches(['<', '>']).to_string())
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
        in_reply_to: get("in-reply-to").map(|s| s.trim_matches(['<', '>']).to_string()).filter(|s| !s.is_empty()),
        references,
        subject: get("subject").filter(|s| !s.trim().is_empty()),
        from,
        date: get("date").and_then(|d| parse_date(&d)),
        body,
    })
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
fn thread(mails: Vec<Mail>) -> Vec<DiscoveredConversation> {
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
            group.sort_by(|a, b| a.date.cmp(&b.date).then(a.message_id.cmp(&b.message_id)));
            let subject = group.iter().find_map(|m| m.subject.clone());
            let messages = group
                .into_iter()
                .map(|m| RawMessage {
                    external_id: m.message_id,
                    author: m.from,
                    sent_at: m.date,
                    body: m.body,
                    metadata: match &m.subject {
                        Some(s) => json!({ "subject": s }),
                        None => json!({}),
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
}
