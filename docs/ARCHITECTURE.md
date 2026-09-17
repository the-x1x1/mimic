# Architecture

```
┌────────────────────────── Mimic desktop (Tauri 2) ──────────────────────────┐
│  React/TS UI ── typed IPC (zod) ── Rust shell (apps/desktop/src-tauri)       │
│                                        │                                     │
│                              mimic-core (crates/mimic-core)                  │
│      ┌──────────┬──────────┬───────────┼───────────┬──────────────┐          │
│      db (SQLite) jobs      edit_dna    capability  diagnostics   ingest      │
│      └──────────┴──────────┴───────────┴───────────┴──────────────┘          │
│            │                       │                        │                │
│   %LOCALAPPDATA%\Formicaria\Mimic  │ bridge (127.0.0.1)     │ engine client  │
│   data/ cache/ models/ logs/ ...   │ token + long-poll      │ NDJSON stdio   │
└────────────────────────────────────┼────────────────────────┼────────────────┘
                                     │                        │
                        Lightroom Classic + Mimic.lrplugin   mimic-engine (Python)
                        (Lua, official SDK only)             scan · xmp · raw · features
```

## Processes and trust

- **Rust shell** (trusted): owns the database, job queue, bridge server, engine child, updater state, discovery file. All filesystem writes happen here or in the engine's own cache directories.
- **Frontend** (untrusted-ish webview): talks only through `invoke` commands enumerated in `lib.rs`; every response is zod-validated (`packages/contracts`). No direct FS access except the asset protocol scoped to the app data folder for previews.
- **Engine** (child process): spawned with an argument array, never a shell string. Reads the same SQLite database read-only (`file:…?mode=ro`) for training and prediction; writes only under `cache/` and `models/`. Speaks NDJSON on stdio; messages above 32 MiB are rejected and the child restarted.
- **Lightroom plugin** (runs inside Lightroom): polls the bridge; executes a fixed command set with SDK calls; never touches the `.lrcat`, XMP, or pixels.

## Data flow: historical ingest

1. UI creates a library (folder or Lightroom) and a Style, enqueues `scan_library` or `ingest_lightroom`.
2. `JobRunner` marks it running, heart-beats, and calls the executor (`ingest::IngestExecutor`).
3. Folder path: engine `scan.folder` → assets + sidecars; per XMP `xmp.parse` (skipped when the sidecar hash is unchanged and a snapshot exists) → `edit_dna::normalize` in Rust → `edit_snapshots` row with `normalized_settings_json`, `raw_settings_json`, `unknown_settings_json`, `mapping_version`.
4. Lightroom path: bridge `get_selected_photos` → per 25 photos `get_develop_settings` → normalize → snapshot with `capability_schema_version` and provenance.
5. `image.analyze_batch` for assets lacking `features_v1`: preview (LibRaw/Pillow, cached), statistics, scene labels, embedding `.npy` → `visual_features` row (embedding referenced by artifact id, never stored in SQLite).
6. `ingest::data_quality_report` aggregates from SQL.

## Data flow: session → apply (0.3.0)

1. `sessions::create_session` makes a hidden `purpose = session` library plus the `sessions` row; `ingest_session` reuses the folder/Lightroom ingest (same normalizer, features, previews) and fills `session_assets` in capture order.
2. `group_session` → engine `session.group` → `scene_clusters` + per-asset cluster/burst ids.
3. `predict_session` → engine `model.predict` with the active version's artifact and the cluster map (`groups`) → `predictions` rows (canonical settings, confidence, components, nearest examples, capability schema version).
4. Review sets `reviewed`/`rejected`; `apply_session` runs `apply_preflight`, resolves Lightroom photo ids, sends batches of 25 through the bridge with snapshot + read-back, verifies each item with `edit_dna::verify_readback`, records `applied_edits` and a `prediction` edit snapshot; `restore_batch` writes the recorded before-values back and verifies again.

## Engine protocol (§20)

Request `{"protocolVersion":1,"requestId":"uuid","method":"…","params":{}}`; response `{"protocolVersion":1,"requestId":"uuid","ok":true,"result":{}}` or `ok:false` with `{code,message,details}`; unsolicited events `{"event":"job.progress","jobId":…,"phase":…,"current":n,"total":n}` and `{"event":"log",…}`. Methods: `engine.hello`, `engine.configure`, `engine.health`, `engine.shutdown`, `scan.folder`, `xmp.parse`, `image.metadata`, `image.analyze`, `image.analyze_batch`, `training.train`, `model.predict`. The Rust client (`engine/mod.rs`) enforces timeouts, correlates by id, restarts on exit with a budget of 5, and fails all pending requests when the child dies.

## Bridge protocol (§10)

See LIGHTROOM_INTEGRATION.md. Fixtures in `fixtures/bridge/` are asserted by Rust (`tests/bridge_protocol.rs`), TypeScript (contracts tests) and Lua (`tests/json_test.lua`).

## Versioning

Root `package.json` is the single source; `scripts/sync-version.mjs` propagates to the desktop package, Tauri config, Cargo workspace, engine (PEP 440), plugin `Info.lua`/`Version.lua`, and checks Cargo.lock and CHANGELOG. Protocol versions (`BRIDGE_PROTOCOL_VERSION`, `ENGINE_PROTOCOL_VERSION`) are independent integers bumped on incompatible wire changes; the plugin refuses to talk to a desktop whose bridge protocol differs.

## Extension points prepared, not built

Job kinds are strings dispatched through `CompositeExecutor`; ingest, training and session executors (ingest/group/predict/apply/restore) are registered today, correction-sync joins the same runner in 0.4.0. Model artifacts are content-addressed files under `models/styles/`. Capability matrix statuses leave room for `supported` local edits once proven.
