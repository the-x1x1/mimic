# Privacy

Mimic reads a person's private correspondence. This document is what that obliges.

## Where the data is

One SQLite file under the per-user application data directory — `%LOCALAPPDATA%\Formicaria\Mimic\data\mimic.db` on Windows. No account, no sync, no server, no telemetry. Deleting that folder deletes everything Mimic knows.

The original files the user imported from are never modified and never moved.

## What leaves the computer

Three things, all of them the user's choice and all of them visible in the interface.

**Drafting.** The provider selected in Settings receives the assembled prompt: the message being replied to, the intent the user typed, a handful of their own past messages, the recipient's display name and relationship, and the measured description of how they write. When that provider is the local one, this does not leave the machine at all, and the top bar says "Local model". When it is a hosted provider, the top bar says "Sends to Claude" and the Settings description spells out exactly what is transmitted.

**Update checks.** A request to the GitHub releases endpoint carrying the current version. Switched off with `updates.automatic`.

**Nothing else.** No usage analytics, no crash reporting, no model training on the user's data. A diagnostics bundle is written locally and shared only if the user chooses to; it contains counts, schema versions and recent error events, never message bodies, never credentials, and by default paths are reduced to their file name.

## What is in the logs

Job progress, error codes, counts and timings. **No message content by default.** Provider errors are truncated to one line and carry the status and the provider's own message, never the request body.

## Other people's words

An email thread contains other people's messages. Mimic imports them, because a conversation without the other half is not a conversation and cannot be used to learn how the user responds. It analyzes only the user's own. It deletes both together.

Onboarding and the Sources screen both say the same thing: **train Mimic only on communication you own or have permission to process.** That is the user's judgement to make, and the product should not pretend it has been made for them.

## Deletion

**Deleting a person** removes: their participant row and identifiers; their messages; every message in a conversation that existed only between them and the user, _including the user's own half of it_; the embeddings of all of those; the representative examples drawn from them; their relationship profile; any preferences set for them; drafts written to them. Every remaining profile that was computed over material including theirs is marked stale and must be recomputed.

The dialog says all of this before it happens, in sentences rather than a table of counts, and it calls out the user's own messages specifically — people do not expect "delete Ada" to delete what they themselves wrote to her, and it does. Group conversations survive with that person's messages removed.

The preview is produced by the same code path as the deletion with the writes skipped, so it cannot understate the consequences. Tested in `privacy::tests::a_preview_changes_nothing_and_matches_what_deletion_does`.

**Deleting a source** removes everything imported through it, and any person Mimic only ever saw through it.

**Deleting everything** empties every communication table and keeps settings, identity and provider configuration. It requires typing a phrase.

None of these are recoverable. `Db::open` writes a database backup before a schema migration, not before a deletion.

## Credentials

Provider API keys are stored in `credentials/credentials.json` with owner-only permissions, not in the OS credential store. This is a known limitation with a name and a place on the roadmap; see `docs/MODEL_PROVIDERS.md`. Credentials are never in the database, never in a log, and stripped from diagnostics at every depth.

## What Mimic will not do

It will not send a message. It has no send access to anything and no code path that could acquire one. Every draft is a draft, and the user moves it into whatever app they actually use.

Trusted Mode — Mimic sending low-stakes replies itself — is designed and deliberately unbuilt. The reason is not technical. A product that can send as you needs a much stronger account of what it will not send, how a mistake is caught, and what "low-stakes" means than this product currently has.
