# ADR-003 — Lightroom via SDK plugin polling a loopback bridge

**Status**: accepted.

**Context**: The cloud Lightroom API is end-of-life; direct `.lrcat` access is unsupported and dangerous; the SDK cannot host a server but can make HTTP requests (`LrHttp`).

**Decision**: The desktop owns a `127.0.0.1` HTTP service with a per-launch token; the plugin discovers it through a per-user file and long-polls for commands. Apply uses plugin-owned develop presets inside `withWriteAccessDo`, preceded by a snapshot and followed by a read-back.

**Consequences**: Latency bounded by the poll interval (1 s idle, immediate under long-poll); plugin must be installed by the user through Plug-in Manager (no preference hacking); everything depends on runtime capability probing because develop-settings tables vary by version; no file-queue fallback in 0.x.
