# Changelog

All notable changes to Mimic are documented here. The format follows Keep a Changelog; versions follow SemVer with pre-release tags for alpha/beta builds.

## [0.10.0-alpha.87] — 2026-10-26

Chats and drafts, tightened (the fifteenth review).

### Fixed

- **A chat to someone you mostly email reads like a chat too.** How you write to a person counts every channel, so a Teams message to someone you've emailed for years still opened with "Hi" and closed with "Thanks" even once your chats were measured. Now, while too little of what you've written to them was in a chat, their email's form is left out as well, and what is under the draft says so.
- **Nothing measured on email slips into a chat by another door.** While a chat is written from your other writing, the reading of your numbers in words isn't given to it, and neither is a greeting, sign-off or length learned from your edits — for this person too, not only for everyone. What you told Mimic yourself still comes last and still wins.
- **Taking off what a model wraps around a reply takes off less.** "Sure, here's the draft agenda:" and "Here's the revised version:" are kept as the reply's own first lines; only an introduction to a reply of its own ("Here's a reply:", "Here's my reply:", "…in your voice:") comes off. A reply with two blocks of code keeps its fences, a subject line comes off only with a blank line after it, and wrapping inside wrapping comes off too.
- **Thinking is taken out of a reply however it is written.** A reply with letters such as "İ" in it kept its thinking; and a "<think>" in the middle of a reply that never closes is now the reply's own words, not a reason to drop the rest.
- **A model that only thought says so** when it sends no words at all, and an endpoint that turns the no-thinking request down with FastAPI's kind of error is asked again without it.
- **A draft still being written when you copy in the next message** is put aside, not shown under the new message; the new one gets its own draft. A message window of classic Outlook ("… - Message (HTML)") is read as email.

## [0.10.0-alpha.86] — 2026-10-26

The rest of what a model does, run for real.

### Added

