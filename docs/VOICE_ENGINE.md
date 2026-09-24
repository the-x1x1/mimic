# The voice engine

How Mimic works out how someone writes, how those findings reach a prompt, and how any claim about accuracy would have to be earned.

## Layers, not an average

A person does not have one writing style. They have a default and a set of adjustments they make without thinking: longer to a client than to a brother, a full stop at the end of an email and none at the end of a text, an apology that sounds nothing like a refusal.

Modelling that as one averaged profile produces a voice nobody has — the mean of "Dear Dr Okafor" and "lol ok". So Mimic computes four layers independently and resolves them outermost to innermost at generation time:

| Layer            | Scope                        | Status                                                                             |
| ---------------- | ---------------------------- | ---------------------------------------------------------------------------------- |
| **GLOBAL**       | everything the user wrote    | implemented                                                                        |
| **CHANNEL**      | per channel (email, sms, …)  | implemented                                                                        |
| **RELATIONSHIP** | per person                   | implemented                                                                        |
| **SITUATIONAL**  | per situation (declining, …) | implemented — filed by rule, by a model on this computer or by the user, see below |

Resolution: global, then channel, then relationship, then situation, then the user's manual preferences, which beat everything. `voice::effective_metrics` folds them so the innermost layer that measured a given metric wins it, and an inner layer that measured nothing does not erase what an outer one knows.

Plus, at every layer, **retrieved examples**: the user's own past messages, chosen by similarity and filtered by metadata, quoted into the prompt. A measured statistic tells the model what to aim for; an example shows it.

## What is measured, and how

Every metric below is arithmetic over text. No model is involved and none is needed — "how long are this person's messages" is a counting problem, and counting is both cheaper and more honest than asking a language model to estimate it. All of it lives in `crates/mimic-core/src/voice/metrics.rs`, computed in exactly one place.

**Length** — `avgWordsPerMessage`, `medianWordsPerMessage`, `p90WordsPerMessage`, `avgSentencesPerMessage`, `multiParagraphRate`. A word is a whitespace-separated run containing at least one alphanumeric character, so stray punctuation does not inflate the count. The median and the p90 matter more than the mean: one long message moves the mean and tells you nothing.

**Punctuation** — `terminalPeriodRate`, `questionRate`, `exclamationRate`, `ellipsisRate`. Taken from the last meaningful character, skipping trailing emoji and quotes, so "on my way. 🚀" still counts as ending with a full stop.

**Capitalization** — `lowercaseStartRate` (first alphabetic character is lowercase) and `allLowercaseRate` (no uppercase anywhere). Two different habits: "yes but ONLY on Tuesday" starts lowercase and is not all-lowercase.

**Emoji** — `emojiRate`: the share of messages containing at least one. Range-based detection rather than a dependency; the exact boundary matters far less than being stable.

**Contractions** — `contractionsPer100Words`. An apostrophe between two letters, typewriter or typographic. A possessive `dogs'` is not one.

**Greetings and sign-offs** — `greetingRate`, `signOffRate`, `topGreetings`, `topSignOffs`. A greeting is recognized only in the opening clause of the first line; a sign-off only in a final line of five words or fewer. "I said hi to her yesterday" is not a greeting, and a long closing sentence containing "thanks" is not a sign-off.

**Phrases** — `topPhrases`: word 3-grams, lowercased, stopword-only grams dropped, seen at least three times. A phrase seen once is a sentence; seen three times it is a habit.

**Response timing** — `medianResponseSeconds`, over messages where both timestamps exist and the reply crosses a direction boundary.

### Two rules that govern all of it

**Twenty messages, or nothing.** Below `MIN_SAMPLE = 20` of the user's own messages in a scope, the profile is written with its sample size and every rate `null`. Rates over a handful of messages are noise wearing a decimal point.

**`null` is not zero.** `emojiRate: 0.0` means this person does not use emoji — a fact, and an instruction to the prompt. `null` means we have not seen enough to say. The distinction survives from the Rust struct through the zod schema to the rendered sentence, and there is a test at each layer that it does.

