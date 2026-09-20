-- Mimic schema v5. Retires the photography domain and creates the
-- communication model (docs/DATA_MODEL.md).
--
-- The photography tables are dropped rather than migrated: they describe a
-- product that no longer exists, and `Db::open` writes a timestamped backup
-- to data/backups/ before applying this migration, so nothing is unrecoverable.
--
-- Timestamps are RFC 3339 UTC TEXT. IDs are UUID v4 TEXT. JSON columns end in
-- _json. Everything here is indexed for a store of 100k-1M+ messages.

DROP TABLE IF EXISTS correction_syncs;
DROP TABLE IF EXISTS corrections;
DROP TABLE IF EXISTS applied_edits;
DROP TABLE IF EXISTS apply_batches;
DROP TABLE IF EXISTS predictions;
DROP TABLE IF EXISTS model_artifacts;
DROP TABLE IF EXISTS model_versions;
DROP TABLE IF EXISTS training_sets;
DROP TABLE IF EXISTS scene_clusters;
DROP TABLE IF EXISTS session_assets;
DROP TABLE IF EXISTS sessions;
DROP TABLE IF EXISTS style_profile_libraries;
DROP TABLE IF EXISTS style_profiles;
DROP TABLE IF EXISTS visual_features;
DROP TABLE IF EXISTS edit_snapshots;
DROP TABLE IF EXISTS sidecars;
DROP TABLE IF EXISTS assets;
DROP TABLE IF EXISTS libraries;
DROP TABLE IF EXISTS lightroom_connections;

-- ---------------------------------------------------------------- identity

-- Who the user is. One row; the app refuses to infer direction without it.
CREATE TABLE user_identity (
  id            TEXT PRIMARY KEY,
  display_name  TEXT NOT NULL,
  created_at    TEXT NOT NULL,
  updated_at    TEXT NOT NULL
);

-- Every address the user writes from. Direction inference is exactly
-- "does this message's author match one of these".
CREATE TABLE user_identifiers (
  id                TEXT PRIMARY KEY,
  user_identity_id  TEXT NOT NULL REFERENCES user_identity(id) ON DELETE CASCADE,
  kind              TEXT NOT NULL CHECK (kind IN ('email','phone','handle','display_name','account_id')),
  value             TEXT NOT NULL,
  normalized_value  TEXT NOT NULL,
  added_at          TEXT NOT NULL
);
CREATE UNIQUE INDEX idx_user_identifiers_value ON user_identifiers(kind, normalized_value);

-- ----------------------------------------------------------------- sources

CREATE TABLE sources (
  id                TEXT PRIMARY KEY,
  connector         TEXT NOT NULL,
  name              TEXT NOT NULL,
  channel           TEXT NOT NULL CHECK (channel IN ('email','sms','chat','forum','other')),
  location          TEXT,
  config_json       TEXT NOT NULL DEFAULT '{}',
  status            TEXT NOT NULL DEFAULT 'new'
                      CHECK (status IN ('new','ready','importing','imported','failed')),
  created_at        TEXT NOT NULL,
  last_imported_at  TEXT,
  message_count     INTEGER NOT NULL DEFAULT 0,
  last_error_json   TEXT
);
CREATE INDEX idx_sources_status ON sources(status);

-- ------------------------------------------------------------- participants

CREATE TABLE participants (
  id            TEXT PRIMARY KEY,
  display_name  TEXT NOT NULL,
  is_self       INTEGER NOT NULL DEFAULT 0,
  relationship  TEXT,
  notes         TEXT,
  created_at    TEXT NOT NULL,
  updated_at    TEXT NOT NULL
);
CREATE INDEX idx_participants_self ON participants(is_self);

CREATE TABLE participant_identifiers (
  id                TEXT PRIMARY KEY,
  participant_id    TEXT NOT NULL REFERENCES participants(id) ON DELETE CASCADE,
  kind              TEXT NOT NULL CHECK (kind IN ('email','phone','handle','display_name','account_id')),
  value             TEXT NOT NULL,
  normalized_value  TEXT NOT NULL,
  first_seen_at     TEXT NOT NULL
);
CREATE UNIQUE INDEX idx_participant_identifiers_value ON participant_identifiers(kind, normalized_value);
CREATE INDEX idx_participant_identifiers_participant ON participant_identifiers(participant_id);

-- ------------------------------------------------------------ conversations

