# Roadmap

Phases, not dates. A phase is complete when its rows in `docs/PROJECT_STATUS.md` say `IMPLEMENTED` and name the test that proves it.

## Phase 0 — Migration · COMPLETE (0.6.0-alpha.1)

Retire the photography product; keep the infrastructure that was worth five releases of hardening. Audit first (`docs/MIGRATION_AUDIT.md`), archive selectively, delete the rest, then build.

## Phase 1 — Mimic Core · COMPLETE (0.6.0-alpha.1 to 0.10.0-alpha.17)

The end-to-end path: import messages, learn how the user writes, draft a reply.

- **Import:** the source-connector contract, with mbox, `mimic_json` and (from 0.10.0-alpha.3) a read-only IMAP mailbox; the streaming import with identity resolution, dedupe across copies and sources, cancellation and resume; re-importing adds only what is new (0.10.0-alpha.13).
- **The layered voice engine:** deterministic metrics per layer (everything, channel, person, situation) with representative examples; manual overrides above them. From 0.10.0-alpha.17 a scope is read a page at a time and counted as it goes (`voice::metrics::Accumulator`), twice — once for its numbers, once for its examples — so no scope is held in memory whole, and after new mail only the layers it touched are measured again, by themselves (`voice::Mode::Changed`).
- **Retrieval** filtered by metadata first and ranked second, by wording and (0.10.0-alpha.17) by meaning.
- **Generation:** the context builder, the prompt assembler and the model-provider abstraction with local and hosted providers.
- **Around it:** real cascading deletion; the draft and feedback record; one screen with a drawer for everything else (0.9.0), with every section reachable from the bar (0.10.0-alpha.16); schema 12.

## Phase 2 — Voice intelligence · COMPLETE (0.10.0-alpha.1 to 0.10.0-alpha.17)

Make the model of the person better, not the prompt longer.

- **Situations** (0.10.0-alpha.1): the six-situation vocabulary, the rule classifier and the situational layer. From 0.10.0-alpha.17 the rules refile only what changed, a page at a time.
- **Situations by a local model** (0.10.0-alpha.17): `situations::read_with_model` files the user's messages as a model on this computer reads them, replacing the rules' reading message by message. A hosted provider is refused: it would mean sending everything the user wrote.
- **Correcting a message's situation by hand** (0.10.0-alpha.17): what the user says stands over the rules and any model until they hand it back (`situation_readings`, migration 0012).
- **A pinned sentence encoder** (0.10.0-alpha.17): all-MiniLM-L6-v2 behind the manifest mechanism (`models/manifests/`), downloaded on request and verified by SHA-256; `message_embeddings` filled in a background job; retrieval blends closeness in meaning with wording behind the same filter.
- **Qualitative interpretation** (0.10.0-alpha.17): a model reads a layer's numbers — only the numbers, and the greetings and sign-offs from Mimic's own lists — and says in words how the user writes, kept beside them as a reading and given to drafts only while the numbers are the ones it read.
- **Credentials sealed on the platform Mimic builds for:** DPAPI on Windows (0.10.0-alpha.6). The macOS store (the login Keychain, 0.10.0-alpha.17) goes with the macOS build, under _Carried over_: compiled, never run.

Not done, and why:

- **A note to a draft is read by the rules alone.** A model call there would slow every draft down for a short text the note cues already read well.
- **The situation vocabulary stays six.** A longer tail would leave every situation below the twenty messages a layer needs.

## Phase 3 — The learning loop closes

- Done in 0.10.0-alpha.2: sent drafts change the next draft at the threshold the previous product earned, and notes the user types are listed and can be taken back. Still open: letting a pattern adjust a measured metric rather than adding an instruction beside it, and editing a structured preference (as opposed to a note) from the screen.
- Done in 0.10.0-alpha.11: the held-out evaluation runs over the user's own mail and writes `evaluations` rows, next to both baselines — a generic reply from the same model, and the reply the user sends most often — with each measure shown on its own and no headline number; the held-out replies are kept out of the voice measurements as well as the examples. Done in 0.10.0-alpha.17: with the sentence encoder downloaded, "overall wording" compares meaning, and the drafts being measured find their examples by meaning too. Still open: running it with a note (as the user would draft).

