//! Getting the text a person wrote out of a MIME message.
//!
//! Almost no real email is a plain-text body under a header block. A Gmail
//! Takeout mbox, and every message an IMAP server returns, is MIME:
//! `multipart/alternative` with a plain and an HTML part, bodies in base64 or
//! quoted-printable, headers in RFC 2047 encoded words, charsets that are not
//! UTF-8. Treating the raw text as the message would measure boundary lines
//! and base64 as the user's writing.
//!
//! What this module does, and does not:
//!
//! * Walks multipart structure recursively and picks the first `text/plain`
//!   part that is not an attachment; failing that, the first `text/html`
//!   part, converted to text. A message with neither has no body.
//! * Decodes base64 and quoted-printable, and the charsets that make up
//!   nearly all mail: UTF-8, US-ASCII, ISO-8859-1 and Windows-1252. Anything
//!   else is decoded as UTF-8 with replacement characters rather than refused.
//! * Decodes RFC 2047 encoded words in headers (`=?UTF-8?B?…?=`).
//! * Does not render HTML. It drops `<style>`, `<script>` and `<head>`, turns
//!   block elements and `<br>` into line breaks, removes the remaining tags and
//!   decodes the common entities. Good enough to recover what someone typed;
//!   not a browser.
//!
//! No dependency is added for any of it, because every piece is small and the
//! behaviour on malformed input — keep going, lose as little as possible — is
//! easier to guarantee in code this size than to configure in someone else's.

/// Header names are lowercased; values are unfolded but not decoded.
pub type Headers = Vec<(String, String)>;

/// Split a raw message into unfolded headers and the raw body bytes.
///
/// Bytes, not text: a body can be in any charset, and turning it into a Rust
/// string before its `Content-Type` has been read would destroy every
/// non-UTF-8 character in it. Headers are ASCII by definition (RFC 5322;
/// anything else in them is encoded words), so they are read leniently.
pub fn split_message(raw: &[u8]) -> (Headers, Vec<u8>) {
    let (head_end, body_start) = match find_blank_line(raw) {
        Some((h, b)) => (h, b),
        None => (raw.len(), raw.len()),
    };
    let head = String::from_utf8_lossy(&raw[..head_end]).replace("\r\n", "\n");
    (parse_headers(&head), raw[body_start..].to_vec())
}

/// The end of the header block and the start of the body: the first empty
/// line, in either line-ending convention.
fn find_blank_line(raw: &[u8]) -> Option<(usize, usize)> {
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'\n' {
            if raw.get(i + 1) == Some(&b'\n') {
                return Some((i, i + 2));
            }
            if raw.get(i + 1) == Some(&b'\r') && raw.get(i + 2) == Some(&b'\n') {
                return Some((i, i + 3));
            }
        }
        i += 1;
    }
    None
}

/// Unfold a header block into `(lowercase name, value)` pairs.
pub fn parse_headers(block: &str) -> Headers {
    let mut headers: Headers = Vec::new();
    for line in block.lines() {
        if line.starts_with(' ') || line.starts_with('\t') {
            if let Some(last) = headers.last_mut() {
                last.1.push(' ');
                last.1.push_str(line.trim());
            }
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            headers.push((k.trim().to_lowercase(), v.trim().to_string()));
        }
    }
    headers
}

pub fn header<'a>(headers: &'a Headers, name: &str) -> Option<&'a str> {
    headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
}

/// The readable text of a message body, given its headers. `None` when the
/// message has no text part at all (an image, a calendar invite, a bounce).
pub fn body_text(headers: &Headers, body: &[u8]) -> Option<String> {
    body_text_at(headers, body, 0)
}

/// Multipart nesting deeper than this is not mail anyone wrote; it is a
/// constructed message, and recursion on it is refused rather than followed.
const MAX_DEPTH: usize = 8;

