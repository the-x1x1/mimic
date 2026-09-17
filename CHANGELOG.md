# Changelog

All notable changes to Mimic are documented here. The format follows Keep a Changelog; versions follow SemVer with pre-release tags for alpha/beta builds.

## [0.1.0-alpha.1] — 2026-09-16

Foundation + real ingest. This is a pre-release: the ingest pipeline is real and tested end to end; training, prediction and Lightroom apply are not part of this build.

### Added

- Tauri 2 desktop shell with dark graphite theme, onboarding flow (Lightroom / folders / clearly labelled DEMO), Home, Styles (list, detail with Overview/Training Data/Versions/Corrections), Sessions and Review empty states, Settings (General, Lightroom, Performance, Storage, Privacy, Updates, Diagnostics).
- SQLite database with forward-only transactional migrations, automatic backup before migrating an existing database, WAL, foreign keys, and all 21 tables from the data model.
- Persistent job system: queued/running/completed/failed/canceled/interrupted, heartbeat, item-level progress, cancellation between items, restart recovery that re-queues only resumable jobs.
- Python engine sidecar (`mimic-engine serve`): NDJSON stdio protocol with request correlation, structured errors, progress events, 32 MiB message cap; automatic restart with a bounded budget.
- Folder scanner: RAW/rendered detection, XMP/ACR pairing by directory + basename, duplicate-basename and orphan-sidecar reporting, fast identity hash, EXIF extraction.
- Read-only XMP parser (`xmp_parser_v1`): attribute and element forms, curves, structured masks and Look tables, unknown key preservation, metadata summary, malformed-file isolation.
- RAW preview decoding (LibRaw via rawpy, embedded preview first) with an on-disk cache; deterministic image statistics (`features_v1`), heuristic scene labels, `stats_v1` embeddings stored as `.npy`, ONNX encoder manager with SHA-256 verification and mandatory fallback.
- Canonical EditDNA (`edit_mapping_v1`, 104 controls across 11 families) shared by Rust, Python and TypeScript, with golden fixtures for modern (PV 15.4 with masks and unknown keys), PV 2012 and legacy PV 2010 XMP.
- Lightroom Classic plugin (`Mimic.lrplugin`): discovery-file handshake, long-poll command loop, runtime capability probe, catalog listing, develop-settings read, before-snapshot, plugin-preset apply with read-back, correction-state collection, Plugin Manager panel, dependency-free Lua JSON.
- Loopback-only bridge server with per-launch 256-bit token, body limits, origin rejection, command queue with timeouts, liveness sweep and reconnect handling; fake-plugin integration tests.
- Capability matrix derived from the probe: supported / observed-not-writable / unsupported per control; apply and snapshot gated on runtime flags.
- Data quality report: assets, valid pairs, missing edits, Lightroom-connected vs sidecar-only coverage, ACR heavy-edit count, local-edit count, camera and shoot-day distribution, recommendation level, honest warnings.
- Diagnostics bundle with no tokens and redacted paths; structured JSON logs with daily rotation.
- Updater plumbing: Tauri updater with embedded public key, 6-hour jittered background checks, install guard that refuses while jobs run, persisted update state; GitHub Actions release workflow that builds, signs, generates `latest.json` and checksums, and creates a draft release.
- CI: frontend, Rust (with the real-engine end-to-end test), Python, Lua plugin, security audit and secret scan.

### Known limitations

- Training (0.2.0), sessions/prediction/review/apply (0.3.0) and correction sync (0.4.0) are not implemented.
- The plugin apply path is verified against fixtures and a fake plugin only: NEEDS REAL-LIGHTROOM QA.
- The updater configuration ships a development public key; a non-alpha release is refused by `verify-release.ps1` and the release workflow until it is replaced.
