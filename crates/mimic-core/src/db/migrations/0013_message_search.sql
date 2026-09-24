-- Finding what was said (0.10.0-alpha.18): a full-text index of the text of
-- every message Mimic has read, the user's and everyone else's.
--
-- `message_search` keeps its own copy of each message's text, so a result
-- can show the words around a match. Its rows share the message's rowid and
-- carry the message's id, which a search joins on. Triggers keep it in step
-- with `messages`: a message inserted is indexed, one whose text changes is
-- indexed again, and one deleted — on its own, or with its writer, source or
-- conversation — leaves the index with it. Indexing a message first clears
-- whatever the index holds under its rowid, by a DELETE, which no statement's
-- conflict clause (the importer's INSERT OR IGNORE) can turn into anything
-- else; so a message is never left unindexed, or indexed twice.
--
-- `secure-delete` makes a deletion remove the message's words from the index
-- itself, rather than leaving them in its segments until a later merge.
--
-- Words are folded to lower case and stripped of accents ("Résumé" finds
-- "resume"), and split on anything that is not a letter or a digit. Chinese,
-- Japanese and Thai, written without spaces, are kept a run at a time: a
-- search finds a run by its start, not by a word inside it.
CREATE VIRTUAL TABLE message_search USING fts5(
  body,
  message_id UNINDEXED,
  tokenize = 'unicode61 remove_diacritics 2'
);
INSERT INTO message_search(message_search, rank) VALUES ('secure-delete', 1);

CREATE TRIGGER message_search_insert AFTER INSERT ON messages BEGIN
  DELETE FROM message_search WHERE rowid = new.rowid;
  INSERT INTO message_search(rowid, body, message_id) VALUES (new.rowid, new.body, new.id);
END;

CREATE TRIGGER message_search_delete AFTER DELETE ON messages BEGIN
  DELETE FROM message_search WHERE rowid = old.rowid;
END;

CREATE TRIGGER message_search_update AFTER UPDATE OF body ON messages BEGIN
  DELETE FROM message_search WHERE rowid = old.rowid;
  INSERT INTO message_search(rowid, body, message_id) VALUES (new.rowid, new.body, new.id);
END;

-- Everything already read.
INSERT INTO message_search(rowid, body, message_id) SELECT rowid, body, id FROM messages;
