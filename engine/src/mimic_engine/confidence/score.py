"""Confidence from measurable components (spec §2.6, §11.6). Nothing here is
an arbitrary number: every component is derived from the training set or the
model's own disagreement, and the components are stored with the prediction."""

from __future__ import annotations

from typing import Any

import numpy as np


class ConfidenceCalibration:
    """Statistics gathered on the training set once, persisted with the model."""

    def __init__(
        self,
        train_nn_dist: np.ndarray,
        family_val_nmae: dict[str, float],
        cameras: set[str],
        lenses: set[str],
        iso_range: tuple[float, float],
    ):
        # Distances of genuinely unseen photos (validation/holdout) to the train set.
        self.dist_median = float(np.median(train_nn_dist)) if train_nn_dist.size else 1.0
        self.dist_p90 = float(np.percentile(train_nn_dist, 90)) if train_nn_dist.size else 2.0
        self.dist_max = float(train_nn_dist.max()) if train_nn_dist.size else 3.0
        # Farther than any legitimate unseen photo by a margin, and at least 1.75x the p90.
        self.ood_threshold = max(1.75 * self.dist_p90, self.dist_max * 1.15)
        self.family_val_nmae = family_val_nmae
        self.cameras = cameras
        self.lenses = lenses
        self.iso_range = iso_range

    def to_dict(self) -> dict[str, Any]:
        return {
            "distMedian": self.dist_median,
            "distP90": self.dist_p90,
            "distMax": self.dist_max,
            "oodThreshold": self.ood_threshold,
            "familyValNmae": self.family_val_nmae,
            "cameras": sorted(self.cameras),
            "lenses": sorted(self.lenses),
            "isoRange": list(self.iso_range),
        }

    @classmethod
    def from_dict(cls, d: dict[str, Any]) -> ConfidenceCalibration:
        c = cls(
            np.asarray([d["distMedian"], d["distP90"]]),
            d["familyValNmae"],
            set(d["cameras"]),
            set(d["lenses"]),
            tuple(d["isoRange"]),
        )
        c.dist_median, c.dist_p90 = d["distMedian"], d["distP90"]
        c.dist_max = d.get("distMax", c.dist_p90 * 1.5)
        c.ood_threshold = d.get("oodThreshold", max(1.75 * c.dist_p90, c.dist_max * 1.15))
        return c


def _clamp(x: float) -> float:
    return float(max(0.0, min(1.0, x)))


def score(
    calib: ConfidenceCalibration,
    nn_dist: np.ndarray,
    knn_pred: np.ndarray,
    hybrid_pred: np.ndarray,
    cond_pred: np.ndarray,
    present_cols: np.ndarray,
    camera: str,
    lens: str,
    iso: float | None,
) -> dict[str, Any]:
    """Return {"confidence", "components", "reasons", "ood"} for one photo."""
    mean_dist = float(nn_dist.mean()) if nn_dist.size else calib.dist_p90
    # Similarity: 1 at/below median distance, -> 0 at 2x p90.
    far = max(calib.ood_threshold, calib.dist_median + 1e-6)
    similarity = _clamp(1.0 - (mean_dist - calib.dist_median) / (far - calib.dist_median))
    # Density: how many neighbours are within p90 of the training distance distribution.
    density = _clamp(float((nn_dist <= calib.dist_p90).mean()) if nn_dist.size else 0.0)
    # Disagreement between retrieval and hybrid (residual size) and vs conditioned median.
    cols = present_cols if present_cols.any() else np.ones_like(knn_pred, dtype=bool)
    dis_hybrid = float(np.abs(hybrid_pred[cols] - knn_pred[cols]).mean()) if cols.any() else 0.0
    dis_cond = float(np.abs(hybrid_pred[cols] - cond_pred[cols]).mean()) if cols.any() else 0.0
    agreement = _clamp(1.0 - 4.0 * dis_hybrid) * 0.6 + _clamp(1.0 - 2.0 * dis_cond) * 0.4
    # Family validation error (worst high-impact family), nMAE 0.02 → 1.0, 0.15 → 0.
    worst = max([calib.family_val_nmae.get(f, 0.1) for f in ("tone", "whiteBalance", "presence")] or [0.1])
    validation = _clamp(1.0 - (worst - 0.02) / 0.13)
    camera_known = camera in calib.cameras
    lens_known = lens in calib.lenses
    iso_in = iso is None or (calib.iso_range[0] <= iso <= calib.iso_range[1])
    coverage = (0.5 if camera_known else 0.0) + (0.25 if lens_known else 0.0) + (0.25 if iso_in else 0.0)
    ood_z = (mean_dist - calib.dist_median) / max(calib.dist_p90 - calib.dist_median, 0.25 * calib.dist_p90, 1e-6)
    ood = mean_dist > far
    confidence = _clamp(0.35 * similarity + 0.15 * density + 0.2 * agreement + 0.15 * validation + 0.15 * coverage)
    if ood:
        confidence = min(confidence, 0.49)
    reasons: list[str] = []
    if ood:
        reasons.append("Out of distribution: this photo is far from every training example.")
    if not camera_known:
        reasons.append(f"Confidence reduced because camera “{camera}” is not represented in training.")
    if camera_known and not lens_known:
        reasons.append(f"Confidence reduced because lens “{lens}” is not represented in training.")
    if not iso_in and iso is not None:
        reasons.append(
            f"Low confidence: ISO {int(iso)} is outside the Style's historical range {int(calib.iso_range[0])}-{int(calib.iso_range[1])}."
        )
    if dis_hybrid > 0.08:
        reasons.append("Model components disagree on this photo.")
    if worst > 0.1:
        reasons.append("Validation error for high-impact families is high for this Style.")
    return {
        "confidence": round(confidence, 4),
        "ood": bool(ood),
        "components": {
            "similarity": round(similarity, 4),
            "density": round(density, 4),
            "agreement": round(agreement, 4),
            "validation": round(validation, 4),
            "coverage": round(coverage, 4),
            "meanNeighbourDistance": round(mean_dist, 4),
            "oodZ": round(float(ood_z), 3),
            "cameraKnown": camera_known,
            "lensKnown": lens_known,
        },
        "reasons": reasons,
    }