### Counting a page at a time

A scope is never held in memory whole. `voice::metrics::Accumulator` takes one message at a time (`push`) and gives the metrics at the end (`finish`); `metrics::compute` over a list is the same arithmetic. Word counts are kept as a histogram — a message is some whole number of words — so the median and the 90th percentile are exact without keeping every length. Phrase counts are the one table that grows with the text: past 200,000 distinct phrases the rarest are forgotten until half the room is free (seen once first, then twice). Below that, a few thousand messages in a scope, every phrase is counted exactly; above it, a habit, which recurs, survives.

Analysis reads each scope twice, a page of 1,000 messages at a time: once for its numbers, once to score each message against them for the representative examples (below).

### Measuring only what changed

Every message is in the layer over everything, so that layer is read whole whenever anything of the user's changed; the rest are measured again only when something in them did.

- **An import, or a mailbox check, marks stale** what the conversations it put a message in could move — when such a conversation holds a message of the user's: the layer over everything, the channels of the user's messages there, the people in it, and the situations those messages are filed under (`Db::mark_conversations_changed`). A new message changes the replies around it too (how long the user took to answer), so a conversation counts when anything in it changed. However an import ends — finished, stopped or failed — what it wrote stays, so it marks stale either way.
- **Filing by situation marks stale** the layers a message joined or left, in the same transaction, so a stop between filing and measuring still leaves them to be measured.
- **`voice::analyze_in(Mode::Changed)`** measures only layers that are stale or were never measured at this analysis version, and leaves the rest. `Mode::Everything` (what the user starts from How you write) measures every layer.
- **A layer whose scope is gone is removed** either way: a channel the user no longer has a message in, someone they now have fewer than twenty messages to, a situation nothing is filed under.
- **It runs by itself.** When an import or a mailbox check that brought something in finishes, the app queues a `measure_voice_changes` job, once, if one is not already waiting — but only when how the user writes has been measured before (`voice::wants_measuring`): the first measurement is the user's to start. It says nothing when it finishes; How you write shows the result.

## Situations

A situation is what a message is _doing_: saying no, setting a time, apologising, thanking, explaining, disagreeing. Six, fixed, seeded by migration 0007 with stable ids that are the situational layer's scope keys. The list is short on purpose: a layer needs twenty of the user's messages behind it, and a long tail of situations would leave every one of them under the floor.

**How messages are filed.** `situations::classify` reads each of the user's own messages — `direction = 'self'` only; what other people wrote is not evidence of how the user says no — against a table of cue phrases per situation, each with a weight. Weights are summed, clamped to 1, and a message is filed under a situation at `RULE_THRESHOLD = 0.6` or above. Some cues take evidence away ("sorry to hear" is sympathy, not an apology; "no thanks" is a refusal, not a thank-you). A short closing block is ignored before reading, so "Thanks," above a name does not make every message a thank-you. Explaining needs twenty words. A message can be filed under more than one situation ("sorry, can't make it" is both).

The rules are tuned for precision rather than recall: an unfiled message costs the layer one sample, a misfiled one teaches it the wrong habit. Filing happens at the start of every analysis, a page at a time, each page in one transaction, and a row changes only where the rules now say something else (`Db::refile_by_rule`), so filing a corpus that has not changed writes nothing. A situation that ends up with nothing filed loses its layer rather than keeping a description of messages it no longer counts.

**Three hands, each over the one before.**

