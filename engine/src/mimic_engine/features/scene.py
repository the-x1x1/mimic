"""Coarse heuristic scene labels (spec §11.1 D).

These are cheap proxies with confidences, not a classifier. They feed the
feature vector and the review UI ("why this edit"), and are never a single
point of failure: every label has a `confidence` and the model treats them
as soft features.
"""

from __future__ import annotations

from typing import Any

SCENE_LABELS_VERSION = "scene_heuristics_v1"


def _clamp(x: float) -> float:
    return max(0.0, min(1.0, float(x)))


def scene_labels(stats: dict[str, Any], metadata: dict[str, Any] | None = None) -> dict[str, Any]:
    md = metadata or {}
    lum = stats["luminance"]
    col = stats["color"]
    clip = stats["clipping"]
    iso = float(md.get("iso") or 0)
    focal = float(md.get("focalLength") or 0)
    aperture = float(md.get("aperture") or 0)
    shutter = float(md.get("shutterSpeed") or 0)

    low_light_c = _clamp((0.35 - lum["mean"]) * 3 + (0.5 if iso >= 3200 else 0.25 if iso >= 1600 else 0.0))
    high_key_c = _clamp((lum["mean"] - 0.62) * 4 + clip["highlights"] * 2)
    low_key_c = _clamp((0.28 - lum["mean"]) * 4 + clip["shadows"] * 2)
    backlit_c = _clamp((lum["border"] - lum["center"]) * 4 + clip["highlights"] * 1.5)
    sky_c = _clamp(col["skyLikeFraction"] * 1.6)
    # Portrait-ish: warm cast, moderate saturation, long-ish focal, wide aperture.
    portrait_c = _clamp(
        0.25 * (col["castRedGreen"] > 0.02)
        + 0.25 * (50 <= focal <= 200)
        + 0.3 * (0 < aperture <= 2.8)
        + 0.2 * (col["saturationMean"] < 0.35)
    )
    landscape_c = _clamp(0.4 * sky_c + 0.3 * (focal and focal <= 35) + 0.3 * (aperture >= 5.6))
    closeup_c = _clamp(
        0.4 * (focal >= 85)
        + 0.3 * (0 < aperture <= 2.0)
        + 0.3 * (stats["sharpness"] > 0.004 and lum["centerBorderDelta"] > 0.05)
    )
    outdoor_c = _clamp(0.5 * sky_c + 0.3 * (iso and iso <= 400) + 0.2 * (shutter and shutter <= 1 / 250))
    indoor_c = _clamp(1.0 - outdoor_c - 0.2 * low_light_c)

    labels = {
        "lowLight": low_light_c,
        "highKey": high_key_c,
        "lowKey": low_key_c,
        "backlit": backlit_c,
        "skyDominant": sky_c,
        "landscape": landscape_c,
        "portraitLike": portrait_c,
        "closeUp": closeup_c,
        "outdoor": outdoor_c,
        "indoor": indoor_c,
    }
    primary = max(labels.items(), key=lambda kv: kv[1])
    return {
        "version": SCENE_LABELS_VERSION,
        "labels": {k: round(v, 4) for k, v in labels.items()},
        "primary": primary[0] if primary[1] >= 0.5 else "unclassified",
        "method": "heuristic",
    }
