# ADR-004 — Canonical EditDNA with a data-driven mapping

**Status**: accepted.

**Decision**: ML never binds to raw Lightroom keys. A versioned JSON mapping (`edit_mapping_v1`) defines canonical controls, families, ranges and legacy aliases; normalization happens once, in mimic-core; unknown keys are preserved verbatim; local/mask data is observed, never written. The same file is consumed by Rust, TypeScript and Python, with golden fixtures in all three.

**Consequences**: Adding a control is a new mapping version; the engine reads normalized snapshots instead of re-deriving them; cross-language tests catch mapping drift and parser regressions.
