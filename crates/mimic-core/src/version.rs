//! Single source of version truth for the native side.
//!
//! `APP_VERSION` comes from Cargo, which `scripts/sync-version.mjs` keeps in
//! lock-step with the root package, the desktop package, `tauri.conf.json`,
//! the Python engine and the Lightroom plugin.

/// Application version (semver, may carry a pre-release tag).
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Version of the Lightroom plugin <-> desktop bridge protocol.
/// Bump when a request/response shape changes incompatibly.
pub const BRIDGE_PROTOCOL_VERSION: u32 = 1;

/// Version of the desktop <-> Python engine NDJSON protocol.
pub const ENGINE_PROTOCOL_VERSION: u32 = 1;

/// Minimum plugin version the desktop accepts on handshake.
pub const MIN_PLUGIN_VERSION: &str = "0.1.0-alpha.1";

/// Compare two dotted semver strings ignoring pre-release tags.
/// Returns `Ordering` of the numeric core (major.minor.patch).
pub fn compare_core_versions(a: &str, b: &str) -> std::cmp::Ordering {
    fn core(v: &str) -> [u64; 3] {
        let v = v.trim_start_matches('v');
        let v = v.split(['-', '+']).next().unwrap_or("0");
        let mut out = [0u64; 3];
        for (i, part) in v.split('.').take(3).enumerate() {
            out[i] = part.parse().unwrap_or(0);
        }
        out
    }
    core(a).cmp(&core(b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cmp::Ordering;

    #[test]
    fn compares_numeric_core() {
        assert_eq!(compare_core_versions("0.1.0-alpha.1", "0.1.0"), Ordering::Equal);
        assert_eq!(compare_core_versions("0.2.0", "0.1.9"), Ordering::Greater);
        assert_eq!(compare_core_versions("v0.1.0", "0.1.1"), Ordering::Less);
    }

    #[test]
    fn app_version_is_semver_like() {
        assert!(APP_VERSION.split('.').count() >= 3, "{APP_VERSION}");
    }
}
