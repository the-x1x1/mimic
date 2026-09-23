# CLAUDE.md — agent entry point for Mimic

You are working on Mimic, a local-first Windows desktop app that learns how a person communicates and helps them draft responses that sound like themselves. Read this file first, then `docs/PROJECT_STATUS.md`, then the code you are about to change.

Mimic was a Lightroom Classic editing assistant through 0.5.0. It is not any more. `docs/MIGRATION_AUDIT.md` records what happened; `archive/legacy-photography/` holds what was worth keeping. Nothing in the archive is built, tested or shipped, and nothing in the live tree should refer to it.

## Source of truth (highest first)

1. Source code
2. Automated tests (`cargo test`, `pytest`, `vitest`)
3. Reproducible runtime evidence (logs, diagnostic bundles, fixtures)
4. GitHub release artifacts
5. `docs/PROJECT_STATUS.md`
6. `docs/ROADMAP.md` and the rest of `docs/`

Documentation follows reality. Never let a doc lead the code.

## Non-negotiable rules

- **Never** treat a message as evidence of how the user writes unless its `direction` is `self`. Received messages are context; they are never a voice metric, a representative example or a retrieval result.
- **Never** show a number that was not measured. A rate that has not been computed is `None` / `null` / "not measured yet". `0` means measured zero. The distinction must survive from the Rust struct through the zod schema to the rendered sentence.
- **Never** show an accuracy or quality score without a row in `evaluations` behind it and a definition in `docs/VOICE_ENGINE.md`. There is no "Mimic Score".
- **Never** infer a relationship, a tone or a situation and present it as a fact the user stated. `participants.relationship` is set by hand and by nothing else.
- **Never** let message content reach a log, a diagnostics bundle, or a provider error string.
- **Never** put a credential in the database or in settings. Providers read them through `SecretStore`.
- **Never** claim a provider is local. Compute it from the endpoint (`LocalHttpProvider::is_loopback`) and let the answer be what it is.
- **Never** add a send path. Mimic drafts; the user sends. Trusted Mode is designed and deliberately unbuilt.
- **Never** write to a mailbox. The IMAP client reads with `EXAMINE` and `BODY.PEEK[]` and has no command that changes anything; the tests fail if one is sent.
- **Never** delete a person without removing what was derived from them and invalidating what was computed over them. The preview must be produced by the same code path as the deletion.
- **Never** drop the user's own words during normalization when a line's role is ambiguous. Keeping a stray quoted line is a smaller error than deleting a real one.
- **Always** add a new numbered migration for a schema change (`crates/mimic-core/src/db/migrations/`), never edit a shipped one, and extend the migration test.
- **Always** give a shared contract a checked-in fixture parsed by both Rust and zod.
- **Always** update `docs/PROJECT_STATUS.md` in the same commit as the behaviour it describes.
- **Always** test version changes (`node scripts/sync-version.mjs --check`, part of `test.ps1` and CI).
- The UI must hide or disable what does not work and say why. No buttons that pretend, no sample numbers shown as real metrics.

## Repository map

```
apps/desktop/            Tauri 2 shell (src-tauri/, Rust) + React/TS frontend (src/); one screen (features/replies) with a drawer over it
crates/mimic-core/       db, sources (mbox, mime, automated, imap, mimic_json), import, voice, situations, learning, retrieval, generation, providers, privacy, jobs, assist, dashboard, localmodel, engine client
engine/                  Python sidecar (uv): NDJSON protocol, text embeddings, similarity, evaluation
packages/contracts/      zod contracts mirroring every IPC payload
packages/ui/             design tokens + primitives
packages/test-fixtures/  fixture path helpers
fixtures/import/         conversation exports used by the importer's tests
fixtures/contracts/      read models written by the Rust e2e test, parsed by the zod suite
models/manifests/        text-encoder manifests (SHA-256 pinned); no binaries committed
scripts/                 bootstrap/dev/test/build/validate/package-engine/verify-release/sync-version
docs/                    PRODUCT, ARCHITECTURE, DATA_MODEL, VOICE_ENGINE, IMPORT_PIPELINE, MODEL_PROVIDERS,
                         PRIVACY, PROJECT_STATUS, ROADMAP, MIGRATION_AUDIT, RELEASE_PROCESS, SECURITY_MODEL,
                         UPDATE_SYSTEM, adr/
archive/legacy-photography/   the old product's Lightroom plugin, mapping and docs. Not built.
```

