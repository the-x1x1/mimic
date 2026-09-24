"""Text embeddings.

Two providers, chosen at runtime:

* A pinned sentence encoder named by a manifest in `models/manifests/*.json`
  (all-MiniLM-L6-v2), run with ONNX Runtime. The app downloads its files when
  the user asks, into the encoders folder, and they are used only while every
  file matches the SHA-256 its manifest pins: checked again each time the
  engine loads it, so a file changed or cut short on disk is never run. Text
  is split into tokens by the encoder's own tokenizer, run through the model,
  averaged over the tokens that are not padding, and L2-normalized.
* `lexical_v1` — the fallback whenever there is no encoder to run: a hashed
  bag of word and character n-grams, L2-normalized. It is not semantic and
  does not pretend to be. What it is, is deterministic, dependency-free, fast
  over a million messages, and a genuine improvement over exact-match
  retrieval.

The engine never touches the network: it reads what the app downloaded.
`status()` states which provider is in use and why, so the app can say so
rather than implying a semantic model where there is none.
"""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
from typing import Any, Protocol

import numpy as np

from mimic_engine.text.tokens import char_ngrams, words

LEXICAL_PROVIDER = "lexical_v1"
LEXICAL_DIM = 256

# How many texts go through the model at once.
BATCH = 32


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


def mean_pool(hidden: np.ndarray, mask: np.ndarray) -> np.ndarray:
    """Average each text's token vectors over the tokens the mask keeps.

    `hidden` is (texts, tokens, dims) and `mask` (texts, tokens), 1 for a real
    token and 0 for padding, so a short text in a batch of long ones is not
    diluted by the padding it was given.
    """
    weights = mask.astype(np.float32)[..., None]
    summed = (hidden.astype(np.float32) * weights).sum(axis=1)
    counts = np.clip(weights.sum(axis=1), 1e-9, None)
    return summed / counts


def l2_normalize(vectors: np.ndarray) -> np.ndarray:
    """Each row scaled to length 1; a zero row stays zero."""
    norms = np.linalg.norm(vectors, axis=1, keepdims=True)
    return np.where(norms > 0, vectors / np.where(norms > 0, norms, 1.0), vectors).astype(np.float32)


def sha256_of(path: Path) -> str:
    h = hashlib.sha256()
    with Path(path).open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def read_manifest(path: Path) -> dict[str, Any]:
    """A manifest must pin every file by URL, size and digest; anything less is refused."""
    data = json.loads(Path(path).read_text(encoding="utf-8"))
    for key in ("id", "name", "dims", "maxTokens", "files"):
        if key not in data:
            raise EncoderUnavailableError(f"manifest {Path(path).name} is missing {key!r}")
    names = set()
    for f in data["files"]:
        for key in ("name", "url", "sha256", "bytes"):
            if key not in f:
                raise EncoderUnavailableError(f"manifest {Path(path).name} has a file without {key!r}")
        if len(f["sha256"]) != 64:
            raise EncoderUnavailableError(f"manifest {Path(path).name} pins {f['name']} with no SHA-256")
        names.add(f["name"])
    for role in ("model.onnx", "tokenizer.json"):
        if role not in names:
            raise EncoderUnavailableError(f"manifest {Path(path).name} has no {role}")
    return data


def installed_problem(manifest: dict[str, Any], folder: Path) -> str | None:
    """Why the files in `folder` cannot be run as `manifest`'s encoder, or None
    when every one is there, of the size and SHA-256 the manifest pins."""
    for f in manifest["files"]:
        path = folder / f["name"]
        if not path.is_file():
            return f"{f['name']} has not been downloaded"
        if path.stat().st_size != int(f["bytes"]):
            return f"{f['name']} is not the size its manifest pins"
        if sha256_of(path) != f["sha256"].lower():
            return f"{f['name']} does not match the SHA-256 its manifest pins"
    return None


class Session(Protocol):
    def get_inputs(self) -> list[Any]: ...
    def get_outputs(self) -> list[Any]: ...
    def run(self, output_names: list[str] | None, feeds: dict[str, np.ndarray]) -> list[np.ndarray]: ...


