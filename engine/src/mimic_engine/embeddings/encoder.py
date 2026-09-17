"""Visual embedding provider (spec §11.1 C, §23).

Two providers, chosen at runtime:
* `onnx:<id>` — a pinned ONNX image encoder from `models/manifests/*.json`,
  downloaded on first need, SHA-256 verified, run with onnxruntime (DirectML
  if available on Windows, CPU otherwise).
* `stats_v1` — mandatory fallback: a 64-d embedding built from a coarse
  colour/luminance layout grid. Always available, no downloads.

Embeddings are stored as float32 `.npy` files in the embeddings cache, never
in SQLite. The provider id is recorded alongside so mixed providers are never
compared.
"""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
from typing import Any

import numpy as np
from PIL import Image

from mimic_engine.artifacts.hashing import atomic_write_bytes, sha256_file

STATS_PROVIDER = "stats_v1"
STATS_DIM = 64


class EncoderUnavailableError(Exception):
    pass


def stats_embedding(rgb_u8: np.ndarray) -> np.ndarray:
    """4x4 grid of mean luminance + 4x4 grid of chroma (rg, by) + 16 global bins → 64-d, L2-normalised."""
    im = Image.fromarray(rgb_u8).resize((64, 64), Image.Resampling.BILINEAR)
    a = np.asarray(im, dtype=np.float32) / 255.0
    lum = 0.2126 * a[..., 0] + 0.7152 * a[..., 1] + 0.0722 * a[..., 2]
    grid = lum.reshape(4, 16, 4, 16).mean(axis=(1, 3)).ravel()  # 16
    rg = (a[..., 0] - a[..., 1]).reshape(4, 16, 4, 16).mean(axis=(1, 3)).ravel()  # 16
    by = (a[..., 2] - (a[..., 0] + a[..., 1]) / 2).reshape(4, 16, 4, 16).mean(axis=(1, 3)).ravel()  # 16
    hist, _ = np.histogram(lum, bins=16, range=(0, 1))
    hist = hist / lum.size  # 16
    v = np.concatenate([grid, rg * 2, by * 2, hist * 4]).astype(np.float32)
    n = float(np.linalg.norm(v))
    return v / n if n > 0 else v


class EncoderManager:
    def __init__(self, encoders_dir: str | Path | None, manifests_dir: str | Path | None):
        self.encoders_dir = Path(encoders_dir) if encoders_dir else None
        self.manifests_dir = Path(manifests_dir) if manifests_dir else None
        self._session = None
        self._manifest: dict[str, Any] | None = None
        self._provider = STATS_PROVIDER
        self._reason = "no ONNX encoder configured; using stats_v1"
        self._probe()

    @property
    def provider(self) -> str:
        return self._provider

    def status(self) -> dict[str, Any]:
        return {
            "provider": self._provider,
            "reason": self._reason,
            "onnxAvailable": self._onnx_available(),
            "manifest": {
                k: self._manifest[k]
                for k in ("id", "version", "sha256", "dim")
                if self._manifest and k in self._manifest
            }
            if self._manifest
            else None,
            "executionProvider": self._execution_provider(),
        }

    @staticmethod
    def _onnx_available() -> bool:
        try:
            import onnxruntime  # noqa: F401

            return True
        except Exception:
            return False

    def _execution_provider(self) -> str | None:
        if self._session is None:
            return None
        try:
            return ",".join(self._session.get_providers())
        except Exception:
            return None

    def _load_manifests(self) -> list[dict[str, Any]]:
        if not self.manifests_dir or not self.manifests_dir.is_dir():
            return []
        out = []
        for p in sorted(self.manifests_dir.glob("*.json")):
            try:
                m = json.loads(p.read_text("utf-8"))
                if m.get("kind") == "image_encoder":
                    out.append(m)
            except (OSError, json.JSONDecodeError):
                continue
        return out

    def _probe(self) -> None:
        manifests = self._load_manifests()
        if not manifests:
            self._reason = "no encoder manifest found; using stats_v1"
            return
        if not self._onnx_available():
            self._reason = "onnxruntime not installed; using stats_v1"
            return
        m = manifests[0]
        self._manifest = m
        if not self.encoders_dir:
            self._reason = "no encoder cache directory; using stats_v1"
            return
        model_path = self.encoders_dir / m["fileName"]
        if not model_path.is_file():
            self._reason = f"encoder {m['id']} not downloaded (Settings > Performance); using stats_v1"
            return
        digest = sha256_file(model_path)
        if digest != m["sha256"]:
            self._reason = f"encoder {m['id']} failed SHA-256 verification; using stats_v1"
            return
        try:
            import onnxruntime as ort

            providers = ["CPUExecutionProvider"]
            if "DmlExecutionProvider" in ort.get_available_providers():
                providers.insert(0, "DmlExecutionProvider")
            self._session = ort.InferenceSession(str(model_path), providers=providers)
            self._provider = f"onnx:{m['id']}@{m['version']}"
            self._reason = "ONNX encoder loaded"
        except Exception as e:
            self._session = None
            self._reason = f"encoder failed to load ({type(e).__name__}); using stats_v1"

    def embed(self, rgb_u8: np.ndarray) -> np.ndarray:
        if self._session is None or self._manifest is None:
            return stats_embedding(rgb_u8)
        m = self._manifest
        size = int(m.get("inputSize", 224))
        im = Image.fromarray(rgb_u8).resize((size, size), Image.Resampling.BILINEAR)
        x = np.asarray(im, dtype=np.float32) / 255.0
        mean = np.asarray(m.get("mean", [0.485, 0.456, 0.406]), dtype=np.float32)
        std = np.asarray(m.get("std", [0.229, 0.224, 0.225]), dtype=np.float32)
        x = (x - mean) / std
        x = np.transpose(x, (2, 0, 1))[None].astype(np.float32)
        inp = self._session.get_inputs()[0].name
        out = self._session.run(None, {inp: x})[0]
        v = np.asarray(out, dtype=np.float32).reshape(-1)
        n = float(np.linalg.norm(v))
        return v / n if n > 0 else v


def embedding_artifact_id(provider: str, fast_hash: str) -> str:
    h = hashlib.sha1(f"{provider}|{fast_hash}".encode()).hexdigest()[:24]
    return f"emb_{h}.npy"


def save_embedding(cache_dir: str | Path, artifact_id: str, vector: np.ndarray, provider: str) -> str:
    path = Path(cache_dir) / artifact_id
    buf = np.asarray(vector, dtype=np.float32)
    import io

    bio = io.BytesIO()
    np.save(bio, buf, allow_pickle=False)
    atomic_write_bytes(path, bio.getvalue())
    atomic_write_bytes(path.with_suffix(".json"), json.dumps({"provider": provider, "dim": int(buf.size)}).encode())
    return str(path)


def load_embedding(cache_dir: str | Path, artifact_id: str) -> tuple[np.ndarray, str | None]:
    path = Path(cache_dir) / artifact_id
    vec = np.load(path, allow_pickle=False)
    provider = None
    meta = path.with_suffix(".json")
    if meta.is_file():
        try:
            provider = json.loads(meta.read_text("utf-8")).get("provider")
        except (OSError, json.JSONDecodeError):
            provider = None
    return vec, provider