- **The live test also has the models on your computer file your messages, put your numbers into words, and be measured.** On the owner's desktop: llama3.2:3b filed 21 of the sample's 22 messages (one answer it gave couldn't be read), put three layers into words, and was measured on six replies; qwen3:14b filed all 22, put three layers into words, and was measured on two. One of llama3.2:3b's readings called a lowercase, contraction-heavy writer "formal and highly polished" — which is why a reading is always shown as a reading, beside the numbers, never instead of them.

## [0.10.0-alpha.85] — 2026-10-26

Drafted by a real model.

### Added

- **A test that drafts with the models on your computer.** `cargo test -p mimic-core --test live_model -- --ignored --nocapture` imports the sample export's made-up people and has each model you name write a chat reply, a text and an email, through everything Mimic does before and after the model. On the owner's desktop it ran with Ollama: llama3.2:3b wrote "yes, tomorrow morning", "yeah from 6" and "i'll take a look" in under three seconds; qwen3:14b, which until 0.10.0-alpha.84 gave empty drafts, wrote "yes, tomorrow morning", "yes from 6" and "sure, i'll take a look this afternoon" in 13 to 17 seconds each.

## [0.10.0-alpha.84] — 2026-10-26

Drafts from any model on your computer.

### Fixed

- **A model that thinks before it answers no longer gives empty drafts.** Through Ollama, qwen3 spent the whole length of a short reply thinking and came back with nothing. Mimic now asks every model on your computer to answer without thinking first — Ollama does this for any model — and asks again without that if the endpoint doesn't know it. Thinking written into a reply is taken out, and a model that only thought says so instead of leaving an empty draft.

### Changed

- **A draft is just the reply.** "Here's a reply in your voice:", a subject line, a code block or quotation marks around the whole reply are taken off what the model wrote. Nothing inside the reply is changed.

## [0.10.0-alpha.83] — 2026-10-26

A chat reads like a chat.

### Changed

- **Replies in a chat or a text read like one, even before Mimic has read many of yours.** Until enough of your own chat messages are in, how you write was measured mostly on email, and a reply into Slack, Teams, Telegram or Google Chat could open with "Hi Sam," and close with "Thanks, C". Now, until your chats can be measured, a chat reply is asked to be a line or two with no greeting line, sign-off or signature, and what is under the draft says so. A greeting or sign-off you taught Mimic on email isn't added to chats.
- **Past replies come from the same kind of conversation first.** Writing to someone you've never written to in a chat, the examples are your chat messages to others before your email; any from another kind are marked as such, for how you talk rather than how long or how laid out.
- **Write knows a chat from an email by the window you copied from.** Opened with the shortcut from WhatsApp, Discord, Messenger, Slack, Teams, Telegram and the like, Write answers a chat; from Phone Link or Messages for web, a text; from Outlook, Gmail or Thunderbird, an email — and says so under Channel. Only the app's own part of the title counts, so a mail whose subject says "WhatsApp" is still mail.

## [0.10.0-alpha.82] — 2026-10-26

Setting up Google Chat.

### Changed

- **Settings says how to set up Google Chat.** Under Signing in with Microsoft and Google, the Google steps now include what Google Chat needs in the same project: the Google Chat API turned on, its app configured, and its permissions added — and that a project inside your Workspace can be Internal, so Google neither warns nor asks you to sign in again every week.

## [0.10.0-alpha.81] — 2026-10-26

Google Chat, tightened.

### Fixed

- **A photo counts as an answer.** If they send a photo or a file after the message I'm holding a reply to, the reply doesn't go by itself — not only when they write words.
- **Nothing becomes a mention.** A reply with `<users/all>` or a link written in Google Chat's own way in it is sent as you wrote it, never as a mention of everyone. The Send panel says that Google Chat shows your app's name beside a reply, and that its own _bold_ and _italic_ still apply.
- **Only the permissions it needs.** Signing in asks to read and to send messages — not to edit or delete them — and if you untick any permission on Google's page, I say to sign in again with every box ticked, instead of failing every check with the wrong advice. An app not yet configured in your Google Cloud project says how to configure it.
- **Gentler with Google.** Asked to slow down without being told for how long, I wait a second, then two, four and eight, before leaving the rest for the next check. A chat with nothing to read (history off) isn't asked again until something new comes.
- **A direct message has a name.** One only you have written in yet is named after the other person, not "Chat".

## [0.10.0-alpha.80] — 2026-10-26

Replying in Google Chat.

### Added

- **Send it, in Google Chat.** A reply to a Google Chat message goes into that conversation, as you, exactly as the Send panel shows it, once you press Send. If Google's answer is lost as it goes, I say it may have gone and don't send it again until you've looked.
- **Google Chat, by itself.** Tick a Google Chat account beside your mailboxes under Replying by myself, and I answer direct messages there the way I do email, Slack, Teams and Telegram: held first, checked again after I've read the account, and — just before it goes — I look at the conversation: if you've answered there, or they've written again, it doesn't go. Never a group chat. **Check it all now** tries your Google Chat sign-in too, sending nothing.

## [0.10.0-alpha.79] — 2026-10-26

Google Chat.

### Added

- **Connect Google Chat.** Under Your mail, sign in with Google in your browser — a Google Workspace account (a work or school address on Google) — and I read your Google Chat direct messages and group chats, both sides, and check for new ones as often as I check your mail. Named spaces and chats with apps aren't read, and reading changes nothing in Google Chat. It goes through the Google registration your mailboxes sign in with; once, in that Google Cloud project, turn on the Google Chat API and give it an app name on its Configuration page. A personal Gmail has no Chat API, and the dialog says so.

## [0.10.0-alpha.78] — 2026-10-26

Telegram, tightened again.

### Fixed

- **A chat with something new is never passed over.** Where I stop looking down your chat list is now judged by Telegram's own clock — the newest message the last full read saw — not this computer's, so a clock that runs fast can't make me miss messages. And when Telegram itself fails to give a chat ("try again later"), I read it again next time instead of passing it over for good.
- **One chat Telegram refuses isn't the account.** A group you were removed from, alone at the top of your list, no longer makes the whole check fail.
- **Large chats join up fast.** Joining a chat read live to an exported one checks each conversation once, not once per message — minutes become a moment for a long chat.
- **Stopping a sign-in, to the last moment.** Cancelling while Telegram finishes letting you in now keeps nothing and signs that session out; a code or password whose answer never came signs out what Telegram may have let in.
- **A Telegram session is kept again only when it changed.** Sending, a test or a reply by itself no longer rewrites it each time, so a check running meanwhile keeps what it learned.

## [0.10.0-alpha.77] — 2026-10-26

Telegram, tightened.

### Fixed

- **A reply that may have gone is never sent twice.** If the connection drops just as Telegram takes a reply, I no longer ask again, and Telegram's "I have that one already" or its own timeout now reads as "it may have gone" — never as a refusal that lets it be sent again.
- **A busy chat is read through.** When more than 2,000 messages came into a chat since I last looked, I read on from where I stopped on the next check, instead of skipping the newest.
- **You answered in another way.** A call, a GIF sent through a bot, a photo — anything you did in the chat after their message — stops a reply I'm holding there.
- **Two Telegram accounts stay apart.** Each account numbers its messages its own way, so your chats with the same person from two accounts (or an export of another account) are no longer merged, and no message is dropped as a copy of a different one.
- **One slow answer doesn't lose a check.** If Telegram slows me down or the connection drops partway, what I'd read is kept and the rest is read next time. I read up to 200 chats with something new per check, and say so when there were more; chats with nothing new don't count.
- **Stopping a sign-in stops it.** Cancel while Telegram is checking your code or password, and the account isn't connected — and if Telegram had already let me in, I sign that session out again.
- **A wrong password can be tried again.** After a wrong password I ask Telegram afresh for what the next try is checked against.
- **A newer sign-in isn't written over.** Each use of a Telegram session keeps what changed only if no newer session was kept meanwhile, and never one Telegram has signed out. A signed-out session stops a held reply at once, saying to sign in again.
- **Telegram's own account isn't a conversation.** Login codes and security notices from Telegram itself are left alone.

## [0.10.0-alpha.76] — 2026-10-26

Replying by myself in Telegram.

### Added

- **Telegram, by itself.** Tick a Telegram account beside your mailboxes under Replying by myself, and I answer one-to-one chats there the way I do email, Slack and Teams: held first, checked again after I've read the account, and — just before it goes — I look at the chat: if you've answered there, or they've written again, it doesn't go. Never a group. **Check it all now** tries your Telegram sign-in too, sending nothing.

## [0.10.0-alpha.75] — 2026-10-26

Replying in Telegram.

### Added

- **Send it, in Telegram.** A reply to a Telegram message goes into that chat, as you, exactly as the Send panel shows it, once you press Send. If Telegram's answer is lost as it goes, I say it may have gone and don't send it again until you've looked.

## [0.10.0-alpha.74] — 2026-10-26

Telegram, as it happens.

### Added

- **Connect Telegram.** Under Your mail, **Connect Telegram** signs in to your Telegram account: you register an app at my.telegram.org once (two minutes; I show you how), then give your phone number, the code Telegram sends you and, if your account has one, its password — which goes to Telegram and nowhere else. I read your one-to-one chats and private groups, not channels, bots or public groups, and check them as often as your mail. A chat you exported before and imported joins up with what I read, each message once. Reading changes nothing in Telegram.

## [0.10.0-alpha.73] — 2026-10-26

Other folders.

### Added

- **Mail your rules file away.** Under each mailbox in Your mail, **Other folders to read** lists the folders I can read besides your inbox and Sent folder — leaving out drafts, deleted and junk mail wherever the server or the folder's name says that's what it holds — and reads the ones you tick (up to ten) on every check, the newest 500 of each the first time. What I find there waits for you: I never reply to it by myself. A folder the server won't open is passed over, and said, without stopping your inbox from being read.

## [0.10.0-alpha.72] — 2026-10-26

Tightened.

### Fixed

- **A reply whose answer was lost is found once.** When Slack or Teams took a reply I sent by myself but its answer never reached me, I recognise it by its words when I read it back — now only once, only among your messages sent after the one it answers, and from then on it is recorded as sent. If you later write the same words yourself, they are yours.
- **A chat service that gives nothing says so.** A Slack or Teams check that could read none of your conversations now fails with the reason, instead of saying there was nothing new. One Teams chat that is always refused, beside chats with nothing new, doesn't count against the account.
- **Copy it for me waits for the copy.** Some apps take a moment to put what you selected on the clipboard; I wait for it rather than reading it too soon. If nothing comes — nothing was selected — Write opens empty rather than on whatever you copied before.
- **Replying for you shows a chat that isn't working.** A Slack workspace or Teams account whose last check failed is shown with what went wrong and a link to Your mail, not as connected.

## [0.10.0-alpha.71] — 2026-10-26

Copy it for me.

### Added

- **Select, then press the shortcut.** Turn on **Copy what I've selected when I press the shortcut** under Answering a message from anywhere, and you no longer have to copy the message first: once you let go of the shortcut, I press Ctrl+Insert in that window to copy it — copy in every app, and never Ctrl+C, which would stop what a terminal is running. Off unless you turn it on.

## [0.10.0-alpha.70] — 2026-10-26

Many chats, checked politely.

### Changed

- When Slack or Teams asks me to slow down for a moment, I wait as asked and carry on, so a workspace with hundreds of conversations is read through in one check instead of a bit at a time. In Teams I only open the chats with something new since I last read them.

## [0.10.0-alpha.69] — 2026-10-26

Slack and Teams in Replying for you.

### Changed

- **Replying for you** in Settings lists the Slack workspaces and Teams accounts you've connected, and **Check it all now** tries each of them too — reading as a check would, sending nothing — and says what it found.

## [0.10.0-alpha.68] — 2026-10-26

Replying in Teams.

### Added

- **Send a reply into Teams.** A draft for a Teams message has **Send**: it shows the account you send as and the chat it goes into, and goes in as you when you press **Send it**.
- **Replying by myself works in Teams too**, under the same rules as Slack: tick the account beside your mailboxes, and I answer one-to-one chats — never a group — looking in the chat just before a reply goes, so if you've answered there, or they've written again, it doesn't go.

## [0.10.0-alpha.67] — 2026-10-26

Teams.

### Added

- **Your Microsoft Teams chats.** Under Your mail, **Connect Teams** signs you in with Microsoft in your browser — a work or school account — and I read your one-to-one and group chats, both sides, and check for new messages as often as I check your mail. Meeting chats and channels aren't read, and nothing in Teams changes. A personal Microsoft account's Teams can't be read this way, and I say so. **Sign in again** keeps what I've read when Microsoft asks you to.

## [0.10.0-alpha.66] — 2026-10-26

Replying in Slack by myself.

### Added

- **Replying by myself works in Slack too.** Tick a workspace beside your mailboxes under Replying by myself, and I answer direct messages there under the same rules as mail — held first, only people you allow, a few a day, rehearsable. I never answer a group conversation by myself. Just before a reply goes I look in the conversation: if you've answered there, or they've written again, it doesn't go. What I send by myself in Slack isn't taken as your writing, even when I read it back first.

## [0.10.0-alpha.65] — 2026-10-26

Replying in Slack.

### Added

- **Send a reply into Slack.** A draft for a Slack message now has **Send**, like one for an email: it shows the workspace you send as and the conversation it goes into, and goes in as you when you press **Send it** — into that conversation only, word for word. If Slack's answer gets lost on the way, I say so and won't send it again until you've looked. **Use this** still copies it instead.

## [0.10.0-alpha.64] — 2026-10-26

Slack.

### Added

- **Your Slack direct messages.** Under Your mail, **Connect Slack** walks you through making a small Slack app of your own from a manifest I give you, installing it to your workspace, and pasting the token it shows. I read your direct messages and group messages in that workspace — both sides, so I learn how you reply there — and check for new ones as often as I check your mail. Channels aren't read, bots and notices are left out, and nothing in Slack changes. The token is kept sealed with your other saved sign-ins, and **New token** gives me another for the same account without losing what I've read.

## [0.10.0-alpha.63] — 2026-10-26

Straight to a draft.

### Changed

- When the shortcut opens Write on a message you copied, I write the draft at once — to whoever the window names, once I've looked — and put the keyboard on **Paste it there** (or **Paste it there and send**), so copy, shortcut, Enter is all a reply takes. You can still change it first. Turn it off under Settings, in Answering a message from anywhere.

## [0.10.0-alpha.62] — 2026-10-26

Text messages.

### Added

- **Your Android text messages.** Back them up with SMS Backup & Restore and import the .xml file it makes. Both sides of each conversation are read — one conversation per person, or per group — so I learn how you text back. The backup marks which messages the phone sent, so you say once that they're yours. Drafts, messages that didn't send, and pictures without words are left out and counted; a later backup adds only what's new.

## [0.10.0-alpha.61] — 2026-10-26

Messenger and Instagram.

### Added

- **Your Messenger and Instagram messages.** Download your information from Facebook or Instagram in JSON (not HTML) and import the zip — or one part of it, if it came in several — or the folder it unzips to. Both sides of each conversation are read, so I learn how you reply; message requests and filtered messages are left out. Meta's download names who wrote each message and nothing more, so you say which name is yours, as for WhatsApp. Calls, people joining or leaving, photos, videos, stickers and files without words, Instagram's likes and reactions, and unsent messages are left out and counted, and the way Meta writes accented letters and emoji is put right. A chat imported again through another export or download — one Telegram chat, then your whole account — is one conversation, each message kept once.

## [0.10.0-alpha.60] — 2026-10-26

Telegram chats.

### Added

- **Telegram exports.** Export a chat from Telegram Desktop (or all your data, Settings → Advanced → Export Telegram data) as machine-readable JSON, and import the result.json it writes. Both sides of each chat are read, so I learn how you reply, as from WhatsApp; chats between two people and private groups, not Saved Messages, bots, channels or public groups. Telegram's own lines, files and stickers without words, and forwarded messages are left out and counted. A whole-account export says whose account it is, so I ask only whether that's you — and already know, when the phone number on it is one of yours.

## [0.10.0-alpha.59] — 2026-10-26

Your own registration with Microsoft and Google.

### Added

- **Signing in with Microsoft and Google**, in Settings. Outlook.com, Hotmail and Microsoft 365 mailboxes, and Gmail without an app password, are signed in to through an app registered with Microsoft or Google. A copy of me built without a registration couldn't sign in with them at all; now you can register me yourself, following the steps there, and paste what Microsoft or Google shows you — the Application (client) ID, or Google's client ID and secret. It's kept with your other saved sign-ins and used from the next sign-in on, and you can forget it to go back to this copy's own. A mailbox already signed in with that provider under another registration then needs Sign in again, which it says. Google's secret is never shown again.

## [0.10.0-alpha.58] — 2026-10-26

Paste it there and send.

### Added

- For chat apps where Enter sends — WhatsApp, Discord, Slack, Teams, Messenger — you can turn on **Paste it there and send** under Settings, in Answering a message from anywhere. It pastes my reply into the window the message came from, as Paste it there does, then presses Enter once, there and only if that window is still in front. Off unless you turn it on; Paste it there stays as it was. Afterwards I still ask whether it went, and record only what you say.

## [0.10.0-alpha.57] — 2026-10-26

Why not by myself.

### Added

- While replying by myself is on, under a reply I won't send by myself I say why: it came with an attachment, you were only copied in, you asked for this one yourself, I've sent as many today as you allow, a reply was already in your Sent folder — whatever it is. A reply I'm holding still says when it goes instead, and a reply to someone you'd rather answer yourself is said in the line about them, as before.

## [0.10.0-alpha.56] — 2026-10-26

One reply, even from two computers.

### Changed

- Just before a reply goes by itself, I look in that mailbox's Sent folder for a reply to the same message — reading only who each recent one answers, opening none. If there's one there — sent by me on your other computer, or by you from your phone since I last read the mailbox — mine doesn't go, and the held reply says why. If I can't look, it waits up to half an hour and I try again; then, or at once if the server won't let me look, it's left for you.
- A reply I sent by myself is now marked in its Message-ID, a header nobody reads, in a way that names nothing. When I read one back from the Sent folder on another computer, I know it as mine: it answers the conversation, and I don't learn from it as your writing. Your own vacation replies carry no mark, so they still hide nothing waiting for you.
- I don't reply by myself to mail that came without a Message-ID: a reply couldn't say what it answers.

## [0.10.0-alpha.55] — 2026-10-26

Answering from anywhere, said in Settings.

### Changed

- Settings' card for answering a message from anywhere now says what it does since alpha.46 and alpha.52: Paste it there, who it's written to from the window's title, and that I read that title only when you press the shortcut.

## [0.10.0-alpha.54] — 2026-10-26

Not when you were only copied in.

### Changed

- I reply by myself only to mail sent to you, not mail you were only copied in on, where someone else was the one asked. That waits for you.

## [0.10.0-alpha.53] — 2026-10-26

Not without reading the attachment.

### Changed

- I don't reply by myself to a message that came with an attachment or an invitation — a contract to sign, a photo, a meeting request — since my reply couldn't have read it — including a PDF or photo Apple's Mail sends inline and a forwarded message, though not a logo in someone's signature or the signature of signed mail. It waits for you. Mail I read before this version waits for you too, since I can't tell.

## [0.10.0-alpha.52] — 2026-10-26

Who it's from, from the window.

### Added

- When you answer a message from anywhere, I look at the title of the window you copied it in — "Ada Lovelace | Messenger", "@ada - Discord" — and when it names one person you've been in touch with, I choose them as who it's from, so the reply sounds like you do with them. I say so, and you can change it. When the title could mean two people, I choose no one.

## [0.10.0-alpha.51] — 2026-10-26

Check it all now.

### Added

- **Check it all now (sends nothing)**, under Replying for you: for each mailbox I sign in to read (looking at its folders, opening no message) and to send (sending nothing), then ask the model that writes your replies whether it answers, and say what each found, one after another.

## [0.10.0-alpha.50] — 2026-10-26

Replying by myself says everything it checks.

### Changed

- What Settings says before you turn on replying by myself now covers everything I check since alpha.44: that I check again when a reply goes, under your rules as they are then; that I only answer mail I read the same day; what in a reply's words keeps it for you; and that you can rehearse first.

## [0.10.0-alpha.49] — 2026-10-26

I read a reply before it goes by itself.

### Changed

- A reply I'd send by myself waits for you instead when it has a gap left to fill in (like "[time]" or "{name}"), talks about being an AI, has a link that wasn't in the message it answers, or runs past 3,000 characters. It says which.

## [0.10.0-alpha.48] — 2026-10-26

More mailboxes known.

### Added

- Connecting a mailbox now fills in the servers, and says where to make the password, for GMX, WEB.DE, mail.com, Yandex, Mail.ru, Xfinity (Comcast), AT&T (and SBCGlobal and BellSouth addresses), Verizon (now at AOL) and QQ Mail — for reading and for sending.

## [0.10.0-alpha.47] — 2026-10-26

Off from the icon.

### Added

- While I'm replying by myself, or rehearsing it, the icon in the notification area says so, and its menu has **Turn replying by myself off**: one click from anywhere, stopping every reply still held, as Settings does.

## [0.10.0-alpha.46] — 2026-10-26

Paste it there.

### Added

- When you answer a message from anywhere with your shortcut, **Paste it there** puts my reply back where the message was: I remember the window you were in, bring it back to the front and paste the reply into it, ready for you to send. I only ever paste into that window, and only once it's in front; if it has closed or won't come forward, the reply is copied for you to paste instead. I never press Send for you there. **Copy and go back** is still beside it.

## [0.10.0-alpha.45] — 2026-10-26

Rehearse before I reply by myself.

### Added

- **Rehearse first**, when you turn on replying by myself, or **Rehearse** under it at any time: I do everything as if I were replying by myself — hold each reply on the home screen, wait for your mailbox to be read again, check every rule — and at the moment it would go, I stop it and leave it for you. Nothing is sent. What I'd have sent, and when, is under Your mail; a notification says when one comes due. When you stop rehearsing, the replies held while rehearsing wait for you rather than going for real.

### Changed

- A reply stopped before it went says why under Your mail.

## [0.10.0-alpha.44] — 2026-10-26

Replying by myself, checked harder.

### Changed

- I don't reply by myself to mail I read more than a day ago, or that reached me days after it was sent — a backlog, however it came to be allowed — nor to mail that came through a mailing list, where a reply goes to everyone on it.
- When a reply I sent by myself may or may not have gone, a notification says so and where to look. With hours set, the one for a reply I'm holding says it goes within them.

### Fixed

- A held reply waited for your mailbox to be read again, but a check that began before its hold ended counted, so an answer you sent from your phone in between could be missed. Only a check that began after the hold ended counts now, and every rule is checked again on what it read.
- Rules you change while held replies are going apply to the rest at once, and a reply whose mailbox's sending address changed while it was held waits for you.
- Pressing Send on a reply I'm holding to send by myself says so, rather than that it has already gone.
- A reply you approve goes only to an address the Send panel offered for it.
- Some mail servers repeat how many messages the inbox holds each time I wait on them; I took that for new mail and checked again and again. Only a change counts now, and a server that keeps ending its wait at once is left alone for a growing while.
- A watch on a mailbox the server turns down — a changed password, or a sign-in refused twice — now asks for a check, which says so under Your mail and in a notification, instead of trying quietly every few minutes.
- After a watch that had been working drops, I try again within half a minute, not after the long pause left from earlier failures; and a check asked for while one is running is queued, so mail that arrived during it isn't left for the schedule.
- A draft for a message copied from anywhere, replaced when you copy in another, is recorded as passed over rather than left waiting.

## [0.10.0-alpha.43] — 2026-10-26

Replying for you, brought up to date.

### Changed

- **Replying for you**, first in Settings, now also says whether each mailbox's replies carry your signature, the hours and the people left out when I reply by myself, and how long Send it waits so you can take a reply back — each with where to change it.

## [0.10.0-alpha.42] — 2026-10-26

Mailboxes in other languages.

### Fixed

- A mail server names folders in its own encoding, so a Sent folder called "Éléments envoyés" or "Gesendete Objekte" under the inbox could go unfound where the server doesn't mark it, leaving me none of your writing to learn from. I now read the names as you would, both to find the Sent folder and to show you what I'll read, and still ask the server for each by its own name.

## [0.10.0-alpha.41] — 2026-10-26

Replying by myself, one person at a time.

### Added

- Under each reply on the home screen, while replying by myself is on: whether I may answer that person by myself, and one click to change it — **Never by myself to them**, **Let me answer them by myself**, or, when only the people you chose are answered, **Answer them by myself too**.

## [0.10.0-alpha.40] — 2026-10-26

A moment to take it back.

### Added

- **Don't send it.** When you press Send it, I wait five seconds before anything goes, counting down, with **Don't send it** to take it back. Choose the wait under Checking your mail in Settings — 5, 10 or 30 seconds, or not at all. Leave the panel while it counts and nothing goes, and I say so.

## [0.10.0-alpha.39] — 2026-10-26

Only at the hours you choose.

### Added

- **Only at the hours I choose**, under Replying by myself: replies I send by myself go only between the hours you set on this computer's clock — 8:00 to 20:00 to begin with, or across midnight if you like. One that would go outside them waits on the home screen until they start, where you can still stop it or send it at once.

## [0.10.0-alpha.38] — 2026-10-26

What the server said.

### Added

- **What the server said**, under a mailbox whose last check failed in Your mail: what I asked its mail server and what it answered, to see why — or to copy and send to whoever is helping you. Your password, your sign-in and your messages are never in it, only what was asked and answered.
- The diagnostics bundle carries the same for a check, a sign-in, a look at a mailbox or a send that failed.

## [0.10.0-alpha.37] — 2026-10-26

People you'd rather answer yourself.

### Added

- **Never by myself to these**, under Replying by myself in Settings: name anyone I shouldn't answer on my own — your manager, your family — and their messages always wait for you, whomever else I answer.
- **Stop it, and never by myself to them**, on a reply I'm holding to send by myself: stops it, and puts them on that list in one go.

## [0.10.0-alpha.36] — 2026-10-26

Your signature.

### Added

- **A signature for each mailbox**, under Your mail: what goes under each reply I send from it — your name, your role, a number — after a "--" line, the way mail programs sign. The Send panel shows it before anything goes, and replies I send by myself carry it too.
- It's never taken as your writing: what I learn from is the words above it.

## [0.10.0-alpha.35] — 2026-10-26

Back where you were.

### Added

- **Copy and go back.** When you've opened me with your shortcut to answer a message from somewhere else, the draft has **Copy and go back**: it copies the reply and I step out of the way — into the notification area, or minimised if I'm not there — so the app you came from is in front to paste into.
- **Did it go?** I can't see what happens after you paste, so the next time Write is open I ask about the reply you copied: **It went as I copied it** (I learn from it, and from anything you changed before copying), **I changed it first** (I leave it alone, since I don't know what you sent), or **It didn't go**.

