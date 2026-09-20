"""Tokenization.

One place, so the encoder, the similarity scorer and the evaluation harness
cannot disagree about what a word is. Deliberately simple and language-neutral:
this is not linguistic analysis, it is a stable way to turn text into
comparable pieces.
"""

from __future__ import annotations

import re

_WORD = re.compile(r"[\w']+", re.UNICODE)


def words(text: str) -> list[str]:
    """Lowercased word tokens, apostrophes kept so `can't` stays one word."""
    return [w for w in _WORD.findall(text.lower()) if w]


def char_ngrams(text: str, n: int = 4) -> list[str]:
    """Character n-grams over a whitespace-collapsed string.

    These carry the signal that word tokens lose: capitalization habits are
    stripped by lowercasing, but spacing, punctuation runs and word shape are
    not, and they are a large part of what makes someone's writing recognisable.
    """
    s = " ".join(text.split())
    if len(s) < n:
        return [s] if s else []
    return [s[i : i + n] for i in range(len(s) - n + 1)]


def jaccard(a: set[str], b: set[str]) -> float:
    """Overlap of two token sets, 0..1. Two empty sets are identical."""
    if not a and not b:
        return 1.0
    if not a or not b:
        return 0.0
    return len(a & b) / len(a | b)
