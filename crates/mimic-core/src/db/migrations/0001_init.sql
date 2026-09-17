-- Mimic schema v1. Every table from docs/DATABASE.md.
-- Timestamps are RFC 3339 UTC TEXT. IDs are UUID v4 TEXT. JSON columns end in _json.

CREATE TABLE app_settings (
  key         TEXT PRIMARY KEY,
  value_json  TEXT NOT NULL,
  updated_at  TEXT NOT NULL
);

CREATE TABLE libraries (
  id                              TEXT PRIMARY KEY,
  name                            TEXT NOT NULL,
  source_type                     TEXT NOT NULL CHECK (source_type IN ('lightroom_catalog','folder_sidecars','demo')),
  root_path                       TEXT,
  lightroom_catalog_fingerprint   TEXT,
  created_at                      TEXT NOT NULL,
  last_scanned_at                 TEXT,
  status                          TEXT NOT NULL DEFAULT 'new'
);

CREATE TABLE assets (
  id                   TEXT PRIMARY KEY,
  library_id           TEXT REFERENCES libraries(id) ON DELETE SET NULL,
  source_path          TEXT NOT NULL,
  normalized_path      TEXT NOT NULL,
  file_name            TEXT NOT NULL,
  extension            TEXT NOT NULL,
  mime_type            TEXT,
  size_bytes           INTEGER NOT NULL DEFAULT 0,
  modified_time        TEXT,
  fast_hash            TEXT NOT NULL,
  full_hash            TEXT,
  camera_make          TEXT,
  camera_model         TEXT,
  lens                 TEXT,
  focal_length         REAL,
  iso                  INTEGER,
  aperture             REAL,
  shutter_speed        REAL,
  captured_at          TEXT,
  width                INTEGER,
  height               INTEGER,
  orientation          INTEGER,
  lightroom_local_id   INTEGER,
  created_at           TEXT NOT NULL,
  updated_at           TEXT NOT NULL
);
CREATE UNIQUE INDEX idx_assets_identity ON assets(normalized_path, fast_hash);
CREATE INDEX idx_assets_library ON assets(library_id);
CREATE INDEX idx_assets_captured ON assets(captured_at);
CREATE INDEX idx_assets_camera ON assets(camera_make, camera_model);
CREATE INDEX idx_assets_lr_local_id ON assets(lightroom_local_id);

CREATE TABLE sidecars (
  id                 TEXT PRIMARY KEY,
  asset_id           TEXT NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
  type               TEXT NOT NULL CHECK (type IN ('xmp','acr')),
  path               TEXT NOT NULL,
  modified_time      TEXT,
  hash               TEXT,
  parse_status       TEXT NOT NULL DEFAULT 'pending' CHECK (parse_status IN ('pending','parsed','opaque','failed')),
  parser_version     TEXT,
  raw_metadata_json  TEXT,
  warnings_json      TEXT,
  detected_at        TEXT NOT NULL
);
CREATE INDEX idx_sidecars_asset ON sidecars(asset_id);
CREATE UNIQUE INDEX idx_sidecars_path ON sidecars(path);

CREATE TABLE edit_snapshots (
  id                          TEXT PRIMARY KEY,
  asset_id                    TEXT NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
  source                      TEXT NOT NULL CHECK (source IN ('xmp','lightroom_sdk','prediction','correction')),
  process_version             TEXT,
  normalized_settings_json    TEXT NOT NULL,
  raw_settings_json           TEXT NOT NULL,
  unknown_settings_json       TEXT NOT NULL DEFAULT '{}',
  mapping_version             TEXT NOT NULL,
  capability_schema_version   TEXT,
  observed_at                 TEXT NOT NULL,
  provenance_json             TEXT NOT NULL DEFAULT '{}'
);
CREATE INDEX idx_edit_snapshots_asset ON edit_snapshots(asset_id, observed_at);
CREATE INDEX idx_edit_snapshots_source ON edit_snapshots(source);

CREATE TABLE visual_features (
  asset_id               TEXT NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
  feature_version        TEXT NOT NULL,
  histogram_json         TEXT NOT NULL,
  luminance_json         TEXT NOT NULL,
  color_json             TEXT NOT NULL,
  sharpness              REAL,
  noise_estimate         REAL,
  clipping_json          TEXT NOT NULL,
  scene_labels_json      TEXT NOT NULL DEFAULT '{}',
  embedding_artifact_id  TEXT,
  preview_path           TEXT,
  computed_at            TEXT NOT NULL,
  PRIMARY KEY (asset_id, feature_version)
);