## [0.10.0-alpha.34] — 2026-10-26

Connecting a mailbox at your own domain.

### Added

- **Who handles your mail.** An address at a domain of your own — a business's, a family's — says nothing about whose servers its mail is on. When you type one into Connect a mailbox and move on, I ask this computer's DNS who handles mail for the domain, and when it's Google Workspace, Microsoft 365, Zoho, Fastmail, iCloud, GoDaddy or Namecheap I say so and set it up as theirs: Google Workspace signs in with Google (or takes an app password), Microsoft 365 with Microsoft. Anything else asks for the server, as before.
- More addresses are known without asking: Yahoo's other domains (ymail.com, rocketmail.com, yahoo.co.uk and others), aol.com, mac.com and Zoho's, and the sending servers of Zoho in Europe, GoDaddy and Namecheap.

## [0.10.0-alpha.33] — 2026-10-26

Copying in the others on a message.

### Added

- **Copy in**, in the Send panel: when the message you're answering went to other people too, they're listed, none ticked, and you tick whoever should get your reply as well (or everyone else on it at once). The panel says who it goes to and who's copied in before it goes, and What I've sent says it after.
- I copy in only people who were on the message you're answering. A reply I send by myself still goes to who wrote and no one else.

## [0.10.0-alpha.32] — 2026-10-26

Replying for you, in one place.

### Added

- **Replying for you**, first in Settings: where each part stands — a mailbox, whether replies can be sent from each, replies prepared in advance, replying by myself, whether I keep running when my window is closed, and notifications — each with where to change it.
- **Try it (sends nothing)**, under each mailbox's sending in Your mail: I sign in to its outgoing server and send nothing, so you know a reply can go before one has to.

## [0.10.0-alpha.31] — 2026-10-26

When a mailbox stops working.

### Added

- **A notification when I can't check a mailbox** — a password changed, a sign-in that ran out, a server that turned me away. It names the mailbox and says why, once; you hear again only if it works and then stops again. Until it works, nothing is read from it and nothing goes out from it, so you'd otherwise only find out by opening me.

## [0.10.0-alpha.30] — 2026-10-26

What the home screen says about your mail.

### Changed

- The line about checking your mail now says when I read new mail as it arrives — and only while your mail server is actually telling me, for each mailbox. Until then it says how often I check, as before.

## [0.10.0-alpha.29] — 2026-10-26

A message from anywhere.

### Added

- **Answer a message from anywhere.** WhatsApp, Discord, Messages on your phone and most other places don't let an app read or send for you, but they all let you copy. Copy a message there, press your shortcut, and I come forward with Write open on it, ready for a reply in your voice to copy back. Choose the shortcut under Settings → Answering a message from anywhere (Ctrl+Shift+Alt with M, R or Space); it's off until you do.
- I read what you copied only when you press the shortcut, and it goes into Write and nowhere else until you ask for a draft. If another app already has the shortcut, Settings says so and keeps the one you had.

## [0.10.0-alpha.28] — 2026-10-26

Running in the background.

### Added

- **Closing my window leaves me running** in the notification area, by the clock, so I keep checking your mail, writing replies and — if you turned it on — sending them. Click my icon to open me; right-click it to quit. You can have closing quit me instead, under Settings → When the window is closed.
- **Start when you sign in to Windows**, with no window, only the icon. Off until you turn it on.
- **Windows notifications** when replies are ready to look over, when one will go by itself soon, has gone, or didn't go. They say how many and where to look — never who wrote or what they said — and none comes while you're looking at me. You can turn them off.

## [0.10.0-alpha.27] — 2026-10-26

Mail as it arrives.

### Added

- **New mail is read as it arrives**, not at the next check. Most mail servers can say when a message comes in, and I keep one quiet connection to each of your mailboxes to hear it; then I read the mailbox as usual. That connection only listens: it doesn't read your mail or change anything. A reply waiting on a fresh check goes sooner too.
- A server that can't say is checked on the schedule, as before. Turning checking off in Settings stops the listening too.

## [0.10.0-alpha.26] — 2026-10-26

Sign in with Google.

### Added

- **Gmail and Google Workspace sign in with Google**, in your browser, the way Outlook.com signs in with Microsoft — no app password to make. I never see your password; what lets me stay signed in is kept, sealed, on this computer. Checking, sending replies and everything else go through that sign-in.
- Gmail still takes an app password: **Use an app password instead** in the connect dialog. A Google Workspace address at your own domain can sign in with Google once you say it's one, under Server settings.
- A Google mailbox can be signed in again from Your mail, keeping everything already read.
- This needs Mimic registered with Google; a copy built without that connects Gmail with an app password, as before. `docs/RELEASE_PROCESS.md` says how.

## [0.10.0-alpha.25] — 2026-10-26

A copy in your Sent folder.

### Added

- **Replies I send land in your Sent folder.** Gmail, Outlook.com, Microsoft 365, Yahoo and AOL put what is sent through them there themselves. For any other provider — iCloud, Fastmail and the rest — I put a copy of each reply there, marked as read, so your mail program shows what went. You can turn the copy on or off for a mailbox under Your mail → sending. It's the one thing I ever write to your mailbox.
- If the copy can't be made, the reply still went: What I've sent says so beside it.

## [0.10.0-alpha.24] — 2026-10-26

Replying by myself.

### Added

- **Replying by myself**, under Settings. With it on, I send the replies I prepare in advance by myself, from your mailbox, as you — through the mailboxes you tick, to people you've written to yourself or only to the people you choose. It's off until you turn it on, and turning it on says what it does before it does anything.
- **Nothing goes at once.** Each reply waits under the message it answers, for ten minutes unless you choose otherwise, saying when it will go and to whom. **Stop it** and the draft waits for you like any other; **Send it now** sends it at once.
- **What it won't answer:** anything that came before you turned it on, mail that wasn't sent to you by name (a list, or a copy you were hidden on), mail that looks automated, a thread you said needs no reply, anything older than your waiting window, a message from someone you haven't written to yourself, a draft you asked for yourself or are choosing between ways of saying, and the same message twice. A reply by itself goes only to the address they wrote from, never to a different reply-to address. It stops at your daily limit (ten unless you choose otherwise), and at two replies by itself in one conversation a day.
- Each reply sent by itself tells other mail programs it was sent automatically (`Auto-Submitted: auto-replied`), so their automatic replies don't answer it.
- Everything checked when a reply is held is checked again when it goes, after your mailbox has been checked again — so an answer you sent from your phone in the meantime counts: if you answered first, wrote since, or turned it off, it's stopped and says why under What I've sent.
- Turning it off stops everything still waiting, at once.

### Changed

- Your mailbox can be checked every 2 minutes, under Settings → Checking your mail. When a reply's wait is over, I check its mailbox at once rather than at the next scheduled check.
- What I send by myself isn't read as how you write: it's left out of what I measure, of the examples I show a draft, of what I learn from what you change, and of how often my drafts go out unchanged.

## [0.10.0-alpha.23] — 2026-10-25

Send it from here.

### Added

- **Send a reply from Mimic.** Under a reply I've written, **Send** shows who it goes to, which of your mailboxes it goes out of and its subject, with the words as you left them. Nothing goes until you press **Send it**. It goes from your own mailbox, as if you'd sent it from your mail program, threaded under the message it answers, to where they asked for replies — or the address they wrote from — and to no one else on the thread. Any other address I have for them can be chosen instead, and the subject changed.
- Once it has gone, the thread is answered at once, and what you changed is learned from, as **Use this** does. When your mailbox's Sent folder is read next, its copy is the same message, not a second one.
- **What I've sent**, under Your mail: every reply sent from here, or tried, with where it went and whether it got there. One that didn't go says why, in the mail server's words.
- **Sending from a mailbox.** Gmail, Outlook.com and Hotmail, Microsoft 365, iCloud, Yahoo, AOL, Fastmail, Zoho, GMX, Mail.ru and Proton Mail Bridge send through their usual server without asking. For any other, set the outgoing server under Your mail → **Set up sending**. I check a server by signing in to it with the mailbox's own password or sign-in, and send nothing, before I keep it.
- Signing in with Microsoft now asks to send as well as read. A work or school mailbox may need its admin to allow sending from apps before replies can go from it.

### Changed

- **Use this** still copies a reply for you to send yourself. Without a connected mailbox that can send, it's the only way, and the line under the draft says why Send isn't there.
- A reply is only sent from one of your own addresses, so that what goes out is filed as yours.
- A reply that may have gone — the connection lost just as it was handed over, or Mimic closing mid-send — is never sent again by itself. I say so and ask you to look in your Sent folder; Send comes back for it only once you say under What I've sent that it isn't there.
- Deleting a person, a mailbox or everything also deletes my record of the replies I sent with them. The replies themselves have gone and can't be taken back, and the preview says so.

### Fixed

- The changelog and the project status page showed punctuation as garbled characters ("â€”" for a dash) since 0.10.0-alpha.22.

## [0.10.0-alpha.22] — 2026-09-24

### Changed

- Welcome screen offers message import or exploration without an email address.
- Name-only setup supports chat imports; mail addresses can be added during preview.
- Supported export formats are clearer, and mailbox connection is optional.
- Model setup follows the welcome screen; failed setup saves can be retried.

## [0.10.0-alpha.21] — 2026-09-23

Other ways to say it.

### Added

- **Other ways to say it, side by side.** Under a reply I've written, **Write it another way** offers **Shorter**, **Longer**, **More casual** and **More professional**. Each writes one more version from the same message and the same note — one request to your model, like any draft — and puts it beside the first, named by how it differs.
- Use whichever you like, changed or not. The others are set aside as passed over, not as turned down, so choosing between them doesn't count against my drafts. **Not this one** puts one version aside; **None of these** puts them all aside.
- The versions stay beside each other until you choose, even if you leave the screen and come back.
- A version you asked for isn't something I learn your habits from: what you change in it, you changed from what you asked for, not from how I write.

### Changed

- A draft written **Shorter**, **Longer**, **More casual** or **More professional** in Write something new is no longer read for your habits either. What you changed in it you changed from what you asked for, so a pattern that rested on those drafts may no longer show under How you write.

### Fixed

- Writing a draft again in Write something new no longer leaves the one it replaced waiting for ever: it's recorded as passed over.

## [0.10.0-alpha.20] — 2026-09-23

Your Discord messages.

### Added

- **Learn from what you've written on Discord.** Ask Discord for your data (Settings → Data & Privacy → Request all of my data); when the package arrives, add it under Your mail → Add a source → **Discord data package**, as the zip or the folder it unzips to. I read every message you sent — in direct messages, groups and servers — one conversation per channel.
- A Discord package holds only what you wrote, never what anyone else said, so it teaches me how you write rather than how you reply to someone, and nothing from it waits on the home screen. I can show a draft some of them as messages you sent, never as replies to anything. The check says so before you import.
- A package is one account's, so I import it only once that account is yours. If it has an email you've given me, it's yours already; otherwise I ask **Is this you?** first, and **Import** waits for your answer.
- Mentions, channel links and custom emoji are written as `@someone`, `#channel` and `:name:`, since the package doesn't say who or what they were, and a time Discord shows as a timestamp is written as that time. A message that was only a picture or a file has no words and is left out. Importing a newer package later adds only what's new.

### Fixed

- An example I show beside a draft says only what it was chosen for: one written to nobody in particular is no longer called "same person and channel".
- A file I can read no messages from no longer also warns that it has no timestamps.

## [0.10.0-alpha.19] — 2026-09-23

WhatsApp chats.

### Added

- **Bring in a WhatsApp chat.** In WhatsApp, open a chat and use **Export chat**; then under Your mail → Add a source, choose **WhatsApp chat** and the file it made — the `.txt`, or the `.zip` from an iPhone or with media. I read each message with who wrote it and when, messages of several lines included, from Android and iPhone exports alike, with a 12- or 24-hour clock.
- **Say which name is yours.** WhatsApp knows people only by the name your phone saved them under, not by an address, so before importing I list everyone who wrote and ask **Which of these is you?** The name you pick becomes one of your addresses, as any address does — and if I'd already filed messages under that name as someone else, I show you who and what would move first. I ask again before taking the name the chat is named after (usually the other person) or a second name, and you can take a name back before importing. Until you say, your messages in the chat read as someone else's.
- Dates in an export don't say whether they're day or month first. I read it from the chat itself — a 13th settles it, and so does which reading keeps the messages in order — and when nothing does, I read them day first and say so before you import.
- Photos, voice messages, stickers, calls, locations, polls and deleted messages have no words of yours, so they're left out, and the check says how many. WhatsApp's own lines — "Messages and calls are end-to-end encrypted", "Ada added Bob" — are left out too.
- Exporting the same chat again later from the same phone and importing it adds only what's new — as long as the file keeps the name WhatsApp gave it and the contact hasn't been renamed.

