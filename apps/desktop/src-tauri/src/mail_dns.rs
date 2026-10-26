//! Who handles a domain's mail: its mail exchangers (MX records), from this
//! computer's own resolver (0.10.0-alpha.34).
//!
//! Asked only for the domain of an address typed into Connect a mailbox
//! when no provider is known for it, so the dialog can say whose servers
//! they are — Google Workspace, Microsoft 365 and the like (contracts'
//! `mailHostFromExchangers`) — and fill them in. It is the same question the
//! computer asks of its resolver before connecting to any server there;
//! nothing is sent anywhere else, and nothing is kept.

/// Most exchangers a domain is read for; more is not a mail setup.
const MOST: usize = 16;

/// A domain as it can be looked up — letters, digits and hyphens in labels
/// of 63 at most, at least two labels, 253 characters in all — lower-cased
/// and without a final dot; anything else is not asked about.
pub(crate) fn lookable(domain: &str) -> Option<String> {
    let d = domain.trim().trim_end_matches('.').to_ascii_lowercase();
    if d.len() > 253 || !d.contains('.') {
        return None;
    }
    let labels_ok = d.split('.').all(|l| {
        !l.is_empty()
            && l.len() <= 63
            && !l.starts_with('-')
            && !l.ends_with('-')
            && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    });
    labels_ok.then_some(d)
}

/// The domain's mail exchangers, most preferred first, each once, lower-cased
/// and without a final dot: none when the domain can't be looked up, has
/// none, doesn't exist, or the resolver can't be reached.
pub(crate) fn exchangers(domain: &str) -> Vec<String> {
    let Some(d) = lookable(domain) else { return Vec::new() };
    in_order(query(&d))
}

/// `(preference, name)` pairs as names, lowest preference first.
fn in_order(mut found: Vec<(u16, String)>) -> Vec<String> {
    found.sort_by_key(|(preference, _)| *preference);
    let mut out: Vec<String> = Vec::new();
    for (_, name) in found {
        let name = name.trim().trim_end_matches('.').to_ascii_lowercase();
        if !name.is_empty() && !out.contains(&name) && out.len() < MOST {
            out.push(name);
        }
    }
    out
}

#[cfg(windows)]
fn query(domain: &str) -> Vec<(u16, String)> {
    use windows_sys::Win32::NetworkManagement::Dns::{
        DnsFree, DnsFreeRecordList, DnsQuery_W, DNS_QUERY_STANDARD, DNS_RECORDA, DNS_RECORDW, DNS_TYPE_MX,
    };

    let name: Vec<u16> = domain.encode_utf16().chain(std::iter::once(0)).collect();
    // The binding names the list's type by its ANSI form; `DnsQuery_W`
    // fills in the wide one, which is laid out the same with wide strings.
    let mut results: *mut DNS_RECORDA = std::ptr::null_mut();
    // SAFETY: `name` is NUL-terminated and outlives the call; the extra and
    // reserved pointers may be null; on success `results` is a list the
    // caller frees with `DnsFreeRecordList`, which is done below.
    let status = unsafe {
        DnsQuery_W(
            name.as_ptr(),
            DNS_TYPE_MX,
            DNS_QUERY_STANDARD,
            std::ptr::null_mut(),
            &mut results,
            std::ptr::null_mut(),
        )
    };
    if status != 0 || results.is_null() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut at = results.cast::<DNS_RECORDW>();
    while !at.is_null() {
        // SAFETY: a record of the list the call returned, not yet freed; its
        // data is an MX record's when its type says so.
        let record = unsafe { &*at };
        if record.wType == DNS_TYPE_MX {
            let mx = unsafe { record.Data.MX };
            if let Some(name) = wide(mx.pNameExchange) {
                out.push((mx.wPreference, name));
            }
        }
        at = record.pNext;
    }
    // SAFETY: the list the call returned, freed once, not touched after.
    unsafe { DnsFree(results.cast_const().cast(), DnsFreeRecordList) };
    out
}

/// A NUL-terminated UTF-16 string from a record still held; at most 1,024
/// units are read.
#[cfg(windows)]
fn wide(p: *const u16) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let mut len = 0usize;
    // SAFETY: the string is NUL-terminated and lives as long as its record.
    unsafe {
        while len < 1024 && *p.add(len) != 0 {
            len += 1;
        }
        String::from_utf16(std::slice::from_raw_parts(p, len)).ok()
    }
}

/// Elsewhere there is no lookup yet: the dialog asks for the server.
#[cfg(not(windows))]
fn query(_domain: &str) -> Vec<(u16, String)> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_domain_is_looked_up() {
        assert_eq!(lookable(" Formicaria.US. ").as_deref(), Some("formicaria.us"));
        assert_eq!(lookable("mail.example.co.uk").as_deref(), Some("mail.example.co.uk"));
        for not in ["", "localhost", "example..com", "-x.com", "x-.com", "ex ample.com", "exa_mple.com", "a@b.com"] {
            assert_eq!(lookable(not), None, "{not:?}");
        }
        assert_eq!(lookable(&format!("{}.com", "a".repeat(64))), None, "a label of 64");
        assert_eq!(lookable(&format!("{}.com", vec!["a".repeat(60); 5].join("."))), None, "longer than 253");
    }

    #[test]
    fn exchangers_come_most_preferred_first_and_once() {
        let found = vec![
            (10, "ALT1.ASPMX.L.GOOGLE.COM.".to_string()),
            (1, "aspmx.l.google.com.".to_string()),
            (10, "alt1.aspmx.l.google.com".to_string()),
            (5, " ".to_string()),
        ];
        assert_eq!(in_order(found), vec!["aspmx.l.google.com", "alt1.aspmx.l.google.com"]);
        let many: Vec<(u16, String)> = (0..40).map(|i| (i, format!("mx{i}.example.com"))).collect();
        assert_eq!(in_order(many).len(), MOST);
    }

    /// The resolver is asked, and a name that can't exist (RFC 2606's
    /// `.invalid`) comes back with nothing, online or not.
    #[test]
    fn a_domain_that_cannot_exist_has_no_exchangers() {
        assert!(exchangers("mimic-test.invalid").is_empty());
        assert!(exchangers("not a domain").is_empty());
    }
}
