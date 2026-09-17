"""Session consistency policy (spec §11.7): a bounded pull toward the scene
group's median for families that should stay coherent across a scene.
Exposure and tone are never flattened; frame-specific decisions survive."""

from __future__ import annotations

import numpy as np

# family -> (blend toward cluster median, max normalized shift allowed)
POLICY: dict[str, tuple[float, float]] = {
    "whiteBalance": (0.5, 0.06),
    "colorGrading": (0.6, 0.08),
    "hsl": (0.5, 0.06),
    "calibration": (0.6, 0.05),
    "presence": (0.3, 0.04),
    "effects": (0.5, 0.06),
    "detail": (0.5, 0.06),
}


def apply_consistency(
    pred: np.ndarray, control_names: list[str], groups: list[str | None]
) -> tuple[np.ndarray, np.ndarray]:
    """Return (adjusted predictions, per-row max shift applied). `pred` is n x c normalized."""
    out = pred.copy()
    shift = np.zeros(pred.shape[0], dtype=np.float32)
    fam_idx: dict[str, list[int]] = {}
    for j, name in enumerate(control_names):
        fam_idx.setdefault(name.split(".", 1)[0], []).append(j)
    by_group: dict[str, list[int]] = {}
    for i, g in enumerate(groups):
        if g is not None:
            by_group.setdefault(g, []).append(i)
    for rows in by_group.values():
        if len(rows) < 3:
            continue
        for fam, (blend, cap) in POLICY.items():
            cols = fam_idx.get(fam)
            if not cols:
                continue
            med = np.median(pred[np.ix_(rows, cols)], axis=0)
            for i in rows:
                target = (1 - blend) * pred[i, cols] + blend * med
                delta = np.clip(target - pred[i, cols], -cap, cap)
                out[i, cols] = np.clip(pred[i, cols] + delta, 0.0, 1.0)
                shift[i] = max(shift[i], float(np.abs(delta).max()) if delta.size else 0.0)
    return out, shift
