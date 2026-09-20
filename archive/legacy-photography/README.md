# Legacy photography material

Mimic was a Lightroom Classic editing assistant through version `0.5.0-alpha.1`. It is now a personal communication model. This directory holds the parts of the old product that were expensive to build and cheap to keep, so they are not lost to a `git log` search.

Everything here was current at `v0.5.0-alpha.1`. **None of it is built, tested, linted, packaged or shipped.** It is excluded from the pnpm workspace, the Cargo workspace, CI, the Tauri bundle and the formatter. Treat it as documentation.

The rest of the photography code — the native `edit_dna`, `bridge`, `capability`, `sessions` and `corrections` modules, the Python image pipeline, the photography UI and its tests — was deleted rather than archived, because it is reconstructible from the git history and would otherwise rot in place. The `v0.5.0-alpha.1` tag is the full record.

## What is here

| Path                                  | What it is                                                                                                                                                                                                                                                            |
| ------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `lightroom/Mimic.lrplugin/`           | A working Lightroom Classic plugin: handshake with the desktop app, long-poll command loop, runtime capability probe, develop-settings read and write, snapshots, metadata, menus. Written against a sparsely documented SDK; not reconstructible from documentation. |
| `lightroom/tests/`                    | The Lua JSON tests that went with it.                                                                                                                                                                                                                                 |
| `contracts/edit_mapping_v1.json`      | The canonical EditDNA mapping: 104 Lightroom develop controls across 11 families, with normalization ranges and the rules for preserving unknown keys.                                                                                                                |
| `docs/EDIT_DNA.md`                    | How that mapping was derived and what each family means.                                                                                                                                                                                                              |
| `docs/LIGHTROOM_INTEGRATION.md`       | How the app talked to Lightroom, and the protocol's failure modes.                                                                                                                                                                                                    |
| `docs/LIGHTROOM_CAPABILITY_MATRIX.md` | What the SDK actually permits at runtime versus what it claims.                                                                                                                                                                                                       |
| `docs/ML_PIPELINE.md`                 | The hybrid KNN + ridge approach and, more usefully, the reasoning behind the shoot-grouped train/validation/holdout split. That reasoning is reused by the new voice-evaluation harness, with conversations in place of shoots.                                       |
| `docs/ADR-003-lightroom-bridge.md`    | Why the app used a loopback HTTP bridge instead of touching the catalog.                                                                                                                                                                                              |
| `docs/ADR-004-edit-dna.md`            | Why develop settings were normalized into a canonical contract.                                                                                                                                                                                                       |
| `docs/ADR-005-ml-baseline.md`         | Why the first model was deterministic regression rather than a language model.                                                                                                                                                                                        |
| `fixtures/xmp/`, `fixtures/expected/` | The golden corpus that proved the mapping: real sidecar inputs and their expected raw and normalized forms.                                                                                                                                                           |

## What was not archived, and why

Synthetic test images (1.3 MB), bridge transcripts, session read models, and the image encoder manifests were deleted. They are generated artifacts, regenerable from the code in the history, and they would have made the archive larger than the live repository's fixtures.
