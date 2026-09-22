-- Email threads are joined across checks and across sources by Message-ID
-- (0.10.0-alpha.3), which looks messages up by `external_id` alone. The
-- existing unique index leads with `source_id`, so it cannot serve that
-- lookup, and without this one every conversation imported would scan the
-- messages table.
CREATE INDEX IF NOT EXISTS idx_messages_external_id ON messages(external_id);
