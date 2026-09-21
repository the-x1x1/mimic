# Mimic

**Drafts that sound like you.**

Mimic is a local-first Windows desktop app from the Formicaria family. It reads communication you already own — exported email, text messages, chat histories — and learns how you actually write: not one style, but the adjustments you make without thinking. Longer to a client than to your brother. A full stop at the end of an email and none at the end of a text. "Let me know if that works", three hundred times across four years.

Then, when you have something to say, you say what you mean in shorthand and Mimic writes it the way you would have.

The distinction that matters: this is not style imitation. The question it answers is not "what does this person's writing look like" but _"how does this person tend to communicate in this situation, with this person, through this channel."_

Everything stays on your computer. The only thing that leaves is what you send to a model provider you chose and can see named in the top bar.

## Current status — `0.9.0-alpha.2`

0.6.0 was the migration from what Mimic used to be (a Lightroom Classic editing assistant, versions 0.1.0 through 0.5.0) plus a working vertical slice of the new product; see [docs/MIGRATION_AUDIT.md](docs/MIGRATION_AUDIT.md) for what was kept, refactored, archived and deleted. 0.7.0 is about the first run: onboarding that cannot strand you, and a model badge that tells the truth about whether anything is listening.

| Area                                                                                                  | Status                                                                                            |
| ----------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------- |
| Desktop shell: one screen, with People / How you write / Your mail / Settings in a drawer over it     | Implemented                                                                                       |
| The one screen: who is waiting, and use / change / drop on each reply                                 | Implemented and tested                                                                            |
| Assisted drafting in the background, off by default                                                   | Implemented and tested; needs a connector before it is "in advance" rather than "after an import" |
| Sending a reply                                                                                       | Deliberately not built                                                                            |
| SQLite schema v6, migrations, backup before upgrade, resumable jobs                                   | Implemented and tested                                                                            |
| Source connectors: standard `.mbox`, and Mimic's own JSON export format                               | Implemented and tested                                                                            |
| Import: identity resolution, direction, dedupe, quoted-reply stripping, cancel and resume             | Implemented and tested                                                                            |
| Voice engine: deterministic metrics, global / channel / relationship layers, representative examples  | Implemented and tested                                                                            |
| Situational voice layer                                                                               | Schema and prompt slot only — nothing classifies yet                                              |
| Retrieval: metadata filter, then lexical ranking                                                      | Implemented; embedding-backed ranking is Phase 2                                                  |
| Generation: context builder, prompt assembler, draft record                                           | Implemented and tested                                                                            |
| Model providers: local OpenAI-compatible endpoint, Anthropic                                          | Implemented; the hosted one has never been exercised in CI                                        |
| Deletion that actually deletes, with a preview that cannot understate it                              | Implemented and tested                                                                            |
| Learning loop: draft → sent → diff → weighted feedback                                                | Recorded; nothing changes a profile from it yet (Phase 3)                                         |
| Held-out evaluation harness                                                                           | Implemented; not yet run over a real corpus, so **no accuracy figure appears anywhere**           |
| First run: onboarding that reacts to finished jobs, recovers from a bad import, and can be left early | Implemented and tested                                                                            |
| Model reachability: the badge and the draft button follow a real check                                | Implemented and tested                                                                            |
| Windows installer and signed updater                                                                  | Plumbing unchanged from 0.5.0 and never observed producing a build end to end                     |

The authoritative, per-feature truth table is [docs/PROJECT_STATUS.md](docs/PROJECT_STATUS.md). If this README and that file disagree, PROJECT_STATUS wins, and the source code wins over both.

## Requirements