- **The rules** (`classified_by = 'rule'`) file every one of the user's messages nobody else has decided about.
- **A model on this computer** (`'model'`) can be asked to read them instead (How you write → What each message is doing; `situations::read_with_model`). It is shown up to eight messages at a time — the user's own words, the closing block dropped, up to 800 characters each — with the six situations and what each is, and answers with JSON: for each message, the situations it is doing, or none. An answer naming anything outside the six, or not an answer at all, is not taken: a batch it cannot read is asked again a message at a time, and a message still not understood is left to the rules. What it says replaces the rules' reading of that message; it is stored with a fixed confidence of 0.9 (a model says which situations, not how sure it is) and, in `situation_readings`, with the model's name and the way it was asked. Up to 400 messages a run, the most recent first; each is kept as it is read, so a stopped run keeps what it read and the next starts at the next unread message. **Only a local provider is ever used.** Reading them sends every one to the model, which is not a thing to do from a background job against a hosted provider, so one is refused (`ModelReadError::NotLocal`), not used.
- **The user** (`'user'`) can say what any one of their messages is doing — one situation, several, or none of them — under it in any conversation. That stands over the rules and any model until they choose "Let the rules decide", which hands the message back and files it as the rules read it now.

A decision by a model or the user is recorded in `situation_readings`, because "doing none of these" leaves no row in `message_situations`, and a message with a reading there is left alone by the rules. The layers a decision moves are marked stale, and measured again by themselves.

**How a draft gets a situation.** Two ways, and the draft records which (`SituationChoice.source`). The user can choose one (`chosen`). Otherwise their note is read by the same rules plus a few instruction-shaped cues ("say no", "push back", "thank them") (`fromNote`). With no note, nothing is inferred: what the other person asked is not what the user has decided to answer. The screen words the second case as a reading — "your note read like saying no" — and never as something the user said.

**What a situation changes.** Its layer resolves innermost, after relationship, so where it is measurable its metrics win. Retrieval asks first for times the user did the same thing with the same person on the same channel, then, if there are fewer than two, for a couple from anyone, and fills the rest as before; each such example's reason starts "a time you said no". The prompt gains one line, and it keeps the distinction the screen keeps: a chosen situation is stated ("In this reply they are saying no to something."); a reading of the note is passed on as a reading ("Their note reads as though they are saying no to something here. Follow the note itself if it says otherwise."). The evidence says how many times the user has done it, or that it has too few to go on. A note opening with "no" counts as a refusal only when it is not "no rush", "no worries", "no problem" and the like and does not say yes anywhere, and an instruction cue after "don't", "not" or "never" does not count.

## Representative examples

Six per scope, chosen deterministically. A message scores well when its length is close to the scope's median and when it carries the features that scope is characterized by — a greeting if they greet, a sign-off if they sign off, a phrase they repeat. Ties break on message id, so two runs over the same corpus choose the same examples.

Near-duplicates are suppressed by a normalized fingerprint: six copies of "sounds good" teach a model less than one. Each wording counts once, by its best message.

They are chosen a message at a time (`voice::ExamplePicker`), holding only six wordings: one is let go only when six others each have a better message, and then it could never have been among the best six — so the result is exactly what sorting every message would give (`examples_chosen_a_message_at_a_time_are_the_ones_sorting_them_all_would_choose`).

## Retrieval

Filter first, rank second. A semantically similar message written to a different person on a different channel is the wrong example — it will teach the prompt the wrong register. So participant, channel, relationship, situation, conversation, source and date range are applied as a `WHERE` clause, and ranking only reorders what survived.

Ranking is by wording, and by meaning once the user has downloaded the sentence encoder.