CREATE TABLE conversations (
  id               TEXT PRIMARY KEY,
  source_id        TEXT NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
  external_id      TEXT NOT NULL,
  channel          TEXT NOT NULL CHECK (channel IN ('email','sms','chat','forum','other')),
  subject          TEXT,
  is_group         INTEGER NOT NULL DEFAULT 0,
  started_at       TEXT,
  last_message_at  TEXT,
  message_count    INTEGER NOT NULL DEFAULT 0,
  created_at       TEXT NOT NULL
);
CREATE UNIQUE INDEX idx_conversations_external ON conversations(source_id, external_id);
CREATE INDEX idx_conversations_last ON conversations(last_message_at);

CREATE TABLE conversation_participants (
  conversation_id  TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
  participant_id   TEXT NOT NULL REFERENCES participants(id) ON DELETE CASCADE,
  message_count    INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (conversation_id, participant_id)
);
CREATE INDEX idx_conversation_participants_participant ON conversation_participants(participant_id);

-- ---------------------------------------------------------------- messages

CREATE TABLE messages (
  id                        TEXT PRIMARY KEY,
  conversation_id           TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
  source_id                 TEXT NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
  participant_id            TEXT REFERENCES participants(id) ON DELETE CASCADE,
  external_id               TEXT NOT NULL,
  direction                 TEXT NOT NULL CHECK (direction IN ('self','other','unknown')),
  channel                   TEXT NOT NULL CHECK (channel IN ('email','sms','chat','forum','other')),
  sent_at                   TEXT,
  sequence_index            INTEGER NOT NULL DEFAULT 0,
  body                      TEXT NOT NULL,
  body_hash                 TEXT NOT NULL,
  word_count                INTEGER NOT NULL DEFAULT 0,
  char_count                INTEGER NOT NULL DEFAULT 0,
  reply_to_message_id       TEXT REFERENCES messages(id) ON DELETE SET NULL,
  response_latency_seconds  INTEGER,
  metadata_json             TEXT NOT NULL DEFAULT '{}',
  imported_at               TEXT NOT NULL
);
CREATE UNIQUE INDEX idx_messages_identity ON messages(source_id, external_id);
CREATE INDEX idx_messages_conversation ON messages(conversation_id, sequence_index);
CREATE INDEX idx_messages_participant ON messages(participant_id, sent_at);
CREATE INDEX idx_messages_direction ON messages(direction, channel, sent_at);
CREATE INDEX idx_messages_sent ON messages(sent_at);

CREATE TABLE message_embeddings (
  message_id         TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
  embedding_version  TEXT NOT NULL,
  dims               INTEGER NOT NULL,
  vector             BLOB NOT NULL,
  computed_at        TEXT NOT NULL,
  PRIMARY KEY (message_id, embedding_version)
);

-- --------------------------------------------------------------- situations

CREATE TABLE situations (
  id           TEXT PRIMARY KEY,
  label        TEXT NOT NULL UNIQUE,
  description  TEXT,
  is_builtin   INTEGER NOT NULL DEFAULT 0,
  created_at   TEXT NOT NULL
);

CREATE TABLE message_situations (
  message_id     TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
  situation_id   TEXT NOT NULL REFERENCES situations(id) ON DELETE CASCADE,
  confidence     REAL NOT NULL DEFAULT 0,
  classified_by  TEXT NOT NULL CHECK (classified_by IN ('rule','model','user')),
  classified_at  TEXT NOT NULL,
  PRIMARY KEY (message_id, situation_id)
);
CREATE INDEX idx_message_situations_situation ON message_situations(situation_id);

-- ------------------------------------------------------------------- voice

-- One row per (layer, scope, analysis version). `scope_key` is '' for the
-- global layer, the channel name for the channel layer, the participant id for
-- the relationship layer and the situation id for the situational layer.
CREATE TABLE voice_profiles (
  id                TEXT PRIMARY KEY,
  layer             TEXT NOT NULL CHECK (layer IN ('global','channel','relationship','situational')),
  scope_key         TEXT NOT NULL,
  participant_id    TEXT REFERENCES participants(id) ON DELETE CASCADE,
  metrics_json      TEXT NOT NULL DEFAULT '{}',
  qualitative_json  TEXT NOT NULL DEFAULT '{}',
  sample_size       INTEGER NOT NULL DEFAULT 0,
  analysis_version  TEXT NOT NULL,
  computed_at       TEXT NOT NULL,
  stale             INTEGER NOT NULL DEFAULT 0
);
CREATE UNIQUE INDEX idx_voice_profiles_scope ON voice_profiles(layer, scope_key, analysis_version);
CREATE INDEX idx_voice_profiles_participant ON voice_profiles(participant_id);