CREATE TABLE style_profiles (
  id                       TEXT PRIMARY KEY,
  name                     TEXT NOT NULL,
  description              TEXT,
  created_at               TEXT NOT NULL,
  updated_at               TEXT NOT NULL,
  active_model_version_id  TEXT,
  status                   TEXT NOT NULL DEFAULT 'empty'
);

CREATE TABLE style_profile_libraries (
  style_profile_id  TEXT NOT NULL REFERENCES style_profiles(id) ON DELETE CASCADE,
  library_id        TEXT NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
  added_at          TEXT NOT NULL,
  PRIMARY KEY (style_profile_id, library_id)
);

CREATE TABLE sessions (
  id                        TEXT PRIMARY KEY,
  name                      TEXT NOT NULL,
  source_path               TEXT,
  source_library_id         TEXT REFERENCES libraries(id) ON DELETE SET NULL,
  captured_start            TEXT,
  captured_end              TEXT,
  status                    TEXT NOT NULL DEFAULT 'new',
  active_style_profile_id   TEXT REFERENCES style_profiles(id) ON DELETE SET NULL,
  created_at                TEXT NOT NULL
);

CREATE TABLE session_assets (
  session_id      TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  asset_id        TEXT NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
  sequence_index  INTEGER NOT NULL,
  cluster_id      TEXT,
  burst_id        TEXT,
  PRIMARY KEY (session_id, asset_id)
);
CREATE INDEX idx_session_assets_cluster ON session_assets(session_id, cluster_id);

CREATE TABLE scene_clusters (
  id                     TEXT PRIMARY KEY,
  session_id             TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  label                  TEXT NOT NULL,
  centroid_artifact_id   TEXT,
  feature_summary_json   TEXT NOT NULL DEFAULT '{}',
  created_at             TEXT NOT NULL
);

CREATE TABLE training_sets (
  id                  TEXT PRIMARY KEY,
  style_profile_id    TEXT NOT NULL REFERENCES style_profiles(id) ON DELETE CASCADE,
  source_query_json   TEXT NOT NULL,
  asset_count         INTEGER NOT NULL DEFAULT 0,
  valid_pair_count    INTEGER NOT NULL DEFAULT 0,
  train_count         INTEGER NOT NULL DEFAULT 0,
  validation_count    INTEGER NOT NULL DEFAULT 0,
  holdout_count       INTEGER NOT NULL DEFAULT 0,
  split_strategy      TEXT NOT NULL,
  fingerprint         TEXT,
  created_at          TEXT NOT NULL
);

CREATE TABLE model_versions (
  id                       TEXT PRIMARY KEY,
  style_profile_id         TEXT NOT NULL REFERENCES style_profiles(id) ON DELETE CASCADE,
  semantic_version         TEXT NOT NULL,
  model_type               TEXT NOT NULL,
  feature_schema_version   TEXT NOT NULL,
  edit_schema_version      TEXT NOT NULL,
  training_set_id          TEXT REFERENCES training_sets(id) ON DELETE SET NULL,
  training_config_json     TEXT NOT NULL DEFAULT '{}',
  metrics_json             TEXT NOT NULL DEFAULT '{}',
  artifact_manifest_json   TEXT NOT NULL DEFAULT '{}',
  created_at               TEXT NOT NULL,
  status                   TEXT NOT NULL CHECK (status IN ('training','ready','failed','archived')),
  is_active                INTEGER NOT NULL DEFAULT 0
);
CREATE UNIQUE INDEX idx_model_versions_semver ON model_versions(style_profile_id, semantic_version);

CREATE TABLE model_artifacts (
  id                 TEXT PRIMARY KEY,
  model_version_id   TEXT REFERENCES model_versions(id) ON DELETE CASCADE,
  kind               TEXT NOT NULL,
  path               TEXT NOT NULL,
  hash               TEXT NOT NULL,
  size_bytes         INTEGER NOT NULL,
  format             TEXT NOT NULL,
  created_at         TEXT NOT NULL
);

CREATE TABLE predictions (
  id                          TEXT PRIMARY KEY,
  session_id                  TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  asset_id                    TEXT NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
  model_version_id            TEXT NOT NULL REFERENCES model_versions(id),
  predicted_settings_json     TEXT NOT NULL,
  raw_model_output_json       TEXT NOT NULL DEFAULT '{}',
  confidence                  REAL NOT NULL,
  confidence_components_json  TEXT NOT NULL DEFAULT '{}',
  nearest_examples_json       TEXT NOT NULL DEFAULT '[]',
  created_at                  TEXT NOT NULL,
  status                      TEXT NOT NULL CHECK (status IN ('pending','reviewed','applied','rejected','superseded'))
);
CREATE INDEX idx_predictions_session ON predictions(session_id, status);
CREATE INDEX idx_predictions_asset ON predictions(asset_id);