- **By wording:** how much of the incoming message's vocabulary appears in the message being answered, weighted by inverse document frequency so rare words count for more than common ones. A real signal, and a shallow one: "drinks on friday?" shares no word with "pub friday?".
- **By meaning:** all-MiniLM-L6-v2, named by `models/manifests/all-minilm-l6-v2.json`, turns a message into 384 numbers, close together for messages that mean much the same thing. It is downloaded only when the user asks (Settings → Finding your past replies by meaning; `crate::encoder`), 91 MB from Hugging Face at a pinned revision, each file checked against the SHA-256 the manifest pins as it arrives; a file that does not match is deleted. The engine runs it (ONNX Runtime, the encoder's own tokenizer, the tokens averaged over those that are not padding, then scaled to length 1) only while every file still matches, and otherwise uses `lexical_v1` and says why. It is the full-precision export: the 8-bit exports work out their scale from everything in a batch, so a message's numbers would depend on what it was read with.
  - **What is turned into numbers:** every message the user replied to, and every one of theirs (a reply is compared by the message it answered, or by itself when it answered nothing stored). They are read in the background (`embed_messages`), 128 at a time, into `message_embeddings` under the encoder's id; vectors from any other encoder are removed. They are read after the download, after each import or mailbox check, and when the app starts, and what is read is kept as it goes.
  - **At draft time,** the message being answered is turned into numbers by the engine (`encoder::query_vector`) — for a draft the user asks for, one prepared in advance, and one written to be measured — and a candidate with a vector from the same encoder scores `0.75 × closeness in meaning + 0.25 × wording`; one without scores by wording as before. The reason says which: "close in meaning", "close in meaning, and in wording: deck, send", or "similar wording: …".
- A reply in a conversation with several people is one candidate, shown against the person who wrote what it answered.

## From measurements to a prompt

`generation::describe` turns numbers into instructions, and only for metrics that were measured:

- `terminalPeriodRate >= 0.8` → "They end messages with a full stop."
- `terminalPeriodRate <= 0.2` → "They usually do not put a full stop at the end."
- in between → "They end about 34% of messages with a full stop."
- `emojiRate <= 0.02` → "They do not use emoji. Do not add any."
- `greetingRate <= 0.1` → "They open straight into the message, with no greeting."

Then, when there is one, a model's reading of those numbers in words (below), labelled as a reading. Then the user's manual preferences, labelled as overriding the measurements. Then up to five retrieved exchanges, marked as things to match in register and not to reuse in content. Then the recipient, their relationship and the channel. Then any adjustment the user asked for.

The output token budget is derived from `p90WordsPerMessage`, so a model cannot answer a two-word texter with four paragraphs.

### In words: a reading of the numbers

A sentence such as "short and warm, lowercase, signs off with thanks" carries register better than twelve rates, and turning numbers into that sentence is the one thing here a model does better than arithmetic. So, when the user asks (How you write → In words; `voice::describe`), the model they chose is given each measurable layer's numbers — **only the numbers**, and the greetings and sign-offs from Mimic's own short lists; never a message, a phrase the user wrote, or who a layer is about (`describe::numbers`) — and asked for two or three plain sentences about register, saying nothing the numbers do not support. Up to 24 layers a run, the layer over everything first.

The reading is kept with the profile (`qualitative_json.description`), with the provider and model that wrote it and a digest of the numbers it was written from. Measuring again keeps it. It is shown under the layer's numbers as the model's reading, never in place of them; one written from numbers that have since changed is shown as that. A draft is given the reading of the innermost measurable layer only while it is of that layer's numbers as they are now — never an outer layer's reading, which describes other numbers — and its evidence says so.

`assemble` is a pure function of the context. The same context produces the same prompt, which is what makes `prompt_hash` meaningful and the prompt testable.

## Measuring whether any of this works

**There is no Mimic Score, and there will not be one until it is defined here.**

The honest way to measure a voice model is the way the previous product measured an edit model, with conversations in place of shoots:

1. Split the user's own replies by conversation, so no thread straddles the boundary (`eval.split`, `conversation_grouped`). A reply from a conversation the system has already seen is a memory test, not an evaluation.
2. Hide the held-out replies. For each, rebuild the same generation context and generate.
3. Compare generated with actual along named axes (`eval.compare`):
   - **length** — ratio of the shorter word count to the longer;
   - **vocabulary** — Jaccard overlap of word sets;
   - **punctuation** — agreement across five binary habits;
   - **embedding** — cosine similarity under the current provider, reported with the provider's name so a lexical score is never mistaken for a semantic one.
4. Report each component and its 10th percentile. **No single headline number**, because the four components are not commensurable and averaging them would invent a quantity nobody defined.

Two baselines any voice model must beat: a generic reply from the same model to the same conversation and message, with none of the user's measurements, examples or habits (the conversation before the message still shows what they wrote in it), and the reply the user sends most often (among the conversations learned from; spacing, case and a closing mark aside). A score with no baseline is a number, not evidence.

Status (0.10.0-alpha.11): the loop runs (`crate::evaluation`, How you write → How close my drafts come). Up to 12 held-out exchanges are chosen, the newest of each held-out conversation first, and answered by Mimic, the generic baseline and the common reply. No held-out message is used as an example, and each exchange's conversation is read only up to the message being answered. How the user writes is measured again for the run without the held-out conversations (`voice::LeavingOut`), because the stored profiles were measured over everything — a person's own layer perhaps mostly from the thread under test. Habits learned from edits to earlier drafts are left out, since some may be edits to the replies being predicted; that leans against Mimic, and the screen says so. Mimic's drafts are written with no note, as replies prepared in advance are, and with the voice always measured afresh, even for a user whose home-screen drafts would get none or a stale one. A case is kept only if every message its answers were written from — the conversation before it, each example and what the example answered — is still there when it is recorded, and deleting anyone stops a measurement that is running, so none of a deleted person's messages survive in one. Deleting any person or source deletes every evaluation, because what was written for a case came from other people's messages too. The figures are worked out from the cases that remain whenever they are shown. **The UI shows each measure separately, with its 10th percentile, next to both baselines, and no headline number.** The rate of drafts sent unedited, counted from `drafts` rows, is still the only rate on the Voice screen that is not a comparison.

## The learning loop

Draft → what was actually sent → diff → weighted feedback.

`generation::feedback::diff_draft` describes the difference: length direction (with a ±10% dead band, because a one-word edit is not a length preference), greeting and sign-off added or removed, emoji, terminal punctuation, and whether the draft was rewritten outright (under 30% of its words survived into a reply of three words or more).

Weights encode one rule: **what the user said explicitly outranks what Mimic inferred from watching them.** A typed correction is 3.0; an inferred edit is 1.0.

### Closing it

`learning::patterns` re-reads every draft the user sent — edited or not — and asks, for five habits (greeting, sign-off, emoji, a full stop at the end, length), which way the user pushed it. An edit that took the greeting out is a vote for fewer greetings; a draft sent unedited _with_ its greeting is a vote against. Both count, because a loop that only listens to edits concludes that every habit is wrong.

A pattern **holds** when at least `MIN_AGREEING = 3` drafts moved the habit the same way **and** they carry more than half of the weight, counting the drafts that went the other way or left it alone. Binary habits weigh one per draft; length weighs by how much the draft changed (a draft cut by 40% weighs 0.4), and a draft sent at the same length weighs the dead band, 0.1. This is the discipline the previous product's corrections system earned: one deleted greeting is an anecdote, and three deletions against four greetings left in place is still an anecdote.

Patterns are computed for everyone and per person. A person's pattern replaces the global one on the same habit, so learning that Ada gets no greeting does not stop Mimic greeting anyone else, and learning that Ada _does_ get one overrides a global habit of leaving them out.

A pattern that holds reaches the prompt as one instruction ("Do not open with a greeting.", "Write about 40% shorter than the measurements alone would suggest.") in its own section after the measurements, and the draft's evidence names it. Nothing is stored: the patterns are a pure function of the drafts table, so deleting a person takes their drafts' patterns with them.

**Notes.** "Tell me what to do differently" under a draft, and the field under "How you write", store what the user typed as a manual preference — about the draft's recipient, or about everyone — under a `said:` key. The prompt quotes it in the user's words, last, under "Things they have told Mimic directly, which override everything above". Every note is listed with a way to take it back.

What the loop still does not do is change a measured metric. `voice_profiles` remain pure measurement of what the user wrote; what they changed in Mimic's drafts is kept separate, as instructions, so the two can always be told apart.
