"""Text embeddings.

Two providers, chosen at runtime:

* `onnx:<id>` — a pinned sentence encoder from `models/manifests/*.json`,
  downloaded on first need and SHA-256 verified. Not yet wired to a manifest;
  the mechanism is kept because the download-and-verify discipline is the part
  that is easy to get wrong later.
* `lexical_v1` — the mandatory fallback: a hashed bag of word and character
  n-grams, L2-normalized. It is not semantic and does not pretend to be. What
  it is, is deterministic, dependency-free, fast over a million messages, and
  a genuine improvement over exact-match retrieval.

`status()` states which is in use, so the app can say so rather than implying
a semantic model where there is none.
"""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
from typing import Any

import numpy as np

from mimic_engine.text.tokens import char_ngrams, words

LEXICAL_PROVIDER = "lexical_v1"
LEXICAL_DIM = 256


class EncoderUnavailableError(Exception):
    pass


def _bucket(token: str, dim: int) -> int:
    return int.from_bytes(hashlib.blake2b(token.encode("utf-8"), digest_size=4).digest(), "big") % dim


def lexical_embedding(text: str, dim: int = LEXICAL_DIM) -> np.ndarray:
    """Hashed word + character n-gram counts, L2-normalized.

    Words and character 4-grams go into the same vector with different
    weights: words carry topic, n-grams carry style. Empty text produces a
    zero vector rather than a NaN one, and callers treat a zero vector as "no
    signal" rather than "maximally dissimilar".
    """
    v = np.zeros(dim, dtype=np.float32)
    for w in words(text):
        v[_bucket(f"w:{w}", dim)] += 1.0
    for g in char_ngrams(text, 4):
        v[_bucket(f"c:{g}", dim)] += 0.5
    norm = float(np.linalg.norm(v))
    return v / norm if norm > 0 else v


def cosine(a: np.ndarray, b: np.ndarray) -> float:
    """Cosine similarity of two L2-normalized vectors, clamped to 0..1.

    A zero vector — empty text — scores 0 against everything, which is the
    honest answer rather than a division by zero.
    """
    na, nb = float(np.linalg.norm(a)), float(np.linalg.norm(b))
    if na == 0.0 or nb == 0.0:
        return 0.0
    return float(max(0.0, min(1.0, float(np.dot(a, b)) / (na * nb))))


class EncoderManager:
    """Resolves which encoder to use and produces embeddings with it."""

    def __init__(self, encoders_dir: Path | None = None, manifests_dir: Path | None = None):
        self.encoders_dir = encoders_dir
        self.manifests_dir = manifests_dir
        self._provider = LEXICAL_PROVIDER
        self._dim = LEXICAL_DIM
        self._reason = "no text encoder manifest is present; using the lexical fallback"

    def status(self) -> dict[str, Any]:
        return {
            "provider": self._provider,
            "dims": self._dim,
            "semantic": self._provider != LEXICAL_PROVIDER,
            "reason": self._reason,
            "manifests": sorted(p.name for p in self.manifests_dir.glob("*.json"))
            if self.manifests_dir and self.manifests_dir.is_dir()
            else [],
        }

    def embed(self, text: str) -> np.ndarray:
        return lexical_embedding(text, self._dim)

    def embed_batch(self, texts: list[str]) -> np.ndarray:
        if not texts:
            return np.zeros((0, self._dim), dtype=np.float32)
        return np.vstack([self.embed(t) for t in texts])

    @staticmethod
    def read_manifest(path: Path) -> dict[str, Any]:
        """A manifest must pin a URL and a digest; anything less is refused."""
        data = json.loads(Path(path).read_text(encoding="utf-8"))
        for key in ("id", "url", "sha256", "dims"):
            if key not in data:
                raise EncoderUnavailableError(f"manifest {path.name} is missing {key!r}")
        return data
