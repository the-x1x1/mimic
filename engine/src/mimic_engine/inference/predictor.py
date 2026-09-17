"""Load a trained model version and predict canonical settings + confidence."""

from __future__ import annotations

from pathlib import Path
from typing import Any

import joblib
import numpy as np

from mimic_engine.confidence.score import ConfidenceCalibration, score
from mimic_engine.features.stats import flat_vector
from mimic_engine.training.mapping import control_by_canonical, raw_for


class Predictor:
    def __init__(self, model_path: str | Path):
        bundle = joblib.load(model_path)
        self.hybrid = bundle["hybrid"]
        self.control_names: list[str] = bundle["controlNames"]
        self.feature_names: list[str] = bundle["featureNames"]
        self.calibration = ConfidenceCalibration.from_dict(bundle["calibration"])
        self.embedding_provider: str | None = bundle.get("embeddingProvider")
        self.train_asset_ids: list[str] = bundle.get("trainAssetIds", [])
        self.controls = [control_by_canonical(n) for n in self.control_names]

    def predict_one(
        self, stats: dict[str, Any], metadata: dict[str, Any], embedding: np.ndarray | None, camera: str, lens: str
    ) -> dict[str, Any]:
        names, vec = flat_vector(stats, metadata)
        if names != self.feature_names:
            raise ValueError("feature schema mismatch between model and features")
        x = np.asarray([vec], dtype=np.float32)
        e = None
        if self.hybrid.use_embedding:
            if embedding is None:
                raise ValueError("model requires an embedding but none was provided")
            e = np.asarray([embedding], dtype=np.float32)
        parts = self.hybrid.predict_parts(x, e, [(camera, lens)])
        z = self.hybrid.features(x, e)
        idx, dist = self.hybrid.knn.neighbours(z)
        present = np.ones(len(self.control_names), dtype=bool)
        conf = score(
            self.calibration,
            dist[0],
            parts["knn"][0],
            parts["hybrid"][0],
            parts["conditioned_median"][0],
            present,
            camera,
            lens,
            metadata.get("iso"),
        )
        settings: dict[str, dict[str, Any]] = {}
        for j, name in enumerate(self.control_names):
            c = self.controls[j]
            if c is None:
                continue
            fam, short = name.split(".", 1)
            value = float(parts["hybrid"][0][j])
            settings.setdefault(fam, {})[short] = {
                "value": round(value, 5),
                "raw": raw_for(c, value),
                "sourceKey": c["lightroomKeys"][0],
            }
        nearest = [
            {
                "assetId": self.train_asset_ids[i] if i < len(self.train_asset_ids) else None,
                "distance": round(float(d), 4),
            }
            for i, d in zip(idx[0].tolist(), dist[0].tolist(), strict=True)
        ]
        return {
            "global": settings,
            "confidence": conf["confidence"],
            "confidenceComponents": conf["components"],
            "ood": conf["ood"],
            "reasons": conf["reasons"],
            "nearestExamples": nearest,
            "rawModelOutput": {
                k: [round(float(v), 5) for v in parts[k][0]]
                for k in ("knn", "residual", "hybrid", "conditioned_median", "global_median")
            },
            "controlNames": self.control_names,
        }
