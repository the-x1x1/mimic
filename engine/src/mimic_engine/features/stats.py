"""Deterministic image statistics (spec §11.1 B). NumPy only.

All values are computed on a bounded sRGB preview normalised to 0..1. The
output is versioned by FEATURE_VERSION; change the version when any number
here changes meaning.
"""

from __future__ import annotations

from typing import Any

import numpy as np

FEATURE_VERSION = "features_v1"
HIST_BINS = 32
PERCENTILES = (1, 5, 25, 50, 75, 95, 99)


def _luminance(rgb: np.ndarray) -> np.ndarray:
    return 0.2126 * rgb[..., 0] + 0.7152 * rgb[..., 1] + 0.0722 * rgb[..., 2]


def _rgb_to_hsv_sat(rgb: np.ndarray) -> np.ndarray:
    mx = rgb.max(axis=-1)
    mn = rgb.min(axis=-1)
    with np.errstate(divide="ignore", invalid="ignore"):
        sat = np.where(mx > 1e-6, (mx - mn) / np.maximum(mx, 1e-6), 0.0)
    return sat


def _laplacian_var(gray: np.ndarray) -> float:
    g = gray
    lap = -4 * g[1:-1, 1:-1] + g[:-2, 1:-1] + g[2:, 1:-1] + g[1:-1, :-2] + g[1:-1, 2:]
    return float(lap.var())


def _noise_estimate(gray: np.ndarray) -> float:
    """MAD of a high-pass residual in flat regions (robust to edges)."""
    g = gray
    box = (
        g[:-2, :-2]
        + g[:-2, 1:-1]
        + g[:-2, 2:]
        + g[1:-1, :-2]
        + g[1:-1, 1:-1]
        + g[1:-1, 2:]
        + g[2:, :-2]
        + g[2:, 1:-1]
        + g[2:, 2:]
    ) / 9.0
    resid = g[1:-1, 1:-1] - box
    gy = np.abs(np.diff(g, axis=0))[:-1, 1:-1]
    gx = np.abs(np.diff(g, axis=1))[1:-1, :-1]
    grad = gy + gx
    flat = grad < np.percentile(grad, 40)
    r = resid[flat] if flat.any() else resid
    mad = np.median(np.abs(r - np.median(r)))
    return float(1.4826 * mad)


def compute_stats(rgb_u8: np.ndarray) -> dict[str, Any]:
    if rgb_u8.ndim != 3 or rgb_u8.shape[2] != 3:
        raise ValueError("expected HxWx3 RGB array")
    rgb = rgb_u8.astype(np.float32) / 255.0
    lum = _luminance(rgb)
    h, w = lum.shape

    hist, _ = np.histogram(lum, bins=HIST_BINS, range=(0.0, 1.0))
    hist = hist / max(1, lum.size)
    lum_pct = np.percentile(lum, PERCENTILES)
    chan_pct = {c: np.percentile(rgb[..., i], (5, 50, 95)).tolist() for i, c in enumerate(("r", "g", "b"))}
    means = rgb.reshape(-1, 3).mean(axis=0)
    gray_world = means / max(float(means.mean()), 1e-6)
    sat = _rgb_to_hsv_sat(rgb)
    # Simple opponent-space cast estimate.
    cast_rg = float(means[0] - means[1])
    cast_by = float(means[2] - (means[0] + means[1]) / 2.0)
    clip_hi = float((lum >= 0.98).mean())
    clip_lo = float((lum <= 0.02).mean())
    chan_clip_hi = [float((rgb[..., i] >= 0.995).mean()) for i in range(3)]
    dr = float(lum_pct[-1] - lum_pct[0])
    contrast = float(lum.std())
    # Centre vs border luminance (backlight proxy).
    cy, cx = h // 2, w // 2
    ch, cw = max(1, h // 4), max(1, w // 4)
    center = float(lum[cy - ch : cy + ch, cx - cw : cx + cw].mean())
    border = float(
        np.concatenate(
            [lum[: h // 8].ravel(), lum[-h // 8 :].ravel(), lum[:, : w // 8].ravel(), lum[:, -w // 8 :].ravel()]
        ).mean()
    )
    top = lum[: max(1, h // 3)]
    top_rgb = rgb[: max(1, h // 3)]
    sky_like = float(((top_rgb[..., 2] > top_rgb[..., 0] + 0.05) & (top > 0.45)).mean())

    return {
        "featureVersion": FEATURE_VERSION,
        "width": int(w),
        "height": int(h),
        "histogram": {"bins": HIST_BINS, "luminance": hist.round(6).tolist()},
        "luminance": {
            "mean": float(lum.mean()),
            "std": contrast,
            "percentiles": dict(zip((f"p{p}" for p in PERCENTILES), lum_pct.round(6).tolist(), strict=True)),
            "dynamicRange": dr,
            "center": center,
            "border": border,
            "centerBorderDelta": center - border,
        },
        "color": {
            "channelMeans": means.round(6).tolist(),
            "channelPercentiles": chan_pct,
            "grayWorldGain": gray_world.round(6).tolist(),
            "castRedGreen": cast_rg,
            "castBlueYellow": cast_by,
            "saturationMean": float(sat.mean()),
            "saturationP95": float(np.percentile(sat, 95)),
            "skyLikeFraction": sky_like,
        },
        "clipping": {"highlights": clip_hi, "shadows": clip_lo, "channelHighlights": chan_clip_hi},
        "sharpness": _laplacian_var(lum),
        "noiseEstimate": _noise_estimate(lum),
    }


def flat_vector(stats: dict[str, Any], metadata: dict[str, Any] | None = None) -> tuple[list[str], list[float]]:
    """Fixed-order numeric vector used by KNN/regressors. Names are stable per FEATURE_VERSION."""
    names: list[str] = []
    values: list[float] = []

    def add(name: str, v: Any) -> None:
        names.append(name)
        try:
            f = float(v)
        except (TypeError, ValueError):
            f = 0.0
        if f != f:
            f = 0.0
        values.append(f)

    lum = stats["luminance"]
    col = stats["color"]
    clip = stats["clipping"]
    for i, hv in enumerate(stats["histogram"]["luminance"]):
        add(f"hist_{i}", hv)
    add("lum_mean", lum["mean"])
    add("lum_std", lum["std"])
    for k, v in lum["percentiles"].items():
        add(f"lum_{k}", v)
    add("lum_dr", lum["dynamicRange"])
    add("lum_center_border", lum["centerBorderDelta"])
    for i, v in enumerate(col["channelMeans"]):
        add(f"mean_{'rgb'[i]}", v)
    for i, v in enumerate(col["grayWorldGain"]):
        add(f"gw_{'rgb'[i]}", v)
    add("cast_rg", col["castRedGreen"])
    add("cast_by", col["castBlueYellow"])
    add("sat_mean", col["saturationMean"])
    add("sat_p95", col["saturationP95"])
    add("sky_frac", col["skyLikeFraction"])
    add("clip_hi", clip["highlights"])
    add("clip_lo", clip["shadows"])
    add("sharpness", np.log1p(stats["sharpness"] * 1000.0))
    add("noise", stats["noiseEstimate"] * 100.0)
    md = metadata or {}
    add("iso_log", np.log2(max(float(md.get("iso") or 100), 25.0) / 100.0))
    add("aperture", float(md.get("aperture") or 0.0))
    add("shutter_log", -np.log2(max(float(md.get("shutterSpeed") or (1 / 125)), 1e-5)))
    add("focal_log", np.log2(max(float(md.get("focalLength") or 35.0), 1.0)))
    add("ev_bias", float(md.get("exposureBias") or 0.0))
    return names, values
