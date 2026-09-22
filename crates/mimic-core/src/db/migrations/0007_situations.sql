-- The situation vocabulary. Schema v5 created `situations` and
-- `message_situations` and nothing wrote to either; 0.10.0 starts filing the
-- user's own messages under these six, by rule, so the situational voice
-- layer has something to measure.
--
-- The ids are stable. They are the scope keys of the situational layer and
-- the values of `drafts.situation_id`, and `crates/mimic-core/src/situations.rs`
-- names the same six in the same order. A user-defined situation, when there
-- is one, gets a generated id and `is_builtin = 0`.
INSERT OR IGNORE INTO situations(id, label, description, is_builtin, created_at) VALUES
  ('declining',   'Saying no',             'Turning something down: an invitation, a request, an offer.', 1, '2026-09-22T00:00:00Z'),
  ('scheduling',  'Setting a time',        'Proposing, accepting or moving a time to meet or talk.',       1, '2026-09-22T00:00:00Z'),
  ('apologising', 'Apologising',           'Saying sorry for something you did or did not do.',            1, '2026-09-22T00:00:00Z'),
  ('thanking',    'Saying thanks',         'Thanking someone for something specific.',                     1, '2026-09-22T00:00:00Z'),
  ('explaining',  'Explaining something',  'Setting out why, or how something works.',                     1, '2026-09-22T00:00:00Z'),
  ('disagreeing', 'Disagreeing',           'Telling someone you see it differently.',                      1, '2026-09-22T00:00:00Z');
