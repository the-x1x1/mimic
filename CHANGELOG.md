# Changelog

All notable changes to Mimic are documented here. The format follows Keep a Changelog; versions follow SemVer with pre-release tags for alpha/beta builds.

## [0.5.0-alpha.1] — 2026-09-17

Session intelligence: reference photos, group editing, per-group and per-camera confidence, and group-outlier flags.

### Added

- Schema v4 (`0004_session_intelligence.sql`): `scene_clusters.reference_asset_id` (FK to assets) and `edited_at`; the seeded upgrade test now runs v1 → v4.
- Engine: `model.predict` accepts `references {groupId: assetId}`; the consistency policy pulls a group toward its reference (blend 0.8, same per-family caps) and never changes the reference itself (`isReference`, `consistencyShift = 0`); `detect_outliers` flags photos whose exposure, temperature or tint sits > 12 % of range from their group's median (groups ≥ 4) as `groupOutlier` with a reason line, judged on raw predictions before blending.
- mimic-core `sessions`: `edit_groups` (rename; set/clear a member-only reference; merge with the target keeping label and reference; move photos into an existing or new group with emptied sources deleted and orphaned references cleared), every edit stamping `edited_at`; `predict_session` passes group references and counts outliers; `session_detail` gains `groupStats` (photos, predicted, mean/min confidence, low-confidence, unfamiliar, outliers, applied, rejected per group), `cameraStats` (photos, mean confidence, known-to-model per camera/lens), `groupingChangedSincePrediction` and `syncSuggested`.
- Command `edit_session_groups`; `GroupEdit` tagged-union contract and `session_detail.json` fixture round-tripped in Rust and zod (the fixture caught a snake_case field leak).
- UI: Scene groups table with inline rename, "use selected as reference", two-step merge, move/split of the multi-selected photos (Ctrl/Cmd/Shift-click in the grid); cameras/lenses table when a session mixes bodies or uses one the model has not seen; banners when groups changed after the last prediction and when a corrections sync is due; outlier badge and reference star on tiles; Review's attention queue includes group outliers.
- Tests: pytest reference/outlier unit tests and service-level reference assertions; repository tests for every group edit; `sessions_e2e` extended with rename → split → invalid reference → merge → predict → reference → stale flag → re-predict; GroupsPanel component tests; contracts tests.

## [0.4.0-alpha.2] — 2026-09-17

Release-pipeline fix only; application code is identical to 0.4.0-alpha.1 (whose Release workflow never produced an installer).

### Fixed

- `scripts/package-engine.ps1` smoke check treated the two-line stdio reply (`engine.hello` + `engine.shutdown`) as a failure: PowerShell's `-notmatch` on an array returns the non-matching lines rather than a boolean. The reply is now joined before matching, and a non-zero exit of the packaged engine is reported on its own. This is why the 0.3.0-alpha.1 and 0.4.0-alpha.1 release jobs failed at "Package engine" although the bundle worked.

## [0.4.0-alpha.1] — 2026-09-17

Continuous learning: Mimic now reads applied photos back after your own pass in Lightroom, keeps what you changed as corrections, measures the No-Touch Rate from what you left alone, and trains the next version on those corrections. Proven against a scripted plugin over the real bridge; real-Lightroom behaviour remains unverified.

### Added