## Phase 4 — Connectors

- Done in 0.10.0-alpha.3: IMAP, read-only, inbox and sent, with a per-folder watermark and a schedule. Done in 0.10.0-alpha.14: signing in with Microsoft (OAuth 2.0 with PKCE, `AUTHENTICATE XOAUTH2`), which Outlook.com and Hotmail require, and every Microsoft 365 mailbox has since 2022–23. Done in 0.10.0-alpha.26: signing in with Google, for Gmail and Google Workspace. Done in 0.10.0-alpha.34: an address at a domain of its own is matched to Google Workspace, Microsoft 365 and a few others by its mail exchangers (Windows only). Done in 0.10.0-alpha.59: the user's own registration with Microsoft or Google, pasted into Settings, for a build made without one. Still open: a first real connection to each major provider.
- Done in 0.10.0-alpha.12: mail in the sent folder, or labelled `Sent` in a Gmail export, from an address the user didn't give is asked about by name. Done in 0.10.0-alpha.15: a file named as the Sent folder (a Thunderbird or Apple Mail export is one mbox per folder, and its name is the only clue) is read as one, and importing a file again gives mail read by an earlier version the readings it lacked. Still open: a connected mailbox's mail from before the reading, and exports that say nothing about the folder.
- Platform exports. Done in 0.10.0-alpha.19: WhatsApp — a chat from Export chat, as a text file or a zip, with the user saying which name in it is theirs (`sources::whatsapp`). Done in 0.10.0-alpha.20: Discord — the data package, which holds only what the user wrote, so it teaches how they write and not how they reply (`sources::discord`). Done in 0.10.0-alpha.60: Telegram — Telegram Desktop's JSON export of a chat or of the whole account, both sides, personal chats and private groups (`sources::telegram`). Done in 0.10.0-alpha.61: Messenger and Instagram — Meta's "Download your information" in JSON, zipped or not (`sources::meta`); a conversation read again through a second Telegram or Meta source joins the first, each message kept once. Done in 0.10.0-alpha.62: Android text messages, from SMS Backup & Restore's XML (`sources::sms`). Still open: iMessage, Signal, and a real export of each.
- Incremental sync for file sources (IMAP has it).

## Phase 5 — Reply assistant

- Done in 0.10.0-alpha.10: the rest of each waiting thread's conversation, before and after the message on its card, read a page at a time when asked. (A reply was already written on the card for its own thread.) Done in 0.10.0-alpha.16: every conversation someone is in can be opened from People, with where it stands, and one left off the list put back on it. Done in 0.10.0-alpha.18: finding what was said — every message, the user's and everyone else's, found by its words (`Db::search_messages` over the `message_search` index, migration 0013), newest first, each shown in its conversation (Find, in the bar). Still open: finding by meaning (only the messages drafts compare are read for meaning, and a search would need them all), words inside Chinese, Japanese or Thai text (the index doesn't split them), and searching one person's conversations from People.
- Done in 0.10.0-alpha.21: multiple drafts side by side — other ways of saying a draft (shorter, longer, more casual, more professional), each asked for and written on its own, shown beside it and chosen between (`generation::compose_another`, `drafts.alternative_to`, migration 0014). A draft written with an adjustment is not evidence for the learning loop: what was changed in it was changed from what the user asked for, not from how Mimic writes. Still open: learning from which way was chosen, and other ways in Compose, which still replaces its draft.
- Per-situation templates derived from the user's own patterns.

## Phase 6 — Assisted automation (ASSISTED mode) · PARTIAL (0.8.0-alpha.1)

Mimic notices what it could answer and prepares drafts in advance. The user still sends every one.

Done: the dashboard of threads awaiting a reply; background drafting for them behind `assist.autoDraft`, off by default, bounded per run and cancellable; approve / modify / reject recording real outcomes.

Done in 0.10.0-alpha.3: with a mailbox connected, each check is followed by assisted drafting when that is on, so replies are prepared as mail arrives.

