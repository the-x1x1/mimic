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

- **MIME.** Almost all real mail is MIME, and until 0.10.0-alpha.3 the connector stored it raw — boundary lines, base64 and all — as if it were the person's writing. `sources::mime` now walks multipart structure and takes the first `text/plain` part that is not an attachment, or failing that the first `text/html` part converted to text; decodes base64 and quoted-printable; decodes UTF-8, US-ASCII, ISO-8859-1 and Windows-1252 (anything else as UTF-8 with replacement characters rather than refused); and decodes RFC 2047 encoded words in `Subject` and `From`. A message with no text part — a picture, a calendar invite — is skipped rather than stored empty. Any mbox imported with an earlier version should be removed and imported again.
- **The Sent folder.** Gmail's export names each message's labels in `X-Gmail-Labels` (decoded, in case a label name needed encoding); a message labelled `Sent` (the whole label, in any case) gets `metadata.sentFolder`, as does everything the connected mailbox reads from its sent folder, and everything in a file whose name says it is the Sent folder (`sources::is_sent_folder_name`: `Sent`, Thunderbird's own file with no extension; `Sent.mbox`; Apple Mail's `Sent Messages.mbox/mbox`, named by its folder; `Sent Items` and Outlook's names for it in the common languages, such as `Gesendete Elemente` or `Éléments envoyés`) — except a message the headers say was passed on (`Resent-From`, `Resent-Sender`), whose `From` is its first writer. What is there is usually the user's, so someone else it is filed under is asked about by name, with how much of their mail was there (`Db::sent_folder_people`, `docs/DATA_MODEL.md`, "Identity"); a Sent folder also holds mail sent for someone, and mail from addresses several people share, so it is never assumed. Two copies of one message read together — the user's post to a list, read back from the inbox with the list as its sender — are threaded with the Sent copy first, and the importer keeps the first copy of a message and attributes nothing for the second, so what the user wrote stays theirs. Like the automated reading, all this is decided at import. Mail read before a reading existed gets it when it is read again ("Readings", below).
- **Automated mail.** While the headers are still at hand, `sources::automated` reads whether a machine sent the message and records why in `metadata.automated`: a list message with an unsubscribe link or list id and nowhere to post (`newsletter`), `Precedence: junk` (`bulk`), `Auto-Submitted: auto-generated`, `auto-replied` or `auto-notified`, `X-Autoreply`, a null `Return-Path` or `Precedence: auto_reply` (`auto_reply`), `multipart/report` (`report`), or a sender whose local part says no-reply outright, at either end (`noreply-apps@`, `comments-noreply@`), with no answerable `Reply-To` — one that is not itself no-reply and not on the sender's own domain, where a reply relay would be (`no_reply_address`). Headers only, never the words, and precision over recall: a discussion list that gives `List-Post`, `Precedence: list` or `bulk` on their own (help desks put `bulk` on their staff's replies), `Auto-Submitted: auto-forwarded`, a contact form's no-reply sender with the visitor in `Reply-To`, and addresses like `info@` or `support@` are all left alone. Nothing is dropped — the message is imported like any other, and the home screen leaves the thread out of what is waiting (`docs/DATA_MODEL.md`, "What needs a reply"). Headers are not kept after parsing, so mail imported before 0.10.0-alpha.4 has no reading until it is read again ("Readings", below). The same code runs for the connected mailbox, which parses with the mbox reader. The `mimic_json` connector removes `automated` and `sentFolder` keys from a file's metadata, since a file has no headers or folder to have read them from.

### Readings

`metadata.sentFolder` and `metadata.automated` are read from headers that are not kept, so a message stored by a version that could not make a reading has none. Reading it again gives it one: when a source is imported again, each message it already stores is offered what this read found — from the first copy of it in the read, the one a first import of the same files keeps, and once per import (`ImportState::reading`, `ImportState.read_again`, `Db::add_readings`) — and a key is added only where the stored metadata has none — never changed, never removed. It is added only to a stored copy with the same author as the copy read now: the user's own message, or a person holding one of its identifiers. So the Sent copy of the user's post to a list never marks the list's copy, stored earlier with the list as its sender, and nobody is attributed or made up for it. The summary counts them (`sentFolderMarked`, `automatedMarked`). This is what **Import again** does for a file. One thing it can do that a first import at this version would not: a thread the user marked as needing no reply is decided by its last message that doesn't look automated, and a message newly read as automated can move that to another message, so the mark (tied to the message it was made on) lapses and the thread may be listed again, as when someone writes again. A connected mailbox is not read again below its watermark, so its mail from before a reading keeps none; removing it and connecting it again reads its newest 2,000 messages per folder afresh, at the cost of everything derived from it.

### `imap` — a connected mailbox

Not a file connector: it has no `location`, and does not implement `CommunicationSource`. `sources::imap::sync` logs in, re-reads the folder list (so a renamed or localized sent folder is found again), and reads the inbox and the sent folder (the RFC 6154 `\Sent` attribute, then the names the common servers use, then the last part of a name `sources::is_sent_folder_name` knows in another language — unaccented only, since IMAP's encoding of accented names isn't decoded).

