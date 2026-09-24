# Roadmap

Phases, not dates. A phase is complete when its rows in `docs/PROJECT_STATUS.md` say `IMPLEMENTED` and name the test that proves it.

## Phase 0 — Migration · COMPLETE (0.6.0-alpha.1)

Retire the photography product; keep the infrastructure that was worth five releases of hardening. Audit first (`docs/MIGRATION_AUDIT.md`), archive selectively, delete the rest, then build.

## Phase 1 — Mimic Core · PARTIAL (0.6.0-alpha.1, first run repaired in 0.7.0-alpha.1)

The end-to-end path: import messages, learn how the user writes, draft a reply.

Done: schema v6; the source-connector contract with two real connectors; the streaming import with identity resolution, dedupe, cancellation and resume; the layered voice engine with deterministic metrics and representative examples; metadata-filtered retrieval; the generation context builder and prompt assembler; the model-provider abstraction with local and hosted providers; real cascading deletion; the five-screen UI; the draft/feedback record.

Not done, and the reason each is not just an oversight:

- **Analysis streams rather than materializing.** A scope's messages are currently loaded into memory before metrics are computed. Fine at a hundred thousand; not at a million.
- **Embeddings are lexical.** `lexical_v1` is honest about what it is and reports `semantic: false`. It is a real improvement over exact match and it is not semantic similarity.

## Phase 2 — Voice intelligence

Make the model of the person better, not the prompt longer.

- A pinned sentence encoder behind the existing manifest mechanism; `message_embeddings` populated in a background job; retrieval's scorer swapped behind the same signature.
- Situation classification by a local model, replacing the rules behind `situations::classify` (the vocabulary, the rule classifier and the situational layer landed in 0.10.0-alpha.1).
- Correcting a message's situation by hand (`classified_by = 'user'` is already respected by the rules).
- Qualitative interpretation: the one genuinely semantic thing a model should do here, turning measured statistics into a description of register that a prompt can use.
- Streaming analysis; incremental recomputation of only the scopes a new import touched.
- Credentials in the Keychain / Secret Service, with macOS. (DPAPI on Windows landed in 0.10.0-alpha.6.)

## Phase 3 — The learning loop closes

- Done in 0.10.0-alpha.2: sent drafts change the next draft at the threshold the previous product earned, and notes the user types are listed and can be taken back. Still open: letting a pattern adjust a measured metric rather than adding an instruction beside it, and editing a structured preference (as opposed to a note) from the screen.
- Done in 0.10.0-alpha.11: the held-out evaluation runs over the user's own mail and writes `evaluations` rows, next to both baselines — a generic reply from the same model, and the reply the user sends most often — with each measure shown on its own and no headline number. Still open: running it with a note (as the user would draft), holding the held-out replies out of the voice measurements as well as the examples, and a semantic encoder behind "overall wording".

## Phase 4 — Connectors

- Done in 0.10.0-alpha.3: IMAP, read-only, inbox and sent, with a per-folder watermark and a schedule. Done in 0.10.0-alpha.14: signing in with Microsoft (OAuth 2.0 with PKCE, `AUTHENTICATE XOAUTH2`), which Outlook.com and Hotmail require, and every Microsoft 365 mailbox has since 2022–23. Still open: signing in with Google, and a first real connection to each major provider.
- Done in 0.10.0-alpha.12: mail in the sent folder, or labelled `Sent` in a Gmail export, from an address the user didn't give is asked about by name. Done in 0.10.0-alpha.15: a file named as the Sent folder (a Thunderbird or Apple Mail export is one mbox per folder, and its name is the only clue) is read as one, and importing a file again gives mail read by an earlier version the readings it lacked. Still open: a connected mailbox's mail from before the reading, and exports that say nothing about the folder.
- Platform exports: iMessage, WhatsApp, Signal, Slack, Discord.
- Incremental sync for file sources (IMAP has it).

## Phase 5 — Reply assistant

- Done in 0.10.0-alpha.10: the rest of each waiting thread's conversation, before and after the message on its card, read a page at a time when asked. (A reply was already written on the card for its own thread.) Still open: a conversation not on the list — one already answered, or left out — can't be opened yet.
- Multiple drafts side by side.
- Per-situation templates derived from the user's own patterns.

## Phase 6 — Assisted automation (ASSISTED mode) · PARTIAL (0.8.0-alpha.1)

Mimic notices what it could answer and prepares drafts in advance. The user still sends every one.

Done: the dashboard of threads awaiting a reply; background drafting for them behind `assist.autoDraft`, off by default, bounded per run and cancellable; approve / modify / reject recording real outcomes.

Done in 0.10.0-alpha.3: with a mailbox connected, each check is followed by assisted drafting when that is on, so replies are prepared as mail arrives.

Done in 0.10.0-alpha.4: mail that looks automated from its headers (`sources::automated`) is left out of what is waiting, counted, and shown on request; any thread can be marked as needing no reply until its next message, or kept on the list against the headers; assisted drafting uses the same definition (`db::repo_waiting`). Not done: nothing decides that a person's thread needs no reply — that stays the user's call; mail imported before 0.10.0-alpha.4 has no reading of its headers until it is read again — from 0.10.0-alpha.15 importing a file again gives it one; a connected mailbox's older mail keeps none.

Done in 0.10.0-alpha.5: senders that have only ever sent automated mail are left out of People and of every person picker, counted, and shown on request.

Done in 0.10.0-alpha.7: threads whose last message is older than a window the user chooses (30 days unless changed) are left out of what is waiting as gone quiet, counted and shown on request, for the list and for assisted drafting alike.

## Phase 7 — Trusted mode

Designed, deliberately not scheduled. See `docs/PRODUCT.md`. The blocker is not implementation.

## Phase 8 — Advanced personalization

- Drift over time: how the user's voice has changed, and which period to write like.
- Multiple personas for one person, where the channel and relationship layers are not enough.
- Group-conversation dynamics.

## Carried over from Phase 0

Small things the migration left behind, listed so they are not lost:

- macOS is still unbuilt; nothing in the new code is Windows-specific, but nothing has been tested there either.
- The updater ships with the development public key, which `verify-release.ps1` warns about and allows for alpha builds. A real key is a prerequisite for a beta.
- The icon set is a plain wordmark rather than a designed one. It is product-neutral, so it is a want and not a blocker.