CREATE TABLE apply_batches (
  id                              TEXT PRIMARY KEY,
  session_id                      TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  lightroom_catalog_fingerprint   TEXT,
  started_at                      TEXT NOT NULL,
  completed_at                    TEXT,
  status                          TEXT NOT NULL,
  applied_count                   INTEGER NOT NULL DEFAULT 0,
  failed_count                    INTEGER NOT NULL DEFAULT 0,
  rollback_available              INTEGER NOT NULL DEFAULT 0,
  error_json                      TEXT
);

CREATE TABLE applied_edits (
  id                        TEXT PRIMARY KEY,
  apply_batch_id            TEXT NOT NULL REFERENCES apply_batches(id) ON DELETE CASCADE,
  prediction_id             TEXT NOT NULL REFERENCES predictions(id),
  asset_id                  TEXT NOT NULL REFERENCES assets(id),
  before_settings_json      TEXT,
  applied_settings_json     TEXT NOT NULL,
  lightroom_snapshot_name   TEXT,
  result                    TEXT NOT NULL CHECK (result IN ('applied','verify_failed','failed','skipped')),
  error_json                TEXT,
  applied_at                TEXT NOT NULL
);
CREATE UNIQUE INDEX idx_applied_edits_batch_prediction ON applied_edits(apply_batch_id, prediction_id);

CREATE TABLE corrections (
  id                              TEXT PRIMARY KEY,
  asset_id                        TEXT NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
  prediction_id                   TEXT NOT NULL REFERENCES predictions(id),
  model_version_id                TEXT NOT NULL REFERENCES model_versions(id),
  predicted_settings_json         TEXT NOT NULL,
  corrected_settings_json         TEXT NOT NULL,
  delta_json                      TEXT NOT NULL,
  correction_magnitude            REAL NOT NULL,
  observed_at                     TEXT NOT NULL,
  included_in_training_version    TEXT
);
CREATE INDEX idx_corrections_model ON corrections(model_version_id);

CREATE TABLE jobs (
  id                TEXT PRIMARY KEY,
  type              TEXT NOT NULL,
  status            TEXT NOT NULL CHECK (status IN ('queued','running','paused','completed','failed','canceled','interrupted')),
  payload_json      TEXT NOT NULL DEFAULT '{}',
  progress_current  INTEGER NOT NULL DEFAULT 0,
  progress_total    INTEGER NOT NULL DEFAULT 0,
  phase             TEXT,
  resumable         INTEGER NOT NULL DEFAULT 0,
  created_at        TEXT NOT NULL,
  started_at        TEXT,
  heartbeat_at      TEXT,
  completed_at      TEXT,
  result_json       TEXT,
  error_json        TEXT
);
CREATE INDEX idx_jobs_status ON jobs(status, created_at);

CREATE TABLE events (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  level        TEXT NOT NULL,
  category     TEXT NOT NULL,
  event_type   TEXT NOT NULL,
  entity_type  TEXT,
  entity_id    TEXT,
  payload_json TEXT NOT NULL DEFAULT '{}',
  created_at   TEXT NOT NULL
);
CREATE INDEX idx_events_created ON events(created_at);
CREATE INDEX idx_events_entity ON events(entity_type, entity_id);

CREATE TABLE lightroom_connections (
  id                    TEXT PRIMARY KEY,
  catalog_fingerprint   TEXT NOT NULL,
  lightroom_version     TEXT,
  sdk_version           TEXT,
  plugin_version        TEXT,
  capabilities_json     TEXT NOT NULL DEFAULT '{}',
  first_seen_at         TEXT NOT NULL,
  last_seen_at          TEXT NOT NULL,
  status                TEXT NOT NULL
);
CREATE UNIQUE INDEX idx_lightroom_connections_catalog ON lightroom_connections(catalog_fingerprint);

CREATE TABLE update_state (
  id                    INTEGER PRIMARY KEY CHECK (id = 1),
  current_version       TEXT NOT NULL,
  latest_seen_version   TEXT,
  staged_version        TEXT,
  channel               TEXT NOT NULL DEFAULT 'stable',
  last_checked_at       TEXT,
  last_update_result    TEXT,
  update_error          TEXT
);
