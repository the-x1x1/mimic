-- Measuring the drafts (0.10.0-alpha.11).
--
-- The evaluation tables from 0005 were never written: the loop that fills
-- them did not exist until now. They are replaced before their first row,
-- for two reasons.
--
-- * Nothing is kept that outlives the messages it was measured on. A case
--   refers to the message answered and to the user's own reply by id, and
--   goes when either does; their text is read from `messages` when shown.
--   What was written for a case was written from more than those two — the
--   conversation before them, examples from other conversations — so
--   deleting any person or mailbox deletes every evaluation (`privacy`).
-- * A case is one reply written one way. Each held-out exchange is answered
--   by Mimic and by two baselines (a generic reply, and the reply the user
--   sends most often), and each answer is measured on its own row. The
--   summary is computed from the rows that remain whenever it is shown, so
--   no figure outlives the cases it came from.
DROP TABLE IF EXISTS evaluation_cases;
DROP TABLE IF EXISTS evaluations;

CREATE TABLE evaluations (
  id                TEXT PRIMARY KEY,
  analysis_version  TEXT NOT NULL,
  created_at        TEXT NOT NULL,
  provider          TEXT NOT NULL,
  model             TEXT,
  -- What was asked for: seed, held-out share, how many exchanges.
  config_json       TEXT NOT NULL DEFAULT '{}',
  -- How the split came out: strategy, conversations, held out, warnings.
  split_json        TEXT NOT NULL DEFAULT '{}'
);

CREATE TABLE evaluation_cases (
  id                   TEXT PRIMARY KEY,
  evaluation_id        TEXT NOT NULL REFERENCES evaluations(id) ON DELETE CASCADE,
  incoming_message_id  TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
  reply_message_id     TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
  -- 'mimic' | 'generic' | 'common_reply'
  system               TEXT NOT NULL,
  generated_text       TEXT NOT NULL,
  -- For the common reply: the message of the user's it was taken from.
  based_on_message_id  TEXT REFERENCES messages(id) ON DELETE CASCADE,
  metrics_json         TEXT NOT NULL DEFAULT '{}'
);
CREATE INDEX idx_evaluation_cases_eval ON evaluation_cases(evaluation_id);
CREATE INDEX idx_evaluation_cases_incoming ON evaluation_cases(incoming_message_id);
CREATE INDEX idx_evaluation_cases_reply ON evaluation_cases(reply_message_id);
CREATE INDEX idx_evaluation_cases_based_on ON evaluation_cases(based_on_message_id);
