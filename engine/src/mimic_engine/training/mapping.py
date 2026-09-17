"""Access to edit_mapping_v1.json from the engine (ranges for denormalization and metrics)."""

from __future__ import annotations

import json
import os
from functools import lru_cache
from pathlib import Path
from typing import Any


def _candidates() -> list[Path]:
    out = []
    env = os.environ.get("MIMIC_EDIT_MAPPING")
    if env:
        out.append(Path(env))
    here = Path(__file__).resolve()
    # repository layout: engine/src/mimic_engine/training/mapping.py → packages/contracts
    out.append(here.parents[4] / "packages" / "contracts" / "edit_mapping_v1.json")
    # packaged layout: next to the executable / resources
    out.append(Path(getattr(__import__("sys"), "_MEIPASS", here.parent)) / "edit_mapping_v1.json")
    out.append(Path(__import__("sys").executable).parent / "edit_mapping_v1.json")
    out.append(here.parent / "edit_mapping_v1.json")
    return out


@lru_cache(maxsize=1)
def load_mapping() -> dict[str, Any]:
    for p in _candidates():
        if p.is_file():
            return json.loads(p.read_text("utf-8"))
    raise FileNotFoundError("edit_mapping_v1.json not found; set MIMIC_EDIT_MAPPING")


@lru_cache(maxsize=1)
def predictable_controls() -> list[dict[str, Any]]:
    m = load_mapping()
    fams = m["families"]
    return [c for c in m["controls"] if fams[c["family"]]["predictable"] and c["valueType"] in ("float", "int")]


def control_by_canonical(canonical: str) -> dict[str, Any] | None:
    for c in load_mapping()["controls"]:
        if c["canonical"] == canonical:
            return c
    return None


def denormalize(control: dict[str, Any], value: float) -> float:
    r = control.get("range")
    if not r:
        return value
    v = min(1.0, max(0.0, float(value)))
    if control.get("normalize") == "log":
        import math

        lo, hi = math.log10(r["min"]), math.log10(r["max"])
        return 10 ** (lo + v * (hi - lo))
    return r["min"] + v * (r["max"] - r["min"])


def raw_for(control: dict[str, Any], value: float) -> float | int:
    raw = denormalize(control, value)
    if control["valueType"] == "int":
        return round(raw)
    return round(raw, 3)
