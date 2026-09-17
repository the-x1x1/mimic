# CLAUDE.md — agent entry point for Mimic

You are working on Mimic, a local-first Windows desktop app that learns a photographer's Lightroom Classic editing behaviour. Read this file first, then `docs/PROJECT_STATUS.md`, then the code you are about to change.

## Source of truth (highest first)

1. Source code
2. Automated tests (`cargo test`, `pytest`, `vitest`, Lua tests)
3. Reproducible runtime evidence (logs, diagnostic bundles, fixtures)
4. GitHub release artifacts
5. `docs/PROJECT_STATUS.md`
6. `docs/ROADMAP.md` and the rest of `docs/`

Documentation follows reality. Never let a doc lead the code.

## Non-negotiable safety rules

- **Never** open, read or mutate a Lightroom `.lrcat` (or its `-data`, `Previews.lrdata`, etc.). Lightroom is reached only through the plugin/SDK.
- **Never** overwrite, rewrite or delete a source RAW/JPEG/TIFF/DNG. Source media is opened read-only.
- **Never** mutate XMP or ACR sidecars. They are read-only training sources.
- **Never** claim an apply worked without read-back verification (`edit_dna::verify_readback`). A non-throwing SDK call is not success.
- **Never** bypass or weaken updater signature verification. The updater private key lives only in a GitHub Actions secret.
- **Never** add an LLM as the numerical editor. Slider values come from deterministic features, retrieval and regression.
- **Never** drop unknown Lightroom settings during normalization. They go to `unknownLightroomSettings`.
- **Never** mark a roadmap or status item complete without test evidence.
- **Always** update `docs/PROJECT_STATUS.md` with implementation changes, in the same commit.
- **Always** add a new numbered migration for any schema change (`crates/mimic-core/src/db/migrations/`), never edit a shipped one, and extend the migration tests.
- **Always** test update-related version changes (`node scripts/sync-version.mjs --check` is part of `test.ps1` and CI).
- **Always** preserve a recovery path before batch edit application (snapshot + `before_settings_json`).
- Capabilities are determined **at runtime** from the plugin probe where Lightroom APIs are unstable. No static assumption may mark a control writable.
- The UI must **hide or disable** unsupported functionality and say why. No buttons that pretend to work, no sample numbers shown as real metrics.
- Bridge binds `127.0.0.1` only, requires the per-launch bearer token, caps body sizes. Child processes are spawned with argument arrays, never shell strings.

## Repository map

```
apps/desktop/            Tauri 2 shell (src-tauri/, Rust) + React/TS frontend (src/)
crates/mimic-core/       Native core: db (SQLite + migrations), edit_dna, bridge, engine client, jobs, ingest, training, sessions, corrections, capability, diagnostics
engine/                  Python sidecar (uv): protocol server, scanner, XMP parser, previews, features, embeddings, training, confidence, inference, session grouping/consistency
lightroom/Mimic.lrplugin Lightroom Classic plugin (Lua): Bridge, Capabilities, Commands, Develop (apply/read-back), Json
packages/contracts/      zod contracts + edit_mapping_v1.json (single source of the EditDNA mapping)
packages/ui/             design tokens + primitives
fixtures/                golden fixtures: xmp/, bridge/, images/ (synthetic), expected/ (raw + normalized goldens), sessions/ (UI read models)
models/                  encoder manifests (SHA-256 pinned downloads); no binaries committed
scripts/                 bootstrap/dev/test/build/validate/package-engine/install-lightroom-plugin/verify-release/sync-version
docs/                    PROJECT_STATUS (truth table), ARCHITECTURE, EDIT_DNA, LIGHTROOM_*, ML_PIPELINE, DATABASE, PRIVACY, SECURITY_MODEL, UPDATE_SYSTEM, RELEASE_PROCESS, TEST_STRATEGY, UI_SPEC, adr/
```

## Commands

```powershell
.\scripts\bootstrap.ps1                 # prerequisites + installs
.\scripts\dev.ps1                       # tauri dev (spawns engine via uv)
.\scripts\test.ps1                      # everything CI runs
.\scripts\validate.ps1 -Full            # test.ps1 + pre-tag release verification
.\scripts\build.ps1                     # engine bundle + installer
node scripts/sync-version.mjs 0.4.0     # bump the single version everywhere (then cargo update -w)
```

Per stack: `pnpm typecheck|lint|test|build`, `cargo fmt/clippy/test --workspace`, `cd engine && uv run ruff check . && uv run pytest`, `cd lightroom && lua5.1 tests/json_test.lua`.

Regenerating goldens (only after a reviewed behaviour change): `cd engine && uv run python -m tests.regen_golden`, then `MIMIC_REGEN_GOLDEN=1 cargo test -p mimic-core --test edit_dna_golden`.

## What is implemented (0.4.0-alpha.1)

Foundation + real ingest: shell, onboarding, DB + migrations + backups, jobs with restart recovery, engine protocol with restart/timeouts/size caps, folder + sidecar scanner, XMP parser, ACR detection, EXIF, previews, features, scene heuristics, `stats_v1` embeddings, EditDNA normalization, Lightroom bridge + plugin with fake-plugin integration tests, capability matrix, data quality report, diagnostics, settings, CI, release workflow with signed updater plumbing. See `docs/PROJECT_STATUS.md` for statuses and evidence per item.

0.2.0 adds the Style Brain: training pipeline (dataset, session-grouped split, baselines, hybrid, metrics, confidence, artifacts), `train_style` job with immutable versions and the activation policy, `model.predict`, Versions UI and onboarding train step.

0.3.0 adds sessions (`crates/mimic-core/src/sessions`, `engine/src/mimic_engine/session`): session creation from a folder or Lightroom scope backed by a hidden `purpose = session` library, `ingest_session`, `group_session` (engine `session.group`), `predict_session` (consistency via cluster groups), review statuses, `apply_preflight` + `apply_session` (batches of 25, snapshot, read-back verification, `prediction` edit snapshots) and `restore_batch`; schema v2; Sessions and Review UI. Proven end to end against a scripted plugin over the real bridge (`tests/sessions_e2e.rs`).

0.4.0 adds continuous learning (`crates/mimic-core/src/corrections`): `sync_corrections` reads applied photos back, records untouched vs corrected (schema v3: `correction_syncs`, one correction per prediction, `correction` edit snapshots), `train_style` consumes pending corrections as training pairs (`correctionAssetIds`), `style_health` derives the measured No-Touch Rate and insights; Corrections tab, version comparison, Home No-Touch metric.

Not implemented: group editing, reference photos, correction weighting. Everything that talks to Lightroom's SDK is `NEEDS REAL-LIGHTROOM QA`.

## How to update PROJECT_STATUS

Each row has a status (`IMPLEMENTED | PARTIAL | BLOCKED | PLANNED | UNSUPPORTED`) and an evidence column naming the test, fixture or manual check that proves it. When you change behaviour: change the row, point evidence at the real test, and move completed items out of `docs/ROADMAP.md`.

## Forbidden shortcuts

Fake success, placeholder services, empty interfaces for bulk, god classes, duplicated schemas without a checked-in contract, unbounded workers, direct `.lrcat` access, XMP mutation as an apply path, LLM slider prediction, silent uploads, mask support claims without a runtime proof, marking anything done without a test.

## Release protocol (owner's rules)

One coherent slice per version with code + tests + docs; every shared contract gets a checked-in fixture; `test.ps1` green before claiming green; state plainly what could not run; version bumped everywhere a version lives (`sync-version.mjs --check` enforces); one clean commit per slice authored `the-x1x1 <connersalt123@outlook.com>`; no AI attribution anywhere.
