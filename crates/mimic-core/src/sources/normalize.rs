//! Turning raw export text into the words the user actually wrote.
//!
//! This matters more than it looks. If a quoted reply chain survives into the
//! database, every metric — message length, greeting rate, vocabulary — is
//! measuring the other person's writing as though it were the user's. The
//! rules here are deliberately conservative: when a line's role is ambiguous
//! it is kept, because dropping the user's words is worse than keeping a
//! stray one.

/// Strip quoted history, forwarded blocks and the "On <date> X wrote:"
/// attribution line that precedes them. Trailing whitespace goes; interior
/// blank lines stay, because paragraph breaks are part of how someone writes.
pub fn strip_quoted_reply(body: &str) -> String {
    let mut kept: Vec<&str> = Vec::new();
    for line in body.lines() {
        let t = line.trim_start();
        if t.starts_with('>') {
            continue;
        }
        if is_attribution(t) {
            // Everything after an attribution line is the quoted message,
            // whether or not the exporter marked it with '>'.
            break;
        }
        if is_forward_banner(t) || is_outlook_header_block(t) {
            break;
        }
        kept.push(line);
    }
    trim_blank_edges(&kept).join("\n")
}

/// Remove a signature block introduced by the conventional `-- ` delimiter, or
/// by a `Sent from my …` line. A sign-off the user typed ("Thanks, C") is NOT
/// a signature and is kept: it is one of the most characteristic things about
/// how a person writes.
pub fn strip_signature(body: &str) -> String {
    let lines: Vec<&str> = body.lines().collect();
    let mut end = lines.len();
    for (i, line) in lines.iter().enumerate() {
        let t = line.trim_end();
        if t == "--" || t == "-- " || t.trim() == "--" {
            end = i;
            break;
        }
        let lower = line.trim().to_lowercase();
        if lower.starts_with("sent from my ") || lower.starts_with("get outlook for ") {
            end = i;
            break;
        }
    }
    trim_blank_edges(&lines[..end]).join("\n")
}

/// The full cleanup a connector applies before handing a body over.
pub fn clean_body(body: &str) -> String {
    let body = body.replace("\r\n", "\n").replace('\r', "\n");
    strip_signature(&strip_quoted_reply(&body))
}

fn trim_blank_edges<'a>(lines: &[&'a str]) -> Vec<&'a str> {
    let mut start = 0;
    let mut end = lines.len();
    while start < end && lines[start].trim().is_empty() {
        start += 1;
    }
    while end > start && lines[end - 1].trim().is_empty() {
        end -= 1;
    }
    lines[start..end].to_vec()
}

/// "On Tue, 3 Feb 2026 at 09:14, Ada <ada@example.com> wrote:" and its many
/// dialects. Requires both an "On"/"El"/"Le" opener and a "wrote:"-style
/// terminator so an ordinary sentence starting with "On" is not eaten.
fn is_attribution(line: &str) -> bool {
    let lower = line.trim().to_lowercase();
    if lower.len() > 300 {
        return false;
    }
    let opens = lower.starts_with("on ") || lower.starts_with("el ") || lower.starts_with("le ");
    let closes = lower.ends_with("wrote:")
        || lower.ends_with("escribió:")
        || lower.ends_with("a écrit :")
        || lower.ends_with("wrote：");
    opens && closes
}

fn is_forward_banner(line: &str) -> bool {
    let t = line.trim().trim_matches('-').trim();
    let lower = t.to_lowercase();
    lower == "forwarded message" || lower == "original message" || lower.starts_with("begin forwarded message")
}

/// Outlook's reply format has no '>' marker; it starts a header block instead.
fn is_outlook_header_block(line: &str) -> bool {
    let lower = line.trim().to_lowercase();
    lower.starts_with("from:") && lower.contains('@')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoted_history_is_removed_but_the_reply_is_kept_whole() {
        let body = "Yes, Tuesday works.\n\nI'll bring the numbers.\n\nOn Tue, 3 Feb 2026 at 09:14, Ada <ada@example.com> wrote:\n> Does Tuesday work?\n> A";
        assert_eq!(strip_quoted_reply(body), "Yes, Tuesday works.\n\nI'll bring the numbers.");
    }

    #[test]
    fn a_sentence_that_merely_begins_with_on_survives() {
        let body = "On reflection I think we should wait.\nLet me know.";
        assert_eq!(strip_quoted_reply(body), body);
    }

    #[test]
    fn outlook_style_replies_with_no_quote_marker_are_cut() {
        let body = "Sounds good.\n\nFrom: Ada <ada@example.com>\nSent: Tuesday\nTo: C\nSubject: Re: numbers\n\nDoes Tuesday work?";
        assert_eq!(strip_quoted_reply(body), "Sounds good.");
    }

    #[test]
    fn forwarded_blocks_are_cut_at_the_banner() {
        let body = "Passing this on.\n\n---------- Forwarded message ----------\nFrom: someone";
        assert_eq!(strip_quoted_reply(body), "Passing this on.");
    }

    #[test]
    fn signatures_go_but_sign_offs_stay() {
        let sig = "Thanks, C\n\n--\nC Haynes\nFormicaria\n+1 555 010 9999";
        assert_eq!(strip_signature(sig), "Thanks, C", "the -- block is a signature");
        let signoff = "Thanks, C";
        assert_eq!(strip_signature(signoff), signoff, "a typed sign-off is how they write");
        assert_eq!(strip_signature("On my way\n\nSent from my iPhone"), "On my way");
    }

    #[test]
    fn cleanup_normalizes_line_endings_and_trims_edges() {
        let body = "\r\n\r\nHello there.\r\n\r\nSecond paragraph.\r\n\r\n";
        assert_eq!(clean_body(body), "Hello there.\n\nSecond paragraph.");
    }

    #[test]
    fn a_message_that_is_only_quoted_text_becomes_empty() {
        // Import drops these rather than counting them as the user's writing.
        assert_eq!(clean_body("> everything\n> was quoted"), "");
    }
}
