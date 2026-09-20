# ADR-002 — Local-first, no telemetry, updates only

**Status**: accepted.

**Decision**: All import, analysis, retrieval and storage are local. Two outbound requests are permitted: the GitHub Releases update check (disableable), and a call to the model provider the user selected — which is a local endpoint by default, and which the interface names in the top bar whenever it is not. Any further network feature is opt-in and documented in PRIVACY.md before it ships.

**Consequences**: No usage analytics to guide product decisions; diagnostics are user-initiated copies; model downloads must be explicit and verified.
