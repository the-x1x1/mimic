# Product

## What Mimic is

Mimic learns how a person communicates and helps them draft responses that sound like themselves.

It reads communication the user already owns — exported email, text messages, chat histories — and builds a model of how they actually write: not one style, but the set of adjustments they make without thinking. Longer to a client than to a brother. A full stop at the end of an email and none at the end of a text. "Let me know if that works" three hundred times across four years.

Then, when the user has something to say, they say what they mean in shorthand and Mimic writes it the way they would have.

The distinction that matters: this is not style imitation. The question Mimic answers is not "what does this person's writing look like" but **"how does this person tend to communicate in this situation, with this person, through this channel."** Those are different questions, and the second one is the useful one.

## What Mimic is not

- **Not an AI email writer.** A generic assistant writes competent, anonymous prose. Mimic writes the user's prose, including the parts a writing assistant would try to fix.
- **Not a chatbot.** There is no conversation with Mimic. There is a compose screen with four fields.
- **Not a clone.** It does not act as the user, decide what they think, or send anything. Every draft is a draft.
- **Not a cloud service.** Messages live in a SQLite file on the user's computer. The only thing that can leave is what is sent to a model provider the user chose and can see named in the top bar.

## Who it is for

Someone with a high volume of written correspondence and a voice they do not want flattened: a consultant whose clients know how she writes, a founder whose short replies are part of his reputation, anyone who has ever rewritten an AI-drafted email entirely because it did not sound like them.

## The core interaction

Four inputs, one output, and a panel that accounts for it.

1. **Recipient and channel.** Who this is going to, and how.
2. **Incoming message.** What is being replied to. Optional — the user may be starting.
3. **Intent.** What they want to say, in their own shorthand: _yes but push to Thursday_; _decline, no reason_; _thank them and ask about the invoice_.
4. **Generated reply**, with Regenerate, Shorter, Longer, More casual, More professional, and Copy.

The intent field is the centre of the product, not an afterthought. Mimic supplies the _how_; the user supplies the _what_. A reply assistant that sees only the incoming message has to guess what the user wants to say, and guessing wrong is worse than writing badly.

Beside the draft, always: what it was based on. Which voice layers applied, how many of the user's messages are behind them, which of their own past replies were retrieved and why. A box that produces text with no account of itself is the thing that makes people stop trusting a tool like this.

## Navigation

Five screens.

| Screen       | What it is for                                                                                                                                                          |
| ------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Replies**  | The only screen. Who is waiting, what they wrote, and what Mimic would say back, with use / change / drop on each one.                                                  |
| **People**   | Everyone Mimic has seen, except senders of nothing but automated mail (counted, and shown on request). Say how you know them; see how much it has learned; delete them. |
| **Voice**    | What Mimic has measured about how you write, per layer, with the examples that back each one.                                                                           |
| **Sources**  | Where messages came from. Add, re-import, remove.                                                                                                                       |
| **Settings** | Your own addresses, the model provider, privacy, deletion.                                                                                                              |

## Autonomy

Three modes are designed. One is implemented, one is half-built, and the third is being built (0.10.0-alpha.24).

**MANUAL** — implemented. Mimic drafts; the user reads, edits and sends. Since 0.10.0-alpha.23 they can send from Mimic: **Send** shows who the reply goes to, which of their mailboxes it goes out of and its subject, and nothing goes until **Send it** — from their own address, through their own provider's server, threaded under the message it answers, to that person only. **Use this** still copies it for them to send in whatever app they use. Either way the learning loop reads what was sent.

**ASSISTED** — partly implemented (0.8.0-alpha.1, watching since 0.10.0-alpha.3). Mimic notices threads it could answer — a conversation whose last message from a person came from someone else and was never replied to, unless it looks automated from its headers or the user said it needs no reply (0.10.0-alpha.4), or its last message is older than the waiting window the user chose (0.10.0-alpha.7) — and prepares drafts for them, which wait on the home screen for use, change or drop. The user still sends, by hand, elsewhere.

Two things make this safe to have on: it is off until switched on in Settings, because with it on a model sees incoming messages nobody handed it; and it is bounded — ten threads per run, one draft per message, cancellable between threads. With a mailbox connected, each check is followed by a run, so "in advance" means "as mail arrives"; without one it still means "after the next import".

**TRUSTED** — being built (0.10.0-alpha.24). Mimic replies by itself, within rules the user sets. It was held back because a product that sends as you needs an account of what it will not send, how a mistake is caught and what it may answer; the owner decided on 2026-10-25 to build it, and this is that account:

- **Off until the user turns it on**, in Settings, for a mailbox — and, if they want it narrower, only for the people they name. Turning it on says in words what it will do. One switch turns all of it off, and anything waiting to go is stopped with it.
- **It answers only what the home screen would show as waiting**: never mail that looks automated, never a thread gone quiet or marked as needing no reply, never someone writing from a no-reply address, never a mailing list. It answers a message once, and never twice in a row without a new message from them.
- **Nothing goes at once.** A reply is held for a while (ten minutes unless changed) on the home screen, where it can be read, changed into one the user sends themselves, or stopped; then it goes.
- **A daily limit** (ten unless changed) on replies sent by itself, across every mailbox; past it, replies wait for the user.
- **It says what it is to other programs** (`Auto-Submitted: auto-replied`), so another automatic responder doesn't answer it and start a loop.
- **Everything it sent is listed**, under Your mail, saying it went by itself.
- What it wrote by itself is not read as how the user writes: the user didn't write or approve it.

## Positioning

> A personal communication model that learns how you actually talk.

Not "AI-powered email". Not "your digital twin". The claim is narrow and checkable: it has read what you wrote, it has measured how you write, and it will show you both.

## What the user must be told

Onboarding and the Sources screen both say it: **train Mimic only on communication you own or have permission to process.** An email thread contains other people's words. Mimic imports them because a conversation without the other half is not a conversation, it analyzes only the user's own, and it deletes both together when asked.

## Design direction

Quiet, restrained, typography-led. The interface is mostly text because the product is mostly text.

Explicitly not: AI gradients, glow effects, giant chat bubbles, a dashboard of gauges, fake activity monitors, neural-network decoration, or any visual language that implies more machinery than exists. A metric is a sentence, not a dial. If a number has not been measured, the space says so rather than showing a zero.
