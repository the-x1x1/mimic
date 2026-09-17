//! Identifier and time helpers shared by every table.

use chrono::{DateTime, SecondsFormat, Utc};

/// New random UUID v4 as a lowercase hyphenated string.
pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Current UTC time as RFC 3339 with millisecond precision (stored in TEXT columns).
pub fn now_rfc3339() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// Format a `DateTime<Utc>` the same way `now_rfc3339` does.
pub fn fmt_rfc3339(dt: DateTime<Utc>) -> String {
    dt.to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// SHA-256 hex digest of bytes.
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(bytes))
}

/// Deterministic hash of a JSON value (keys sorted) so identical settings
/// produce identical `rawSettingsHash` values regardless of source ordering.
pub fn stable_json_hash(value: &serde_json::Value) -> String {
    fn canon(v: &serde_json::Value, out: &mut String) {
        match v {
            serde_json::Value::Object(map) => {
                let mut keys: Vec<&String> = map.keys().collect();
                keys.sort();
                out.push('{');
                for (i, k) in keys.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push_str(&serde_json::to_string(k).unwrap_or_default());
                    out.push(':');
                    canon(&map[*k], out);
                }
                out.push('}');
            }
            serde_json::Value::Array(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    canon(item, out);
                }
                out.push(']');
            }
            other => out.push_str(&other.to_string()),
        }
    }
    let mut s = String::new();
    canon(value, &mut s);
    sha256_hex(s.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn stable_hash_ignores_key_order() {
        let a = json!({"b": 1, "a": {"y": 2, "x": [1, 2]}});
        let b = json!({"a": {"x": [1, 2], "y": 2}, "b": 1});
        assert_eq!(stable_json_hash(&a), stable_json_hash(&b));
        let c = json!({"a": {"x": [2, 1], "y": 2}, "b": 1});
        assert_ne!(stable_json_hash(&a), stable_json_hash(&c));
    }

    #[test]
    fn ids_are_unique() {
        assert_ne!(new_id(), new_id());
        assert_eq!(new_id().len(), 36);
    }
}
