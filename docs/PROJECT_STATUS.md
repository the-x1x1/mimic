# PROJECT_STATUS — Mimic 0.4.0-alpha.1

Brutally factual. Statuses: **IMPLEMENTED** (test or reproducible check exists) · **PARTIAL** · **BLOCKED** · **PLANNED** · **UNSUPPORTED**. Evidence names the test that proves the row. Anything marked _NEEDS REAL-LIGHTROOM QA_ has not been run against a real Lightroom Classic (there is none in CI).

## Repository

| Item                                                                                                    | Status                                   | Evidence                                                |
| ------------------------------------------------------------------------------------------------------- | ---------------------------------------- | ------------------------------------------------------- |
| Monorepo layout (apps/desktop, crates/mimic-core, engine, lightroom, packages, fixtures, scripts, docs) | IMPLEMENTED                              | tree; `CLAUDE.md` map                                   |
| Lockfiles committed (pnpm-lock.yaml, Cargo.lock, engine/uv.lock)                                        | IMPLEMENTED                              | files in repo                                           |
| Single version source + sync/check                                                                      | IMPLEMENTED                              | `scripts/sync-version.mjs --check` in `test.ps1` and CI |
| CI: frontend, Rust, Python, plugin, security                                                            | IMPLEMENTED                              | `.github/workflows/ci.yml`                              |
| Release workflow (Windows x64 NSIS, signed updater, latest.json, checksums, draft release)              | IMPLEMENTED (not yet exercised by a tag) | `.github/workflows/release.yml`                         |
| Nightly Windows debug build                                                                             | IMPLEMENTED (workflow)                   | `.github/workflows/nightly-smoke.yml`                   |

## Desktop shell

| Item                                                                                                               | Status      | Evidence                                                                                |
| ------------------------------------------------------------------------------------------------------------------ | ----------- | --------------------------------------------------------------------------------------- |
| Tauri 2 app boots even if engine/bridge fail; errors surfaced in UI                                                | IMPLEMENTED | `apps/desktop/src-tauri/src/startup.rs`; `App.tsx` gate + `ErrorBoundary`               |
| Onboarding (source choice, Lightroom setup, first Style + scan, data quality)                                      | IMPLEMENTED | `features/onboarding/OnboardingFlow.tsx`; “Connected” only after real handshake         |
| Onboarding train step                                                                                              | IMPLEMENTED | `OnboardingFlow.tsx` step 5; enabled only when `training.available`                     |
| Home, Styles list/detail, Settings (7 sections)                                                                    | IMPLEMENTED | components + `vitest` (DataQualityPanel, CapabilitySummary, VersionList, format, gate)  |
| Sessions list, New Session dialog, session detail (grid, group filters, confidence badges, apply history, Restore) | IMPLEMENTED | `features/sessions/*`; `PredictionPanel.test.tsx`, `ConfirmApplyDialog.test.tsx`        |
| Review (attention-only default, filmstrip/preview/panel, Looks right / Reject / Apply this photo, filters)         | IMPLEMENTED | `features/review/*`; `reviewQueue.test.ts`; contracts `needsAttention` tests            |
| Confirm-apply dialog shows backend blockers verbatim; confirm disabled unless preflight ok                         | IMPLEMENTED | `ConfirmApplyDialog.test.tsx`; fixture `fixtures/sessions/apply_preflight.refused.json` |
| Design tokens, dark theme, reduced motion, keyboard focus                                                          | IMPLEMENTED | `packages/ui/src/tokens.css`, `ui.css`                                                  |
| Light theme                                                                                                        | PARTIAL     | tokens exist; visually unreviewed                                                       |
| Toasts, job tray with cancel, item-based progress                                                                  | IMPLEMENTED | `JobTray.tsx`, `ProgressBar`                                                            |
| Typed IPC with zod validation on every command                                                                     | IMPLEMENTED | `lib/ipc.ts`; contracts tests                                                           |
| Demo mode (synthetic DEMO library, never a fake Lightroom connection)                                              | IMPLEMENTED | `commands/app.rs::enable_demo_mode`                                                     |

