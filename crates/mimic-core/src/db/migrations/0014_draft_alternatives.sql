-- Other ways to say it (0.10.0-alpha.21): a draft written as another way of
-- saying what a first draft said — for the same message, from the same note,
-- shorter, longer, more casual or more professional — to be shown beside it.
--
-- `alternative_to` is the first draft, never another alternative, so a set
-- is the first draft and every draft that names it. An alternative goes with
-- its first draft. Using any draft of a set puts the rest aside as
-- `regenerated`; putting the first aside puts every alternative aside with
-- it; putting an alternative aside touches nothing else.
ALTER TABLE drafts ADD COLUMN alternative_to TEXT REFERENCES drafts(id) ON DELETE CASCADE;
CREATE INDEX idx_drafts_alternative_to ON drafts(alternative_to) WHERE alternative_to IS NOT NULL;
