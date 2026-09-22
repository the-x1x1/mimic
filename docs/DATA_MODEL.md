# Data model

SQLite, schema version 6. Timestamps are RFC 3339 UTC `TEXT`, ids are UUID v4 `TEXT`, JSON columns end in `_json`. The authoritative definition is `crates/mimic-core/src/db/migrations/0005_communication.sql`, with `0006_themes.sql` carrying an older install's theme name over; this file explains why the tables are shaped the way they are.

## Identity

**`user_identity`** — one row: who the user is. **`user_identifiers`** — every address they write from, with a `normalized_value` that matching uses (lowercased for email and handles, digits-only for phone numbers, with a leading country code stripped when it leaves a plausible number).

This is the most load-bearing table in the schema for a reason that is easy to miss: **direction is decided by matching an imported message's author against these rows, and nothing else.** No heuristics, no "the most frequent sender is probably you". If an address is missing, the messages sent from it import as somebody else's and silently corrupt every metric. The importer refuses to run when no identifier is set, and the Sources screen shows the most frequent addresses in a file before import so the user can check.

## Sources

**`sources`** — one row per thing the user pointed Mimic at: which connector reads it, which channel it defaults to, where it is, how many messages it produced, and the last error if it failed. `message_count` is recomputed from `messages` rather than incremented, so a deletion or a re-import cannot leave it lying.

## People

**`participants`** and **`participant_identifiers`**, with the same normalization rules as the user's own. Resolution is: look for a participant owning any of the author's identifiers; if found, attach any identifier that is new and upgrade a placeholder display name to a real one; if not, create.

Two decisions worth stating. An identifier already owned by someone else is **not** moved — two people sharing a family address stay two people rather than being silently merged. An author with no usable identifier resolves to nobody and the message lands as `unknown`, rather than being pooled into a single "unnamed" participant that would blend a dozen strangers into one voice.

`relationship` is free text the user sets and Mimic never infers. It feeds the prompt and the retrieval filter.

## Conversations and messages

**`conversations`** — `(source_id, external_id)` unique, so a re-import finds the same thread. **`conversation_participants`** — who is in it, which is also what distinguishes a group thread from a one-to-one and therefore what deletion depends on.

**`messages`** is the table everything else is computed from.

| Column                     | Why it exists                                                                                                                          |
| -------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| `(source_id, external_id)` | Unique. The import identity key: this is what makes re-importing free.                                                                 |
| `direction`                | `self` \| `other` \| `unknown`. Only `self` is evidence.                                                                               |
| `channel`                  | Denormalized from the conversation so the channel voice layer is one index scan.                                                       |
| `sequence_index`           | Position within the conversation, assigned at import. Ordering does not depend on timestamps, which exports lose.                      |
| `body`, `body_hash`        | The cleaned text and its digest.                                                                                                       |
| `word_count`, `char_count` | Computed once at insert; every length metric reads these rather than re-tokenizing a million rows.                                     |
| `reply_to_message_id`      | Derived after the batch, because a reply can appear in an export before what it answers.                                               |
| `response_latency_seconds` | Only when both timestamps exist and the reply crosses a direction boundary. Two of your own messages in a row are not a response time. |

**`message_embeddings`** keeps vectors out of the message row, keyed by `(message_id, embedding_version)` so two providers' vectors are never compared.

## Situations

**`situations`** holds the vocabulary: six built-in rows (`is_builtin = 1`) seeded by migration 0007 — `declining`, `scheduling`, `apologising`, `thanking`, `explaining`, `disagreeing`. Their ids are stable, because they are also the situational layer's `scope_key` and the value of `drafts.situation_id`.

**`message_situations`** files messages under situations: `(message_id, situation_id)` with a `confidence` and a `classified_by` of `rule`, `model` or `user`. Only the user's own messages are filed. Rule rows are replaced on every analysis; user rows are never touched by it. Deleting a message cascades here; "delete everything" empties it and keeps the vocabulary.

## Voice

**`voice_profiles`** — one row per `(layer, scope_key, analysis_version)`. `scope_key` is `''` for global, the channel name for channel, the participant id for relationship, the situation id for situational. `metrics_json` is a serialized `VoiceMetrics`; `sample_size` is stored separately so the UI can show it without parsing. `stale` marks a profile whose underlying messages have changed.

Keying on `analysis_version` means a new analysis version is computed alongside the old one rather than overwriting it, so the numbers on screen never become a mixture of two definitions.

**`voice_preferences`** — manual overrides, scoped the same way. These beat the statistics; the prompt assembler applies them last and labels them as overriding.

**`representative_examples`** — the user's own messages chosen to show a scope's register, with the reason each was chosen. Replaced wholesale per scope when analysis runs.

## Drafts and feedback

**`drafts`** records what was asked for, what was generated, what was sent, which provider and model, the context, the `prompt_hash` and the evidence. **`draft_feedback`** records what the difference meant, one row per `(draft_id, kind)` so re-recording updates rather than accumulating. `weight` encodes the rule that a stated preference (3.0) outranks an inferred edit (1.0).

## Analysis and evaluation

**`analysis_runs`** is what lets the Voice screen say "last analyzed three days ago over 4,182 of your messages" instead of showing a number with no provenance. **`evaluations`** and **`evaluation_cases`** hold held-out results; **no score shown in the UI may exist without a row here.**

## Deletion

Foreign keys cascade from `participants` and `sources` to messages, identifiers, conversation links, embeddings and drafts. That handles most of it. It does not handle the two things people actually care about, so `privacy` does them explicitly:

1. A one-to-one conversation with a deleted person is deleted entirely, **including the user's own half of it**. A conversation cannot be half-deleted, and leaving the user's side behind would leave their words in a thread with a ghost. Group conversations survive with that person's messages removed.
2. Everything derived from the deleted material is invalidated: profiles that included it are marked stale and must be recomputed before they are trusted again.

`preview_participant_deletion` runs the same counting code with the writes skipped, so the confirmation dialog cannot understate the consequences.

## Migration from v4

Migration `0005` drops all nineteen photography tables and creates the communication schema. `app_settings`, `jobs`, `events`, `update_state` and `schema_migrations` survive with their rows. A v4 database loses its photography data, which is correct, and `Db::open` writes a timestamped backup to `data/backups/` before the migration runs, so it is recoverable if anyone ever needs it. Tested in `migrations.rs::photography_database_upgrades_to_the_communication_schema`.