## Database

| Item                                                                                                                                                                                                                   | Status                | Evidence                                                                     |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------- | ---------------------------------------------------------------------------- |
| Schema v1: all 21 tables + indexes + FKs + WAL                                                                                                                                                                         | IMPLEMENTED           | `db/migrations/0001_init.sql`; `db::tests::fresh_install_migrates_to_latest` |
| Schema v2 (`0002_sessions.sql`): library purpose, restore columns, prediction capability/cluster columns; v1 → v2 upgrade keeps rows                                                                                   | IMPLEMENTED           | `migrations::tests::v1_database_with_data_upgrades_to_v2_keeping_rows`       |
| Schema v3 (`0003_corrections.sql`): `correction_syncs`, one correction per prediction; v1 → v3 upgrade keeps rows                                                                                                      | IMPLEMENTED           | `migrations::tests::v1_database_with_data_upgrades_to_latest_keeping_rows`   |
| Backup before migrating an existing DB                                                                                                                                                                                 | IMPLEMENTED           | `db::tests::upgrade_from_older_schema_creates_backup`                        |
| Repositories (libraries, assets, sidecars, snapshots, features, jobs, lightroom connections, styles, model versions, sessions, predictions, apply batches, applied edits, corrections, settings, events, update state) | IMPLEMENTED           | repo tests in `crates/mimic-core/src/db/*`                                   |
| Immutable model versions, single active per style                                                                                                                                                                      | IMPLEMENTED (storage) | `repo_styles::tests::versions_are_immutable_and_single_active`               |
| Apply batch never “completed” with failures; idempotent item results; restore recorded per item without rewriting the apply row; session delete keeps assets                                                           | IMPLEMENTED           | `repo_sessions::tests::*`                                                    |

## Jobs

| Item                                                                                             | Status                                       | Evidence                                                         |
| ------------------------------------------------------------------------------------------------ | -------------------------------------------- | ---------------------------------------------------------------- |
| Persistent queue, progress, phase, heartbeat, cancel between items                               | IMPLEMENTED                                  | `jobs::tests::*`                                                 |
| Restart recovery: running → interrupted; resumable kinds re-queued; apply never blindly repeated | IMPLEMENTED                                  | `repo_jobs::tests::interrupted_recovery_requeues_only_resumable` |
| Bounded concurrency                                                                              | IMPLEMENTED (1 orchestrator; engine batches) | `JobRunner::run_loop`                                            |

## Engine (Python sidecar)

| Item                                                                                                  | Status                                  | Evidence                                                                                                             |
| ----------------------------------------------------------------------------------------------------- | --------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| NDJSON protocol: correlation, structured errors, events, size cap, shutdown                           | IMPLEMENTED                             | `engine/tests/test_protocol.py`; Rust `tests/engine_protocol.rs` (fake engine: timeout, crash, restart, oversized)   |
| Auto start via `uv run` in dev, bundled exe in release                                                | IMPLEMENTED / PARTIAL                   | `engine::resolve_engine_command`; bundle produced by `scripts/package-engine.ps1` (not run in CI-less sandbox)       |
| Folder scanner with pairing rules, duplicates, orphans, unsupported                                   | IMPLEMENTED                             | `test_scanner.py`                                                                                                    |
| XMP parser (read-only, unknown keys kept, masks structured, malformed isolated)                       | IMPLEMENTED                             | `test_xmp.py` incl. goldens `fixtures/expected/*.raw.json`                                                           |
| ACR sidecar detection (opaque)                                                                        | IMPLEMENTED                             | scanner + ingest; e2e                                                                                                |
| EXIF metadata                                                                                         | IMPLEMENTED                             | `read_metadata`; e2e checks width/height on synthetic JPEG                                                           |
| RAW preview via LibRaw + cache                                                                        | IMPLEMENTED (code) / PARTIAL (evidence) | `preview.py`; fixtures are JPEG/TIFF — no RAW sample in repo (licensing). Set `MIMIC_TEST_RAW` manually to exercise  |
| Image statistics, scene heuristics, `stats_v1` embedding                                              | IMPLEMENTED                             | `test_features.py` (determinism, label intent, cache)                                                                |
| ONNX encoder manager (manifest, SHA-256, DirectML/CPU)                                                | PARTIAL                                 | code + fallback tested; no manifest shipped, no model downloaded in tests                                            |
| Training pipeline (dataset, grouped split, baselines, hybrid, metrics, confidence, artifacts)         | IMPLEMENTED                             | `engine/tests/test_training.py`; Rust `tests/training_e2e.rs`                                                        |
| `model.predict` with optional session consistency (`groups`, `consistency`)                           | IMPLEMENTED                             | `test_session.py::test_service_session_group_and_consistent_predict` (exposure untouched, bounded shift, off switch) |
| `session.group`: time blocks, seeded k-means, min cluster size, bursts, untimed frames, deterministic | IMPLEMENTED                             | `test_session.py`                                                                                                    |