fn body_text_at(headers: &Headers, body: &[u8], depth: usize) -> Option<String> {
    if depth > MAX_DEPTH {
        return None;
    }
    let content_type = header(headers, "content-type").unwrap_or("text/plain");
    let (mime, params) = parse_content_type(content_type);
    let encoding = header(headers, "content-transfer-encoding").unwrap_or("7bit").trim().to_lowercase();
    let disposition = header(headers, "content-disposition").unwrap_or("").to_lowercase();
    if disposition.starts_with("attachment") {
        return None;
    }
    if mime.starts_with("multipart/") {
        let boundary = param(&params, "boundary")?;
        let parts = split_multipart(body, &boundary);
        // Parse every part once; prefer plain text, then HTML, in order.
        let parsed: Vec<(Headers, Vec<u8>)> = parts.iter().map(|p| split_message(p)).collect();
        let pick = |want: &str| {
            parsed.iter().find_map(|(h, b)| {
                let (m, _) = parse_content_type(header(h, "content-type").unwrap_or("text/plain"));
                let is_multipart = m.starts_with("multipart/");
                if is_multipart || m == want {
                    body_text_at(h, b, depth + 1).filter(|t| !t.trim().is_empty())
                } else {
                    None
                }
            })
        };
        return pick("text/plain").or_else(|| pick("text/html"));
    }
    if mime != "text/plain" && mime != "text/html" {
        return None;
    }
    let bytes = match encoding.as_str() {
        "base64" => decode_base64(&String::from_utf8_lossy(body)),
        "quoted-printable" => decode_quoted_printable_bytes(body),
        _ => body.to_vec(),
    };
    let text = decode_charset(&bytes, param(&params, "charset").as_deref().unwrap_or("utf-8"));
    let text = text.replace("\r\n", "\n");
    Some(if mime == "text/html" { html_to_text(&text) } else { text })
}

/// `text/plain; charset="utf-8"` → (`text/plain`, [("charset", "utf-8")]).
pub fn parse_content_type(value: &str) -> (String, Vec<(String, String)>) {
    let mut pieces = split_params(value).into_iter();
    let mime = pieces.next().unwrap_or_default().trim().to_lowercase();
    let params = pieces
        .filter_map(|p| {
            let (k, v) = p.split_once('=')?;
            Some((k.trim().to_lowercase(), v.trim().trim_matches('"').to_string()))
        })
        .collect();
    (mime, params)
}

/// Split on semicolons that are not inside quotes.
fn split_params(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    for ch in value.chars() {
        match ch {
            '"' => {
                quoted = !quoted;
                cur.push(ch);
            }
            ';' if !quoted => out.push(std::mem::take(&mut cur)),
            _ => cur.push(ch),
        }
    }
    out.push(cur);
    out
}

fn param(params: &[(String, String)], name: &str) -> Option<String> {
    params.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone())
}

/// The parts between `--boundary` lines, excluding the preamble and anything
/// after the closing `--boundary--`.
fn split_multipart(body: &[u8], boundary: &str) -> Vec<Vec<u8>> {
    let delimiter = format!("--{boundary}");
    let close = format!("--{boundary}--");
    let mut parts = Vec::new();
    let mut current: Option<Vec<&[u8]>> = None;
    for line in body.split(|b| *b == b'\n') {
        let t = trim_end_ascii(line);
        if t == close.as_bytes() {
            if let Some(p) = current.take() {
                parts.push(p.join(&b'\n'));
            }
            break;
        }
        if t == delimiter.as_bytes() {
            if let Some(p) = current.take() {
                parts.push(p.join(&b'\n'));
            }
            current = Some(Vec::new());
            continue;
        }
        if let Some(p) = current.as_mut() {
            p.push(line);
        }
    }
    // A message cut off before its closing delimiter still has its last part.
    if let Some(p) = current.take() {
        parts.push(p.join(&b'\n'));
    }
    parts
}

fn trim_end_ascii(line: &[u8]) -> &[u8] {
    let mut end = line.len();
    while end > 0 && matches!(line[end - 1], b' ' | b'\t' | b'\r') {
        end -= 1;
    }
    &line[..end]
}