- **Signing in.** `ImapAccount.auth` says how: `password` sends `LOGIN` with an app password kept under `imap:<source id>`; `microsoft` sends `AUTHENTICATE XOAUTH2` with an access token (`sources::oauth`). For those, `imap::Credentials` keeps the refresh token under the same key, trades it for an access token when no kept one has five minutes left, saves the refresh token Microsoft hands back in its place, and forgets the access token when the server turns it down, so the next check asks again. A check that can't get a credential records the failure and says what to do (give a new password, or sign in again under Your mail), as a failing login does.
- **Fetch everything, then import.** Every folder's new mail is fetched and parsed first, then threaded together and handed to `import::Importer`, which runs exactly the attribution, dedupe and threading a file import does. So the order folders are read in cannot split a thread, and a check that is canceled or drops its connection imports nothing and loses nothing.
- **Position.** Per folder, the server's `UIDVALIDITY` and the highest UID read, moved only after the import finishes. A changed `UIDVALIDITY` re-reads the folder; `Message-ID` dedupe means nothing is doubled. An empty folder is not searched (some servers answer that search with an error).
- **Size.** Sizes are asked for first (`RFC822.SIZE`); a message over 25 MiB is skipped unfetched and the position moves past it, so one enormous attachment cannot stall every check. Fetches are batched by count (50) and by bytes (20 MiB), and one command's reply is capped at 64 MiB.
- **First check.** At most the newest 2,000 messages per folder. Older mail comes in from an export; a message in both is stored once.
- **Schedule.** `mail.checkEveryMinutes`, default 15, minimum 5, 0 for only-when-asked. Every attempt is recorded, successful or not; a failing mailbox waits four intervals (at most six hours) before the next attempt; "being checked" is decided by the job queue, so a status left behind by a crash cannot stop checking. A finished check is followed by assisted drafting when that is on, as an import is.

## Email threads across checks and sources

Any conversation on the email channel from a source whose ids are real Message-IDs (`mbox` and `imap`; `import::MESSAGE_ID_CONNECTORS`) is joined wherever it already is, by `Message-ID` — never by a generic export's own ids ("a1") or by the content hash made up for mail with no Message-ID (`sha-…`): `ImportState` looks for a conversation, in any source, holding one of the new messages or one of the messages they reference (`metadata.refs`, from `In-Reply-To` and `References`), and adds to it, putting it back in time order (`Db::resequence_by_time`). A message whose `Message-ID` is already stored through another source is counted as a duplicate rather than stored twice. This is what keeps an mbox export and the connected mailbox it came from from double-counting the user's writing or splitting a thread, and what lets a reply that arrives in a later check join the thread it answers. Migration 0008 indexes `messages.external_id` for the lookup.

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

**An address declared after the import** can be applied to the messages already read, not only to the next import: whoever was filed as a person under it becomes the user again, with their messages — once the user has been shown who that is and said yes, or without asking when every address they hold is already the user's and the user recorded nothing about them (`Db::add_user_address`, `Db::claim_user_mail`, which the desktop runs whenever a job finishes; see `DATA_MODEL.md`). Reading the source again would not do it — `insert_messages` keeps the first copy of every message — so nothing asks the user to. An address the user types is refused while mail is being read, because a running import matches authors against the addresses it read when it started; one added by connecting a mailbox mid-read is caught by that fold once the work in progress has finished, or listed in Settings when its holder is not only the user.

**The importer refuses to run when the user has declared no identifiers at all.** Every message would import as `unknown`, no voice profile could be built from it, and the failure would be silent. Better to stop and say so.

## Batching, dedupe and cancellation

Messages are inserted 500 at a time inside a transaction with `INSERT OR IGNORE`. Duplicates — same `(source_id, external_id)` — are counted and skipped, and they are recognised before anyone is attributed for them (`Db::ids_in_source`, and a second copy within one conversation), so a message read again with its sender written differently — a new name, another address, a list's copy — adds nobody to the conversation. Empty bodies are counted and dropped.

A conversation already here that is not joined by Message-ID — a later export of the same chat — gets its new messages after the ones it has (`Db::last_position`), in the order the export gives them; when every message in it has a time `julianday` can read, time then decides (`Db::every_message_timed`, `Db::resequence_by_time`, which orders by `julianday`, not by the text, so offsets compare as times). Before 0.10.0-alpha.13 they were numbered from zero, sat among the old ones, and the wrong message could decide the thread. A conversation with a message whose time is missing or not a time a clock reads ("03/02/2026 09:14", as a chat export may write it) keeps the order given, since sorting would put that message first or compare text. Two limits: in such a conversation, a message a full re-export adds in the middle goes at the end; and a `mimic_json` message without an `id` is known by its position (`t1#3`), so a later export that adds one earlier in the conversation shifts those ids.

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
