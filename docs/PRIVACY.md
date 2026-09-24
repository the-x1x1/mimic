# Privacy

Mimic reads a person's private correspondence. This document is what that obliges.

## Where the data is

One SQLite file under the per-user application data directory — `%LOCALAPPDATA%\Formicaria\Mimic\data\mimic.db` on Windows. No account, no sync, no server, no telemetry. Deleting that folder deletes everything Mimic knows.

The original files the user imported from are never modified and never moved.

## What leaves the computer

Four things, all of them the user's choice and all of them visible in the interface.

**Drafting.** The provider selected in Settings receives the assembled prompt: the message being replied to, the intent the user typed, a handful of their own past messages, the recipient's display name and relationship, and the measured description of how they write. When that provider is the local one, this does not leave the machine at all, and the top bar says "Local model". When it is a hosted provider, the top bar says "Sends to Claude" and the Settings description spells out exactly what is transmitted.

**A connected mailbox.** When the user connects one, Mimic logs in to _their_ mail server over TLS, with the app password they gave it or the Microsoft sign-in below, and reads — the inbox and the sent folder, with `EXAMINE` and `BODY.PEEK[]`, so nothing is marked as read, moved or deleted. What travels is the login and the requests for mail; what comes back stays on this computer. It checks on the schedule in Settings (every 15 minutes by default, or only when asked), and the home screen says that it does. Until a mailbox is connected, nothing arrives on its own, and the screens say that instead.

**Signing in with Microsoft.** Outlook.com, Hotmail and Microsoft 365 mailboxes take no password. For those, the user signs in in their own browser, on Microsoft's page, and Mimic never sees the password. What Microsoft hands back is a sign-in that lasts (a refresh token); when Mimic checks the mailbox it sends that, with Mimic's client id, to Microsoft's sign-in service for an access token that lasts about an hour, and then reads the mailbox as above. No mail and nothing about it goes to the sign-in service, and nothing does for a mailbox with an app password.

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

**Deleting a source** removes everything imported through it, and any person Mimic only ever saw through it. For a connected mailbox it also removes the stored app password or Microsoft sign-in, and stops the checking. Microsoft isn't told: to withdraw Mimic's access there too, remove it from the apps with access to your Microsoft account. A thread can hold mail from two sources — an export and the mailbox it came from join by Message-ID — and deleting one of them removes exactly that source's messages: a thread it owned that also holds the other's mail is handed to the other source rather than removed, and the report counts only what actually went.

**Connecting a mailbox** adds its address to the user's own addresses only after the login works, and only when the user ticks that it is theirs — a shared mailbox (team@, support@) is left out, so what colleagues sent from it is not read as the user's writing.

**Deleting everything** empties every communication table and keeps settings, identity and provider configuration. It requires typing a phrase.

None of these are recoverable. `Db::open` writes a database backup before a schema migration, not before a deletion.

## Credentials

Provider API keys, mailbox app passwords and a Microsoft mailbox's sign-in (each under `imap:<source id>`) are stored in `credentials/secrets.json`, each sealed to your Windows account with DPAPI, so a copy of the file cannot be opened without your Windows password. A program running as you, an administrator of the computer, or on a work account your organisation's IT, can still unseal them, and the Privacy card says so. Other builds keep them unsealed in an owner-only file and say that instead; see `docs/MODEL_PROVIDERS.md`. Credentials are never in the database, never in a log, and stripped from diagnostics at every depth. The hour-long access token a sign-in is traded for is kept in memory only and never written.

## What Mimic will not do

It will not send a message. It has no send access to anything and no code path that could acquire one. Every draft is a draft, and the user moves it into whatever app they actually use.

Trusted Mode — Mimic sending low-stakes replies itself — is designed and deliberately unbuilt. The reason is not technical. A product that can send as you needs a much stronger account of what it will not send, how a mistake is caught, and what "low-stakes" means than this product currently has.
