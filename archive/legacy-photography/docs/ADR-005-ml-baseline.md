# ADR-005 — Deterministic features + retrieval/regression baselines first

**Status**: accepted (implementation lands in 0.2.0).

**Decision**: No LLM for numeric prediction; no PyTorch hard dependency in 0.x. Features are deterministic NumPy statistics, heuristic scene labels and a statistical embedding with an optional pinned ONNX encoder. Training follows a measured baseline hierarchy (median → conditioned median → KNN → per-family regressors → hybrid) with session-grouped splits and reproducible configs.

**Consequences**: Interpretable predictions with nearest examples; small install; accuracy ceiling accepted until measurements justify heavier models.
