# ADR-002 — Local-first, no telemetry, updates only

**Status**: accepted.

**Decision**: All analysis, training, inference and storage are local. The only outbound request is the GitHub Releases update check (disableable). Future network features are opt-in and documented in PRIVACY.md before shipping.

**Consequences**: No usage analytics to guide product decisions; diagnostics are user-initiated copies; model downloads must be explicit and verified.
