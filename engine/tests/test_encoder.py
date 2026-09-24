"""The sentence encoder: pooling, verification, and the fallback.

The model itself is not run here — the tests' environment does not carry
ONNX Runtime — so a stand-in session and tokenizer take its place, and what is
tested is everything Mimic does around it.
"""

import hashlib
import json
from pathlib import Path
from types import SimpleNamespace

import numpy as np
import pytest

from mimic_engine.embeddings.encoder import (
    LEXICAL_PROVIDER,
    EncoderManager,
    EncoderUnavailableError,
    OnnxEncoder,
    installed_problem,
    l2_normalize,
    mean_pool,
    read_manifest,
)

REPO = Path(__file__).resolve().parents[2]


class Tokens:
    """Each word a token: id 1 + its length, padded with 0 to the longest."""

    def encode_batch(self, texts):
        rows = [[len(w) + 1 for w in t.split()] or [1] for t in texts]
        width = max(len(r) for r in rows)
        return [
            SimpleNamespace(
                ids=r + [0] * (width - len(r)),
                attention_mask=[1] * len(r) + [0] * (width - len(r)),
                type_ids=[0] * width,
            )
            for r in rows
        ]


class Session:
    """A model whose token vectors are [id, 1]: padding comes out as [0, 1],
    so pooling that counted it would show."""

    def __init__(self, with_types=True):
        self.inputs = ["input_ids", "attention_mask"] + (["token_type_ids"] if with_types else [])
        self.fed = []

    def get_inputs(self):
        return [SimpleNamespace(name=n) for n in self.inputs]

    def get_outputs(self):
        return [SimpleNamespace(name="last_hidden_state"), SimpleNamespace(name="pooler_output")]

    def run(self, names, feeds):
        assert names == ["last_hidden_state"]
        self.fed.append(sorted(feeds))
        ids = feeds["input_ids"].astype(np.float32)
        return [np.stack([ids, np.ones_like(ids)], axis=-1)]


def test_pooling_averages_only_real_tokens_and_normalizing_keeps_zero_at_zero():
    hidden = np.array([[[2.0, 0.0], [4.0, 0.0], [100.0, 100.0]]])
    mask = np.array([[1, 1, 0]])
    assert mean_pool(hidden, mask).tolist() == [[3.0, 0.0]]
    assert l2_normalize(np.array([[3.0, 4.0], [0.0, 0.0]])) == pytest.approx(np.array([[0.6, 0.8], [0.0, 0.0]]))


def test_the_encoder_pools_over_its_own_tokens_normalizes_and_says_nothing_for_nothing():
    session = Session()
    enc = OnnxEncoder(session, Tokens(), dims=2)
    v = enc.embed_batch(["ab", "ab abcd", "   "])
    # "ab" is one token [3, 1]; padding to the longer text must not move it.
    expected = np.array([3.0, 1.0]) / np.linalg.norm([3.0, 1.0])
    assert v[0] == pytest.approx(expected, abs=1e-6)
    assert np.linalg.norm(v[1]) == pytest.approx(1.0, abs=1e-6)
    assert v[2].tolist() == [0.0, 0.0], "blank text is the zero vector, as the fallback gives"
    assert session.fed == [["attention_mask", "input_ids", "token_type_ids"]]
    assert enc.embed_batch([]).shape == (0, 2)

    # A model that takes no token types is not given them.
    plain = Session(with_types=False)
    OnnxEncoder(plain, Tokens(), dims=2).embed_batch(["x"])
    assert plain.fed == [["attention_mask", "input_ids"]]


def write_encoder(tmp_path, files):
    """A manifest in manifests/ and, in encoders/<id>/, the files given."""
    manifests, encoders = tmp_path / "manifests", tmp_path / "encoders"
    manifests.mkdir()
    (encoders / "tiny").mkdir(parents=True)
    entries = []
    for name, content in (("model.onnx", b"model bytes"), ("tokenizer.json", b"{}")):
        entries.append(
            {
                "name": name,
                "url": f"https://example.invalid/{name}",
                "sha256": hashlib.sha256(content).hexdigest(),
                "bytes": len(content),
            }
        )
        if name in files:
            (encoders / "tiny" / name).write_bytes(files[name])
    manifest = {"id": "tiny", "name": "Tiny", "dims": 2, "maxTokens": 8, "files": entries}
    (manifests / "tiny.json").write_text(json.dumps(manifest), encoding="utf-8")
    return encoders, manifests


def test_a_downloaded_encoder_is_used_only_while_every_file_matches_its_manifest(tmp_path):
    loaded = []

    def load(folder, dims, max_tokens):
        loaded.append((folder.name, dims, max_tokens))
        return OnnxEncoder(Session(), Tokens(), dims)

    good = {"model.onnx": b"model bytes", "tokenizer.json": b"{}"}
    encoders, manifests = write_encoder(tmp_path, good)
    manager = EncoderManager(encoders, manifests, load=load)
    status = manager.status()
    assert status["provider"] == "tiny"
    assert status["semantic"] is True
    assert status["dims"] == 2
    assert "downloaded and verified" in status["reason"]
    assert loaded == [("tiny", 2, 8)]
    assert manager.embed("ab").shape == (2,)


@pytest.mark.parametrize(
    ("files", "why"),
    [
        ({"tokenizer.json": b"{}"}, "model.onnx has not been downloaded"),
        ({"model.onnx": b"model byteZ", "tokenizer.json": b"{}"}, "does not match the SHA-256"),
        ({"model.onnx": b"model", "tokenizer.json": b"{}"}, "is not the size"),
    ],
)
def test_a_missing_or_altered_file_is_never_run(tmp_path, files, why):
    encoders, manifests = write_encoder(tmp_path, files)
    manager = EncoderManager(encoders, manifests, load=lambda *a: pytest.fail("must not load"))
    status = manager.status()
    assert status["provider"] == LEXICAL_PROVIDER
    assert status["semantic"] is False
    assert why in status["reason"]
    assert "lexical fallback" in status["reason"]


def test_a_manifest_must_pin_every_file(tmp_path):
    path = tmp_path / "m.json"
    base = {"id": "x", "name": "X", "dims": 2, "maxTokens": 8}
    path.write_text(json.dumps({**base, "files": [{"name": "model.onnx", "url": "u", "bytes": 1}]}))
    with pytest.raises(EncoderUnavailableError, match="sha256"):
        read_manifest(path)
    path.write_text(json.dumps({**base, "files": [{"name": "model.onnx", "url": "u", "bytes": 1, "sha256": "0" * 64}]}))
    with pytest.raises(EncoderUnavailableError, match=r"tokenizer\.json"):
        read_manifest(path)


def test_the_bundled_manifests_pin_what_they_download():
    paths = sorted((REPO / "models" / "manifests").glob("*.json"))
    assert paths, "the encoder the app offers is named by a manifest"
    for path in paths:
        manifest = read_manifest(path)
        for f in manifest["files"]:
            assert f["url"].startswith("https://huggingface.co/"), f["url"]
            # Pinned to a revision, not a branch that can move under the digest.
            assert "/resolve/main/" not in f["url"]
            assert f["bytes"] > 0


def test_nothing_downloaded_is_the_lexical_fallback_saying_so(tmp_path):
    encoders, manifests = write_encoder(tmp_path, {})
    assert installed_problem(read_manifest(manifests / "tiny.json"), encoders / "tiny") == (
        "model.onnx has not been downloaded"
    )
    manager = EncoderManager(encoders, manifests)
    assert manager.status()["manifests"] == ["tiny.json"]
    assert manager.embed_batch(["hello", "there"]).shape == (2, 256)