## Commands

```powershell
.\scripts\bootstrap.ps1                 # prerequisites + installs
.\scripts\dev.ps1                       # tauri dev (spawns the engine via uv)
.\scripts\test.ps1                      # everything CI runs
.\scripts\validate.ps1 -Full            # test.ps1 + pre-tag release verification
.\scripts\build.ps1                     # engine bundle + installer
node scripts/sync-version.mjs 0.7.0     # bump the single version everywhere (then cargo update -w)
```

Per stack: `pnpm typecheck|lint|test|build`, `cargo fmt/clippy/test --workspace`, `cd engine && uv run ruff check . && uv run pytest`.

Regenerating the contract fixtures after a deliberate shape change: `MIMIC_REGEN_FIXTURES=1 cargo test -p mimic-core --test pipeline_e2e`.

## What is implemented (0.10.0-alpha.4)

Phase 0 (migration) is complete; Phase 1 (Mimic Core) is partial. Working end to end: schema v9 with a clean upgrade from the photography schema; the `CommunicationSource` contract with `mbox` and `mimic_json` connectors; streaming import with identity-based direction, dedupe, cancellation and free resume; the layered voice engine (global, channel, relationship, situational) with deterministic metrics, a 20-message floor and deterministic representative examples; metadata-filtered lexical retrieval; the generation context builder and pure prompt assembler; the model-provider abstraction with a local endpoint and Anthropic; real cascading deletion with an honest preview; the one-screen interface, with People / How you write / Your mail / Settings in a drawer over it; and the learning loop, from what was sent to what the next draft does differently. As of 0.7.0 the first run is the part that has had attention: onboarding subscribes to native job events above the gate, names the state where an import matched none of the user's addresses, and lets someone with fewer than twenty own messages leave anyway; the provider badge and the Compose button follow a real reachability check instead of assuming the local endpoint is up.

0.10.0-alpha.4 is only what needs a reply. `sources::automated` reads at import, from headers alone, whether a machine sent a message (list mail with nowhere to post, `Precedence: junk`, automatic replies, delivery reports, no-reply senders without an answerable `Reply-To`) and records why in `metadata.automated`; `db::repo_waiting` is the one definition of what is waiting, used by the home screen, its counts and assisted drafting alike. A thread is decided by its last message that does not look automated when that is someone else's (so an out-of-office cannot hide a question), and otherwise by its last message (so a notification after the user's reply is left out and counted, not lost); it needs a reply when the deciding message is someone else's and not automated, unless the user said otherwise (`thread_marks`, migration 0009, tied to the message they were looking at, so it lapses when they write again). What was left out is always counted and can be shown, and the user's mark outranks the headers both ways. A draft records the message it answers (`drafts.incoming_message_id`) and is shown under that message only.

0.10.0-alpha.3 is real mail. `sources/mime.rs` decodes MIME (multipart, base64, quoted-printable, the four charsets that cover nearly all mail, encoded-word headers, HTML-only mail) — before it, mbox bodies were stored raw. `sources/imap.rs` connects a mailbox read-only over TLS (plain only to loopback), reads the inbox and sent folder with a per-folder `UIDVALIDITY`/UID watermark, and feeds `import::Importer`; later replies join the thread they reference and the thread is renumbered by time. Checks run on `mail.checkEveryMinutes` and are followed by assisted drafting when that is on. The app password lives in the secret store under `imap:<source id>`.