/// Standard base64, ignoring whitespace and anything outside the alphabet,
/// so a line-wrapped or slightly damaged body still decodes.
pub fn decode_base64(input: &str) -> Vec<u8> {
    fn val(c: u8) -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'+' | b'-' => Some(62),
            b'/' | b'_' => Some(63),
            _ => None,
        }
    }
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let mut buf: u32 = 0;
    let mut bits = 0;
    for &c in input.as_bytes() {
        if c == b'=' {
            break;
        }
        let Some(v) = val(c) else { continue };
        buf = (buf << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    out
}

/// RFC 2045 quoted-printable: `=XX` escapes and `=` soft line breaks.
pub fn decode_quoted_printable(input: &str) -> Vec<u8> {
    decode_quoted_printable_bytes(input.as_bytes())
}

pub fn decode_quoted_printable_bytes(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'=' {
            // Soft line break: "=" at the end of a line.
            if bytes.get(i + 1) == Some(&b'\n') {
                i += 2;
                continue;
            }
            if bytes.get(i + 1) == Some(&b'\r') && bytes.get(i + 2) == Some(&b'\n') {
                i += 3;
                continue;
            }
            let hex = |b: u8| (b as char).to_digit(16);
            if let (Some(h), Some(l)) = (bytes.get(i + 1).and_then(|b| hex(*b)), bytes.get(i + 2).and_then(|b| hex(*b)))
            {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

/// Windows-1252's 0x80–0x9F, which ISO-8859-1 leaves as control codes and
/// which mail from Windows clients is full of: curly quotes, dashes, the euro.
const CP1252_HIGH: [char; 32] = [
    '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8D}', 'Ž', '\u{8F}', '\u{90}', '‘', '’',
    '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9D}', 'ž', 'Ÿ',
];

pub fn decode_charset(bytes: &[u8], charset: &str) -> String {
    match charset.trim().to_lowercase().as_str() {
        "iso-8859-1" | "latin1" | "latin-1" | "iso8859-1" | "windows-1252" | "cp1252" | "iso-8859-15" => bytes
            .iter()
            .map(|&b| if (0x80..0xA0).contains(&b) { CP1252_HIGH[(b - 0x80) as usize] } else { b as char })
            .collect(),
        _ => String::from_utf8_lossy(bytes).into_owned(),
    }
}

/// Decode RFC 2047 encoded words in a header value. Whitespace between two
/// adjacent encoded words is dropped, as the RFC requires.
pub fn decode_header(value: &str) -> String {
    let mut out = String::new();
    let mut rest = value;
    let mut last_was_word = false;
    while let Some(start) = rest.find("=?") {
        let (before, after) = rest.split_at(start);
        let Some(word) = parse_encoded_word(after) else {
            out.push_str(before);
            out.push_str("=?");
            rest = &after[2..];
            last_was_word = false;
            continue;
        };
        if !(last_was_word && before.trim().is_empty()) {
            out.push_str(before);
        }
        out.push_str(&word.0);
        rest = &after[word.1..];
        last_was_word = true;
    }
    out.push_str(rest);
    out
}

/// `=?charset?B|Q?text?=` → (decoded text, bytes consumed).
fn parse_encoded_word(s: &str) -> Option<(String, usize)> {
    let inner = s.strip_prefix("=?")?;
    let (charset, inner) = inner.split_once('?')?;
    let (enc, inner) = inner.split_once('?')?;
    let end = inner.find("?=")?;
    let text = &inner[..end];
    if text.contains(' ') {
        return None;
    }
    let bytes = match enc.to_ascii_uppercase().as_str() {
        "B" => decode_base64(text),
        "Q" => decode_quoted_printable(&text.replace('_', " ")),
        _ => return None,
    };
    let consumed = 2 + charset.len() + 1 + enc.len() + 1 + end + 2;
    // A language suffix ("utf-8*en") is allowed and ignored.
    let charset = charset.split('*').next().unwrap_or(charset);
    Some((decode_charset(&bytes, charset), consumed))
}

/// HTML to the text a person would read, roughly.
pub fn html_to_text(html: &str) -> String {
    let bytes = html.as_bytes();
    let mut out = String::with_capacity(html.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'<' {
            let Some(close) = html[i..].find('>') else { break };
            // Tag names are ASCII; everything is compared ASCII-case-
            // insensitively on the original bytes, so a character whose
            // lowercase form has a different length (İ, K, Ω) can never shift
            // an offset onto the middle of a character.
            let tag = html[i + 1..i + close].trim();
            let closing = tag.starts_with('/');
            let name: String = tag
                .trim_start_matches('/')
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric())
                .map(|c| c.to_ascii_lowercase())
                .collect();
            // Skip the whole element for things that are never text.
            if !closing && matches!(name.as_str(), "style" | "script" | "head" | "title") {
                let end_tag = format!("</{name}");
                match find_ascii_ci(&bytes[i..], end_tag.as_bytes()) {
                    Some(e) => {
                        let after = i + e;
                        i = after + html[after..].find('>').map(|x| x + 1).unwrap_or(bytes.len() - after);
                    }
                    None => i = bytes.len(),
                }
                continue;
            }
            if matches!(name.as_str(), "br" | "p" | "div" | "tr" | "li" | "h1" | "h2" | "h3" | "blockquote")
                && !out.ends_with('\n')
            {
                out.push('\n');
            }
            i += close + 1;
            continue;
        }
        let next = html[i..].find('<').map(|n| i + n).unwrap_or(bytes.len());
        out.push_str(&decode_entities(&html[i..next]));
        i = next;
    }
    // Collapse the whitespace HTML ignores, keep line breaks.
    let lines: Vec<String> = out.lines().map(|l| l.split_whitespace().collect::<Vec<_>>().join(" ")).collect();
    let mut text = lines.join("\n");
    while text.contains("\n\n\n") {
        text = text.replace("\n\n\n", "\n\n");
    }
    text.trim().to_string()
}

/// Byte offset of `needle` in `hay`, comparing ASCII letters without case.
/// `needle` is ASCII, so any match starts on a character boundary of `hay`.
fn find_ascii_ci(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    (0..=hay.len() - needle.len()).find(|&i| hay[i..i + needle.len()].eq_ignore_ascii_case(needle))
}

fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let tail = &rest[amp..];
        let Some(semi) = tail.find(';').filter(|&n| n <= 10) else {
            out.push('&');
            rest = &tail[1..];
            continue;
        };
        let entity = &tail[1..semi];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" | "#39" => Some('\''),
            "nbsp" => Some(' '),
            "rsquo" => Some('’'),
            "lsquo" => Some('‘'),
            "rdquo" => Some('”'),
            "ldquo" => Some('“'),
            "mdash" => Some('—'),
            "ndash" => Some('–'),
            "hellip" => Some('…'),
            e if e.starts_with("#x") || e.starts_with("#X") => {
                u32::from_str_radix(&e[2..], 16).ok().and_then(char::from_u32)
            }
            e if e.starts_with('#') => e[1..].parse::<u32>().ok().and_then(char::from_u32),
            _ => None,
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &tail[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_of(raw: &str) -> Option<String> {
        text_of_bytes(raw.as_bytes())
    }

    fn text_of_bytes(raw: &[u8]) -> Option<String> {
        let (h, b) = split_message(raw);
        body_text(&h, &b)
    }

    #[test]
    fn a_plain_message_is_its_own_body() {
        assert_eq!(text_of("From: a@b.c\nSubject: hi\n\nyep, sending now\n").as_deref(), Some("yep, sending now\n"));
    }

    #[test]
    fn multipart_alternative_takes_the_plain_part_not_the_html() {
        let raw = "Content-Type: multipart/alternative; boundary=\"XYZ\"\n\nThis is a MIME message.\n--XYZ\nContent-Type: text/plain; charset=utf-8\n\nsure, tuesday works\n--XYZ\nContent-Type: text/html; charset=utf-8\n\n<div>sure, <b>tuesday</b> works</div>\n--XYZ--\n";
        assert_eq!(text_of(raw).unwrap().trim(), "sure, tuesday works");
    }

    #[test]
    fn html_only_mail_is_converted_rather_than_dropped() {
        let raw = "Content-Type: text/html; charset=utf-8\n\n<html><head><style>p{color:red}</style></head><body><p>Hi Ada,</p><p>That&rsquo;s fine &amp; tuesday works.<br>C</p></body></html>";
        assert_eq!(text_of(raw).unwrap(), "Hi Ada,\nThat’s fine & tuesday works.\nC");
    }

    #[test]
    fn base64_and_quoted_printable_bodies_are_decoded() {
        // "yes — I'll be there" in UTF-8, base64 over two lines.
        let b64 = "Content-Type: text/plain; charset=utf-8\nContent-Transfer-Encoding: base64\n\neWVzIOKAlCBJJ2xs\nIGJlIHRoZXJl\n";
        assert_eq!(text_of(b64).unwrap(), "yes — I'll be there");
        let qp = "Content-Type: text/plain; charset=utf-8\nContent-Transfer-Encoding: quoted-printable\n\nyes =E2=80=94 I'll be th=\nere\n";
        assert_eq!(text_of(qp).unwrap().trim(), "yes — I'll be there");
    }

    #[test]
    fn windows_1252_punctuation_survives() {
        let raw = "Content-Type: text/plain; charset=windows-1252\nContent-Transfer-Encoding: quoted-printable\n\nthat=92s fine =96 see you=85\n";
        assert_eq!(text_of(raw).unwrap().trim(), "that’s fine – see you…");
    }

    #[test]
    fn nested_multipart_with_an_attachment_finds_the_words() {
        let raw = concat!(
            "Content-Type: multipart/mixed; boundary=outer\n\n",
            "--outer\n",
            "Content-Type: multipart/alternative; boundary=inner\n\n",
            "--inner\nContent-Type: text/plain\n\nnotes attached, shout if anything's off\n",
            "--inner\nContent-Type: text/html\n\n<p>notes attached</p>\n",
            "--inner--\n",
            "--outer\n",
            "Content-Type: text/plain; name=notes.txt\nContent-Disposition: attachment; filename=notes.txt\n\nNOT THE MESSAGE\n",
            "--outer--\n"
        );
        assert_eq!(text_of(raw).unwrap().trim(), "notes attached, shout if anything's off");
    }

    #[test]
    fn a_message_with_no_text_has_no_body() {
        let raw = "Content-Type: multipart/mixed; boundary=b\n\n--b\nContent-Type: image/png\nContent-Transfer-Encoding: base64\n\niVBORw0KGgo=\n--b--\n";
        assert_eq!(text_of(raw), None);
        assert_eq!(text_of("Content-Type: text/calendar\n\nBEGIN:VCALENDAR\n"), None);
    }

    #[test]
    fn encoded_word_headers_are_decoded() {
        assert_eq!(decode_header("=?UTF-8?B?QWRhIEzDvGJlY2s=?= <ada@example.com>"), "Ada Lübeck <ada@example.com>");
        assert_eq!(decode_header("=?iso-8859-1?Q?Caf=E9_tomorrow?="), "Café tomorrow");
        assert_eq!(
            decode_header("Re: =?UTF-8?Q?a?= =?UTF-8?Q?b?="),
            "Re: ab",
            "whitespace between adjacent words goes"
        );
        assert_eq!(decode_header("plain subject"), "plain subject");
        assert_eq!(decode_header("broken =?x"), "broken =?x");
    }

    #[test]
    fn a_truncated_multipart_keeps_what_arrived() {
        let raw = "Content-Type: multipart/alternative; boundary=\"q\"\n\n--q\nContent-Type: text/plain\n\ncut off mid";
        assert_eq!(text_of(raw).unwrap(), "cut off mid");
    }

    #[test]
    fn crlf_line_endings_parse_like_lf() {
        let raw = "Content-Type: text/plain\r\nSubject: x\r\n\r\nhello there\r\n";
        assert_eq!(text_of(raw).unwrap().trim(), "hello there");
    }

    #[test]
    fn entities_and_numeric_references_decode() {
        assert_eq!(decode_entities("a &lt;b&gt; &#8212; &#x2019; &unknown; & done"), "a <b> — ’ &unknown; & done");
    }

    /// Characters whose lowercase form is a different length used to shift
    /// every offset after them, which panicked or leaked CSS into the body.
    #[test]
    fn html_with_length_changing_characters_neither_panics_nor_leaks() {
        let raw = "Content-Type: text/html; charset=utf-8\n\n<p>\u{130}\u{130}\u{e9}</p><head>x</head><div>\u{130}yi</div><STYLE>p{color:red}</style>ok";
        let text = text_of(raw).unwrap();
        assert!(!text.contains("color:red"), "{text}");
        assert!(text.contains("\u{130}yi") && text.ends_with("ok"), "{text}");
        assert_eq!(html_to_text("<p>\u{212a}elvin \u{2126}hm</p>"), "\u{212a}elvin \u{2126}hm");
    }

    /// Latin-1 sent as 8bit is the common case from older clients; decoding
    /// it as UTF-8 first would put replacement characters in the user's words.
    #[test]
    fn an_eight_bit_body_is_decoded_from_its_own_charset() {
        let mut raw =
            b"Content-Type: text/plain; charset=iso-8859-1\nContent-Transfer-Encoding: 8bit\n\nJ'arrive ".to_vec();
        raw.push(0xE0); // "à" in ISO-8859-1
        raw.extend_from_slice(b" midi\n");
        assert_eq!(text_of_bytes(&raw).unwrap().trim(), "J'arrive \u{e0} midi");
    }

    #[test]
    fn absurd_nesting_is_refused_rather_than_followed() {
        let mut raw = String::new();
        for d in 0..20 {
            raw.push_str(&format!("Content-Type: multipart/mixed; boundary=b{d}\n\n--b{d}\n"));
        }
        raw.push_str("Content-Type: text/plain\n\ndeep\n");
        assert_eq!(text_of(&raw), None);
    }
}
