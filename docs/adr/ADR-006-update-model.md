# ADR-006 — Signed GitHub Releases updater, install only when idle

**Status**: accepted.

**Decision**: Tauri updater reading `latest.json` from the latest GitHub Release; minisign key pair with the private key only in CI secrets; background checks every 6 h with jitter; automatic download by default; installation only when no job is active and on restart; development key permitted for alpha tags only.

**Consequences**: Requires the owner to generate and register a real key pair before stable; per-user NSIS install keeps updates unattended; no auto-downgrade.
