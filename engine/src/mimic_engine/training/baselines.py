"""Baseline hierarchy and the 0.x hybrid predictor (spec §11.3).

All models operate on a standardized feature matrix X (optionally with an
embedding concatenated) and a target matrix Y of normalized 0..1 control
values with a presence mask M. Missing targets never contribute to a fit.
Every model exposes `predict(X) -> Y_hat` with the same shape.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any

import numpy as np


def masked_median(y: np.ndarray, m: np.ndarray, fallback: float = 0.5) -> np.ndarray:
    out = np.full(y.shape[1], fallback, dtype=np.float32)
    for j in range(y.shape[1]):
        col = y[m[:, j], j]
        if col.size:
            out[j] = float(np.median(col))
    return out


@dataclass
class GlobalMedian:
    median: np.ndarray = field(default_factory=lambda: np.zeros(0))
    name: str = "global_median"

    def fit(
        self, x: np.ndarray, y: np.ndarray, m: np.ndarray, meta: list[tuple[str, str]] | None = None
    ) -> GlobalMedian:
        self.median = masked_median(y, m)
        return self

    def predict(self, x: np.ndarray, meta: list[tuple[str, str]] | None = None) -> np.ndarray:
        return np.tile(self.median, (x.shape[0], 1))


@dataclass
class ConditionedMedian:
    """Median per (camera, lens), falling back to camera, then global."""

    by_pair: dict[tuple[str, str], np.ndarray] = field(default_factory=dict)
    by_camera: dict[str, np.ndarray] = field(default_factory=dict)
    global_median: np.ndarray = field(default_factory=lambda: np.zeros(0))
    min_support: int = 5
    name: str = "camera_lens_median"

    def fit(
        self, x: np.ndarray, y: np.ndarray, m: np.ndarray, meta: list[tuple[str, str]] | None = None
    ) -> ConditionedMedian:
        meta = meta or [("unknown", "unknown")] * y.shape[0]
        self.global_median = masked_median(y, m)
        pairs: dict[tuple[str, str], list[int]] = {}
        cams: dict[str, list[int]] = {}
        for i, (cam, lens) in enumerate(meta):
            pairs.setdefault((cam, lens), []).append(i)
            cams.setdefault(cam, []).append(i)
        for key, idx in pairs.items():
            if len(idx) >= self.min_support:
                self.by_pair[key] = masked_median(y[idx], m[idx])
        for key, idx in cams.items():
            if len(idx) >= self.min_support:
                self.by_camera[key] = masked_median(y[idx], m[idx])
        return self

    def predict(self, x: np.ndarray, meta: list[tuple[str, str]] | None = None) -> np.ndarray:
        meta = meta or [("unknown", "unknown")] * x.shape[0]
        out = np.empty((x.shape[0], self.global_median.shape[0]), dtype=np.float32)
        for i, (cam, lens) in enumerate(meta):
            out[i] = self.by_pair.get((cam, lens), self.by_camera.get(cam, self.global_median))
        return out


@dataclass
class KnnRegressor:
    """Distance-weighted K nearest neighbours in standardized feature (+embedding) space."""

    k: int = 8
    x: np.ndarray = field(default_factory=lambda: np.zeros((0, 0)))
    y: np.ndarray = field(default_factory=lambda: np.zeros((0, 0)))
    m: np.ndarray = field(default_factory=lambda: np.zeros((0, 0), bool))
    name: str = "knn"

    def fit(self, x: np.ndarray, y: np.ndarray, m: np.ndarray, meta: Any = None) -> KnnRegressor:
        self.x, self.y, self.m = x.astype(np.float32), y.astype(np.float32), m.astype(bool)
        return self

    def neighbours(self, x: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
        """Return (indices, distances) of the k nearest training rows for each query."""
        k = min(self.k, self.x.shape[0])
        d = np.sqrt(np.maximum(((x[:, None, :] - self.x[None, :, :]) ** 2).sum(axis=2), 0.0))
        idx = np.argsort(d, axis=1)[:, :k]
        dist = np.take_along_axis(d, idx, axis=1)
        return idx, dist

    def predict(self, x: np.ndarray, meta: Any = None) -> np.ndarray:
        idx, dist = self.neighbours(x)
        w = 1.0 / (dist + 1e-3)
        out = np.empty((x.shape[0], self.y.shape[1]), dtype=np.float32)
        fallback = masked_median(self.y, self.m)
        for i in range(x.shape[0]):
            yi = self.y[idx[i]]
            mi = self.m[idx[i]]
            wi = w[i][:, None] * mi
            denom = wi.sum(axis=0)
            num = (wi * yi).sum(axis=0)
            out[i] = np.where(denom > 0, num / np.maximum(denom, 1e-9), fallback)
        return out


@dataclass
class RidgeResidual:
    """Per-control ridge regression on top of a base prediction (hybrid residual)."""

    alpha: float = 1.0
    coef: np.ndarray = field(default_factory=lambda: np.zeros((0, 0)))
    intercept: np.ndarray = field(default_factory=lambda: np.zeros(0))
    name: str = "ridge_residual"

    def fit(self, x: np.ndarray, residual: np.ndarray, m: np.ndarray, meta: Any = None) -> RidgeResidual:
        n, d = x.shape
        c = residual.shape[1]
        self.coef = np.zeros((c, d), dtype=np.float32)
        self.intercept = np.zeros(c, dtype=np.float32)
        xb = np.hstack([x, np.ones((n, 1), dtype=np.float32)])
        reg = np.eye(d + 1, dtype=np.float32) * self.alpha
        reg[-1, -1] = 0.0
        for j in range(c):
            rows = m[:, j]
            if rows.sum() < max(8, d // 4):
                continue  # not enough support: residual stays zero for this control
            a = xb[rows]
            b = residual[rows, j]
            try:
                w = np.linalg.solve(a.T @ a + reg, a.T @ b)
            except np.linalg.LinAlgError:
                continue
            self.coef[j] = w[:-1]
            self.intercept[j] = w[-1]
        return self

    def predict(self, x: np.ndarray, meta: Any = None) -> np.ndarray:
        return x @ self.coef.T + self.intercept


@dataclass
class Standardizer:
    mean: np.ndarray = field(default_factory=lambda: np.zeros(0))
    std: np.ndarray = field(default_factory=lambda: np.ones(0))

    def fit(self, x: np.ndarray) -> Standardizer:
        self.mean = x.mean(axis=0).astype(np.float32)
        self.std = (x.std(axis=0) + 1e-6).astype(np.float32)
        return self

    def transform(self, x: np.ndarray) -> np.ndarray:
        return ((x - self.mean) / self.std).astype(np.float32)


@dataclass
class HybridModel:
    """KNN retrieval + ridge residual, clipped to 0..1. The 0.x predictor."""

    scaler: Standardizer
    knn: KnnRegressor
    residual: RidgeResidual
    conditioned: ConditionedMedian
    global_median: GlobalMedian
    use_embedding: bool
    embedding_weight: float
    name: str = "hybrid_knn_residual"

    def features(self, x: np.ndarray, e: np.ndarray | None) -> np.ndarray:
        z = self.scaler.transform(x)
        if self.use_embedding and e is not None:
            return np.hstack([z, e.astype(np.float32) * self.embedding_weight * np.sqrt(z.shape[1])])
        return z

    def predict_parts(
        self, x: np.ndarray, e: np.ndarray | None, meta: list[tuple[str, str]] | None = None
    ) -> dict[str, np.ndarray]:
        z = self.features(x, e)
        knn = self.knn.predict(z)
        res = self.residual.predict(z)
        blend = np.clip(knn + res, 0.0, 1.0)
        return {
            "knn": knn,
            "residual": res,
            "hybrid": blend,
            "conditioned_median": self.conditioned.predict(x, meta),
            "global_median": self.global_median.predict(x),
        }

    def predict(self, x: np.ndarray, e: np.ndarray | None, meta: list[tuple[str, str]] | None = None) -> np.ndarray:
        return self.predict_parts(x, e, meta)["hybrid"]