Done in 0.10.0-alpha.4: mail that looks automated from its headers (`sources::automated`) is left out of what is waiting, counted, and shown on request; any thread can be marked as needing no reply until its next message, or kept on the list against the headers; assisted drafting uses the same definition (`db::repo_waiting`). Not done: nothing decides that a person's thread needs no reply — that stays the user's call; mail imported before 0.10.0-alpha.4 has no reading of its headers until it is read again — from 0.10.0-alpha.15 importing a file again gives it one; a connected mailbox's older mail keeps none.

Done in 0.10.0-alpha.5: senders that have only ever sent automated mail are left out of People and of every person picker, counted, and shown on request.

Done in 0.10.0-alpha.7: threads whose last message is older than a window the user chooses (30 days unless changed) are left out of what is waiting as gone quiet, counted and shown on request, for the list and for assisted drafting alike.

## Phase 7 — Sending, and Trusted mode

Scheduled by the owner on 2026-10-25. See `docs/PRODUCT.md` for what each will and will not do.

Done in 0.10.0-alpha.23: replies the user approves are sent from their own mailbox (`sources::smtp`, `sources::outgoing`, `crate::sending`), recorded before and after in `outbox`, never sent twice and never again after a send that may have gone; how each mailbox sends is its provider's usual server or one set under Your mail, checked by signing in first. Not done: never run against a real mail server. Done in 0.10.0-alpha.25: a copy in a Sent folder the provider doesn't fill. Done in 0.10.0-alpha.33: copying in the others on the message being answered, each ticked by the user (replies sent by themselves still go only to who wrote).

Done in 0.10.0-alpha.24: Trusted mode (`crate::autoreply`) — replies sent by themselves under the rules in `docs/PRODUCT.md`: off by default, per mailbox and per person, held before they go and checked again when they go, a daily limit and two a conversation a day, nothing automated, quiet, asked for or already answered, `Auto-Submitted`, one switch to stop it all, and none of it read as the user's writing. Not done: never run against a real mail server.

Done in 0.10.0-alpha.25: a copy of each reply in the Sent folder where the provider keeps none; mail checked every 2 minutes at most, and at once when a held reply is waiting on a check.

Done in 0.10.0-alpha.26: signing in with Google. Done in 0.10.0-alpha.27: mail read as it arrives, where the server can say so (IMAP `IDLE`). Done in 0.10.0-alpha.28: running in the notification area, starting with Windows when asked, and Windows notifications. Done in 0.10.0-alpha.29: answering a message from anywhere — copy it, press a shortcut, and Write opens on it. Done in 0.10.0-alpha.35: copying the reply back and stepping aside, and asking afterwards whether it went. Done in 0.10.0-alpha.46 and .58: pasting it back into the window it came from, and — turned on for chat apps — pressing Enter there, only while that window shows the same conversation. Done in 0.10.0-alpha.56: one reply even from two computers (the Sent folder read for a reply already there just before one goes by itself). Done in 0.10.0-alpha.57: why a reply won't go by itself, said under it.

Done in 0.10.0-alpha.64: Slack — a workspace's direct and group messages, read with a user token from the user's own Slack app (`sources::slack`), on the mail schedule. Done in 0.10.0-alpha.65: a reply the user approves, sent into the Slack conversation it answers. Done in 0.10.0-alpha.66: replying in Slack by itself, to direct messages, under the same rules. Not done: never run against a real workspace; threads inside a direct message (only what reaches the conversation itself is read).

Next: other places messages arrive (Teams).

## Phase 8 — Advanced personalization

- Drift over time: how the user's voice has changed, and which period to write like.
- Multiple personas for one person, where the channel and relationship layers are not enough.
- Group-conversation dynamics.

## Carried over from Phase 0

Small things the migration left behind, listed so they are not lost:

- macOS is still unbuilt. Nothing in the new code is Windows-specific except the credential sealing, which has a macOS store from 0.10.0-alpha.17 — sealed with a key kept in the login Keychain (`crates/keychain`). It is compiled for `aarch64-apple-darwin` and its sealing is tested elsewhere; the Keychain calls themselves have never been made. Nothing has been tested on a Mac.
- The updater ships with the development public key, which `verify-release.ps1` warns about and allows for alpha builds. A real key is a prerequisite for a beta.
- The icon set is a plain wordmark rather than a designed one. It is product-neutral, so it is a want and not a blocker.