## EditDNA

| Item                                                                                            | Status      | Evidence                                                                        |
| ----------------------------------------------------------------------------------------------- | ----------- | ------------------------------------------------------------------------------- |
| `edit_mapping_v1.json` — 104 controls, 11 families, metadata keys, local/heavy prefixes         | IMPLEMENTED | `mapping::tests`, contracts vitest                                              |
| Normalization: modern/legacy keys, curves, bools, enums, unknown preservation, local `observed` | IMPLEMENTED | `edit_dna::tests`; goldens `fixtures/expected/*.normalized.json`                |
| Canonical → Lightroom write with capability gating; read-back verification with tolerances      | IMPLEMENTED | `roundtrip_to_lightroom_respects_capability`, `verify_readback_uses_tolerances` |
| Full EditDNA document per pair                                                                  | IMPLEMENTED | `build_edit_dna`; exposed by `get_asset_detail`                                 |

## Lightroom

| Item                                                                                                                 | Status                                       | Evidence                                                                                                                           |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| Loopback bridge: token auth, origin rejection, body cap, handshake, long-poll, results, events, capabilities refresh | IMPLEMENTED                                  | `tests/bridge_fake_plugin.rs` (5 tests), `tests/bridge_protocol.rs`                                                                |
| Timeout / disconnect / reconnect handling                                                                            | IMPLEMENTED                                  | `timeout_disconnect_and_reconnect`                                                                                                 |
| Discovery file with owner-only permissions                                                                           | IMPLEMENTED                                  | same test (unix mode 0600)                                                                                                         |
| Plugin: handshake, poll loop, backoff, Plugin Manager panel, menu items                                              | IMPLEMENTED (code) — NEEDS REAL-LIGHTROOM QA | Lua syntax-checked; `lightroom/tests/json_test.lua`                                                                                |
| Plugin: capability probe                                                                                             | IMPLEMENTED (code) — NEEDS REAL-LIGHTROOM QA | `Capabilities.lua`                                                                                                                 |
| Plugin: get_selected_photos / get_develop_settings / metadata                                                        | IMPLEMENTED (code) — NEEDS REAL-LIGHTROOM QA | `Commands.lua`, `Catalog.lua`                                                                                                      |
| Plugin: create_before_snapshot, apply_settings_as_plugin_preset with read-back, collect_correction_state             | IMPLEMENTED (code) — NEEDS REAL-LIGHTROOM QA | `Develop.lua`; fixtures `apply_settings_as_plugin_preset.*.json`                                                                   |
| Capability matrix derivation                                                                                         | IMPLEMENTED                                  | `capability::tests`                                                                                                                |
| Lightroom-connected ingest job                                                                                       | IMPLEMENTED (code) — NEEDS REAL-LIGHTROOM QA | `ingest::ingest_lightroom`; bridge path proven with fake plugin                                                                    |
| Restore payload shape (`createSnapshot:false`, `readBack:true`, `restore:true`) and catalog listing fixture          | IMPLEMENTED                                  | `fixtures/bridge/apply_settings_as_plugin_preset.command.restore.json`, `get_selected_photos.result.json`; Rust + TS fixture tests |
| Mask / local adjustment support                                                                                      | UNSUPPORTED (by design in 0.x)               | matrix reports `unsupported`                                                                                                       |

