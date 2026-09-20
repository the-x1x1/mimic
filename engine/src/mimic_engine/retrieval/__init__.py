"""Similarity search over message embeddings."""

from __future__ import annotations

import numpy as np

from mimic_engine.embeddings.encoder import cosine


def top_k(query: np.ndarray, candidates: np.ndarray, k: int) -> list[tuple[int, float]]:
    """Indices and scores of the `k` closest candidates, best first.

    Ties break on index so two runs over the same data return the same order —
    the UI shows these as "why this draft looks like this", and an answer that
    reshuffles on every click is not an explanation.
    """
    if candidates.size == 0 or k <= 0:
        return []
    scores = [(i, cosine(query, candidates[i])) for i in range(candidates.shape[0])]
    scores.sort(key=lambda pair: (-pair[1], pair[0]))
    return scores[: min(k, len(scores))]
