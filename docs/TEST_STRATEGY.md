# Test strategy

Truth priority is code → tests → runtime evidence; every PROJECT_STATUS row points at a test.

## Layers

| Layer            | Where                                                                                 | What                                                                                                                                                                                                                   |
| ---------------- | ------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Rust unit        | `crates/mimic-core/src/**` `#[cfg(test)]`                                             | migrations, repositories, EditDNA normalization/roundtrip/verification, capability matrix, jobs, diagnostics, paths, ids                                                                                               |
| Rust golden      | `tests/edit_dna_golden.rs`                                                            | Python-parsed raw fixtures → normalized goldens (regenerate with `MIMIC_REGEN_GOLDEN=1`)                                                                                                                               |
| Rust integration | `tests/bridge_fake_plugin.rs`, `tests/bridge_protocol.rs`, `tests/engine_protocol.rs` | real HTTP server + fake plugin; fixtures vs types; real child process (fake engine) covering timeout, crash, restart, oversized messages                                                                               |
| Rust e2e         | `tests/ingest_e2e.rs`                                                                 | real Python engine via `uv`, real DB, real job runner, fixture library; skips only if `uv` is absent                                                                                                                   |
| Python           | `engine/tests`                                                                        | protocol server, XMP parser incl. goldens and nested-description leak guard, scanner pairing rules, features determinism, scene intent, preview cache, embedding fallback, service methods incl. batch error isolation |
| TypeScript       | `packages/contracts/test`, `apps/desktop/src/**/*.test.tsx`                           | fixtures vs zod contracts, mapping invariants, cross-language unknown-key expectations, latest.json shape, updater cadence, component honesty (no invented metrics, no claimed apply support), non-Tauri gate          |
| Lua              | `lightroom/tests/json_test.lua` + `luac -p`                                           | JSON codec against bridge fixtures; syntax of every plugin file                                                                                                                                                        |
| Scripts          | `scripts/test.ps1`                                                                    | orchestrates all of the above in CI order                                                                                                                                                                              |

## Fixtures

`fixtures/xmp` (five eras/edge cases), `fixtures/bridge` (handshake, commands, results incl. partial failure and error), `fixtures/images` (synthetic, generated, non-copyright), `fixtures/expected` (raw + normalized goldens). A contract change must update fixtures and pass in every language that consumes them — the modern-masks fixture caught a real parser leak this way.

## Not automated

Real Lightroom Classic (no CI runner has it) — tracked as NEEDS REAL-LIGHTROOM QA; Windows installer smoke (release workflow builds it; nightly builds a debug binary); UI journeys in a real webview (Playwright over the Vite app is planned once Sessions/Review exist; the current pages are covered by component tests).