## Training job (mimic-core)

| Item                                                                                                                 | Status      | Evidence                      |
| -------------------------------------------------------------------------------------------------------------------- | ----------- | ----------------------------- |
| `train_style` job: version row in `training`, finalize once, training set + artifacts recorded, failed runs recorded | IMPLEMENTED | `tests/training_e2e.rs`       |
| Activation policy (first version; later only if holdout not worse); manual activate = rollback; archive              | IMPLEMENTED | same; `training::activate`    |
| Engine progress forwarded into job records                                                                           | IMPLEMENTED | same (`progress_total > 0`)   |
| Training never resumed after interruption (must be re-run)                                                           | IMPLEMENTED | `resumable == false` asserted |

## Sessions (mimic-core `sessions`)

| Item                                                                                                                                                                                                                                         | Status                                                       | Evidence                                                                                                                    |
| -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------- |
| Create session from folder or Lightroom scope (hidden `purpose = session` library), `ingest_session` reusing the ingest pipeline, capture-ordered membership                                                                                 | IMPLEMENTED                                                  | `tests/sessions_e2e.rs` (real engine, 5 fixture images)                                                                     |
| `group_session` job → `scene_clusters` + per-asset cluster/burst ids; re-grouping replaces cleanly                                                                                                                                           | IMPLEMENTED                                                  | same                                                                                                                        |
| `predict_session` job: active version only, feature-schema check, cluster map → consistency, supersede on re-run, capability schema version recorded                                                                                         | IMPLEMENTED                                                  | same                                                                                                                        |
| Review status transitions (`pending`/`reviewed`/`rejected`; applied/superseded immutable)                                                                                                                                                    | IMPLEMENTED                                                  | `sessions::tests::review_status_transitions`; e2e rejected photo skipped                                                    |
| `apply_preflight`: connected, canApply/canSnapshot, writable controls, same catalog, stale capability schema, apply already running; warnings for inactive versions                                                                          | IMPLEMENTED                                                  | `sessions::tests::preflight_*`; e2e stale-capability refusal                                                                |
| `apply_session`: photo resolution (local id or path via catalog listing), batches of 25 with snapshot + read-back, `verify_readback` per item, `prediction` edit snapshot, cancellation between batches, `outcome_unknown` on bridge failure | IMPLEMENTED (bridge + fake plugin) — NEEDS REAL-LIGHTROOM QA | `tests/sessions_e2e.rs`: 2 applied, 1 verify_failed, 1 skipped (not in catalog), rejected excluded, only writable keys sent |
| `restore_batch`: before-values of written keys only, verified by read-back, per-item restore result, predictions back to pending, rollback flag cleared                                                                                      | IMPLEMENTED (bridge + fake plugin) — NEEDS REAL-LIGHTROOM QA | same; `restore_settings_uses_only_written_keys_with_before_values`                                                          |
| Apply/restore never re-queued after an interruption                                                                                                                                                                                          | IMPLEMENTED                                                  | `SessionExecutor::resumable`                                                                                                |
| `sync_corrections` job: read-back via `collect_correction_state`, untouched vs corrected by read-back tolerance, per-control normalized deltas, `correction` edit snapshots, idempotent re-sync, `correction_syncs` rows                     | IMPLEMENTED (bridge + fake plugin) — NEEDS REAL-LIGHTROOM QA | `tests/sessions_e2e.rs` (1 corrected, 1 untouched, re-sync stable); `corrections::tests::diff_*`                            |
| No-Touch Rate per version derived from synced sessions only; restored edits never count                                                                                                                                                      | IMPLEMENTED                                                  | `repo_sessions::tests` (no_touch_stats), e2e `active_no_touch_rate == 0.5`                                                  |
| Retraining with corrections: pending corrections become pairs (`correctionAssetIds`), marked `included_in_training_version`                                                                                                                  | IMPLEMENTED                                                  | `engine/tests/test_training.py::test_dataset_includes_only_corrected_session_assets`; e2e `correctionPairs == 1`            |
| Style health insights (No-Touch, pending corrections, most-corrected controls with bias)                                                                                                                                                     | IMPLEMENTED                                                  | `corrections::style_health`; e2e + fixture round-trip                                                                       |
| Corrections tab, version comparison, Home No-Touch metric, session Sync button + history                                                                                                                                                     | IMPLEMENTED                                                  | `CorrectionsPanel.test.tsx`, `VersionCompare.test.tsx`; fixtures `style_health.json`, `correction_row.json`                 |
| Correction weighting in training, automatic sync prompts                                                                                                                                                                                     | PLANNED (0.4.x)                                              | —                                                                                                                           |

