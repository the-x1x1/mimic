# Mimic

**Teach Lightroom Classic your editing habits.**

Mimic is a local-first Windows desktop app from the Formicaria family. It learns how you edit photographs in Adobe Lightroom Classic — from your existing RAW + edit history — and, release by release, takes over the repetitive part of your develop workflow: predicting your global edits for a new shoot, scoring its own confidence, letting you review only the uncertain photos, applying the rest into Lightroom with a safety snapshot, and learning from the corrections you make afterwards.

The product promise is not “apply an AI preset”. It is _“learn how I edit and do the repetitive part the way I would.”_

## Current status — `0.2.0-alpha.1` (first Style Brain)

| Area                                                                                                                                                                             | Status                                                                                                  |
| -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- |
| Desktop shell (Tauri 2 + React), onboarding, Home/Styles/Sessions/Review/Settings                                                                                                | Implemented; Sessions and Review are honest empty states until 0.3.0                                    |
| SQLite database with migrations, backups before migration, persistent resumable jobs                                                                                             | Implemented and tested                                                                                  |
| Python engine sidecar (NDJSON over stdio): folder scanner, XMP parser, ACR detection, EXIF, previews, visual features, heuristic scene labels, embeddings (statistical fallback) | Implemented and tested against fixtures                                                                 |
| Canonical EditDNA normalization (104 controls, `edit_mapping_v1`), unknown-setting preservation                                                                                  | Implemented; golden-tested in Rust, Python and TypeScript                                               |
| Lightroom Classic plugin + loopback bridge (handshake, command polling, capability probe, get develop settings, apply/read-back protocol)                                        | Implemented; protocol proven with a fake plugin in CI. **Needs real-Lightroom QA** (no Lightroom in CI) |
| Data quality report                                                                                                                                                              | Implemented                                                                                             |
| Training a Style Brain                                                                                                                                                           | **Not in this build** (0.2.0)                                                                           |
| New-session prediction, review, apply to Lightroom                                                                                                                               | **Not in this build** (0.3.0)                                                                           |
| Windows installer + signed updater plumbing + GitHub Release workflow                                                                                                            | Implemented in CI; the first published installer is produced by the release workflow, not committed     |

The authoritative, per-feature truth table is [docs/PROJECT_STATUS.md](docs/PROJECT_STATUS.md). If this README and that file ever disagree, PROJECT_STATUS wins, and the source code wins over both.

## Download

Releases are published on [GitHub Releases](https://github.com/the-x1x1/mimic/releases). Each stable release ships a per-user Windows x64 installer (`Mimic_<version>_x64-setup.exe`), a signed updater artifact, `latest.json`, `SHA256SUMS.txt`, and a zipped copy of the Lightroom plugin. Alpha builds are marked pre-release.

## Requirements

- Windows 11 x64 (Windows 10 where WebView2 is available). macOS is planned (see [ROADMAP](docs/ROADMAP.md)).
- Adobe Lightroom Classic (tested plugin API: SDK 6.0+; probed at runtime — see the [capability matrix](docs/LIGHTROOM_CAPABILITY_MATRIX.md)).
- For sidecar-only training: RAW files with `.xmp` sidecars (Lightroom › Catalog Settings › Metadata › _Automatically write changes into XMP_).

## Quick start

1. Install Mimic and launch it. The onboarding asks how you want to teach it.
2. **Connect Lightroom Classic**: Mimic prepares `Mimic.lrplugin` under `%LOCALAPPDATA%\Formicaria\Mimic\plugin\`; add that folder in Lightroom › File › Plug-in Manager. “Connected” appears only after a real handshake.
3. **Or train from folders + sidecars**: pick a folder of edited RAW files. Nothing is written to the folder.
4. Read the **data quality report**: valid edit pairs, missing edits, ACR-only edits, camera mix, and whether the dataset is sufficient (minimum 30 pairs).
5. **Train**: a new immutable version is trained on shoots split so validation never sees a training shoot; you get real holdout numbers, and older versions stay available for rollback.

## Development

```powershell
.\scripts\bootstrap.ps1     # verifies node/pnpm/cargo/uv, installs everything
.\scripts\dev.ps1           # Vite + Tauri; the shell spawns the Python engine via uv
.\scripts\test.ps1          # everything CI runs: version check, prettier, tsc, eslint, vitest, cargo fmt/clippy/test, ruff, pytest, plugin checks
.\scripts\build.ps1         # engine bundle + installer (updater artifacts need TAURI_SIGNING_PRIVATE_KEY)
```

Root package scripts: `pnpm install`, `pnpm dev`, `pnpm test`, `pnpm lint`, `pnpm typecheck`, `pnpm build`.

Repository map and agent rules: [CLAUDE.md](CLAUDE.md). Architecture: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Privacy

Everything stays on this computer. Image analysis, metadata, training data, models and edit history are stored under `%LOCALAPPDATA%\Formicaria\Mimic`. The only network request Mimic makes is the update check against GitHub Releases, which can be disabled. Details: [docs/PRIVACY.md](docs/PRIVACY.md).

## Architecture in one paragraph

A Tauri 2 shell (Rust) owns the SQLite database, a persistent job queue, a loopback-only HTTP bridge that the Lightroom plugin polls, and a Python engine child process spoken to over newline-delimited JSON. The engine does deterministic image work: scanning, XMP parsing, RAW previews, statistics, embeddings and (from 0.2.0) training and prediction. A single JSON contract, `packages/contracts/edit_mapping_v1.json`, defines the canonical EditDNA representation shared by Rust, Python and TypeScript, and golden fixtures pin its behaviour in all three. Lightroom stays the source of truth: Mimic never opens the `.lrcat`, never overwrites a RAW, and never mutates XMP.

## Known limitations (0.2.0-alpha.1)

- No sessions, review or apply yet — prediction exists in the engine but has no UI until 0.3.0.
- The model is a KNN + ridge hybrid on statistical features; it is measured against baselines, not against a photographer's acceptance yet.
- The Lightroom plugin has not been exercised against a real Lightroom Classic installation by CI; the protocol is verified with a fake plugin. Field reports welcome via the _Lightroom compatibility_ issue template.
- Masks, local adjustments, AI Denoise and other ACR-sidecar “heavy edits” are detected and preserved but never learned or written.
- The visual embedding is a statistical fallback (`stats_v1`); ONNX encoders are manifest-driven and SHA-256 verified but no manifest ships yet.
- DNG files without an `.xmp` sidecar are ingested without their embedded develop settings.
- Light theme is token-complete but visually unreviewed.

## Roadmap

[docs/ROADMAP.md](docs/ROADMAP.md) — future work only.

## License

MIT — see [LICENSE](LICENSE).
