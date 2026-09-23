# Data model

SQLite, schema version 9. Timestamps are RFC 3339 UTC `TEXT`, ids are UUID v4 `TEXT`, JSON columns end in `_json`. The authoritative definition is `crates/mimic-core/src/db/migrations/0005_communication.sql` and the migrations after it: `0006_themes.sql` carries an older install's theme name over, `0007_situations.sql` seeds the situation vocabulary, `0008_message_ids.sql` indexes `messages.external_id` for joining email threads by Message-ID, and `0009_thread_marks.sql` adds what the user said about a thread's reply. This file explains why the tables are shaped the way they are.

## Identity

**`user_identity`** — one row: who the user is. **`user_identifiers`** — every address they write from, with a `normalized_value` that matching uses (lowercased for email and handles, digits-only for phone numbers, with a leading country code stripped when it leaves a plausible number).

This is the most load-bearing table in the schema for a reason that is easy to miss: **direction is decided by matching an imported message's author against these rows, and nothing else.** No heuristics, no "the most frequent sender is probably you". If an address is missing, the messages sent from it import as somebody else's and silently corrupt every metric. The importer refuses to run when no identifier is set, and the Sources screen shows the most frequent addresses in a file before import so the user can check.

## Sources

**`sources`** — one row per thing the user pointed Mimic at: which connector reads it, which channel it defaults to, where it is, how many messages it produced, and the last error if it failed. `message_count` is recomputed from `messages` rather than incremented, so a deletion or a re-import cannot leave it lying.

## People

**`participants`** and **`participant_identifiers`**, with the same normalization rules as the user's own. Resolution is: look for a participant owning any of the author's identifiers; if found, attach any identifier that is new and upgrade a placeholder display name to a real one; if not, create.

Two decisions worth stating. An identifier already owned by someone else is **not** moved — two people sharing a family address stay two people rather than being silently merged. An author with no usable identifier resolves to nobody and the message lands as `unknown`, rather than being pooled into a single "unnamed" participant that would blend a dozen strangers into one voice.

`relationship` is free text the user sets and Mimic never infers. It feeds the prompt and the retrieval filter.

Whether a participant is a person or a sender of automated mail is computed, not stored (`Db::list_people`): a sender is automated when they sent at least one message and every message they sent carries an `automated` reading (see "What needs a reply"), and the user has not written in any conversation they are in, has not set their `relationship`, and has not marked one of their threads `needs_reply`. One message without a reading makes them a person; so does any of those. People, and every place a person is picked, list only people; the senders are counted and listed on request.

## Conversations and messages

**`conversations`** — `(source_id, external_id)` unique, so a re-import finds the same thread. **`conversation_participants`** — who is in it, which is also what distinguishes a group thread from a one-to-one and therefore what deletion depends on.

**`messages`** is the table everything else is computed from.

| Column                     | Why it exists                                                                                                                                                                                                                                                                                                                                                   |
| -------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `(source_id, external_id)` | Unique. The import identity key: this is what makes re-importing free.                                                                                                                                                                                                                                                                                          |
| `direction`                | `self` \| `other` \| `unknown`. Only `self` is evidence.                                                                                                                                                                                                                                                                                                        |
| `channel`                  | Denormalized from the conversation so the channel voice layer is one index scan.                                                                                                                                                                                                                                                                                |
| `sequence_index`           | Position within the conversation, assigned at import. Ordering does not depend on timestamps, which exports lose.                                                                                                                                                                                                                                               |
| `body`, `body_hash`        | The cleaned text and its digest.                                                                                                                                                                                                                                                                                                                                |
| `word_count`, `char_count` | Computed once at insert; every length metric reads these rather than re-tokenizing a million rows.                                                                                                                                                                                                                                                              |
| `reply_to_message_id`      | Derived after the batch, because a reply can appear in an export before what it answers.                                                                                                                                                                                                                                                                        |
| `response_latency_seconds` | Only when both timestamps exist and the reply crosses a direction boundary. Two of your own messages in a row are not a response time.                                                                                                                                                                                                                          |
| `metadata_json`            | What a connector knew that has no column. For email: `subject`, `refs` (the Message-IDs it answers, which is how a later reply finds its thread), and `automated` when the headers say a machine sent it — `newsletter`, `bulk`, `auto_reply`, `report` or `no_reply_address` (`sources::automated`). Headers are not kept, so this is decided once, at import. |

**`message_embeddings`** keeps vectors out of the message row, keyed by `(message_id, embedding_version)` so two providers' vectors are never compared.

