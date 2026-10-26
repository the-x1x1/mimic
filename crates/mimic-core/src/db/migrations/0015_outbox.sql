-- Sending (0.10.0-alpha.23): every reply Mimic sends, or holds to send, is
-- one row here, written before anything is said to the mail server.
--
-- `how` is `approved` — the user pressed Send on it — or `autonomous`, sent
-- on its own under the rules the user set (0.10.0-alpha.24). An approved
-- reply starts `sending`; one sent on its own starts `held` until
-- `send_after`, and can be canceled until then. Each ends `sent`, `failed`
-- or `canceled` — or `unsure`: the connection was lost after the message was
-- handed over, or Mimic stopped while it was being sent (a row still
-- `sending` at start-up), so it may or may not have gone. Nothing sends an
-- `unsure` reply's draft again until the user says it didn't go, which makes
-- it `failed`.
--
-- A row goes with the person it was to, the mailbox it went through and the
-- conversation it answered: what it says is theirs as much as the user's.
-- At most one row per draft is held, sending, sent or unsure, so a second
-- window's Send on the same draft cannot send it twice.
CREATE TABLE outbox (
  id                  TEXT PRIMARY KEY,
  draft_id            TEXT REFERENCES drafts(id) ON DELETE SET NULL,
  source_id           TEXT NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
  conversation_id     TEXT REFERENCES conversations(id) ON DELETE CASCADE,
  answers_message_id  TEXT REFERENCES messages(id) ON DELETE SET NULL,
  participant_id      TEXT REFERENCES participants(id) ON DELETE CASCADE,
  from_address        TEXT NOT NULL,
  to_json             TEXT NOT NULL,
  cc_json             TEXT NOT NULL DEFAULT '[]',
  subject             TEXT NOT NULL,
  body                TEXT NOT NULL,
  message_id          TEXT NOT NULL,
  how                 TEXT NOT NULL CHECK (how IN ('approved','autonomous')),
  status              TEXT NOT NULL CHECK (status IN ('held','sending','sent','unsure','failed','canceled')),
  send_after          TEXT NOT NULL,
  error               TEXT,
  created_at          TEXT NOT NULL,
  updated_at          TEXT NOT NULL,
  sent_at             TEXT
);
CREATE INDEX idx_outbox_created ON outbox(created_at, id);
CREATE INDEX idx_outbox_due ON outbox(status, send_after);
CREATE UNIQUE INDEX idx_outbox_one_per_draft ON outbox(draft_id)
  WHERE draft_id IS NOT NULL AND status IN ('held','sending','sent','unsure');
