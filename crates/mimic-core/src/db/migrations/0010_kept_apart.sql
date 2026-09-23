-- Whose mail is whose (0.10.0-alpha.8).
--
-- An address the user adds can be filed, in mail already read, under someone
-- who is really the user; Mimic asks, and folds them in on a yes. `kept_apart`
-- is the user's no: this person, under one of the user's addresses, is not
-- them. It is never overridden without asking — not even once what made the
-- person worth asking about (a relationship, notes) has been cleared — and it
-- goes with the person, like everything else the user said about them.
ALTER TABLE participants ADD COLUMN kept_apart INTEGER NOT NULL DEFAULT 0;
