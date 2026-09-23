# Architecture

One desktop application, three processes' worth of code, one SQLite file.

```
┌─ apps/desktop ──────────────────────────────────────────────────┐
│  React 19 + Vite + TanStack Query                               │
│  Compose · People · Voice · Sources · Settings                  │
│         ↕ typed IPC, every payload zod-validated (lib/ipc.ts)   │
│  Tauri 2 shell (Rust): commands, secrets, updater, logging      │
└──────────────────────────┬──────────────────────────────────────┘
                           │
┌─ crates/mimic-core ──────┴──────────────────────────────────────┐
│  db          SQLite, forward-only migrations, backup on upgrade │
│  sources     CommunicationSource connectors (mbox, mimic_json)  │
│  import      streaming import, identity resolution, dedupe      │
│  voice       deterministic metrics, layered profiles, examples  │
│  retrieval   metadata-filtered, ranked candidate exchanges      │
│  generation  context builder → prompt assembler → draft         │
│  providers   ModelProvider trait: local HTTP, Anthropic, mock   │
│  privacy     cascading deletion and rebuild                     │
│  jobs        persistent queue, progress, cancellation, recovery │
│  engine      NDJSON sidecar client                              │
└──────────────────────────┬──────────────────────────────────────┘
                           │ NDJSON over stdio
┌─ engine (Python) ────────┴──────────────────────────────────────┐
│  text embeddings · similarity · held-out evaluation             │
└─────────────────────────────────────────────────────────────────┘
```

## Why this shape

**Rust holds the domain.** Import, voice metrics, retrieval filtering, prompt assembly and deletion are all in `mimic-core`, which has no Tauri dependency and is therefore testable headlessly in CI. The shell is thin: it resolves paths, owns the credential store, maps errors to `{code, message}`, and forwards events.

**Python holds only what numpy is better at.** In this release that is text embeddings, similarity ranking and the evaluation harness. It is a sidecar rather than a library because an ML dependency tree should not be able to take the application down with it, and because the restart-and-recover machinery already exists and is tested.

**One SQLite file.** No server, no sync, no account. WAL, foreign keys on, forward-only numbered migrations embedded in the binary with a timestamped backup written before any migration touches an existing install.

## Data flow

**Import.** A connector streams conversations; the importer resolves each author against the user's declared identifiers (self), then against known participants (other), then gives up honestly (unknown); messages are written in batches of 500 inside a transaction, replies are linked and latency derived per conversation, and every profile is marked stale at the end. Cancellation is checked between conversations, so stopping leaves a smaller consistent database rather than a broken one. Re-importing the same file inserts nothing: `(source_id, external_id)` is unique.

**Analysis.** For each scope — global, then each channel, then each person the user has written to at least twenty times — the user's own messages are paged in, `voice::metrics::compute` runs over them, and a profile row plus a set of representative examples is written. A scope below the threshold gets a row with its sample size and no metrics.

**Generation.** `build_context` gathers evidence: the resolved voice layers, retrieved past exchanges, the tail of the conversation, the person. `assemble` turns that into a prompt — a pure function, which is what makes `prompt_hash` worth recording and the prompt worth testing. The provider runs. The draft is stored with its context and its evidence.

**Feedback.** The user says what they actually sent. `feedback::diff_draft` describes the difference; an explicit correction the user types outweighs an inferred edit three to one. Nothing changes a profile on the strength of one diff.

## The rules that shape the code

**Nothing is evidence unless its direction is `self`.** Every metric, every representative example, every retrieved exchange is drawn from `direction = 'self'`. Messages from other people are kept — a conversation needs both halves — and never counted as how the user writes.

**A number that was not measured is `None`, not zero.** This runs from `VoiceMetrics` through the zod contracts to the components that render them. `emojiRate: 0.0` means "this person does not use emoji". `null` means "we have not seen enough".

**Deletion is explicit and reported.** `privacy` enumerates what will go before it goes, and the preview is produced by the same code path as the deletion, so it cannot understate it. Foreign keys do the cascade; the rebuild of derived aggregates is an explicit step.

**Credentials never touch the database.** Providers receive them through `SecretStore`, implemented in the shell. Diagnostics strip them. Logs never carry message content.

**Every shared shape has a checked-in fixture.** `crates/mimic-core/tests/pipeline_e2e.rs` writes the read models to `fixtures/contracts/`; `packages/contracts/test/contracts.test.ts` parses them with zod. A change on one side that is not made on the other fails a test.

## Scale

The schema and the queries are written for 100k–1M+ messages: indexes on `(conversation_id, sequence_index)`, `(participant_id, sent_at)`, `(direction, channel, sent_at)`; keyset pagination rather than `OFFSET`; batched inserts; participant resolution through an in-memory cache during import; embeddings out of the row. Analysis and import are background jobs with progress and cancellation. The one known weak point is that analysis currently materializes a scope's messages in memory before computing over them — fine at a hundred thousand, not at a million. See `docs/ROADMAP.md`.

## Process model

The shell starts, takes the data folder's lock (`instance::InstanceLock` on `data/mimic.lock`: one Mimic per data folder, held by the operating system for the life of the process), opens and migrates the database, loads credentials, builds the provider registry, resolves and spawns the engine, recovers interrupted jobs, and only then makes the window. Engine failure does not block launch; its status is shown. A second launch never gets that far: `tauri-plugin-single-instance` hands it to the running Mimic, whose window comes to the front, and one that slips past finds the lock held, touches nothing, shows no window, says so and exits. A launch handed to a Mimic that is closing is remembered, and the closing Mimic starts a new one on its way out, which waits up to ten seconds on the lock while the old process exits. The job runner drains one job at a time, heart-beating every three seconds; a job that dies with the process becomes `interrupted` on the next start and is re-queued if its kind is resumable. Import is resumable (re-running skips what is already there); analysis is not (a half-recomputed profile set would mix two corpora).

## Decisions recorded elsewhere

`docs/adr/` holds the decisions that predate this product and still hold: Tauri as the shell, local-first as the data model, the signed-updater arrangement. The photography-era decisions are in `archive/legacy-photography/docs/`.
