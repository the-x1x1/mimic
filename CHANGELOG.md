# Changelog

All notable changes to Mimic are documented here. The format follows Keep a Changelog; versions follow SemVer with pre-release tags for alpha/beta builds.

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
