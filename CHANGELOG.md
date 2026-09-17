# Changelog

All notable changes to Mimic are documented here. The format follows Keep a Changelog; versions follow SemVer with pre-release tags for alpha/beta builds.

## [0.2.0-alpha.1] — 2026-09-17

First Style Brain. Training is real, reproducible and measured; prediction is exposed through the engine and exercised end to end, but there is still no Sessions/Review UI or Lightroom apply (0.3.0).

### Added

- Training pipeline in the engine (`training.train`): training-set builder over normalized EditDNA pairs with filters (no snapshot, no features, no meaningful edits, too many unknown keys), session-grouped train/validation/holdout split that never puts one shoot on both sides (deterministic per seed; honest fallbacks for two shoots and single-shoot libraries), baselines (global median, camera/lens-conditioned median, distance-weighted KNN), per-control ridge residual on top of leave-one-out KNN (`hybrid_knn_residual`), per-control and per-family metrics in raw and normalized units (MAE/RMSE/nMAE/p50/p90/p95), acceptance proxy explicitly labelled as not a No-Touch Rate, baseline comparison, reproducible training config (seed, versions, fingerprint, dependency versions), SHA-256 hashed artifacts.
- Confidence calibration persisted with every model: unseen-photo neighbour distances, family validation error, camera/lens/ISO coverage; per-photo confidence with stored components, out-of-distribution flag capped at 0.49, and plain-language reasons.
- Prediction method (`model.predict`) returning canonical settings (normalized + raw), nearest training examples, raw component outputs and confidence per asset.
- `train_style` job in mimic-core: immutable `model_versions` row created in `training` state, finalized once with metrics + artifact manifest, training set recorded, artifacts registered; activation policy — first version activates, later versions activate only when holdout error is not worse than the active one; failed runs leave a `failed` row with the structured error. Composite job executor and engine progress forwarding into job records.
- Commands: `train_style`, `activate_model_version` (rollback), `archive_model_version`, `get_model_version`; Style detail reports training availability with the exact reason.
- Styles UI: Train New Version (enabled only when data, engine and no running training allow it), live training progress, active-version card with holdout nMAE, Versions tab with real metrics (overall nMAE, exposure MAE in EV, evaluation set, beats-median) and Activate/Archive; Home shows holdout error; onboarding gains a Train step with phase-based progress and the resulting metrics.
- Tests: pytest training suite on a synthetic database built from the real migration (dataset filters, leak-free deterministic split, reproducibility, hybrid beats global median by a wide margin on exposure, prediction + OOD behaviour, insufficient-data failure); Rust end-to-end training test with the real engine through the job runner (versions, activation policy, rollback, prediction, insufficient data precheck); VersionList component tests; contracts tests for metric helpers.

### Fixed

- Engine exit code was read with `try_wait()` immediately after stdout closed and came back `None` on Windows; the client now awaits the real exit status.

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
