# Database

SQLite at `%LOCALAPPDATA%\Formicaria\Mimic\data\mimic.db`, WAL, `foreign_keys=ON`, `busy_timeout=5000`. Migrations are numbered SQL files embedded in `mimic-core` (`db/migrations/NNNN_name.sql`), applied forward-only, each in its own transaction, recorded in `schema_migrations`. Before migrating an **existing** database a backup is written to `data/backups/mimic.db.v<from>.<timestamp>.bak` via the SQLite backup API. Down migrations are not provided; rollback = restore the backup.

Conventions: UUID v4 TEXT ids; RFC 3339 UTC TEXT timestamps; JSON columns end in `_json` and are returned to the UI as parsed objects.

## Tables (schema v1)

| Table                                          | Purpose                             | Notes                                                                                                                                                                                         |
| ---------------------------------------------- | ----------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `app_settings`                                 | key → JSON value                    | allow-listed keys in `commands/settings.rs`                                                                                                                                                   |
| `libraries`                                    | training sources                    | `source_type ∈ lightroom_catalog, folder_sidecars, demo`; catalog fingerprint pinned on first Lightroom ingest                                                                                |
| `assets`                                       | photographs                         | identity = `(normalized_path, fast_hash)`; metadata columns COALESCE on update; `lightroom_local_id`                                                                                          |
| `sidecars`                                     | XMP/ACR files                       | `parse_status ∈ pending, parsed, opaque, failed`; unique on `path`; `hash` = scanner fast hash used for change detection                                                                      |
| `edit_snapshots`                               | observed develop state              | `source ∈ xmp, lightroom_sdk, prediction, correction`; normalized + raw + unknown JSON; `mapping_version`; `capability_schema_version`                                                        |
| `visual_features`                              | per asset per `feature_version`     | histogram/luminance/color/clipping JSON, sharpness, noise, scene labels, `embedding_artifact_id`, `preview_path`                                                                              |
| `style_profiles`, `style_profile_libraries`    | Style Brains and their sources      | `active_model_version_id`                                                                                                                                                                     |
| `training_sets`                                | dataset snapshots used by a version | counts per split, strategy, fingerprint                                                                                                                                                       |
| `model_versions`                               | immutable versions                  | unique `(style, semver)`; `status ∈ training, ready, failed, archived`; `finalize` only from `training`; exactly one `is_active` per style                                                    |
| `model_artifacts`                              | files on disk with SHA-256          | never stored in SQLite                                                                                                                                                                        |
| `sessions`, `session_assets`, `scene_clusters` | new shoots                          | cluster/burst ids per asset                                                                                                                                                                   |
| `predictions`                                  | per asset per session               | `status ∈ pending, reviewed, applied, rejected, superseded`; inserting a new prediction supersedes the pending one                                                                            |
| `apply_batches`, `applied_edits`               | apply history                       | unique `(batch, prediction)` — retries never double count; counters recomputed from rows; `rollback_available` when a before-state exists; `result ∈ applied, verify_failed, failed, skipped` |
| `corrections`                                  | prediction vs final delta           | `included_in_training_version`                                                                                                                                                                |
| `jobs`                                         | persistent queue                    | `status` includes `interrupted`; `resumable` flag; payload carries `cancelRequested`                                                                                                          |
| `events`                                       | structured log                      | level/category/type/entity                                                                                                                                                                    |
| `lightroom_connections`                        | one row per catalog fingerprint     | capabilities JSON, last seen                                                                                                                                                                  |
| `update_state`                                 | single row                          | current/latest/staged version, channel, last result                                                                                                                                           |

Indexes exist on every foreign key used in joins plus `assets(captured_at)`, `assets(camera_make, camera_model)`, `jobs(status, created_at)`, `events(created_at)`.

## Engine access

The engine receives the database path in `engine.configure` and (from 0.2.0) opens it read-only for training data. Writes to the database happen only in mimic-core.

## Tests

`db::tests` cover fresh install, idempotent reopen, upgrade with backup, FK enforcement, settings/events; each repository has its own tests; `tests/ingest_e2e.rs` exercises the real write path.
