"""Path helpers."""

from __future__ import annotations

import os
from pathlib import Path


def normalize_path(p: str | os.PathLike[str]) -> str:
    """Forward slashes; case-folded on Windows-style paths (mirrors mimic-core::db::normalize_path)."""
    s = str(p).replace("\\", "/")
    while len(s) > 1 and s.endswith("/"):
        s = s[:-1]
    if os.name == "nt" or (len(s) > 1 and s[1] == ":"):
        s = s.lower()
    return s


def ensure_dir(p: str | os.PathLike[str]) -> Path:
    path = Path(p)
    path.mkdir(parents=True, exist_ok=True)
    return path
