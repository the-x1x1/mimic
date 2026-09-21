# Roadmap

Phases, not dates. A phase is complete when its rows in `docs/PROJECT_STATUS.md` say `IMPLEMENTED` and name the test that proves it.

## Phase 0 — Migration · COMPLETE (0.6.0-alpha.1)

Retire the photography product; keep the infrastructure that was worth five releases of hardening. Audit first (`docs/MIGRATION_AUDIT.md`), archive selectively, delete the rest, then build.

## Phase 1 — Mimic Core · PARTIAL (0.6.0-alpha.1, first run repaired in 0.7.0-alpha.1)

The end-to-end path: import messages, learn how the user writes, draft a reply.

Done: schema v6; the source-connector contract with two real connectors; the streaming import with identity resolution, dedupe, cancellation and resume; the layered voice engine with deterministic metrics and representative examples; metadata-filtered retrieval; the generation context builder and prompt assembler; the model-provider abstraction with local and hosted providers; real cascading deletion; the five-screen UI; the draft/feedback record.

Not done, and the reason each is not just an oversight:

- **Situational classification.** The tables exist; nothing writes to them. This is the first place a language model is genuinely needed rather than convenient, and it needs a defined situation vocabulary before it is worth building.
- **Analysis streams rather than materializing.** A scope's messages are currently loaded into memory before metrics are computed. Fine at a hundred thousand; not at a million.
- **Embeddings are lexical.** `lexical_v1` is honest about what it is and reports `semantic: false`. It is a real improvement over exact match and it is not semantic similarity.

## Phase 2 — Voice intelligence

Make the model of the person better, not the prompt longer.

- A pinned sentence encoder behind the existing manifest mechanism; `message_embeddings` populated in a background job; retrieval's scorer swapped behind the same signature.
- Situation classification — declining, scheduling, apologising, thanking, explaining, disagreeing — with the situational layer resolved into generation.
- Qualitative interpretation: the one genuinely semantic thing a model should do here, turning measured statistics into a description of register that a prompt can use.
- Streaming analysis; incremental recomputation of only the scopes a new import touched.
- Credentials moved to DPAPI on Windows and Keychain/Secret Service elsewhere.

## Phase 3 — The learning loop closes

- The feedback already recorded starts changing profiles, at the threshold the previous product earned: a pattern counts only when at least three observations agree on a direction and account for the majority of the magnitude.
- Manual preferences get a first-class editor on the Voice screen rather than living only in the API.
- The held-out evaluation runs over a real corpus and writes `evaluations` rows, with both baselines implemented: a generic assistant reply, and the user's most common phrasing. Until then no accuracy figure appears anywhere in the UI.

## Phase 4 — Connectors

- IMAP, so mail arrives without an export.
- Platform exports: iMessage, WhatsApp, Signal, Slack, Discord.
- Incremental sync with a watermark, rather than a full re-read.

## Phase 5 — Reply assistant

- A conversation view, so Compose can be opened from a thread rather than by pasting.
- Multiple drafts side by side.
- Per-situation templates derived from the user's own patterns.

## Phase 6 — Assisted automation (ASSISTED mode) · PARTIAL (0.8.0-alpha.1)

Mimic notices what it could answer and prepares drafts in advance. The user still sends every one.

Done: the dashboard of threads awaiting a reply; background drafting for them behind `assist.autoDraft`, off by default, bounded per run and cancellable; approve / modify / reject recording real outcomes.

Not done: watching a _connected_ inbox, which needs Phase 4 — nothing arrives on its own yet, so "in advance" currently means "after an import". Nothing decides that a thread does not need a reply; every unanswered thread is shown.

## Phase 7 — Trusted mode

Designed, deliberately not scheduled. See `docs/PRODUCT.md`. The blocker is not implementation.

## Phase 8 — Advanced personalization

- Drift over time: how the user's voice has changed, and which period to write like.
- Multiple personas for one person, where the channel and relationship layers are not enough.
- Group-conversation dynamics.

## Carried over from Phase 0

Small things the migration left behind, listed so they are not lost:

- The nightly smoke workflow was rewritten but has never been observed running.
- macOS is still unbuilt; nothing in the new code is Windows-specific, but nothing has been tested there either.
- The updater ships with the development public key, which `verify-release.ps1` warns about and allows for alpha builds. A real key is a prerequisite for a beta.
- The icon set is a plain wordmark rather than a designed one. It is product-neutral, so it is a want and not a blocker.
