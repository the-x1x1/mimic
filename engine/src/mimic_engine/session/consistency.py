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


# When the photographer marks a reference photo for a group, the group is
# pulled toward that photo instead of its median, with a stronger blend but
# the same per-family caps (never a copy).
REFERENCE_BLEND = 0.8

# Controls whose disagreement with the group marks a photo as an outlier, and
# the normalized distance from the group median that counts as "far".
OUTLIER_CONTROLS = ("tone.exposure", "whiteBalance.temperature", "whiteBalance.tint")
OUTLIER_THRESHOLD = 0.12
OUTLIER_MIN_GROUP = 4


def _family_index(control_names: list[str]) -> dict[str, list[int]]:
    fam_idx: dict[str, list[int]] = {}
    for j, name in enumerate(control_names):
        fam_idx.setdefault(name.split(".", 1)[0], []).append(j)
    return fam_idx


def _by_group(groups: list[str | None]) -> dict[str, list[int]]:
    by_group: dict[str, list[int]] = {}
    for i, g in enumerate(groups):
        if g is not None:
            by_group.setdefault(g, []).append(i)
    return by_group


def apply_consistency(
    pred: np.ndarray,
    control_names: list[str],
    groups: list[str | None],
    references: dict[str, int] | None = None,
) -> tuple[np.ndarray, np.ndarray]:
    """Return (adjusted predictions, per-row max shift applied). `pred` is n x c
    normalized. `references` maps a group id to the row index of its reference
    photo; that row is left untouched and becomes the group's target."""
    out = pred.copy()
    shift = np.zeros(pred.shape[0], dtype=np.float32)
    fam_idx = _family_index(control_names)
    references = references or {}
    for gid, rows in _by_group(groups).items():
        ref = references.get(gid)
        if ref is not None and ref not in rows:
            ref = None
        if ref is None and len(rows) < 3:
            continue
        for fam, (blend, cap) in POLICY.items():
            cols = fam_idx.get(fam)
            if not cols:
                continue
            if ref is not None:
                target_vec = pred[ref, cols]
                b = REFERENCE_BLEND
            else:
                target_vec = np.median(pred[np.ix_(rows, cols)], axis=0)
                b = blend
            for i in rows:
                if i == ref:
                    continue
                target = (1 - b) * pred[i, cols] + b * target_vec
                delta = np.clip(target - pred[i, cols], -cap, cap)
                out[i, cols] = np.clip(pred[i, cols] + delta, 0.0, 1.0)
                shift[i] = max(shift[i], float(np.abs(delta).max()) if delta.size else 0.0)
    return out, shift


def detect_outliers(
    pred: np.ndarray, control_names: list[str], groups: list[str | None]
) -> list[tuple[str, float] | None]:
    """Per row: (control, normalized distance from the group median) for the
    control that deviates most when it exceeds OUTLIER_THRESHOLD, else None.
    Computed on the raw predictions (before consistency) inside groups of at
    least OUTLIER_MIN_GROUP photos. A flagged photo is not changed; it is
    surfaced for review because its scene group disagrees with it."""
    flags: list[tuple[str, float] | None] = [None] * pred.shape[0]
    cols = [(name, j) for j, name in enumerate(control_names) if name in OUTLIER_CONTROLS]
    if not cols:
        return flags
    for rows in _by_group(groups).values():
        if len(rows) < OUTLIER_MIN_GROUP:
            continue
        for name, j in cols:
            med = float(np.median(pred[rows, j]))
            for i in rows:
                d = abs(float(pred[i, j]) - med)
                if d > OUTLIER_THRESHOLD and (flags[i] is None or d > flags[i][1]):
                    flags[i] = (name, round(d, 4))
    return flags
