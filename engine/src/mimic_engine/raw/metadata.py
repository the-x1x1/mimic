"""EXIF/capture metadata extraction (read-only) with exifread + Pillow fallback."""

from __future__ import annotations

import re
from datetime import datetime
from fractions import Fraction
from pathlib import Path
from typing import Any

FIELDS = (
    "cameraMake",
    "cameraModel",
    "lens",
    "focalLength",
    "iso",
    "aperture",
    "shutterSpeed",
    "capturedAt",
    "width",
    "height",
    "orientation",
    "exposureBias",
    "flash",
)


def _ratio(v: Any) -> float | None:
    try:
        if hasattr(v, "num") and hasattr(v, "den"):
            return float(v.num) / float(v.den) if v.den else None
        if isinstance(v, int | float):
            return float(v)
        s = str(v).strip()
        if "/" in s:
            return float(Fraction(s))
        return float(s)
    except (ValueError, ZeroDivisionError, TypeError):
        return None


def _first(values: Any) -> Any:
    if isinstance(values, list | tuple) and values:
        return values[0]
    return values


def _parse_exif_datetime(s: str) -> str | None:
    s = s.strip()
    for fmt in ("%Y:%m:%d %H:%M:%S", "%Y-%m-%dT%H:%M:%S", "%Y-%m-%d %H:%M:%S"):
        try:
            return datetime.strptime(s[:19], fmt).isoformat(timespec="seconds")
        except ValueError:
            continue
    return None


def read_metadata(path: str | Path) -> dict[str, Any]:
    p = Path(path)
    out: dict[str, Any] = {}
    try:
        import exifread

        with open(p, "rb") as f:
            tags = exifread.process_file(f, details=False, stop_tag="JPEGThumbnail")
    except Exception:
        tags = {}

    def tag(*names: str) -> Any:
        for n in names:
            if n in tags:
                return tags[n]
        return None

    def sval(t: Any) -> str | None:
        if t is None:
            return None
        s = str(t.printable if hasattr(t, "printable") else t).strip()
        return s or None

    def rval(t: Any) -> float | None:
        if t is None:
            return None
        vals = getattr(t, "values", None)
        if vals is not None:
            return _ratio(_first(vals))
        return _ratio(t)

    out["cameraMake"] = sval(tag("Image Make"))
    out["cameraModel"] = sval(tag("Image Model"))
    out["lens"] = sval(tag("EXIF LensModel", "MakerNote LensModel", "EXIF LensSpecification"))
    out["focalLength"] = rval(tag("EXIF FocalLength"))
    iso = rval(tag("EXIF ISOSpeedRatings", "EXIF PhotographicSensitivity"))
    out["iso"] = int(iso) if iso else None
    out["aperture"] = rval(tag("EXIF FNumber"))
    out["shutterSpeed"] = rval(tag("EXIF ExposureTime"))
    dt = sval(tag("EXIF DateTimeOriginal", "EXIF DateTimeDigitized", "Image DateTime"))
    out["capturedAt"] = _parse_exif_datetime(dt) if dt else None
    w = rval(tag("EXIF ExifImageWidth", "Image ImageWidth"))
    h = rval(tag("EXIF ExifImageLength", "Image ImageLength"))
    out["width"] = int(w) if w else None
    out["height"] = int(h) if h else None
    orient = tag("Image Orientation")
    out["orientation"] = int(_first(orient.values)) if orient is not None and getattr(orient, "values", None) else None
    out["exposureBias"] = rval(tag("EXIF ExposureBiasValue"))
    flash = tag("EXIF Flash")
    out["flash"] = sval(flash)

    if out["width"] is None or out["cameraMake"] is None:
        try:
            from PIL import Image

            with Image.open(p) as im:
                out["width"] = out["width"] or im.width
                out["height"] = out["height"] or im.height
                exif = im.getexif()
                out["cameraMake"] = out["cameraMake"] or (str(exif.get(271)).strip() if exif.get(271) else None)
                out["cameraModel"] = out["cameraModel"] or (str(exif.get(272)).strip() if exif.get(272) else None)
                if out["orientation"] is None and exif.get(274):
                    out["orientation"] = int(exif.get(274))
        except Exception:
            pass
    if (
        out.get("cameraModel")
        and out.get("cameraMake")
        and out["cameraModel"].lower().startswith(out["cameraMake"].lower())
    ):
        out["cameraModel"] = (
            re.sub(rf"^{re.escape(out['cameraMake'])}\s*", "", out["cameraModel"], flags=re.IGNORECASE)
            or out["cameraModel"]
        )
    return {k: v for k, v in out.items() if v is not None}