## Ingest and data quality

| Item                                                                                                                                                       | Status      | Evidence              |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------- | --------------------- |
| Folder scan job end to end with the real engine (assets, sidecars, snapshots, features, previews, embeddings, report, idempotent rescan, source untouched) | IMPLEMENTED | `tests/ingest_e2e.rs` |
| Data quality report + warnings (ACR, no edits, single camera, single day, local edits, failed sidecars)                                                    | IMPLEMENTED | `ingest::tests`, e2e  |

## Updater and release

| Item                                                                                       | Status                  | Evidence                                                                         |
| ------------------------------------------------------------------------------------------ | ----------------------- | -------------------------------------------------------------------------------- |
| Tauri updater plugin configured with embedded pubkey, `latest.json` endpoint               | IMPLEMENTED             | `tauri.conf.json`                                                                |
| Background checks (initial delay, 6 h ± 20 min), auto-download when enabled, install guard | IMPLEMENTED             | `useUpdater.ts`, `can_install_update_now`, contracts test `nextCheckDelayMs`     |
| Update state persisted                                                                     | IMPLEMENTED             | `db::update_state`                                                               |
| Development pubkey refused for non-alpha releases                                          | IMPLEMENTED             | `verify-release.ps1 -PreTag`, release workflow `verify` job                      |
| Tested v0.1.0 → v0.1.1 update path                                                         | PLANNED                 | documented procedure in `docs/UPDATE_SYSTEM.md`; requires two published releases |
| Windows installer produced                                                                 | PLANNED until first tag | release workflow                                                                 |

## Observability and diagnostics

| Item                                                         | Status      | Evidence                                |
| ------------------------------------------------------------ | ----------- | --------------------------------------- |
| JSON logs with rotation (14 files)                           | IMPLEMENTED | `logging.rs`                            |
| Diagnostic bundle: no tokens, paths redacted unless opted in | IMPLEMENTED | `diagnostics::tests`                    |
| Event log table + Diagnostics UI                             | IMPLEMENTED | `recent_events`, Settings › Diagnostics |

## What the sandbox that produced this release could not run

- `tauri build` for Windows (no Windows toolchain); the Rust crate compiles and passes clippy on Linux with the Tauri Linux deps.
- Any interaction with a real Lightroom Classic: the apply/restore path is proven only against a scripted plugin speaking the real bridge protocol. Real-catalog behaviour of `createDevelopSnapshot`, `addDevelopPresetForPlugin`, `applyDevelopPreset` and read-back equality is still `NEEDS REAL-LIGHTROOM QA`.
- PyInstaller packaging of the engine (`scripts/package-engine.ps1` is written for the Windows runner).
