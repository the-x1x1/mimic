# Contributing

1. `.\scripts\bootstrap.ps1`, then `.\scripts\dev.ps1`.
2. Read `CLAUDE.md` — the safety rules apply to humans too.
3. Keep one coherent change per PR with code, tests and docs together. Shared contracts (IPC payloads, the source-export format, engine methods) need a checked-in fixture and tests on every side that consumes them.
4. `.\scripts\test.ps1` must be green. CI runs the same steps.
5. Update `docs/PROJECT_STATUS.md` when implementation status changes; move finished items out of `docs/ROADMAP.md`.
6. Schema changes: new numbered migration + test. Never edit a shipped migration.
7. Commits are authored under your own identity; no generated attribution trailers.

A change to an importer should come with a fixture export that exercises it: `fixtures/import/` is read by the connector tests, and a real-world shape that broke is worth keeping forever.