0.10.0-alpha.2 closes the learning loop: `learning.rs` reads every sent draft (edited or not) and a habit — greeting, sign-off, emoji, closing full stop, length — changes the next draft once three drafts agree and outweigh the ones that did not, globally or per person. Patterns are recomputed on every call, never stored, and reach the prompt as instructions beside the measurements rather than altering them. Notes the user types are manual preferences under a `said:` key, quoted in their words and forgettable.

0.10.0-alpha.1 adds situations: a fixed vocabulary of six (saying no, setting a time, apologising, thanking, explaining, disagreeing) seeded by migration 0007; the user's own messages filed under them by rule at the start of every analysis (`situations.rs` — precision over recall, `classified_by = 'rule'`, never over a user's call); a situational voice layer per situation; and generation that takes a situation the user chose, or reads one from their note and says it is a reading (`SituationChoice.source`). Nothing is inferred from the incoming message.

0.9.0-alpha.3 makes setup survivable by someone who has never installed a model: `localmodel` observes what is on the machine and computes one next action from it, and Mimic pulls the model itself with a real progress bar. It deliberately does not download or run the host's installer — see the note at the top of `localmodel.rs`.

0.9.0-alpha.2 collapses the interface to one screen — who is waiting, what they said, what Mimic would say — with everything else in a drawer over it, and rewrites the copy in Mimic's own first person. No screen says direction, corpus, provider or profile.

0.9.0-alpha.1 is the design system the redesign is built on: anything that is writing is set in IBM Plex Serif and reads like a letter (`.letter`), everything around it is IBM Plex Sans and reads like a tool, and three real themes — plain, paper, night — come from one token set, with paper drawing rules where the others draw boxes (`--card-border`, `--card-padding`). Both faces are bundled, never fetched.

0.8.0-alpha.3 is a layout pass: the shell is exactly the window and only the content pane scrolls (it used to take the sidebar's nav and the top bar off-screen with it), every text control including `<select>` is styled rather than falling back to the system's, and page content sits on one 1160px measure.

0.8.0-alpha.2 makes onboarding leavable: with identity declared, "Look around first" opens the app before anything has been imported, and says plainly that it will be empty. Identity itself stays mandatory, because the importer decides `direction` by it.

0.8.0 adds the home screen: threads whose last message came from someone else and was never answered, the draft Mimic has for each, and approve / modify / reject that records a real outcome. Assisted drafting (`assist.autoDraft`) prepares those drafts in the background after an import — off by default, bounded per run, and it never reaches a provider while off.

Not implemented, deliberately: any send path; model-backed situation classification; embedding-backed retrieval; learning that alters a measured metric; the evaluation loop over a real corpus; any accuracy figure in the UI; IMAP OAuth; TRUSTED mode. See `docs/PROJECT_STATUS.md` for the row-by-row picture and `docs/ROADMAP.md` for where each lands.

## How to update PROJECT_STATUS

Each row has a status (`IMPLEMENTED | PARTIAL | PLANNED | UNSUPPORTED`) and an evidence column naming the test that proves it. When you change behaviour: change the row, point evidence at the real test, and move completed items out of `docs/ROADMAP.md`.

## Forbidden shortcuts

Fake success. Placeholder services. Empty interfaces "for bulk". God classes. Duplicated schemas without a checked-in contract. Unbounded workers. A metric computed in two places. A percentage with no computation behind it. Silent uploads. Treating a received message as the user's writing. Marking anything done without a test.

## Release protocol (owner's rules)

One coherent slice per version with code, tests and docs together; every shared contract gets a checked-in fixture; `test.ps1` green before claiming green; state plainly what could not run; the version bumped everywhere a version lives (`sync-version.mjs --check` enforces it); one clean commit per slice authored `the-x1x1 <connersalt123@outlook.com>`; and no AI attribution anywhere — not in a commit message, not in a PR description, not in release notes.
