"""Per-control and per-family metrics (spec §14). Values are reported in
normalized units (0..1 of range) AND raw units where a range exists."""

from __future__ import annotations

from typing import Any

import numpy as np

from mimic_engine.training.mapping import predictable_controls


def control_metrics(
    y_true: np.ndarray, y_pred: np.ndarray, present: np.ndarray, control_names: list[str]
) -> dict[str, Any]:
    controls = {c["canonical"]: c for c in predictable_controls()}
    per_control: dict[str, Any] = {}
    fam_err: dict[str, list[float]] = {}
    fam_n: dict[str, int] = {}
    all_abs: list[float] = []
    for j, name in enumerate(control_names):
        rows = present[:, j]
        n = int(rows.sum())
        if n == 0:
            per_control[name] = {"n": 0}
            continue
        err = y_pred[rows, j] - y_true[rows, j]
        abs_err = np.abs(err)
        c = controls.get(name, {})
        span = (c.get("range", {}).get("max", 1) - c.get("range", {}).get("min", 0)) if c.get("range") else 1.0
        per_control[name] = {
            "n": n,
            "nMae": float(abs_err.mean()),
            "nRmse": float(np.sqrt((err**2).mean())),
            "mae": float(abs_err.mean() * span),
            "rmse": float(np.sqrt((err**2).mean()) * span),
            "p50": float(np.percentile(abs_err, 50) * span),
            "p90": float(np.percentile(abs_err, 90) * span),
            "p95": float(np.percentile(abs_err, 95) * span),
            "unit": "raw" if c.get("range") else "normalized",
        }
        fam = name.split(".", 1)[0]
        fam_err.setdefault(fam, []).extend(abs_err.tolist())
        fam_n[fam] = fam_n.get(fam, 0) + n
        all_abs.extend(abs_err.tolist())
    per_family = {
        fam: {"n": fam_n[fam], "nMae": float(np.mean(v)), "p90": float(np.percentile(v, 90))}
        for fam, v in fam_err.items()
        if v
    }
    overall = {
        "n": len(all_abs),
        "nMae": float(np.mean(all_abs)) if all_abs else None,
        "p90": float(np.percentile(all_abs, 90)) if all_abs else None,
    }
    return {"overall": overall, "perFamily": per_family, "perControl": per_control}


def acceptance_proxy(
    y_true: np.ndarray, y_pred: np.ndarray, present: np.ndarray, control_names: list[str], tolerance: float = 0.05
) -> dict[str, Any]:
    """Fraction of photos whose every present high-impact control is within tolerance
    (normalized). This is a PROXY, never reported as No-Touch Rate (spec §14.3)."""
    high_impact = [i for i, n in enumerate(control_names) if n.split(".")[0] in ("whiteBalance", "tone", "presence")]
    if not high_impact or y_true.shape[0] == 0:
        return {"rate": None, "tolerance": tolerance, "n": 0}
    ok = 0
    for r in range(y_true.shape[0]):
        cols = [j for j in high_impact if present[r, j]]
        if not cols:
            continue
        if all(abs(y_pred[r, j] - y_true[r, j]) <= tolerance for j in cols):
            ok += 1
    return {
        "rate": ok / y_true.shape[0],
        "tolerance": tolerance,
        "n": int(y_true.shape[0]),
        "note": "proxy: high-impact families within tolerance; not an observed No-Touch Rate",
    }
