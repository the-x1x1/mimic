-- Mimic schema v4 (0.5.0): session intelligence.
--
-- A scene group may carry a reference photo chosen by the photographer; the
-- consistency policy then pulls the group toward that photo instead of the
-- median. Groups can be renamed/merged/split, so `edited_at` records when a
-- human last changed the grouping (predictions made before that are shown
-- as needing a re-run).

ALTER TABLE scene_clusters ADD COLUMN reference_asset_id TEXT REFERENCES assets(id) ON DELETE SET NULL;
ALTER TABLE scene_clusters ADD COLUMN edited_at TEXT;
