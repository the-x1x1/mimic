# ADR-001 — Tauri 2 desktop shell

**Status**: accepted (2026-09).

**Context**: Windows-first desktop app, needs native file dialogs, a child process, a signed updater from GitHub Releases, and small footprint.

**Decision**: Tauri 2 with Rust for everything trusted (db, bridge, engine supervision, updater state) and a React/TypeScript webview for UI. No Electron.

**Consequences**: Rust toolchain required for contributors; WebView2 dependency on Windows (bootstrapper bundled silently); UI cannot touch the filesystem except via commands and the scoped asset protocol; Linux CI needs webkit2gtk dev packages to compile the shell crate.
