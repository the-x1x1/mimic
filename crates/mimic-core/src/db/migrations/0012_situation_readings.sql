-- Who decided what one of the user's messages is doing, when it was not the
-- rules (0.10.0-alpha.17).
--
-- `message_situations` says which situations a message is filed under, one
-- row each, and by whom. It cannot say "this message is doing none of them"
-- — that is the absence of rows, which is also what a message the rules
-- found nothing in looks like. So a decision is recorded here, one row per
-- message, and its situations (none, one or several) in `message_situations`
-- under the same hand.
--
-- * `user`: the user said what the message is doing. It stands until they
--   hand it back to the rules; nothing else writes over it.
-- * `model`: a model on this computer read it. `version` says which model,
--   and which way of asking. A person's decision replaces it.
--
-- A message with a row here is left alone by the rules. One without is the
-- rules' to file, as before.
CREATE TABLE situation_readings (
  message_id  TEXT PRIMARY KEY REFERENCES messages(id) ON DELETE CASCADE,
  read_by     TEXT NOT NULL CHECK (read_by IN ('model','user')),
  version     TEXT NOT NULL,
  read_at     TEXT NOT NULL
);
CREATE INDEX idx_situation_readings_by ON situation_readings(read_by);

-- A person filed messages by hand before there was anywhere to say so (the
-- column allowed it; nothing wrote it). Any such message is theirs.
INSERT OR IGNORE INTO situation_readings(message_id, read_by, version, read_at)
  SELECT message_id, 'user', 'user', MAX(classified_at) FROM message_situations
  WHERE classified_by = 'user' GROUP BY message_id;
-- And the rules' rows beside a person's decision go.
DELETE FROM message_situations
  WHERE classified_by <> 'user'
    AND message_id IN (SELECT message_id FROM situation_readings WHERE read_by = 'user');
