-- Sending through Microsoft Graph (0.10.0-alpha.94). A Microsoft 365 mailbox
-- whose organisation turned off sending from apps by SMTP sends the same
-- message through Graph's sendMail instead. `via` says a reply went that way
-- ('graph'); NULL is the usual way, the mailbox's own sending server or the
-- chat service's own.
--
-- Graph doesn't say which Message-ID a message went under, so `read_back` is
-- NULL until its copy in the Sent folder is read: then it holds the id that
-- copy was read under — its own, if Exchange kept it, or another, matched
-- once by its words — and the copy is not kept a second time.
ALTER TABLE outbox ADD COLUMN via TEXT;
ALTER TABLE outbox ADD COLUMN read_back TEXT;
