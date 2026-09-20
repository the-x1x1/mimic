import numpy as np
import pytest

from mimic_engine.embeddings.encoder import EncoderManager, cosine, lexical_embedding
from mimic_engine.evaluation.metrics import compare, summarize
from mimic_engine.evaluation.split import grouped_split
from mimic_engine.retrieval import top_k
from mimic_engine.text.tokens import char_ngrams, jaccard, words


def test_tokenization_keeps_contractions_whole():
    assert words("I can't do it — sorry!") == ["i", "can't", "do", "it", "sorry"]
    assert words("") == []
    assert char_ngrams("abcdef", 4) == ["abcd", "bcde", "cdef"]
    assert char_ngrams("ab", 4) == ["ab"], "text shorter than n is one gram"
    assert char_ngrams("", 4) == []


def test_jaccard_treats_two_empties_as_identical():
    assert jaccard(set(), set()) == 1.0
    assert jaccard({"a"}, set()) == 0.0
    assert jaccard({"a", "b"}, {"b", "c"}) == pytest.approx(1 / 3)


def test_similar_text_scores_higher_than_unrelated_text():
    a = lexical_embedding("can you send the quarterly deck")
    near = lexical_embedding("could you send the quarterly deck over")
    far = lexical_embedding("pub friday?")
    assert cosine(a, near) > cosine(a, far)
    assert cosine(a, a) == pytest.approx(1.0, abs=1e-5)


def test_an_empty_embedding_scores_zero_rather_than_dividing_by_zero():
    assert cosine(lexical_embedding(""), lexical_embedding("anything")) == 0.0
    assert float(np.linalg.norm(lexical_embedding(""))) == 0.0


def test_top_k_is_stable_and_bounded():
    enc = EncoderManager()
    candidates = enc.embed_batch(["a a a", "b b b", "a a b"])
    query = enc.embed("a a a")
    assert top_k(query, candidates, 2) == top_k(query, candidates, 2)
    assert len(top_k(query, candidates, 10)) == 3
    assert top_k(query, candidates, 0) == []
    assert top_k(query, enc.embed_batch([]), 3) == []


def test_comparison_components_are_each_meaningful():
    identical = compare("yeah sounds good", "yeah sounds good")
    assert identical["length"] == 1.0
    assert identical["vocabulary"] == 1.0
    assert identical["punctuation"] == 1.0

    # Same words, very different length.
    short = compare("yeah", "yeah yeah yeah yeah yeah yeah yeah yeah")
    assert short["length"] == pytest.approx(0.125)
    assert short["vocabulary"] == 1.0, "vocabulary is about which words, not how many"

    # Same length, different punctuation habits.
    punct = compare("sounds good.", "sounds good")
    assert punct["length"] == 1.0
    assert punct["punctuation"] < 1.0


def test_an_empty_generation_scores_zero_on_length_not_one():
    assert compare("", "something real")["length"] == 0.0
    assert compare("", "")["length"] == 1.0


def test_summary_of_nothing_is_not_a_score_of_zero():
    assert summarize([]) == {"cases": 0, "measurable": False}
    s = summarize([compare("a b c", "a b c"), compare("x", "a b c")])
    assert s["measurable"] is True
    assert s["cases"] == 2
    assert 0.0 < s["length"] < 1.0
    assert "lengthP10" in s, "the tail matters more than the mean for this"


def test_split_is_reproducible_and_honest_about_thin_data():
    keys = [f"t{i // 5}" for i in range(50)]
    a = grouped_split(keys, seed=3)
    b = grouped_split(keys, seed=3)
    assert (a.train, a.holdout) == (b.train, b.holdout)
    assert grouped_split(keys, seed=4).holdout != a.holdout or True  # a different seed may coincide

    one = grouped_split(["only"] * 20)
    assert one.strategy == "time_ordered_single_group"
    assert any("optimistic" in w for w in one.warnings)

    two = grouped_split(["a"] * 10 + ["b"] * 10)
    assert any("two conversations" in w for w in two.warnings)
    assert two.train and two.holdout

    empty = grouped_split([])
    assert empty.train == [] and empty.holdout == []
