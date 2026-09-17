"""JSON helpers: numpy-safe dumps and a stable hash."""

from __future__ import annotations

import hashlib
import json
from typing import Any

import numpy as np


def to_jsonable(value: Any) -> Any:
    """Convert numpy scalars/arrays and other non-JSON types recursively."""
    if isinstance(value, dict):
        return {str(k): to_jsonable(v) for k, v in value.items()}
    if isinstance(value, list | tuple):
        return [to_jsonable(v) for v in value]
    if isinstance(value, np.ndarray):
        return [to_jsonable(v) for v in value.tolist()]
    if isinstance(value, np.generic):
        out = value.item()
        if isinstance(out, float) and (out != out or out in (float("inf"), float("-inf"))):
            return None
        return out
    if isinstance(value, float) and (value != value or value in (float("inf"), float("-inf"))):
        return None
    return value


def dumps(value: Any) -> str:
    return json.dumps(to_jsonable(value), ensure_ascii=False, separators=(",", ":"))


def stable_hash(value: Any) -> str:
    """SHA-256 of canonical JSON (sorted keys). Matches mimic-core's stable_json_hash for plain data."""
    canon = json.dumps(to_jsonable(value), sort_keys=True, separators=(",", ":"), ensure_ascii=False)
    return hashlib.sha256(canon.encode("utf-8")).hexdigest()
