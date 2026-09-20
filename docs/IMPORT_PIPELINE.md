# Import pipeline

## The connector contract

Every source implements one trait (`crates/mimic-core/src/sources/mod.rs`):

```rust
pub trait CommunicationSource: Send + Sync {
    fn metadata(&self) -> SourceMetadata;
    fn discover(&self, location: &Path) -> SourceResult<Vec<PathBuf>>;
    fn validate(&self, location: &Path) -> SourceResult<ValidationReport>;
    fn import(&self, location: &Path,
              sink: &mut dyn FnMut(DiscoveredConversation) -> SourceResult<()>) -> SourceResult<()>;
}
```

A connector's whole job is to turn some export format into the canonical model: conversations made of messages, each with an author that can be matched to an identity. Nothing downstream knows what an mbox is.

`import` streams. It hands one conversation at a time to a sink rather than returning a `Vec`, so a 2 GB mailbox is bounded by the largest single thread in it and not by the file size. The sink returning `Err` aborts — that is how cancellation reaches a connector.

`validate_by_dry_run` builds a report by walking the connector's own `import`, so validation can never disagree with what the import will actually do.

## Shipped connectors

### `mbox` — email

Standard RFC 4155 mailboxes, as exported by Gmail Takeout, Thunderbird and most mail clients.

Threading uses `References` and `In-Reply-To` where the export kept them, and falls back to a normalized subject — because a large share of real mail has neither header intact after passing through a webmail client. The fallback is deliberately narrow: subject matching only joins messages that also share an address, so two unrelated "Re: lunch" threads with different people stay apart.

Two details that matter more than they look:

- **Separator detection.** The format says a body line beginning with `From ` is escaped as `>From `, and plenty of real exports do not do it. So the shape of the line decides: `From <address> <date…>` with an address that looks like one. "From the look of it, Tuesday works." is body text, and treating it as a separator would split one message into two and corrupt every metric downstream.
- **No Message-ID.** A content-derived id is used instead, so re-importing the same file produces the same ids rather than a second copy of everything.

Unparseable dates become `None` rather than a guess. A wrong timestamp is worse than no timestamp, because response latency is computed from them.

### `mimic_json` — everything else

The documented generic format. Anything without a dedicated connector can be converted into it.

```json
{
  "channel": "chat",
  "conversations": [
    {
      "id": "thread-1",
      "subject": "Lunch",
      "channel": "chat",
      "messages": [
        {
          "id": "m1",
          "sentAt": "2026-02-03T09:14:00Z",
          "from": { "name": "Ada", "email": "ada@example.com" },
          "body": "Does Tuesday work?"
        }
      ]
    }
  ]
}
```

| Field                 | Required | Notes                                                                                                           |
| --------------------- | -------- | --------------------------------------------------------------------------------------------------------------- |
| `channel`             | no       | `email` \| `sms` \| `chat` \| `forum` \| `other`. Defaults to `other`; a conversation may override it.          |
| `conversations[].id`  | yes      | Stable within the file. This is what makes re-importing free.                                                   |
| `messages[].id`       | no       | Falls back to `<conversationId>#<index>`, which is stable for the same file.                                    |
| `messages[].sentAt`   | no       | RFC 3339. Without it, response timing cannot be learned.                                                        |
| `messages[].from`     | yes      | Needs at least one of `email`, `phone`, `handle`, `accountId`. A `name` alone cannot be matched to an identity. |
| `messages[].body`     | yes      | Plain text.                                                                                                     |
| `messages[].metadata` | no       | Object, stored as-is.                                                                                           |

A worked example lives at `fixtures/import/sample_export.json` and is the input to both the Rust end-to-end test and the Python tests.

## Normalization

`sources/normalize.rs` turns raw export text into the words the user actually wrote. This matters more than it looks: if a quoted reply chain survives into the database, every metric is measuring the other person's writing as though it were the user's.

Removed: lines beginning with `>`; everything after an attribution line ("On Tue, 3 Feb 2026 at 09:14, Ada wrote:", and its Spanish, French and full-width variants); forwarded-message banners; Outlook's header-block reply format, which carries no quote marker at all; signature blocks after a `--` delimiter or a "Sent from my …" line.

Kept: **sign-offs the user typed.** "Thanks, C" is not a signature — it is one of the most characteristic things about how a person writes, and the difference between it and a signature block is the `--`.

The rules are deliberately conservative. Where a line's role is ambiguous it is kept, because dropping the user's words is worse than keeping a stray one. "On reflection I think we should wait" survives; it opens with "On" but does not end with "wrote:".

A message that is nothing but quoted text becomes empty and is dropped at insert.

## Identity resolution and direction

For each author:

1. If any of their identifiers matches one of the user's declared identifiers → `direction = self`, no participant. The user is not someone they talk to.
2. Otherwise resolve to a participant, creating one if the address is new → `direction = other`.
3. If the author has no usable identifier → `direction = unknown`, no participant. Kept as conversational context, never counted as evidence.

Resolution runs through an in-memory cache keyed on the author's lowest identifier key, because the alternative is a `SELECT` per message and a mailbox has a million of them.

**The importer refuses to run when the user has declared no identifiers at all.** Every message would import as `unknown`, no voice profile could be built from it, and the failure would be silent. Better to stop and say so.

## Batching, dedupe and cancellation

Messages are inserted 500 at a time inside a transaction with `INSERT OR IGNORE`. Duplicates — same `(source_id, external_id)` — are counted and skipped. Empty bodies are counted and dropped.

After a conversation's messages are in: `link_replies` resolves reply pointers by sequence and derives latency, then `refresh_conversation_stats` recomputes the counters from the rows.

Cancellation is checked between conversations. Stopping mid-import leaves the source `ready` rather than `failed`, keeps everything written so far, and re-running finishes the job — the second pass reports the already-imported messages as duplicates and inserts only what is new.

At the end of a successful import every voice profile is marked stale, because they were computed over a corpus that has just changed.

## What an import reports

```
ImportSummary { conversations, inserted, duplicates, empty, fromSelf, unattributed, participantsCreated }
```

The UI renders this as a sentence that names what was skipped: "44 messages from 3 conversations, 22 written by you, 2 already imported, 1 with no identifiable author". "44 messages imported" would hide the one that could not be attributed, and that one is exactly what the user needs to know about.

## Adding a connector

1. Implement `CommunicationSource` in `crates/mimic-core/src/sources/`.
2. Use `validate_by_dry_run` unless the format lets you say something more specific — the `mimic_json` connector adds a warning about authors with no address, which a dry run cannot infer.
3. Derive a deterministic `external_id` for every message. If the format has no id, hash the content; a re-import must not double.
4. Register it in `sources::all()`. The test in that module asserts every connector has a distinct key and a known channel.
5. Add a fixture and a test. Every connector in the tree has tests for threading, re-import stability, and what happens to a malformed file.