- Windows 11 x64 (Windows 10 where WebView2 is available). macOS is planned; nothing in the code is Windows-specific and nothing has been tested there.
- For local drafting: [Ollama](https://ollama.com), LM Studio, or anything else that speaks the OpenAI chat-completions API on `127.0.0.1`. Optional — you can use a hosted provider instead, and Mimic will say so in the top bar every time.
- Messages to learn from: a `.mbox` export from Gmail Takeout or Thunderbird, or any conversation history converted to [Mimic's JSON format](docs/IMPORT_PIPELINE.md).

## Quick start

1. Install and launch. Onboarding asks four things, in order. Only the first is required: once Mimic knows which addresses are yours, "Look around first" opens the empty app and setup waits for you under Sources.
2. **Tell Mimic which addresses are yours.** This is not a profile page: it is how Mimic tells the messages you wrote from the ones you received, and everything downstream depends on it.
3. **Add a source** — point Mimic at your export. It reads the file and tells you what it found before importing: how many messages, what date range, which addresses appear most often, and anything it could not make sense of.
4. **Import.** Nothing is uploaded. Re-importing the same file later is free.
5. **Analyze.** Mimic measures your own messages: length, punctuation, capitalization, how you open and close, the phrases you repeat, how quickly you reply. This is arithmetic over your text — no model is involved.
6. **The one screen.** Mimic opens on whoever is waiting on a reply, with what they wrote and what Mimic would say back, and use / change / drop beside each one.
7. **Writing something new** lives behind “Write something new”: pick who it is going to, say what you want to say in shorthand, and read the draft. Beside it, always: what it was based on.
8. **Tell Mimic what you actually sent.** It compares its draft with your version and records the difference. This is the only way it improves, and it never sends anything for you.

Only import communication you own or have permission to process.

## What Mimic will not do

It will not send a message. It has no send access to anything. Every draft is a draft, and you move it into whatever app you actually use.

It will not show you a number it has not measured. A rate that has not been computed says "not measured yet"; a rate of zero means zero. There is no accuracy score, because the evaluation that would earn one has not been run over real data.

## Development

```powershell
.\scripts\bootstrap.ps1     # verifies node/pnpm/cargo/uv, installs everything
.\scripts\dev.ps1           # Vite + Tauri; the shell spawns the Python engine via uv
.\scripts\test.ps1          # everything CI runs
.\scripts\validate.ps1 -Full # test.ps1 + pre-tag release verification
.\scripts\build.ps1         # engine bundle + installer (updater artifacts need TAURI_SIGNING_PRIVATE_KEY)
```

Root package scripts: `pnpm install`, `pnpm dev`, `pnpm test`, `pnpm lint`, `pnpm typecheck`, `pnpm build`.

Regenerating the cross-language contract fixtures after a deliberate shape change:

```
MIMIC_REGEN_FIXTURES=1 cargo test -p mimic-core --test pipeline_e2e
```

## Documentation

| Document                                   | What it covers                                                             |
| ------------------------------------------ | -------------------------------------------------------------------------- |
| [PRODUCT](docs/PRODUCT.md)                 | What this is, what it is not, the core interaction, the autonomy modes     |
| [ARCHITECTURE](docs/ARCHITECTURE.md)       | The three layers, why they are split that way, the rules that shape them   |
| [DATA_MODEL](docs/DATA_MODEL.md)           | Schema v6, and why each table is shaped the way it is                      |
| [VOICE_ENGINE](docs/VOICE_ENGINE.md)       | Every metric, how layers resolve, and how accuracy would have to be earned |
| [IMPORT_PIPELINE](docs/IMPORT_PIPELINE.md) | The connector contract, the shipped connectors, the JSON format            |
| [MODEL_PROVIDERS](docs/MODEL_PROVIDERS.md) | The provider boundary, credentials, and a known limitation                 |
| [PRIVACY](docs/PRIVACY.md)                 | Where the data is, what leaves, and what deletion really removes           |
| [PROJECT_STATUS](docs/PROJECT_STATUS.md)   | The truth table                                                            |
| [ROADMAP](docs/ROADMAP.md)                 | Phases 0 through 8                                                         |
| [MIGRATION_AUDIT](docs/MIGRATION_AUDIT.md) | What the photography product left behind, and why                          |
| [CLAUDE.md](CLAUDE.md)                     | Agent entry point and the repository's rules                               |

Everything from the photography era that was worth keeping is under [`archive/legacy-photography/`](archive/legacy-photography/README.md). It is not built, tested or shipped.