## [0.10.0-alpha.18] — 2026-09-23

Find what was said.

### Added

- **Find anything said in the mail I've read.** Find, first in the bar, looks through every message I've read — yours and other people's — for the words you type, and lists the messages that say them, the newest first, twenty at a time, with the words around each match marked. Every word has to be there, in any order; words in quotes have to be together, in that order; capitals and accents don't matter, and a last word of three letters or more can be the start of a longer one, so "fri" finds "Friday". **Written by** narrows it to what you wrote, or to everything you didn't. **Show it in its conversation** shows the message whole with what came before and after it, a page at a time. What you type is never read as a search language — "OR" and "-" are just words — and it isn't logged.
- Chinese, Japanese and Thai are written without spaces, so the index keeps a whole run as one word: a word there is found by the start of its run — as the last word you type, one character is enough, as it is in Korean — but not from inside it.
- The search happens here, in the database on this computer. To do it, the database keeps a copy of each message's text in a search index, and the copy goes whenever the message does — with a person, a source or everything — straight away. Mail I read before this version is added to the index the first time this version opens, before its window appears — with a large mailbox that can take a few seconds.

## [0.10.0-alpha.17] — 2026-09-23

Finding your past replies by meaning, and the rest of how you write.

### Added

- **Past replies found by what they mean, not only by the words they share.** A draft is shown a few of your past replies to messages like the one you're answering. Until now I found them by shared words, so "drinks on friday?" never found "pub friday?". Under Settings → Finding your past replies by meaning you can download a small sentence encoder, all-MiniLM-L6-v2 (91 MB, from Hugging Face, Apache-2.0); after that I find them mostly by meaning and a little by wording, and a draft's "why" says which. It downloads only when you ask, and it's used only while every file matches the SHA-256 this version pins — a file that doesn't match is deleted, and one changed on disk later is never run. It runs on this computer: the messages you've answered, and your own, are read into it once, in the background, and from then on as mail comes in. Reading them for meaning sends nothing anywhere. The drafts I prepare in advance and the drafts measured under How close my drafts come find their examples the same way, and How close my drafts come measures "overall wording" by meaning with it.
- **Say what one of your messages was doing.** The "When you say no", "When you thank someone" layers are measured over your messages filed by what they were doing, and the rules that file them can be wrong. Under any message of yours in a conversation — on the home screen or under People — there's now a line saying what it's filed under and by whom, with **Change** (or **What was this doing?** under one filed under nothing). What you say — one situation, several, or none of them — stands over the rules and over any model until you choose **Let the rules decide**. The layers it moves are measured again by themselves.
- **A model on this computer can read what each message is doing.** How you write → What each message is doing counts how your messages came to be filed — by the rules, by a model, by you — and, with a model on this computer that's answering, has it read them: up to 400 at a time, the most recent first, replacing what the rules said. Reading them sends every one to the model, so I only do it with a model on this computer; a hosted one is never used for this.
- **How you write, in words.** How you write → In words has the model you chose read each layer's numbers and say in two or three sentences how you write — short and warm, say, or careful and formal. Only the numbers go to it, with the greetings and sign-offs you use from my own short list ("hi", "thanks"): never a message, a phrase you wrote, or who a layer is about. If it doesn't answer, I say so rather than saying it finished. The reading is shown under the numbers as the model's reading, and a draft is given it beside the numbers, until the numbers change; a reading of older numbers is shown as that and given to no draft.
- **Saved passwords on macOS are kept with the Keychain.** A build for macOS seals each saved key and password with a key kept in your login Keychain, as Windows builds seal them to your Windows account. There is no build for macOS yet, and this has been compiled for it but never run on a Mac.

### Changed

- **How you write is measured again by itself after new mail, and only where something changed.** After an import or a mailbox check that brought in anything, what it touched — the layer over everything, the channels and people and kinds of message it's in — is measured again in the background, and the rest is left as it was. The first measurement is still yours to start. Measuring reads your messages a page at a time rather than all at once, so a large mailbox no longer has to fit in memory to be measured, and the numbers are the same either way.
- **A channel, person or kind of message with nothing left in it loses its layer** when it's measured again, rather than describing messages that are gone.
- **A reply in a conversation with several people is one example, not one per person.**

## [0.10.0-alpha.16] — 2026-09-23

Every conversation, not only what's waiting.

### Added

- **People → Conversations shows every conversation I've read with someone**, the most recent first, not only the one waiting on the home screen. Each one says where it stands, by the same reading the home screen makes: you wrote last, it's on your list, or it's not on your list and why — the message it waits on looks automated, or is older than your waiting window, or you said it needs no reply.
- **Read it** opens a conversation from its last message, and further back a page at a time. What you wrote is marked as yours, and a message whose headers say a machine sent it says so.
- **Put it on my list** puts a conversation that was left off back on the home screen, where I can draft a reply to it — the same as saying it needs a reply there.

### Fixed

- **People and How you write can be opened.** Since 0.9.0-alpha.2 the bar at the top had only Settings, and nothing anywhere led to People or How you write — so telling me how you know someone, deleting someone, and measuring how close my drafts come could not be reached. The bar now has Your mail, People, How you write and Settings, and the same row sits at the top of the drawer, with the one you're in marked, so you can go from one to the next. Opening one takes the keyboard into it, closing it gives the keyboard back, and Escape in a dialog inside it closes only that dialog. On a narrow window the bar wraps rather than cutting any of them off.
- **Links that look like buttons are one control.** The home screen's way to Your mail was a button inside a link, which a screen reader announces as two things, and How you write's was a link that didn't look like a button at all.

## [0.10.0-alpha.15] — 2026-09-23

Your Sent folder, from any mail program.

### Added

