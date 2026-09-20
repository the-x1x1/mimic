"""Comparing a generated reply with the one the user actually sent.

This is the only place a "how well does it sound like me" number may come
from, and every component of it is named and defined here so the UI can show
the parts rather than a single mystery score.

None of these is a judgement of quality. They measure resemblance along axes
that are cheap to compute and hard to fake: length, vocabulary, punctuation
and — where an encoder is available — embedding similarity. A high score means
the generated reply is shaped like the real one. It does not mean it was a
good reply, and the docs say so.
"""

from __future__ import annotations

from typing import Any

import numpy as np

from mimic_engine.embeddings.encoder import EncoderManager, cosine
from mimic_engine.text.tokens import jaccard, words


def _punctuation_profile(text: str) -> dict[str, float]:
    stripped = text.strip()
    return {
        "endsWithPeriod": 1.0 if stripped.endswith(".") else 0.0,
        "hasQuestion": 1.0 if "?" in text else 0.0,
        "hasExclamation": 1.0 if "!" in text else 0.0,
        "startsLower": 1.0 if stripped[:1].islower() else 0.0,
        "hasParagraphBreak": 1.0 if "\n\n" in text else 0.0,
    }


def compare(generated: str, actual: str, encoder: EncoderManager | None = None) -> dict[str, Any]:
    """Resemblance of one generated reply to the real one.

    Every component is 0..1 where 1 is "identical in this respect".
    """
    g_words, a_words = words(generated), words(actual)
    g_len, a_len = len(g_words), len(a_words)

    # Length: the ratio of the shorter to the longer, so 5 vs 10 and 10 vs 5
    # score the same.
    if g_len == 0 and a_len == 0:
        length = 1.0
    elif g_len == 0 or a_len == 0:
        length = 0.0
    else:
        length = min(g_len, a_len) / max(g_len, a_len)

    vocabulary = jaccard(set(g_words), set(a_words))

    g_punct, a_punct = _punctuation_profile(generated), _punctuation_profile(actual)
    punctuation = float(np.mean([1.0 - abs(g_punct[k] - a_punct[k]) for k in g_punct]))

    components: dict[str, Any] = {
        "length": round(length, 4),
        "vocabulary": round(vocabulary, 4),
        "punctuation": round(punctuation, 4),
        "generatedWords": g_len,
        "actualWords": a_len,
    }
    if encoder is not None:
        components["embedding"] = round(cosine(encoder.embed(generated), encoder.embed(actual)), 4)
        components["embeddingProvider"] = encoder.status()["provider"]
    return components


def summarize(cases: list[dict[str, Any]]) -> dict[str, Any]:
    """Aggregate per-case comparisons.

    Returns `measurable: False` and no averages when there are no cases, rather
    than a row of zeroes that would read as a very bad score.
    """
    if not cases:
        return {"cases": 0, "measurable": False}
    keys = ["length", "vocabulary", "punctuation", "embedding"]
    out: dict[str, Any] = {"cases": len(cases), "measurable": True}
    for key in keys:
        values = [c[key] for c in cases if isinstance(c.get(key), (int, float))]
        if values:
            out[key] = round(float(np.mean(values)), 4)
            out[f"{key}P10"] = round(float(np.percentile(values, 10)), 4)
    # No single headline number. A caller that wants one must define it and
    # say how, in docs/VOICE_ENGINE.md.
    return out
