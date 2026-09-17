-- Mimic schema v2 (0.3.0): sessions, apply and restore.
--
-- * libraries.purpose separates training libraries from the per-session
--   ingest libraries that back a session's photos ('training' | 'session').
-- * applied_edits gains restore bookkeeping so a batch rollback is recorded
--   per item with its own read-back result, never by rewriting the apply row.
-- * predictions remember which capability schema was current when they were
--   produced so the apply path can refuse stale predictions.

ALTER TABLE libraries ADD COLUMN purpose TEXT NOT NULL DEFAULT 'training'
  CHECK (purpose IN ('training','session'));

ALTER TABLE applied_edits ADD COLUMN restored_at TEXT;
ALTER TABLE applied_edits ADD COLUMN restore_result TEXT
  CHECK (restore_result IS NULL OR restore_result IN ('restored','verify_failed','failed','skipped'));
ALTER TABLE applied_edits ADD COLUMN restore_error_json TEXT;

ALTER TABLE predictions ADD COLUMN capability_schema_version TEXT;
ALTER TABLE predictions ADD COLUMN cluster_id TEXT;

CREATE INDEX idx_libraries_purpose ON libraries(purpose);
CREATE INDEX idx_applied_edits_asset ON applied_edits(asset_id, applied_at);