class OnnxEncoder:
    """A sentence encoder run with ONNX Runtime.

    `session` and `tokenizer` are passed in, so the pooling and normalizing
    can be tested without the model; `load` builds the real ones.
    """

    def __init__(self, session: Session, tokenizer: Any, dims: int):
        self.session = session
        self.tokenizer = tokenizer
        self.dims = dims
        self._inputs = {i.name for i in session.get_inputs()}
        outputs = [o.name for o in session.get_outputs()]
        self._output = "last_hidden_state" if "last_hidden_state" in outputs else outputs[0]

    @classmethod
    def load(cls, folder: Path, dims: int, max_tokens: int) -> OnnxEncoder:
        try:
            import onnxruntime as ort
            from tokenizers import Tokenizer
        except Exception as e:  # pragma: no cover - depends on the build
            raise EncoderUnavailableError(f"this build cannot run an encoder: {e}") from e
        tokenizer = Tokenizer.from_file(str(folder / "tokenizer.json"))
        tokenizer.enable_truncation(max_length=max_tokens)
        pad = tokenizer.token_to_id("[PAD]")
        tokenizer.enable_padding(pad_id=pad if pad is not None else 0, pad_token="[PAD]")
        wanted = ["DmlExecutionProvider", "CPUExecutionProvider"]
        providers = [p for p in wanted if p in ort.get_available_providers()] or ["CPUExecutionProvider"]
        session = ort.InferenceSession(str(folder / "model.onnx"), providers=providers)
        return cls(session, tokenizer, dims)

    def embed_batch(self, texts: list[str]) -> np.ndarray:
        if not texts:
            return np.zeros((0, self.dims), dtype=np.float32)
        out = []
        for start in range(0, len(texts), BATCH):
            chunk = texts[start : start + BATCH]
            encodings = self.tokenizer.encode_batch(chunk)
            ids = np.array([e.ids for e in encodings], dtype=np.int64)
            mask = np.array([e.attention_mask for e in encodings], dtype=np.int64)
            feeds = {"input_ids": ids, "attention_mask": mask}
            if "token_type_ids" in self._inputs:
                feeds["token_type_ids"] = np.array([e.type_ids for e in encodings], dtype=np.int64)
            hidden = self.session.run([self._output], feeds)[0]
            vectors = l2_normalize(mean_pool(hidden, mask))
            # Text with nothing in it says nothing: the zero vector, as the
            # lexical fallback gives, not the model's reading of two markers.
            for i, t in enumerate(chunk):
                if not t.strip():
                    vectors[i] = 0.0
            out.append(vectors)
        return np.vstack(out)


class EncoderManager:
    """Resolves which encoder to use and produces embeddings with it."""

    def __init__(self, encoders_dir: Path | None = None, manifests_dir: Path | None = None, *, load=None):
        self.encoders_dir = encoders_dir
        self.manifests_dir = manifests_dir
        self._provider = LEXICAL_PROVIDER
        self._dim = LEXICAL_DIM
        self._encoder: OnnxEncoder | None = None
        self._reason = "no text encoder manifest is present; using the lexical fallback"
        self._load = load or OnnxEncoder.load
        self._resolve()

    def manifests(self) -> list[tuple[Path, dict[str, Any] | None, str | None]]:
        """Every manifest, read, or why it could not be."""
        if not self.manifests_dir or not self.manifests_dir.is_dir():
            return []
        out = []
        for path in sorted(self.manifests_dir.glob("*.json")):
            try:
                out.append((path, read_manifest(path), None))
            except (EncoderUnavailableError, ValueError, OSError) as e:
                out.append((path, None, str(e)))
        return out

    def _resolve(self) -> None:
        found = [(p, m) for p, m, _ in self.manifests() if m is not None]
        if not found:
            return
        reasons = []
        for _path, manifest in found:
            folder = (self.encoders_dir / manifest["id"]) if self.encoders_dir else None
            problem = installed_problem(manifest, folder) if folder else "there is no encoders folder"
            if problem:
                reasons.append(f"{manifest['name']}: {problem}")
                continue
            try:
                self._encoder = self._load(folder, int(manifest["dims"]), int(manifest["maxTokens"]))
            except Exception as e:
                reasons.append(f"{manifest['name']}: could not be loaded ({e})")
                continue
            self._provider = manifest["id"]
            self._dim = int(manifest["dims"])
            self._reason = f"{manifest['name']}, downloaded and verified, runs on this computer"
            return
        self._reason = "; ".join(reasons) + "; using the lexical fallback"

    def status(self) -> dict[str, Any]:
        return {
            "provider": self._provider,
            "dims": self._dim,
            "semantic": self._encoder is not None,
            "reason": self._reason,
            "manifests": [p.name for p, _, _ in self.manifests()],
        }

    def embed(self, text: str) -> np.ndarray:
        return self.embed_batch([text])[0]

    def embed_batch(self, texts: list[str]) -> np.ndarray:
        if self._encoder is not None:
            return self._encoder.embed_batch(texts)
        if not texts:
            return np.zeros((0, self._dim), dtype=np.float32)
        return np.vstack([lexical_embedding(t, self._dim) for t in texts])

    @staticmethod
    def read_manifest(path: Path) -> dict[str, Any]:
        return read_manifest(path)
