-- What needs a reply (0.10.0-alpha.4).
--
-- `thread_marks` is what the user said about whether a thread needs a reply,
-- tied to the message they said it about. It applies only while that message
-- is still the one the thread is waiting on, so a thread taken off the list
-- comes back by itself when the person writes again, and a thread kept on the
-- list stops being special once it moves on. One mark per thread; a new one
-- replaces it.
--
-- `needs_reply` exists for the threads Mimic reads as automated from their
-- headers and the user says are not: the user's call outranks the rule, as a
-- situation the user chose outranks one the rules filed.
CREATE TABLE thread_marks (
  conversation_id TEXT PRIMARY KEY REFERENCES conversations(id) ON DELETE CASCADE,
  message_id      TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
  mark            TEXT NOT NULL CHECK (mark IN ('no_reply_needed','needs_reply')),
  marked_at       TEXT NOT NULL
);
CREATE INDEX idx_thread_marks_message ON thread_marks(message_id);

-- A draft answers one message. Matching a draft to the message on screen by
-- its text put a draft for an earlier "can you call me?" under a later one;
-- drafts written from now on record which message they answer. Older drafts
-- have NULL here and are matched by text, as before.
ALTER TABLE drafts ADD COLUMN incoming_message_id TEXT REFERENCES messages(id) ON DELETE SET NULL;
CREATE INDEX idx_drafts_incoming_message ON drafts(incoming_message_id);
