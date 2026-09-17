# Contributing

1. `.\scripts\bootstrap.ps1`, then `.\scripts\dev.ps1`.
2. Read `CLAUDE.md` — the safety rules apply to humans too.
3. Keep one coherent change per PR with code, tests and docs together. Shared contracts (bridge protocol, EditDNA mapping, engine methods) need a checked-in fixture and tests on every side that consumes them.
4. `.\scripts\test.ps1` must be green. CI runs the same steps.
5. Update `docs/PROJECT_STATUS.md` when implementation status changes; move finished items out of `docs/ROADMAP.md`.
6. Schema changes: new numbered migration + test. Never edit a shipped migration.
7. Commits are authored under your own identity; no generated attribution trailers.

Lightroom-specific changes should include the capability matrix from the Lightroom version you tested against (Settings › Lightroom).