## What needs a reply

Whether a thread is waiting is computed, not stored (`db::repo_waiting`). A thread is decided by its _deciding message_: the last message with a known direction that does not look automated, if that came from someone else — so an out-of-office reply threaded in after a colleague's question does not hide the question — and otherwise the last message, so something automated that arrived after the user's own reply is counted as left out rather than lost. The thread is unanswered when its deciding message came from someone else. Two things can leave an unanswered thread out, in this order:

**`thread_marks`** — at most one row per conversation: `no_reply_needed` or `needs_reply`, tied by `message_id` to the message the user was looking at when they said it. It applies only while that message is still the deciding one, so a thread taken off the list comes back when the person writes again without anything being cleared, and a click that arrives after they have written again changes nothing. It cascades from both the conversation and the message.

Otherwise, the deciding message's `metadata_json.automated` — which it can carry only when the whole thread does. A reading from headers, never from the words; only a text value counts; and the user's mark outranks it in both directions.

## Situations

**`situations`** holds the vocabulary: six built-in rows (`is_builtin = 1`) seeded by migration 0007 — `declining`, `scheduling`, `apologising`, `thanking`, `explaining`, `disagreeing`. Their ids are stable, because they are also the situational layer's `scope_key` and the value of `drafts.situation_id`.

**`message_situations`** files messages under situations: `(message_id, situation_id)` with a `confidence` and a `classified_by` of `rule`, `model` or `user`. Only the user's own messages are filed. Rule rows are replaced on every analysis; user rows are never touched by it. Deleting a message cascades here; "delete everything" empties it and keeps the vocabulary.

## Voice

**`voice_profiles`** — one row per `(layer, scope_key, analysis_version)`. `scope_key` is `''` for global, the channel name for channel, the participant id for relationship, the situation id for situational. `metrics_json` is a serialized `VoiceMetrics`; `sample_size` is stored separately so the UI can show it without parsing. `stale` marks a profile whose underlying messages have changed.

Keying on `analysis_version` means a new analysis version is computed alongside the old one rather than overwriting it, so the numbers on screen never become a mixture of two definitions.

**`voice_preferences`** — manual overrides, scoped the same way. These beat the statistics; the prompt assembler applies them last and labels them as overriding.

**`representative_examples`** — the user's own messages chosen to show a scope's register, with the reason each was chosen. Replaced wholesale per scope when analysis runs.

## Drafts and feedback

**`drafts`** records what was asked for, what was generated, what was sent, which provider and model, the context, the `prompt_hash` and the evidence. `incoming_message_id` (0.10.0-alpha.4) is the stored message a draft answers, so the home screen shows a draft under that message and no other — two messages with the same words are two questions. Drafts made before it have only `incoming_message` and are matched by text; a draft made since with no id answered pasted text and matches nothing. Once a draft for a message has been used or dropped, no other draft for it is offered. **`draft_feedback`** records what the difference meant, one row per `(draft_id, kind)` so re-recording updates rather than accumulating. `weight` encodes the rule that a stated preference (3.0) outranks an inferred edit (1.0).

## Analysis and evaluation

**`analysis_runs`** is what lets the Voice screen say "last analyzed three days ago over 4,182 of your messages" instead of showing a number with no provenance. **`evaluations`** and **`evaluation_cases`** hold held-out results; **no score shown in the UI may exist without a row here.**

## Deletion

Foreign keys cascade from `participants` and `sources` to messages, identifiers, conversation links, embeddings and drafts. That handles most of it. It does not handle the two things people actually care about, so `privacy` does them explicitly:

1. A one-to-one conversation with a deleted person is deleted entirely, **including the user's own half of it**. A conversation cannot be half-deleted, and leaving the user's side behind would leave their words in a thread with a ghost. Group conversations survive with that person's messages removed.
2. Everything derived from the deleted material is invalidated: profiles that included it are marked stale and must be recomputed before they are trusted again.

`preview_participant_deletion` runs the same counting code with the writes skipped, so the confirmation dialog cannot understate the consequences.

## Migration from v4

Migration `0005` drops all nineteen photography tables and creates the communication schema. `app_settings`, `jobs`, `events`, `update_state` and `schema_migrations` survive with their rows. A v4 database loses its photography data, which is correct, and `Db::open` writes a timestamped backup to `data/backups/` before the migration runs, so it is recoverable if anyone ever needs it. Tested in `migrations.rs::photography_database_upgrades_to_the_communication_schema`.
