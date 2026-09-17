-- Mimic schema v3 (0.4.0): corrections sync bookkeeping.
--
-- One row per sync of a session: how many applied photos were checked, how
-- many the photographer left untouched, how many were corrected. The
-- No-Touch Rate is derived from these rows plus `corrections`, never stored
-- as a free-standing number.

CREATE TABLE correction_syncs (
  id                              TEXT PRIMARY KEY,
  session_id                      TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  lightroom_catalog_fingerprint   TEXT,
  synced_at                       TEXT NOT NULL,
  checked_count                   INTEGER NOT NULL DEFAULT 0,
  untouched_count                 INTEGER NOT NULL DEFAULT 0,
  corrected_count                 INTEGER NOT NULL DEFAULT 0,
  unresolved_count                INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_correction_syncs_session ON correction_syncs(session_id, synced_at);

-- A correction belongs to exactly one prediction; re-syncing replaces it
-- unless it was already used by a training run.
CREATE UNIQUE INDEX idx_corrections_prediction ON corrections(prediction_id);
