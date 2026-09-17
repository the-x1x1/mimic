"""Bounded-resolution preview decoding with an on-disk cache (spec §21).

Order: embedded preview (rawpy thumbnail) → half-size LibRaw demosaic → Pillow.
The preview is for feature extraction only; it is never presented as a
Lightroom-accurate rendering.
"""

from __future__ import annotations

import contextlib
import io
from pathlib import Path

import numpy as np
from PIL import Image, ImageOps

from mimic_engine.artifacts.hashing import atomic_write_bytes

DECODER_VERSION = "preview_v1"
MAX_EDGE = 768

RAW_EXTENSIONS = {
    "cr2",
    "cr3",
    "nef",
    "nrw",
    "arw",
    "srf",
    "sr2",
    "raf",
    "orf",
    "rw2",
    "pef",
    "dng",
    "3fr",
    "fff",
    "iiq",
    "erf",
    "mrw",
    "x3f",
    "srw",
    "kdc",
    "dcr",
    "mef",
    "mos",
    "rwl",
}


class PreviewError(Exception):
    pass


def _fit(im: Image.Image, max_edge: int = MAX_EDGE) -> Image.Image:
    im = ImageOps.exif_transpose(im) or im
    if im.mode not in ("RGB", "L"):
        im = im.convert("RGB")
    if im.mode == "L":
        im = im.convert("RGB")
    im.thumbnail((max_edge, max_edge), Image.Resampling.LANCZOS)
    return im


def _decode_raw(path: Path) -> tuple[Image.Image, str]:
    try:
        import rawpy
    except ImportError as e:
        raise PreviewError("rawpy is not installed; RAW previews unavailable") from e
    with rawpy.imread(str(path)) as raw:
        try:
            thumb = raw.extract_thumb()
            if thumb.format == rawpy.ThumbFormat.JPEG:
                im = Image.open(io.BytesIO(thumb.data))
                im.load()
                if max(im.size) >= 480:
                    return _fit(im), "embedded_jpeg"
            elif thumb.format == rawpy.ThumbFormat.BITMAP:
                im = Image.fromarray(thumb.data)
                if max(im.size) >= 480:
                    return _fit(im), "embedded_bitmap"
        except Exception:
            pass
        rgb = raw.postprocess(half_size=True, use_camera_wb=True, no_auto_bright=True, output_bps=8, user_flip=None)
        return _fit(Image.fromarray(rgb)), "libraw_half"


def decode_preview(path: str | Path) -> tuple[Image.Image, str]:
    p = Path(path)
    ext = p.suffix.lower().lstrip(".")
    if ext in RAW_EXTENSIONS:
        try:
            return _decode_raw(p)
        except PreviewError:
            raise
        except Exception as e:
            # DNGs (and mislabeled files) may still open with Pillow.
            with contextlib.suppress(Exception), Image.open(p) as im:
                im.load()
                return _fit(im), "pillow_fallback"
            raise PreviewError(f"cannot decode RAW {p.name}: {type(e).__name__}: {e}") from e
    try:
        with Image.open(p) as im:
            im.load()
            return _fit(im), "pillow"
    except Exception as e:
        raise PreviewError(f"cannot decode {p.name}: {type(e).__name__}: {e}") from e


def cached_preview(
    path: str | Path, cache_dir: str | Path | None, fast_hash: str | None
) -> tuple[np.ndarray, str | None, str]:
    """Return (uint8 RGB array, cached jpeg path or None, decoder used)."""
    key = f"{(fast_hash or 'nohash').replace(':', '_')}.{DECODER_VERSION}.jpg"
    cache_path = Path(cache_dir) / key if cache_dir else None
    if cache_path and cache_path.is_file():
        try:
            with Image.open(cache_path) as im:
                im.load()
                return np.asarray(im.convert("RGB")), str(cache_path), "cache"
        except Exception:
            pass
    im, decoder = decode_preview(path)
    arr = np.asarray(im.convert("RGB"))
    if cache_path:
        buf = io.BytesIO()
        im.save(buf, format="JPEG", quality=88)
        try:
            atomic_write_bytes(cache_path, buf.getvalue())
        except OSError:
            cache_path = None
    return arr, (str(cache_path) if cache_path else None), decoder