- Schema v3 (`0003_corrections.sql`): `correction_syncs` (one row per sync: checked / untouched / corrected / unresolved) and a unique `corrections(prediction_id)`; the migration test now upgrades a seeded v1 database through every version.
- mimic-core `corrections`: `sync_corrections` job (same catalog required; photos resolved like apply; `collect_correction_state` in chunks of 25; keys Mimic wrote compared with read-back tolerances; per-control normalized deltas and magnitude; `correction` edit snapshot with the photographer's final settings; re-sync replaces an unused correction and keeps one already used by training), `no_touch_stats` (per version, synced sessions only, restored edits excluded), `style_health` (active No-Touch, corrections pending training, most-corrected controls with signed bias, computed insight sentences).
- Training with corrections: `train_style` passes the Style's pending corrections as `correctionAssetIds`; the engine's dataset loader adds those assets only when their latest snapshot is a `correction`, groups them as their own shoot, reports `correctionPairs`; the job marks them `included_in_training_version`.
- Commands: `sync_corrections`, `get_style_health`, `list_corrections`; `StyleSummary.noTouchRate`; `StyleDetail.health`; `SessionDetail.correctionSyncs`; zod contracts with fixtures shared with the Rust round-trip tests (`style_health.json`, `correction_row.json`) and a `collect_correction_state` bridge fixture.
- UI: Style Corrections tab (No-Touch per version, insights, most-corrected controls, correction list with trained/pending state, honest empty state), Versions tab side-by-side comparison (overall and per-family holdout error, measured No-Touch), session _Sync corrections_ button with reasons and a sync history table, Home and active-version cards show the measured No-Touch Rate or “—”.
- Tests: pytest dataset test for correction assets, Rust unit tests for the diff and health, `sessions_e2e` extended with sync → idempotent re-sync → retrain consuming the correction → clean failure on a session without applies, component tests for the Corrections panel and version comparison.

## [0.3.0-alpha.1] — 2026-09-17

Sessions, scene grouping, prediction with confidence, Review, and the Lightroom apply/restore path with read-back verification. Everything that touches a catalog is proven against a scripted plugin over the real bridge; behaviour on a real Lightroom Classic is still unverified.

### Added

- Schema v2 (`0002_sessions.sql`): `libraries.purpose` (training vs session-backing libraries, hidden from the Libraries UI), `applied_edits.restored_at / restore_result / restore_error_json`, `predictions.capability_schema_version / cluster_id`; migration test seeds a v1 database with rows and upgrades it; the engine's test database builder now applies every checked-in migration.
- Engine `session.group`: capture-time blocks (20 min gap, untimed frames share one block), seeded k-means on standardized visual statistics (+ embedding when present) with a minimum cluster size, burst detection, deterministic output. Engine `model.predict` gains `groups` + `consistency`: a bounded per-family pull toward the scene-group median (white balance, colour, presence); exposure and tone are never blended, the maximum shift is reported per photo.
- mimic-core `sessions`: `create_session` (folder or Lightroom scope), `ingest_session` (reuses the folder/Lightroom ingest, capture-ordered membership), `group_session`, `predict_session` (active version only, feature-schema check, supersedes earlier predictions, records the Lightroom capability schema version), review transitions, `apply_preflight` (connected, canApply/canSnapshot, writable controls, same catalog, stale capability schema, apply already running), `apply_session` (photo resolution by local id or normalized path against the catalog listing; batches of 25 with `Mimic Before` snapshot and read-back; `verify_readback` per item; `prediction` edit snapshot on success; cancellation between batches; `outcome_unknown` recorded when the bridge fails mid-batch), `restore_batch` (before-values of the written keys only, read-back verified, per-item restore result, predictions back to pending). Apply and restore are never re-queued after an interruption.
- Commands: `list_sessions`, `create_session`, `get_session_detail`, `list_session_photos`, `set_session_style`, `delete_session`, `group_session`, `predict_session`, `set_prediction_review`, `get_apply_preflight`, `apply_session`, `list_applied_edits`, `restore_apply_batch`, `get_prediction`; typed contracts with checked-in fixtures (`fixtures/sessions/*`) asserted by Rust round-trip and zod tests.
- Sessions UI: list, New Session dialog (folder or Lightroom scope, optional Style), detail page with Analyze scenes → Predict → Apply to Lightroom (each disabled with a reason), live job card, metrics, per-group and needs-attention filters, confidence badges on every tile, prediction panel (predicted Lightroom values, confidence components and reasons, Lightroom outcome with read-back mismatches, Looks right / Reject / Apply this photo), apply history with Restore, confirm dialog that states the safety steps and shows the backend's blockers verbatim.
- Review UI: attention-only default (below the medium threshold, unfamiliar, failed apply), All pending, Every prediction; filmstrip, large preview, prev/next, the same prediction panel; works offline, Apply requires Lightroom.
- Bridge fixtures for the restore payload and the catalog listing; Lightroom integration, ML pipeline, database, architecture and UI docs updated.
- Tests: pytest session suite (grouping determinism, time gaps, visual split, untimed frames, consistency policy, service-level `session.group` + consistent predict), Rust `tests/sessions_e2e.rs` (real engine ingest of fixture images, grouping, prediction, re-prediction supersedes, rejected photo excluded, apply refused without Lightroom, apply against a scripted plugin with one read-back mismatch and one missing photo, stale-capability refusal, restore with verification), migration upgrade test, repository tests for restore bookkeeping and session delete, frontend tests for the prediction panel, confidence badge, confirm dialog and review queue.

### Changed

- Home and Settings no longer describe Sessions/Review as future work; the unhonoured “create a snapshot before applying” toggle was removed — snapshot + read-back are mandatory and stated as such.
- `apps/desktop/tsconfig.tsbuildinfo` is no longer tracked (it is a build cache and blocked fast-forward pulls).
- Styles with predictions cannot be deleted (apply history references their versions); delete the sessions first. The UI reports the reason.

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