- **A Sent folder from Thunderbird or Apple Mail counts as your Sent folder.** Those programs keep and export one file per folder, and until now only a Gmail export said which folder a message was in, so the question "Is that you?" about the addresses you write from never came up for anyone else. Now a file named as a Sent folder is read as one, except mail you passed on: `Sent` (Thunderbird's own file), `Sent.mbox`, Apple Mail's `Sent Messages.mbox` (choose the file named `mbox` inside it), and the names Outlook.com, Exchange and other servers give it in English, German, French, Spanish, Portuguese, Italian, Dutch, the Nordic languages and Polish — `Sent Items`, `Gesendete Elemente`, `Éléments envoyés` — when Thunderbird or Apple Mail holds a copy of that account. Before you import, I say so and how many of its messages count. (Outlook's own exports are .pst files, which I can't read.)
- **A connected mailbox's sent folder is found by its name in those languages too** when the server doesn't mark it and the name has no accents (`Gesendete Elemente`, `Enviados`, `INBOX.Verzonden`).
- **You can choose a file with no extension**, which is how Thunderbird keeps each folder: the file picker offers All files as well.
- **A file in Mimic's own format can no longer say a message was in your Sent folder**, as it already couldn't say a machine sent one: only a mailbox's folder, Gmail's label or a file's name can.

### Fixed

- **Importing a file again catches up mail an earlier version read.** Whether a message came from your Sent folder (from 0.10.0-alpha.12) and whether a machine sent it (from 0.10.0-alpha.4) is read from its headers, which I don't keep, so mail read before then had neither, and importing the same file again added nothing. Now **Import again** gives mail already here what it lacked, and says how many messages it marked. Nothing already read is overwritten, only what's missing is added, and it comes only from the copy of a message a first import of the same files would keep — not from another copy of it, such as your post to a mailing list as the list sent it back.

### Not in this release

- A connected mailbox isn't read again, so its mail from before those readings still has none.

## [0.10.0-alpha.14] — 2026-09-23

Outlook.com and Microsoft 365, signed in with Microsoft.

### Added

- **Outlook.com, Hotmail and Microsoft 365 mailboxes can be connected.** Microsoft stopped taking passwords from apps like me in September 2024, so until now these couldn't be connected at all. Now you give your address and press **Sign in with Microsoft**, and you sign in in your browser, on Microsoft's own page — I never see your password, and a second step or a question from your organisation happens there. When you come back I show what I'd read, as for any other mailbox, and nothing is imported until you say so. Microsoft's addresses in other countries — hotmail.co.uk, outlook.fr, live.com.au — are recognised too. For a Microsoft 365 work or school address, or any other address on Microsoft, tick "This is a Microsoft account" under Server settings. If your organisation hasn't allowed apps like this, Microsoft says so when you sign in.
- **What keeps me signed in is kept like a password.** Microsoft hands back a sign-in that lasts. I lock it to your Windows account, as I do an app password, and when I check your mail I trade it with Microsoft for a pass that lasts an hour, which I only ever hold in memory. Removing the mailbox removes it.
- **Sign in again**, under Your mail, for when Microsoft asks — after you change your password, say — without removing the mailbox or anything read from it. The new sign-in is kept only if it opens that mailbox.

### Not in this release

- Gmail, Yahoo and iCloud still take an app password. Signing in with Google isn't built.
- No real Microsoft mailbox has been signed in to from the build machine or from CI: the browser's return, the exchange with Microsoft and the mailbox's sign-in are tested against stand-ins on the same computer.
- A copy of Mimic built without Mimic's registration with Microsoft says it can't sign in with Microsoft, and offers nothing that would fail.

## [0.10.0-alpha.13] — 2026-09-23

Importing again only adds what's new.

### Fixed

- **A later export of a chat goes after what was already there.** Importing a newer export of the same conversations numbered its new messages from the start, so they sat among the old ones, and the wrong message could decide whether someone is waiting on you. New messages now go after the ones already there, and into time order when every message says when it was sent.
- **Reading a message again makes nobody up.** When a message I already had came back with its sender written differently — a new name, another address — I could add that sender to the conversation as someone new, even though I kept the copy I already had. Now I recognise a message I have before I look at who sent it.

## [0.10.0-alpha.12] — 2026-09-23

Your Sent folder knows your addresses.

### Added

- **I ask about the addresses your Sent folder shows you write from.** Mail in your Sent folder is usually yours, so when some of it came from an address you didn't give me — a work address, an alias, one you've since stopped using — it's often you. Until now I filed it under a person of its own, counted what you wrote from it as someone else's, and put your own replies on the home screen as people waiting on you. Now the home screen says how much of that person's mail was there and asks: "All 12 messages I have from C at work (c@work.example) were in your Sent folder. If that's you, I'm counting what you wrote as someone else's." **Is that you?** says why it might not be — someone else sends for you, you passed their mail on, or other people send from that address too — and shows exactly what saying yes would change, the same question adding the address by hand asks. A yes makes it yours and moves what was sent from it over. A no is kept, and I don't ask about them again. The likeliest is asked first; everyone found this way is listed under Settings → You.
- **When nothing I read was written by you, the import step asks first about the address your Sent folder names**, instead of leaving you to guess which one you wrote from.
- It works for a connected mailbox (its sent folder) and for a Gmail export (mail labelled "Sent"). Other exports don't say which folder a message came from. Mail you passed on to someone, which keeps its first writer's address, isn't counted.

### Fixed

- **A version is released once.** Two builds started for 0.10.0-alpha.9 and each made a release page, leaving an unfinished copy behind. A second build for the same version now waits for the first, and stops if the first made one.
- **Your post to a mailing list counts as yours.** When a list sent your own message back to you with its own address as the sender, and I read both copies — the list's from your inbox, yours from your sent folder — I could keep the list's copy and file what you wrote under the list. I now keep the copy you sent, and make nobody up for the other.

### Not in this release

Only mail read from now on is marked as coming from your Sent folder: a mailbox isn't read again, and importing the same file again adds nothing, so mail you've already imported isn't asked about. You can still add an address under Settings → You, which asks the same question.

## [0.10.0-alpha.11] — 2026-09-22

How close my drafts come — measured, not claimed.

### Added

- **How you write → How close my drafts come.** I now check my drafts against what you actually wrote. I hold back some of your conversations, answer messages in them without looking at what you wrote back, and compare each with what you sent — next to a generic reply from the same model, and the reply you send most often. Four measures are shown on their own, each with its 10th percentile: length, words in common, punctuation habits, and overall wording. Nothing adds them up into a score, because they aren't the same kind of thing, and none of them says whether a reply was a good one. **Show the replies** puts yours, mine and the generic one side by side.
- It answers up to 12 messages people sent you, asking your writing model for two replies to each. On a local model nothing leaves your computer. On a hosted one, those messages, the messages before each, and some of your past replies with the messages they answered are sent to it and billed like any other draft — I choose the messages, not you, so the card says this before you start. Mail isn't checked until it's done.
- For the measurement I work out how you write again without the conversations I held back, and take no examples from them, so nothing I'm shown was measured on a reply I'm trying to match. What I've learned from your edits to my drafts is left out too, since some of those edits could be to these very replies; that leans against me, and the card says so. It also says what "overall wording" is: with no text encoder installed, it compares words and letters, not meaning.
- Nothing written while measuring is a draft. Deleting a person or a mailbox deletes the measurement — what I wrote for it came partly from other people's messages — and the deletion preview says so. Deleting someone while I'm measuring stops the measurement, and keeps nothing written from their mail. If someone turns out to be you, replies measured on their messages stop counting, and the card says how many are left.

### Fixed

- **Something canceled just as it was about to start no longer starts anyway.** Stopping a queued mailbox check or measurement could lose to it starting, and it then ran as if nothing had been said.
- **A request to the part of Mimic that measures can't hold anything up.** A request's time limit now covers sending it as well as waiting for the answer, and stopping it never waits for a request to finish being sent. Before, if it got stuck and stopped reading, a large request could wait on it forever.

### Changed

- The database moves to schema 11. The tables for measurements, never written until now, are replaced with ones that refer to your messages rather than copying them, and go with them. A backup is written before the upgrade, as always.

### Not in this release

The drafts are measured with no note from you, the way replies prepared in advance are written; a draft you've said something about would likely come closer. A measurement needs replies of yours in at least two conversations.

## [0.10.0-alpha.10] — 2026-09-22

The whole conversation.

### Added

- **Each card on the home screen can show the rest of its conversation.** A card showed the one message someone is waiting on, and nothing of what led up to it, so answering meant finding the thread somewhere else first. Now **Show the 80 messages before this one** reads what came before it back in order, oldest first — who wrote each one, when, and your own side marked as yours — the nearest twenty first, with **Show 60 earlier messages** reading further back. Nothing is read until you ask, and **Hide what came before** puts it away.
- **What came in after the message on a card is shown too.** When something that looks automated — an out-of-office reply, a read receipt — arrives after someone's question, the question stays the one on the card, as before. Until now what came after it wasn't shown anywhere. Now the card says there's more after it, and **Show the message after this one** lists it with why it looks automated, or that I couldn't tell who wrote it.

### Not in this release

Only what's already been read is shown; nothing is fetched from the mailbox to fill in a conversation. Messages are shown as plain text, the way they were read, so formatting, pictures and attachments aren't there.

## [0.10.0-alpha.9] — 2026-09-22

One Mimic at a time.

### Fixed

- **Opening Mimic when it's already open brings the open one to the front.** Before, opening it a second time — a second click on its icon, say — started a second copy on the same data, running its own background work alongside the first's. The second marked the first's work in progress as interrupted and started it again, both checked the same mailbox at once, and both wrote the same saved-passwords file. Now the second launch hands over to the first and closes.
- **And if a second copy gets that far anyway**, it finds the data in use, touches nothing, shows no window, says "Another Mimic is using your data right now, so this one won't start", and closes. The data folder is locked for as long as Mimic runs, and the lock goes when Mimic does, however it ends, so a crash never locks Mimic out of its own data.
- **Opening Mimic while it's still closing opens it once it has closed**, instead of handing the launch to the copy that is going. Closing can take a few seconds while background work stops.

## [0.10.0-alpha.8] — 2026-09-22

Your own mail, whichever address it came from.

### Fixed

- **An address you add now counts for mail I've already read.** If you wrote from an address you hadn't told me about, what you sent from it was filed under a person and read as someone else's: your replies didn't count as answers, your own notes looked like someone waiting on you, and none of it was learned from. Adding the address used to change only mail read afterwards, and reading the mail again changed nothing, because I keep the first copy of every message. Now, when mail from the address you add is filed under someone, I ask first — "Is C (work) you?" — and say how many messages would become yours, which of their other addresses would become yours too, and what you told me about them that would go. It can't be undone, and I say that too. Say yes and it happens in one step: the messages are yours, that person is gone from People, reply times are worked out again, and I look at how you write again. Say no and nothing changes. If anything I told you changed before you answered — more mail filed under them, say — I don't act on the old answer; I ask again, as it is now.
- **Someone whose every address is yours is folded back without a question**, as long as you've set no relationship, notes or preferences for them. That happens the first time this version starts (for an address you added in an earlier version after its mail was read), when you connect a mailbox of yours, and whenever something I was doing in the background finishes. Anyone else still under one of your addresses is listed under Settings → You, with the same question — and if you say they aren't you, I remember that, and never fold them in without asking.
- **Setup's "nothing in that file was written by you" had a fix that didn't work.** It asked for the missing address and read the file again, which kept everything as it was. Now adding the address is the fix, with nothing read again, and if nothing came from that address either I say so.
- **An address isn't added while I'm reading mail.** One added mid-read was missed by that read. Now I say what I'm busy with and ask you to try again when it's finished — and moving someone's messages over also waits while I'm working out how you write or writing replies. A reply being drafted for someone who is folded back meanwhile is kept, addressed to nobody, rather than failing.
- Adding an address that is already yours says so, instead of nothing.
- The database moves to schema 10: a person can now carry your answer that they aren't you. A backup is written before the upgrade, as always.
- The same text as two kinds of address — a phone number and a handle that read alike — is two addresses, not one.
- Setup's last step shows "Analyzing…" while an analysis runs. It was looking for a job that doesn't exist, so it never did.

### Not in this release

Removing an address changes only mail read after that. Which of your addresses an older message came from isn't kept, so it can't be handed back to someone else; Settings says so.

## [0.10.0-alpha.7] — 2026-09-22

Only what is still waiting. A connected mailbox brings in years of mail, and every thread that ended with someone else looked like someone waiting on you — a question from two summers ago counted the same as one from this morning.

### Added

- **Threads that have gone quiet are left out of what's waiting.** When the last message in a thread is more than 30 days old, I take it that nobody is still waiting on a reply. It's a reading of the date and nothing else, so the line under the heading counts them — "I left out 1,200 threads whose last message is more than 30 days old" — and **Show them** lists them with **It needs a reply** on each. What I left out is now listed in groups by why, each with its own most recent few, so a month of newsletters can't push the rest out of sight. Anything you said needs a reply stays on the list, whatever its age, and says so. A message with no date, or one I can't read, is never counted as old.
- **Settings → What counts as waiting** chooses the window: the last week, the last two weeks, the last 30 days, the last 90 days, or any time.
- Replies prepared in advance skip threads that have gone quiet, the same as the list does, so a first run over a real mailbox doesn't spend its drafts answering last year's mail.

### Changed

- With nothing waiting but something left out, the heading says "Nothing I've read looks like it's waiting on you" rather than that nobody is — the second is a reading of headers, dates and what you said.

### Not in this release

The window is measured from the message's own date, so mail with a wrong date is judged by it, and a date in an unusual form — some exports write their own — is never counted as old. Nothing about a quiet thread is changed or hidden: it's counted, listed on request, and comes back by itself when someone writes again.

## [0.10.0-alpha.6] — 2026-09-22

Passwords locked to your account. Your next step is typing a real mailbox's app password into Mimic, so this comes first.

### Changed

- **Your API key and mailbox passwords are locked to your Windows account.** Each one is sealed with Windows' own protection for saved passwords (DPAPI) before it is written, so a copy of the file — a backup, a sync, the disk read by another account or another operating system — can't be opened without your Windows password. What it doesn't stop, and the Privacy card says so: a program you run yourself could still unlock them, and so could an administrator of this computer or, on a work account, your organisation's IT — the same as your browser's saved passwords.
- **What you saved before is moved in the first time this version starts.** The old file held it as plain text; it is deleted once everything in it has been moved in and read back. If Windows won't seal, nothing is written unsealed: the old file stays and keeps working, the Privacy card says so, and a new key or password isn't saved until it can be locked. Deleting the old file doesn't erase it from backups or from the disk, so a key or password you saved before this version may be worth replacing.
- **Settings says how your passwords are kept**, from what was actually done rather than what was meant to happen, and the mailbox dialog says the password will be locked to your Windows account only when this copy of Mimic locks what it saves.

### Added

- **New password** for a connected mailbox, under Your mail. When your provider's app password changes, or a saved one can't be unlocked on this Windows account, you can give it again without removing the mailbox and everything read from it. I log in with it first and keep it only if that works.

### Fixed

- **Saving a key could fail without a word.** The Save button now says when a key wasn't saved, and why.
- A crash while saving could leave the credentials file half-written, and a half-written file was read as empty and then overwritten, losing every saved password. Saves now replace the file in one step, and a file that can't be read is set aside rather than overwritten.
- A mailbox whose password was missing told you to remove it and connect it again, which throws away everything read from it. It now points at **New password**.

### Not in this release

A key or password saved on another Windows account or computer, or before your Windows password was reset by someone else, can't be unlocked here. It is kept rather than deleted, the Privacy card counts it, and the key field and **New password** take it again. Going back to 0.10.0-alpha.5 or earlier after this version has started means entering the key and mailbox passwords again, because older versions only read the old file; if you do, this version picks up what you entered there when you come back, unless it's the same key or password it moved in before. macOS, which would use the Keychain, is still unbuilt.

## [0.10.0-alpha.5] — 2026-09-22

People are people. With a mailbox connected, **People** was mostly newsletters.

### Added

- **Senders whose mail all looks automated are left out of People** — newsletters, notification services, no-reply addresses — by the same reading of their headers that keeps their threads off the home screen. Your word and your writing come first: anyone you've written to, anyone you've said how you know, and anyone whose thread you said needs a reply stays a person, however their mail reads. So does anyone who sends one ordinary message. The line under the heading says how many I left out, and **Show them** lists them; saying how you know one of them puts them back among people.
- **Picking someone to write to** lists people only, and says when the list is cut short.

### Fixed

- **People could stop at 200 without saying so**, and with a mailbox the most recent 200 were mostly newsletters, so someone you write to could fall off the bottom unseen. The list now says when it is cut short, and newsletters no longer take the places.
- The subtitle said "Everyone I've seen you write to", but the list is everyone who has written to you. It now says so.
- The people count in the app's status counts people, not senders.

### Not in this release

Nothing here changes what is learned: leaving a sender out of the list touches none of their messages and no profile. Forwarding or answering a sender's mail counts as writing to them, so a newsletter you replied "unsubscribe" to stays among people. Mail imported before 0.10.0-alpha.4 carries no reading of its headers, so its senders count as people until its source is removed and added again.

## [0.10.0-alpha.4] — 2026-09-22

Only what needs a reply. With a real mailbox connected, most of what arrives is not a person.

### Added

- **Newsletters, receipts and notifications are left out of what's waiting.** I decide from the headers the sending system put on the message, never from its words: a mailing list with nowhere to reply to the list, mail marked as junk, automatic replies and out-of-office messages, bounces and read receipts, and addresses that say "no-reply" outright. When I'm unsure, it stays on the list — a newsletter that gets through costs you a glance, while a person I filed away is a reply you never see. So a discussion list your team writes to stays in; so does mail a help desk marks as bulk, because its staff write those replies; so does a contact form's message when it names the person who filled it in; and so do `info@` and `support@`, because people read those.
- **An automatic reply doesn't hide a question.** If a colleague's out-of-office lands in a thread after someone asked you something, the question is still waiting. And a notification that arrives after you replied — a comment on an issue you answered by email — is counted with what I left out, where you can put it back, rather than disappearing.
- **I say how many I left out, and show them when you ask.** A line under the heading counts what I left out and why, with **Show them**. Each one says why it looks automated to me, and **It needs a reply** puts it back — for that message; a newer one is judged again.
- **"Doesn't need a reply"** under every message. The thread comes off the list until that person writes again, and waits with what I left out in case you change your mind. If they wrote again while you were reading, nothing changes and I say so.
- Replies prepared in advance skip everything left out, so a run spends its ten drafts on people rather than on newsletters.

### Fixed

- **A draft could sit under a message it was not written for.** When someone wrote again before you used a draft, the draft for their earlier message was shown under the new one — and one already on screen stayed there after the new message arrived — while a draft for the new message was never prepared. A draft now records the message it answers and appears only under that one; two messages with the same words are two questions.
- **"Write the rest now" could not reach the rest.** Each run looked only at the ten newest waiting threads, so once those had drafts it found nothing to do, however many more were waiting. It now looks past the ones it has already written — and never writes again for a message you already used or turned down a draft for.
- The home screen said "I've written a reply for each one" when it hadn't, and Settings described replies prepared in advance as covering every thread after an import. Both now say what happens.

### Not in this release

Mail already imported carries no reading of its headers, because the headers are not kept once a message is read: to have it sorted, remove the mailbox or export and add it again. Nothing is left out of **People** yet — a no-reply sender is still listed there. Nothing decides that a thread from a person needs no reply; that is yours to say. A draft left behind by a newer message stays unresolved; it no longer appears anywhere.

## [0.10.0-alpha.3] — 2026-09-22

Real mail. Connect a mailbox, and read MIME properly wherever mail comes from.

### Added

- **Connect a mailbox.** Your email address and an app password, and I read your inbox and your sent mail — the sent folder matters most, because it is the only place your own writing is. I log in and look first, and tell you what I found before anything is read, including when there is no sent folder. Then I check every 15 minutes (5, 30, an hour, or only when you ask — in Settings), and with replies prepared in advance turned on, a reply is waiting before you open the app.
- **Read-only, and only ever read.** Folders are opened with `EXAMINE` and messages fetched with `BODY.PEEK[]`, so nothing is marked as read, moved or deleted; there is no command in the client that could, and the tests fail if one is ever sent. TLS always, against the bundled root certificates; an unencrypted connection is refused before connecting unless it is to this computer (for local bridges).
- **Only new mail, every time.** Each folder remembers where it was left, and moves on only once what it read is in: a check that is canceled or loses its connection imports nothing and loses nothing. A server that renumbers a folder gets it read again, with nothing doubled. A message over 25 MB is skipped without being downloaded, so one enormous attachment cannot stall every check. The first check reads the newest 2,000 messages per folder; older mail can come in from an export.
- **Threads stay threads — across checks, folders and sources.** Your message in Sent and the reply to it in the inbox are one conversation whichever folder is read first. A reply that arrives in a later check joins the conversation it answers. Mail that came in through an export and through the mailbox is stored once, in one thread, so your own writing is not counted twice.
- **A failing mailbox says so, quietly.** The home screen says the last check failed and why; the next try waits an hour rather than a few minutes; routine checks never pop up a notice.
- The password lives with the other credentials, never in the database, and goes when the mailbox is removed.
- Its address becomes one of yours once the login works — unless you untick "this is my own address" for a mailbox you share, so what colleagues sent from it isn't taken as your writing.
- Removing one of two sources that share a thread (an export and the mailbox it came from) removes exactly that source's mail; the other's stays, in its thread, and the confirmation counts only what goes.

### Fixed

- **Mail was stored as MIME.** Almost every real message is multipart, base64 or quoted-printable, and the mbox reader took the raw text as the message — so a Gmail export's "writing" was boundary lines and base64, and the measurements were measurements of that. Mail is now decoded: the plain-text part (or the HTML one, converted), in its charset, with encoded-word subjects and names. **An mbox imported with an earlier version should be removed and imported again.**
- **Settings did not show what you had just set.** Every control on the Settings screen saved correctly and then kept showing the old value — a theme swatch did nothing visible, a checkbox sprang back — until something reloaded the screen. The saved settings now go straight to it.
- The home screen, the mail screen and the caught-up message said "nothing arrives on its own" unconditionally. With a mailbox connected they now say how often it is checked; without one they still say nothing arrives.
- **A guess about a note was told to the model as a fact** (0.10.0-alpha.1). "In this reply they are saying no" was written the same way whether you chose that or I read it from your note; now only your choice is stated, and a reading is passed on as a reading, with your note in charge. "No rush, tell her tuesday works", "no worries, yes please" and "don't decline, say yes" are no longer read as refusals.
- **An mbox with Latin-1 in it failed the whole import** at its first accented byte, and an 8-bit Latin-1 body lost its accents. Mail is now read as bytes and decoded from the charset it declares.
- **One bad job could stop every job after it.** A panic inside a job — for instance on HTML containing certain non-English capital letters, which is also fixed — ended the loop that runs jobs, so nothing ran again until a restart, and the same job re-ran and panicked on every launch. Each job now runs on its own, and a panic fails only that job.

### Not in this release

OAuth. Gmail, iCloud, Fastmail and Yahoo accept an app password. **Outlook.com and Hotmail do not** — Microsoft stopped accepting passwords for IMAP on personal accounts in September 2024 — and neither does a work or school account that allows only single sign-on; the dialog says so and points to an export instead. The IMAP code has been tested against a scripted server on this machine, not against any real provider.

## [0.10.0-alpha.2] — 2026-09-22

The learning loop closes. What you change three times, I start doing myself.

### Added

- **Sent drafts change the next draft.** Every draft you send — edited or not — is compared with what I wrote, for five habits: the greeting, the sign-off, emoji, a full stop at the end, and length. Once three drafts moved a habit the same way, and they outweigh the drafts that went the other way or left it alone, the next draft does it without being told. Two is an anecdote, and so is three against four you left as they were.
- **Per person.** Take the greeting off three drafts to Ada and hers stop having one; everyone else's keep theirs. And a habit you have with one person overrides the one you have with everyone.
- **"Tell me what to do differently"** under a draft is now a field on the page rather than a system dialog, and what you type is kept as a note about whoever the draft was for. Notes reach the model in your words, last, above everything I worked out myself.
- **"What I've learned from you"** under How you write: every pattern that holds, in a sentence ("You took the greeting out of my drafts to Ada 3 times, so I've stopped adding one"), the ones still forming and how far off they are, and every note you've given me with a way to take it back.
- `get_learning_overview`, `add_voice_note`; `LearnedPattern`, `StatedNote`, `LearningOverview`; the `learning.json` fixture parsed by both sides. `GenerationContext` carries the patterns that applied.

### Fixed

- "Why I wrote it this way" asked "What should I do differently next time?" — the label said one thing and the dialog another, and what you typed was recorded against the draft and then never read by anything. It is now labelled for what it does, and what you type is used.

### Not in this release

A pattern changing a measured number. What you wrote and what you changed in my drafts stay separate — the first is measurement, the second is instruction — so the screen can always tell you which is which.

## [0.10.0-alpha.1] — 2026-09-22

Situations. Mimic learns how you say no separately from how you say thanks.

### Added

- **Six situations, and your own messages filed under them.** Saying no, setting a time, apologising, saying thanks, explaining something, disagreeing — seeded by migration 0007 with ids that stay fixed. At the start of every analysis each message you sent (and only those — what other people wrote is not evidence of how you say no) is read against a table of cue phrases, and filed where the evidence clears a threshold. The rules would rather miss a message than misfile one: "sorry to hear" is not an apology, "no thanks" is not a thank-you, and "Thanks," at the bottom of every email is a sign-off rather than a situation. A message can be doing two things at once.
- **A voice layer per situation**, innermost after the relationship layer, with the same twenty-message floor as every other layer. "How you write" shows each one — "When you say no" — with its count, or how far it is from being measurable.
- **Replies know what they are doing.** Tell Mimic "say no, busy that week" and it reads that as saying no, leans on the times you have said no before (to this person first, then to anyone), and says so under the draft: _your note read like saying no_. Or pick the kind of reply yourself from a new, optional control under the note. The draft records which of the two it was, because only one of them is something you said. With no note, nothing is guessed from the other person's message.
- `list_situations`, `SituationSummary`, `SituationChoice`, and two fixtures parsed by both sides: `situations.json` and `generation_context_situation.json`.

### Changed

- Schema v7.
- `voice::resolve` takes the situation; `GenerationContext` carries it; a draft's `situation_id` is now the situation it was actually written for, and its context records where that came from.

### Not in this release

A model deciding situations. It would classify better, and it would mean sending everything you have ever written to it; `situations::classify` is the seam for a local one. Correcting a message's situation by hand: the database already keeps a hand-made call above any rule, and nothing on screen makes one yet.

## [0.9.0-alpha.4] — 2026-09-21

### Fixed

- **Setup could not be scrolled, so most of it could not be clicked.** `body` has `overflow: hidden` and the rule that won for `.onboarding` set `min-height: 100vh` with no scrolling of its own, so a setup screen taller than the window had no scrollbar and everything below the fold was unreachable — which is what adding the writing-engine step to it did. It scrolls now, and aligns to the top rather than centring, because centring a tall page pushes its first step off the screen.
- `.onboarding` was declared twice, the later rule quietly overriding the earlier one. The duplicate is gone, and `layout.test.ts` now fails if any full-height screen is declared more than once, since a rule that exists twice is one nobody can reason about.

## [0.9.0-alpha.3] — 2026-09-21

Setup no longer requires knowing what Ollama is.

### Added

- **Mimic gets the model itself.** `localmodel.rs` observes what is actually on the machine — is anything listening, what does it have, is it the model Mimic wants — and computes the single next action from that: ready, get the host, or pull the model. Pulling is Mimic's job and it does the whole thing, streaming Ollama's NDJSON progress into a real progress bar with gigabyte figures, cancellable between lines rather than after several gigabytes, resilient to a line that does not parse, and never letting a multi-line error from the host reach the screen.
- **Setup is three plain steps and none of them uses a technical word.** Your email address, your old mail, and the part that does the writing. The third runs in parallel rather than in sequence, because making someone watch a two-gigabyte download before they are allowed to go and export their mailbox wastes the one part of setup that takes real time. The same three states appear in Settings, first after Appearance, because "it isn't writing anything" is what brings people there.
- Requests to the model host bypass any proxy in the environment. It is on this machine; a proxy has no business in that path, and on a managed machine it will swallow the request and report the host as absent.

### Changed

- The default local model is `llama3.2:3b` rather than `llama3.1:8b`. Setup now downloads this for people who have never installed one, and two gigabytes on a laptop is a different proposition from five. Anyone who wants a larger one changes it in Settings.

### Not in this release

**Mimic does not download or run the model host's installer.** Fetching an executable and launching it is only safe when the bytes are pinned to a hash that ships with the app, and such a hash goes stale the moment upstream publishes a new build. So that one step opens the official download page in the browser, the person installs it themselves, and Mimic watches and takes over again the moment it appears. One click, no terminal, and nothing executed that the user did not run.

## [0.9.0-alpha.2] — 2026-09-21

One screen. Everything else is behind it, and Mimic says "I".

### Changed

- **The five-item sidebar is gone.** There is one screen — who is waiting, what they said, what Mimic would say back — and People, How you write, Your mail and Settings open in a drawer over it and close back onto it. The routes are unchanged, so every link and deep link still works; they land on the drawer instead of a page of their own. A sidebar asks someone to choose where to go before they have seen anything, and on this app the answer is always "the replies".
- **Mimic writes in the first person.** "I'd say", "I haven't written this one yet", "I never send anything". The buttons say what they do in the words a person would use: Use this, Change it, Not this one. Nothing on screen says direction, corpus, provider or profile any more — the model badge says "Writing on this computer" or "Writing at Claude", which is the fact that actually matters, and the engine badge says whether it is working rather than naming a sidecar.
- **`describeWaiting` replaces `describeFeed`.** It counts people only when every waiting thread really is one person: a group is not a person and an unattributed thread is nobody, so it steps back to "conversation" rather than guessing. With nothing imported it says so first, before anything else on the screen.
- A thread with no draft now takes a line of shorthand and writes from that, rather than only offering a bare "Draft a reply" — and when Mimic has not seen enough of your mail with someone to know how you write to them, it says so there instead of drafting as if it had.
- Escape closes the drawer, and there is a "Back to replies" button, because a panel dismissible only by a small × is a panel people get stuck in.

## [0.9.0-alpha.1] — 2026-09-21

A design system with an idea in it, and three themes that are actually different.

### Added

- **The serif carries the writing.** Anything that is a piece of writing — a message someone sent you, a reply Mimic drafted, an example of how you put things — is set in IBM Plex Serif and reads like a letter. Everything around it, the buttons and labels and counts, is IBM Plex Sans and reads like a tool. That one distinction is what the rest of the look is built on, so it lives in the primitives as `.letter` rather than in any one screen.
- **Three themes: plain, paper, night**, all from the same token names. Plain is the default: warm grey, near-black, one deep blue, and type large enough to read without leaning in. Paper drops boxes entirely — a card there is a rule and some air — which the `--card-border` and `--card-padding` tokens carry, so no component branches on the theme name. Night is the same layout on near-black. Chosen in Settings by looking at three swatches painted in their own colours rather than by picking a word from a list.
- Both faces are bundled with the app rather than fetched. A webfont request to a CDN would be the one thing on screen contradicting the promise that nothing leaves the computer.
- Migration 0006 carries an older install's theme over: `dark` becomes `night`, `light` becomes `plain`, anything else becomes `plain`. Without it the first render after an upgrade fails contract validation on a value the app itself wrote.

### Changed

- Every button a person is meant to press is at least 44px tall, radii dropped to 2–4px, and shadows are gone except on things that genuinely float. A badge is a quiet outline that colours its own text, not a filled pill competing with the primary action.
- An empty state is the page rather than a dashed box floating on it.

## [0.8.0-alpha.3] — 2026-09-21

The window stops scrolling away from you, and the controls stop looking like someone else's app.

### Fixed

- **The whole shell scrolled instead of the page.** `.main` had no `min-height: 0` and the grid had no explicit row, so a long page grew the shell past the window and the first things to leave the screen were the sidebar's navigation and the top bar — the two things that exist to stay put. The shell is now exactly the window, and `.content` is the only thing that scrolls.
- **Drop-downs rendered as white Windows combo boxes** in a dark app, because a bare `<select>` was never styled. Every text control — `select`, `textarea`, and the text-like `input` types — now carries the same surface, border and focus ring whether or not the markup remembered to ask for it, with the arrow drawn back in the text colour after `appearance: none` removed the system one.
- **Nothing shared an edge.** Pages had no measure, so on a wide window a 520px centred empty state sat above a full-width two-column form. Page content is now held to a single 1160px measure, and an empty state no longer adds a screen-high margin above whatever follows it.
- Two fields side by side now share the width instead of each taking whatever its widest option needed.

## [0.8.0-alpha.2] — 2026-09-21

Onboarding no longer holds the app shut until a mailbox has been exported.

### Fixed

- **"Look around first"** on the source and import steps. Until now the only way past them was `canFinishOnboarding`, which requires messages of your own to already be imported, so a first run with nothing imported had no button on it at all — the app could not be looked at before committing to exporting a mailbox. Leaving early is now allowed from any step after identity, and the link says what to expect: an empty dashboard, an empty People and an empty Voice, with setup waiting under Sources.
- Identity stays mandatory, and the reason is recorded next to the check rather than implied: the importer decides each message's `direction` by matching it against the declared identifiers, so an import run before one exists attributes nothing to anyone and produces a corpus with no evidence of the user's writing in it.

## [0.8.0-alpha.1] — 2026-09-20

A home screen. Mimic opens on what is waiting for a reply, with the draft it has written for each one, and approve / modify / reject on every draft. Compose is folded into it rather than being a screen of its own.

### Added

- **Dashboard**, the new home screen. Threads whose last message came from someone else and was never answered, newest first, each with the message in full, who it is from, whether a draft would be shaped by how you write _to them_ or only in general, and any draft Mimic already has. Counts of what has been imported, and when the last import ran.
- **Approve, modify, reject.** Approving copies the reply and records it as sent unedited; modifying records what you changed, which is the only thing the learning loop can learn from; rejecting discards it. "Tell Mimic why" records a stated preference, which outweighs anything inferred from an edit. None of these send: Mimic has no send path, and adding one stays a separate decision.
- **Assisted drafting** (`assist.autoDraft`, off by default): with it on, Mimic drafts a reply for each waiting thread after every import or analysis, and the dashboard shows them waiting for approval. Bounded to ten threads per run, never a thread that already has an unresolved draft, cancellable between threads, and it never runs while the setting is off — a disabled run reaches no provider at all. The Settings copy says plainly that this sends incoming messages to the configured model without being asked each time, and names whether that model is local.
- `threads_awaiting_reply` and its count: a conversation is waiting because its last message has `direction = 'other'`, not because anything inferred urgency. Messages whose direction could not be established never make a thread look answered either way.
- `fixtures/contracts/dashboard.json`, written by the Rust end-to-end test and parsed by the zod suite, including a thread carrying a prepared draft.

### Changed

- Navigation is Dashboard / People / Voice / Sources / Settings. The Compose screen is now the "Write something new" panel on the dashboard; the intent field is still the largest input on it.
- The dashboard describes itself honestly: nothing arrives on its own, every message on it came from an import, and with nothing imported it says so rather than showing an empty feed.
- `fixtures/import/sample_export.json` gained a trailing inbound message so the corpus actually contains a thread awaiting a reply (46 messages, 45 attributable).

### Not in this release

A send path. Mimic drafts; you send. Everything here is built so that turning that on later is one deliberate change rather than a slide.

## [0.7.0-alpha.1] — 2026-09-20

The first run works. 0.6.0 built the pipeline and could be walked through by someone who knew where the walls were; this release is about a person installing Mimic, opening it, and getting to a draft without being stranded or misled.

### Fixed

- **Onboarding no longer sits on "Importing…" forever.** Native job events were subscribed inside the application shell, which onboarding does not render, so a finished import or analysis never reached the screen that was waiting for it. The only way past was to quit and reopen the app. The subscription now lives above the gate, and onboarding shows the running job's progress.
- **An import that finds nothing of yours says so.** When the file is read and not one message matches a declared identifier — the likeliest first-run mistake, and the one the add-source dialog warns about — the step used to render a paragraph and no button at all. It now names the addresses it looked for, takes another one inline, and re-reads the file (which costs nothing, because messages are keyed on their own identifiers).
- **A small mailbox is no longer a lock-out.** Finishing onboarding required a measurable voice profile, which needs twenty of the user's own messages; below that, the last step re-rendered forever. Anyone with their own messages imported can now leave onboarding, and Compose already says when it is drafting without a measured style.
- **Adding a second address no longer throws you forward.** The identity step is left on a click rather than on the first identifier appearing.
- **A failed import offers to retry or to remove the source**, rather than leaving the step in the same shape as a successful one.

### Changed

- **The model badge stops claiming a model is there.** The local provider is always registered, so the top bar showed a green "Local model" on a machine with nothing listening on the endpoint, and every draft failed with a toast underneath it. The badge now reflects a real reachability check, and until that check has run the answer is "unknown", not "fine".
- **Compose refuses to draft when the model is not answering**, and says which one and why. `composeReadiness` takes the provider's state and is the one place that decides; the button is disabled rather than pretending.
- Settings gained the three things it configured but could not do: create a diagnostics bundle (the "include full paths" checkbox now has something to affect), restart the engine when it is not running, and check for or install an update. The update badge in the top bar links to that section instead of to the top of the page.
- The Voice empty state links to Sources instead of describing where to go.
- `PeoplePage` uses the shared `MIN_SAMPLE` rather than its own copy of `20`.
- `scripts/test.ps1 -Quick` now means something: it skips the production frontend build, which is what `validate.ps1` without `-Full` advertised and did not do.
- Stale text removed: `CONTRIBUTING.md` no longer asks for a Lightroom capability matrix or names the bridge and EditDNA contracts; the Tauri capability description no longer mentions an asset protocol that is disabled; the install guard's comment names imports and analyses rather than photography jobs.

### Packaging

- The release job installs the engine's Python environment **before** `cargo test --workspace`. It ran after, so the four engine-protocol tests — the ones that exercise the thing the release ships — skipped themselves on every release build while CI ran them properly.
- The release job now fails if the bundle was built without a packaged engine, or if the installer is implausibly small to contain one. The previous failure mode was an installer that opened to a permanent "Engine unavailable".
- `models/manifests/` is no longer bundled: it contains one README and no encoder ships in this version.
- `package-engine.ps1` writes back the tracked `resources/engine/README.txt` it deletes, so packaging locally no longer shows up as a deleted file.

## [0.6.0-alpha.2] — 2026-09-20

Release-pipeline fix only; application code is identical to 0.6.0-alpha.1 (whose Release workflow never produced an installer).

### Fixed

- `scripts/package-engine.ps1` still copied `packages/contracts/edit_mapping_v1.json` into the packaged engine. The migration moved that file to `archive/legacy-photography/contracts/`, so the "Package engine" step failed with `Cannot find path ... edit_mapping_v1.json` after PyInstaller had already succeeded, and no draft release was ever created. The copy is gone, along with the `rawpy` binary collection and the `PIL._tkinter_finder` hidden import — both left over from the image pipeline, neither a dependency of the text engine.

## [0.6.0-alpha.1] — 2026-09-20

**Mimic is now a different product.** It no longer learns how you edit photographs in Lightroom Classic. It learns how you communicate, from messages you have already written, and helps you draft replies that sound like yourself.

Versions 0.1.0 through 0.5.0 were a Lightroom Classic editing assistant. Everything below describes retiring that product and building the first working slice of this one. `docs/MIGRATION_AUDIT.md` records the whole decision, component by component.

### Removed

- The photography domain in full: `edit_dna` (Lightroom develop-setting normalization), `bridge` (the loopback HTTP server the Lightroom plugin talked to), `capability` (the runtime SDK probe), `sessions` (shoots, scene clusters, predictions, apply batches, restores), `ingest` and `training` in their photography form, the Python image pipeline (XMP parsing, RAW previews, EXIF, visual features, scene heuristics, the hybrid KNN + ridge trainer), and roughly 5,500 lines of photography UI.
- Nineteen database tables, dropped by migration `0005`: `libraries`, `assets`, `sidecars`, `edit_snapshots`, `visual_features`, `style_profiles`, `style_profile_libraries`, `sessions`, `session_assets`, `scene_clusters`, `training_sets`, `model_versions`, `model_artifacts`, `predictions`, `apply_batches`, `applied_edits`, `corrections`, `correction_syncs`, `lightroom_connections`.
- Dependencies: `axum`, `tower`, `tower-http` and `reqwest` (dev) on the Rust side; `pillow`, `exifread` and the `rawpy` extra on the Python side; the Tauri `protocol-asset` feature and the asset protocol itself.
- The `plugin` CI job, the Lua version targets in `sync-version.mjs`, the plugin zip in the release workflow, `install-lightroom-plugin.ps1` and `test-plugin.ps1`, the synthetic image fixtures, and the image encoder manifests.

### Archived

Moved to `archive/legacy-photography/`, excluded from every workspace, from CI and from the bundle: the Lightroom Classic plugin and its tests; `edit_mapping_v1.json` with `EDIT_DNA.md`; the Lightroom integration and capability-matrix documents; `ML_PIPELINE.md`, for the shoot-grouped split reasoning that the new evaluation harness reuses; ADRs 003, 004 and 005; and the golden XMP corpus that proved the mapping. None of it is built or shipped.

### Changed

- `mimic-core` is now database, sources, import, voice, retrieval, generation, providers, privacy and jobs. The SQLite layer, the migration mechanism, the job queue, the engine sidecar client, `paths`, `ids`, `version` and `diagnostics` carry over unchanged in substance.
- The Python engine is a text engine: embeddings, similarity and held-out evaluation. The NDJSON server, dispatch, progress and error envelopes are untouched.
- Navigation is Compose / People / Voice / Sources / Settings. Compose is the home screen.
- Diagnostics report providers instead of a Lightroom bridge, and strip `token`, `apiKey`, `api_key`, `password` and `secret` at every depth.
- The app data layout loses `cache/previews`, `models/styles`, `bridge/` and `plugin/`, and gains `credentials/`.
- `docs/ARCHITECTURE.md`, `PRIVACY.md`, `PROJECT_STATUS.md` and `ROADMAP.md` were rewritten from scratch; `DATABASE.md`, `TEST_STRATEGY.md` and `UI_SPEC.md` were replaced by `DATA_MODEL.md`, the testing section of `ARCHITECTURE.md`, and `PRODUCT.md`.

### Added

- **Schema v5** (`0005_communication.sql`): user identity and identifiers; sources; participants and their identifiers; conversations, conversation participants and messages; message embeddings; situations; layered voice profiles, manual preferences and representative examples; drafts and draft feedback; analysis runs; evaluations and evaluation cases. Indexed for 100k–1M+ messages. A v4 photography database upgrades cleanly, keeping settings, jobs and events.
- **Source connectors** behind one `CommunicationSource` trait that streams conversations to a sink: `mbox` (RFC 4155, reference-chain threading with a narrow subject fallback, separator detection that does not split on a body line beginning with "From ", content-derived ids when `Message-ID` is missing) and `mimic_json` (the documented generic format). Validation is a dry run of the connector's own import, so it cannot disagree with what the import will do.
- **Normalization** that removes quoted history, attribution lines in four languages, forwarded banners, Outlook reply blocks and signatures — while keeping sign-offs the user typed, because "Thanks, C" is how someone writes and a `--` block is not.
- **Import**: identity-based direction (`self` / `other` / `unknown`, never guessed), participant resolution through a cache, batched inserts of 500 in a transaction, reply linking and response latency derived per conversation, cancellation between conversations, and a resume that costs nothing because `(source_id, external_id)` is unique. Refuses to run when no identity is declared.
- **The voice engine**: deterministic metrics over the user's own messages (length distribution, terminal punctuation, capitalization two ways, emoji, contractions per hundred words, greetings, sign-offs, repeated phrases, response latency), computed per layer — global, channel, relationship — with a 20-message floor below which a scope reports its sample size and no rates. Representative examples chosen deterministically and de-duplicated.
- **Retrieval** that filters on participant, channel, relationship, situation, conversation, source and date range before ranking, and ranks lexically with inverse document frequency.
- **Generation**: a context builder that gathers evidence, a prompt assembler that is a pure function of that context, measured habits turned into instructions rather than quoted as numbers, an output budget derived from the user's own p90 message length, and four adjustments.
- **Model providers** behind one trait: a local OpenAI-compatible endpoint (Ollama, LM Studio, llama.cpp) whose locality claim is computed from the URL rather than asserted, the Claude Messages API, and a deterministic mock for tests. The default is always a local provider.
- **Deletion that deletes**: a preview produced by the same code path as the deletion, removal of the user's own half of a one-to-one conversation, survival of group threads minus that person, invalidation of every aggregate computed over the removed material, source deletion, and a delete-everything that keeps settings and identity.
- **The learning loop's recording half**: draft, what was actually sent, a described diff, and weights in which a stated preference outranks an inferred edit three to one.
- **A held-out evaluation harness** in the engine: conversation-grouped splitting so no thread straddles the boundary, and comparison along named components — length, vocabulary, punctuation, embedding — with **no headline score**.
- **The interface**: Compose with the intent field as its centre and an evidence panel beside every draft; People with per-person counts and a deletion dialog that states consequences in sentences; Voice, which shows what was measured and what was not; Sources with pre-import validation; Settings covering identity, provider, privacy and deletion.
- **Cross-language contract fixtures**: the Rust end-to-end test writes `fixtures/contracts/`, the zod suite parses them, and a shape change on one side that is not made on the other fails a test.

### Migration notes

- **A v4 database loses its photography data.** That data describes a product that no longer exists. `Db::open` writes a timestamped backup to `data/backups/` before the migration runs, so it is recoverable if anyone needs it.
- Settings, jobs, the event log and update state survive. `review.highThreshold`, `review.mediumThreshold`, `performance.accelerator`, `performance.previewCacheMaxMb` and `performance.inferenceBatchSize` are gone; `generation.provider`, `generation.localUrl`, `generation.localModel` and `generation.anthropicModel` are new.
- The Lightroom plugin is no longer installed or updated by the app. An existing copy under `%LOCALAPPDATA%\Formicaria\Mimic\plugin\` is left alone rather than deleted, and can be removed by hand.
- Anyone who wants the photography product should use the `v0.5.0-alpha.1` tag; it is the last release before this one and the full record of what was removed.

### Known limitations

- **No accuracy figure anywhere**, because the evaluation harness has not been run over a real corpus and neither baseline is implemented. This is deliberate; see `docs/VOICE_ENGINE.md`.
- Embeddings are `lexical_v1` — hashed word and character n-grams. Genuinely useful, not semantic, and the engine reports `semantic: false` so the app cannot imply otherwise.
- The situational voice layer has a schema, a resolution path and a prompt slot; nothing classifies into it yet.
- Provider credentials live in an owner-only file, not the OS credential store.
- Voice analysis materializes a scope's messages in memory before computing. Fine at a hundred thousand; not at a million.
- The Anthropic provider has never been exercised against the live API in CI.
- `tauri build` has not been run in this environment, so the Windows installer and the signed updater path are unverified for this release.
- The nightly smoke workflow was rewritten against the packaged engine but has never been observed running.

## [0.5.0-alpha.1] — 2026-09-17

Session intelligence: reference photos, group editing, per-group and per-camera confidence, and group-outlier flags.

### Added

- Schema v4 (`0004_session_intelligence.sql`): `scene_clusters.reference_asset_id` (FK to assets) and `edited_at`; the seeded upgrade test now runs v1 → v4.
- Engine: `model.predict` accepts `references {groupId: assetId}`; the consistency policy pulls a group toward its reference (blend 0.8, same per-family caps) and never changes the reference itself (`isReference`, `consistencyShift = 0`); `detect_outliers` flags photos whose exposure, temperature or tint sits > 12 % of range from their group's median (groups ≥ 4) as `groupOutlier` with a reason line, judged on raw predictions before blending.
- mimic-core `sessions`: `edit_groups` (rename; set/clear a member-only reference; merge with the target keeping label and reference; move photos into an existing or new group with emptied sources deleted and orphaned references cleared), every edit stamping `edited_at`; `predict_session` passes group references and counts outliers; `session_detail` gains `groupStats` (photos, predicted, mean/min confidence, low-confidence, unfamiliar, outliers, applied, rejected per group), `cameraStats` (photos, mean confidence, known-to-model per camera/lens), `groupingChangedSincePrediction` and `syncSuggested`.
- Command `edit_session_groups`; `GroupEdit` tagged-union contract and `session_detail.json` fixture round-tripped in Rust and zod (the fixture caught a snake_case field leak).
- UI: Scene groups table with inline rename, "use selected as reference", two-step merge, move/split of the multi-selected photos (Ctrl/Cmd/Shift-click in the grid); cameras/lenses table when a session mixes bodies or uses one the model has not seen; banners when groups changed after the last prediction and when a corrections sync is due; outlier badge and reference star on tiles; Review's attention queue includes group outliers.
- Tests: pytest reference/outlier unit tests and service-level reference assertions; repository tests for every group edit; `sessions_e2e` extended with rename → split → invalid reference → merge → predict → reference → stale flag → re-predict; GroupsPanel component tests; contracts tests.

## [0.4.0-alpha.2] — 2026-09-17

Release-pipeline fix only; application code is identical to 0.4.0-alpha.1 (whose Release workflow never produced an installer).

### Fixed

- `scripts/package-engine.ps1` smoke check treated the two-line stdio reply (`engine.hello` + `engine.shutdown`) as a failure: PowerShell's `-notmatch` on an array returns the non-matching lines rather than a boolean. The reply is now joined before matching, and a non-zero exit of the packaged engine is reported on its own. This is why the 0.3.0-alpha.1 and 0.4.0-alpha.1 release jobs failed at "Package engine" although the bundle worked.

## [0.4.0-alpha.1] — 2026-09-17

Continuous learning: Mimic now reads applied photos back after your own pass in Lightroom, keeps what you changed as corrections, measures the No-Touch Rate from what you left alone, and trains the next version on those corrections. Proven against a scripted plugin over the real bridge; real-Lightroom behaviour remains unverified.

### Added

- Schema v3 (`0003_corrections.sql`): `correction_syncs` (one row per sync: checked / untouched / corrected / unresolved) and a unique `corrections(prediction_id)`; the migration test now upgrades a seeded v1 database through every version.
- mimic-core `corrections`: `sync_corrections` job (same catalog required; photos resolved like apply; `collect_correction_state` in chunks of 25; keys Mimic wrote compared with read-back tolerances; per-control normalized deltas and magnitude; `correction` edit snapshot with the photographer's final settings; re-sync replaces an unused correction and keeps one already used by training), `no_touch_stats` (per version, synced sessions only, restored edits excluded), `style_health` (active No-Touch, corrections pending training, most-corrected controls with signed bias, computed insight sentences).
- Training with corrections: `train_style` passes the Style's pending corrections as `correctionAssetIds`; the engine's dataset loader adds those assets only when their latest snapshot is a `correction`, groups them as their own shoot, reports `correctionPairs`; the job marks them `included_in_training_version`.
- Commands: `sync_corrections`, `get_style_health`, `list_corrections`; `StyleSummary.noTouchRate`; `StyleDetail.health`; `SessionDetail.correctionSyncs`; zod contracts with fixtures shared with the Rust round-trip tests (`style_health.json`, `correction_row.json`) and a `collect_correction_state` bridge fixture.
- UI: Style Corrections tab (No-Touch per version, insights, most-corrected controls, correction list with trained/pending state, honest empty state), Versions tab side-by-side comparison (overall and per-family holdout error, measured No-Touch), session _Sync corrections_ button with reasons and a sync history table, Home and active-version cards show the measured No-Touch Rate or “—”.
- Tests: pytest dataset test for correction assets, Rust unit tests for the diff and health, `sessions_e2e` extended with sync → idempotent re-sync → retrain consuming the correction → clean failure on a session without applies, component tests for the Corrections panel and version comparison.

## [0.3.0-alpha.1] — 2026-09-17

Sessions, scene grouping, prediction with confidence, Review, and the Lightroom apply/restore path with read-back verification. Everything that touches a catalog is proven against a scripted plugin over the real bridge; behaviour on a real Lightroom Classic is still unverified.

### Added

- Schema v2 (`0002_sessions.sql`): `libraries.purpose` (training vs session-backing libraries, hidden from the Libraries UI), `applied_edits.restored_at / restore_result / restore_error_json`, `predictions.capability_schema_version / cluster_id`; migration test seeds a v1 database with rows and upgrades it; the engine's test database builder now applies every checked-in migration.
- Engine `session.group`: capture-time blocks (20 min gap, untimed frames share one block), seeded k-means on standardized visual statistics (+ embedding when present) with a minimum cluster size, burst detection, deterministic output. Engine `model.predict` gains `groups` + `consistency`: a bounded per-family pull toward the scene-group median (white balance, colour, presence); exposure and tone are never blended, the maximum shift is reported per photo.
- mimic-core `sessions`: `create_session` (folder or Lightroom scope), `ingest_session` (reuses the folder/Lightroom ingest, capture-ordered membership), `group_session`, `predict_session` (active version only, feature-schema check, supersedes earlier predictions, records the Lightroom capability schema version), review transitions, `apply_preflight` (connected, canApply/canSnapshot, writable controls, same catalog, stale capability schema, apply already running), `apply_session` (photo resolution by local id or normalized path against the catalog listing; batches of 25 with `Mimic Before` snapshot and read-back; `verify_readback` per item; `prediction` edit snapshot on success; cancellation between batches; `outcome_unknown` recorded when the bridge fails mid-batch), `restore_batch` (before-values of the written keys only, read-back verified, per-item restore result, predictions back to pending). Apply and restore are never re-queued after an interruption.
- Commands: `list_sessions`, `create_session`, `get_session_detail`, `list_session_photos`, `set_session_style`, `delete_session`, `group_session`, `predict_session`, `set_prediction_review`, `get_apply_preflight`, `apply_session`, `list_applied_edits`, `restore_apply_batch`, `get_prediction`; typed contracts with checked-in fixtures (`fixtures/sessions/*`) asserted by Rust round-trip and zod tests.
- Sessions UI: list, New Session dialog (folder or Lightroom scope, optional Style), detail page with Analyze scenes → Predict → Apply to Lightroom (each disabled with a reason), live job card, metrics, per-group and needs-attention filters, confidence badges on every tile, prediction panel (predicted Lightroom values, confidence components and reasons, Lightroom outcome with read-back mismatches, Looks right / Reject / Apply this photo), apply history with Restore, confirm dialog that states the safety steps and shows the backend's blockers verbatim.
- Review UI: attention-only default (below the medium threshold, unfamiliar, failed apply), All pending, Every prediction; filmstrip, large preview, prev/next, the same prediction panel; works offline, Apply requires Lightroom.
- Bridge fixtures for the restore payload and the catalog listing; Lightroom integration, ML pipeline, database, architecture and UI docs updated.
- Tests: pytest session suite (grouping determinism, time gaps, visual split, untimed frames, consistency policy, service-level `session.group` + consistent predict), Rust `tests/sessions_e2e.rs` (real engine ingest of fixture images, grouping, prediction, re-prediction supersedes, rejected photo excluded, apply refused without Lightroom, apply against a scripted plugin with one read-back mismatch and one missing photo, stale-capability refusal, restore with verification), migration upgrade test, repository tests for restore bookkeeping and session delete, frontend tests for the prediction panel, confidence badge, confirm dialog and review queue.

### Changed

- Home and Settings no longer describe Sessions/Review as future work; the unhonoured “create a snapshot before applying” toggle was removed — snapshot + read-back are mandatory and stated as such.
- `apps/desktop/tsconfig.tsbuildinfo` is no longer tracked (it is a build cache and blocked fast-forward pulls).
- Styles with predictions cannot be deleted (apply history references their versions); delete the sessions first. The UI reports the reason.

## [0.2.0-alpha.1] — 2026-09-17

First Style Brain. Training is real, reproducible and measured; prediction is exposed through the engine and exercised end to end, but there is still no Sessions/Review UI or Lightroom apply (0.3.0).

### Added

- Training pipeline in the engine (`training.train`): training-set builder over normalized EditDNA pairs with filters (no snapshot, no features, no meaningful edits, too many unknown keys), session-grouped train/validation/holdout split that never puts one shoot on both sides (deterministic per seed; honest fallbacks for two shoots and single-shoot libraries), baselines (global median, camera/lens-conditioned median, distance-weighted KNN), per-control ridge residual on top of leave-one-out KNN (`hybrid_knn_residual`), per-control and per-family metrics in raw and normalized units (MAE/RMSE/nMAE/p50/p90/p95), acceptance proxy explicitly labelled as not a No-Touch Rate, baseline comparison, reproducible training config (seed, versions, fingerprint, dependency versions), SHA-256 hashed artifacts.
- Confidence calibration persisted with every model: unseen-photo neighbour distances, family validation error, camera/lens/ISO coverage; per-photo confidence with stored components, out-of-distribution flag capped at 0.49, and plain-language reasons.
- Prediction method (`model.predict`) returning canonical settings (normalized + raw), nearest training examples, raw component outputs and confidence per asset.
- `train_style` job in mimic-core: immutable `model_versions` row created in `training` state, finalized once with metrics + artifact manifest, training set recorded, artifacts registered; activation policy — first version activates, later versions activate only when holdout error is not worse than the active one; failed runs leave a `failed` row with the structured error. Composite job executor and engine progress forwarding into job records.
- Commands: `train_style`, `activate_model_version` (rollback), `archive_model_version`, `get_model_version`; Style detail reports training availability with the exact reason.
- Styles UI: Train New Version (enabled only when data, engine and no running training allow it), live training progress, active-version card with holdout nMAE, Versions tab with real metrics (overall nMAE, exposure MAE in EV, evaluation set, beats-median) and Activate/Archive; Home shows holdout error; onboarding gains a Train step with phase-based progress and the resulting metrics.
- Tests: pytest training suite on a synthetic database built from the real migration (dataset filters, leak-free deterministic split, reproducibility, hybrid beats global median by a wide margin on exposure, prediction + OOD behaviour, insufficient-data failure); Rust end-to-end training test with the real engine through the job runner (versions, activation policy, rollback, prediction, insufficient data precheck); VersionList component tests; contracts tests for metric helpers.

### Fixed

- Engine exit code was read with `try_wait()` immediately after stdout closed and came back `None` on Windows; the client now awaits the real exit status.

## [0.1.0-alpha.1] — 2026-09-16

Foundation + real ingest. This is a pre-release: the ingest pipeline is real and tested end to end; training, prediction and Lightroom apply are not part of this build.

### Added

- Tauri 2 desktop shell with dark graphite theme, onboarding flow (Lightroom / folders / clearly labelled DEMO), Home, Styles (list, detail with Overview/Training Data/Versions/Corrections), Sessions and Review empty states, Settings (General, Lightroom, Performance, Storage, Privacy, Updates, Diagnostics).
- SQLite database with forward-only transactional migrations, automatic backup before migrating an existing database, WAL, foreign keys, and all 21 tables from the data model.
- Persistent job system: queued/running/completed/failed/canceled/interrupted, heartbeat, item-level progress, cancellation between items, restart recovery that re-queues only resumable jobs.
- Python engine sidecar (`mimic-engine serve`): NDJSON stdio protocol with request correlation, structured errors, progress events, 32 MiB message cap; automatic restart with a bounded budget.
- Folder scanner: RAW/rendered detection, XMP/ACR pairing by directory + basename, duplicate-basename and orphan-sidecar reporting, fast identity hash, EXIF extraction.
- Read-only XMP parser (`xmp_parser_v1`): attribute and element forms, curves, structured masks and Look tables, unknown key preservation, metadata summary, malformed-file isolation.
- RAW preview decoding (LibRaw via rawpy, embedded preview first) with an on-disk cache; deterministic image statistics (`features_v1`), heuristic scene labels, `stats_v1` embeddings stored as `.npy`, ONNX encoder manager with SHA-256 verification and mandatory fallback.
- Canonical EditDNA (`edit_mapping_v1`, 104 controls across 11 families) shared by Rust, Python and TypeScript, with golden fixtures for modern (PV 15.4 with masks and unknown keys), PV 2012 and legacy PV 2010 XMP.
- Lightroom Classic plugin (`Mimic.lrplugin`): discovery-file handshake, long-poll command loop, runtime capability probe, catalog listing, develop-settings read, before-snapshot, plugin-preset apply with read-back, correction-state collection, Plugin Manager panel, dependency-free Lua JSON.
- Loopback-only bridge server with per-launch 256-bit token, body limits, origin rejection, command queue with timeouts, liveness sweep and reconnect handling; fake-plugin integration tests.
- Capability matrix derived from the probe: supported / observed-not-writable / unsupported per control; apply and snapshot gated on runtime flags.
- Data quality report: assets, valid pairs, missing edits, Lightroom-connected vs sidecar-only coverage, ACR heavy-edit count, local-edit count, camera and shoot-day distribution, recommendation level, honest warnings.
- Diagnostics bundle with no tokens and redacted paths; structured JSON logs with daily rotation.
- Updater plumbing: Tauri updater with embedded public key, 6-hour jittered background checks, install guard that refuses while jobs run, persisted update state; GitHub Actions release workflow that builds, signs, generates `latest.json` and checksums, and creates a draft release.
- CI: frontend, Rust (with the real-engine end-to-end test), Python, Lua plugin, security audit and secret scan.

### Known limitations

- Training (0.2.0), sessions/prediction/review/apply (0.3.0) and correction sync (0.4.0) are not implemented.
- The plugin apply path is verified against fixtures and a fake plugin only: NEEDS REAL-LIGHTROOM QA.
- The updater configuration ships a development public key; a non-alpha release is refused by `verify-release.ps1` and the release workflow until it is replaced.