-- Manual overrides. These beat anything the statistics say.
CREATE TABLE voice_preferences (
  id          TEXT PRIMARY KEY,
  layer       TEXT NOT NULL CHECK (layer IN ('global','channel','relationship','situational')),
  scope_key   TEXT NOT NULL,
  key         TEXT NOT NULL,
  value_json  TEXT NOT NULL,
  note        TEXT,
  created_at  TEXT NOT NULL,
  updated_at  TEXT NOT NULL
);
CREATE UNIQUE INDEX idx_voice_preferences_key ON voice_preferences(layer, scope_key, key);

CREATE TABLE representative_examples (
  id              TEXT PRIMARY KEY,
  message_id      TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
  layer           TEXT NOT NULL CHECK (layer IN ('global','channel','relationship','situational')),
  scope_key       TEXT NOT NULL,
  participant_id  TEXT REFERENCES participants(id) ON DELETE CASCADE,
  reason          TEXT NOT NULL,
  score           REAL NOT NULL DEFAULT 0,
  selected_at     TEXT NOT NULL
);
CREATE UNIQUE INDEX idx_representative_unique ON representative_examples(layer, scope_key, message_id);
CREATE INDEX idx_representative_scope ON representative_examples(layer, scope_key, score);

-- ------------------------------------------------------------------ drafts

CREATE TABLE drafts (
  id                TEXT PRIMARY KEY,
  participant_id    TEXT REFERENCES participants(id) ON DELETE CASCADE,
  conversation_id   TEXT REFERENCES conversations(id) ON DELETE SET NULL,
  channel           TEXT NOT NULL CHECK (channel IN ('email','sms','chat','forum','other')),
  situation_id      TEXT REFERENCES situations(id) ON DELETE SET NULL,
  incoming_message  TEXT,
  intent            TEXT,
  generated_text    TEXT NOT NULL,
  final_text        TEXT,
  provider          TEXT NOT NULL,
  model             TEXT NOT NULL,
  context_json      TEXT NOT NULL DEFAULT '{}',
  prompt_hash       TEXT NOT NULL,
  evidence_json     TEXT NOT NULL DEFAULT '{}',
  created_at        TEXT NOT NULL,
  resolved_at       TEXT,
  outcome           TEXT CHECK (outcome IN ('sent_unedited','sent_edited','discarded','regenerated'))
);
CREATE INDEX idx_drafts_participant ON drafts(participant_id, created_at);
CREATE INDEX idx_drafts_outcome ON drafts(outcome, created_at);

CREATE TABLE draft_feedback (
  id                          TEXT PRIMARY KEY,
  draft_id                    TEXT NOT NULL REFERENCES drafts(id) ON DELETE CASCADE,
  kind                        TEXT NOT NULL CHECK (kind IN ('edit','preference','rating')),
  weight                      REAL NOT NULL DEFAULT 1.0,
  diff_json                   TEXT NOT NULL DEFAULT '{}',
  note                        TEXT,
  created_at                  TEXT NOT NULL,
  applied_to_analysis_version TEXT
);
CREATE UNIQUE INDEX idx_draft_feedback_kind ON draft_feedback(draft_id, kind);

-- -------------------------------------------------------- analysis and eval

CREATE TABLE analysis_runs (
  id                   TEXT PRIMARY KEY,
  kind                 TEXT NOT NULL,
  analysis_version     TEXT NOT NULL,
  scope_json           TEXT NOT NULL DEFAULT '{}',
  started_at           TEXT NOT NULL,
  completed_at         TEXT,
  status               TEXT NOT NULL CHECK (status IN ('running','completed','failed','canceled')),
  messages_considered  INTEGER NOT NULL DEFAULT 0,
  profiles_written     INTEGER NOT NULL DEFAULT 0,
  error_json           TEXT
);
CREATE INDEX idx_analysis_runs_started ON analysis_runs(started_at);

-- Held-out evaluation. Nothing in the UI may show a score that does not
-- trace back to a row here (docs/VOICE_ENGINE.md §"Measuring").
CREATE TABLE evaluations (
  id                TEXT PRIMARY KEY,
  analysis_version  TEXT NOT NULL,
  created_at        TEXT NOT NULL,
  holdout_size      INTEGER NOT NULL,
  metrics_json      TEXT NOT NULL DEFAULT '{}',
  config_json       TEXT NOT NULL DEFAULT '{}'
);

CREATE TABLE evaluation_cases (
  id              TEXT PRIMARY KEY,
  evaluation_id   TEXT NOT NULL REFERENCES evaluations(id) ON DELETE CASCADE,
  message_id      TEXT REFERENCES messages(id) ON DELETE SET NULL,
  generated_text  TEXT NOT NULL,
  actual_text     TEXT NOT NULL,
  metrics_json    TEXT NOT NULL DEFAULT '{}'
);
CREATE INDEX idx_evaluation_cases_eval ON evaluation_cases(evaluation_id);
