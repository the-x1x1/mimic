"""Folder scanner: finds source photos and pairs them with XMP/ACR sidecars (spec §9.2).

Pairing rule: a sidecar belongs to the photo in the SAME directory whose stem
matches (case-insensitive on Windows-style paths). Duplicate stems in one
directory (e.g. `IMG_1.CR3` and `IMG_1.JPG`) are reported and the sidecar is
attached to the RAW candidate first, then reported as ambiguous if two RAWs
share a stem. Nothing is ever written.
"""

from __future__ import annotations

import os
from collections.abc import Callable, Iterator
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

from mimic_engine.artifacts.hashing import fast_hash
from mimic_engine.raw.metadata import read_metadata

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
RENDERED_EXTENSIONS = {"jpg", "jpeg", "tif", "tiff", "heic", "heif", "psd"}
SUPPORTED_EXTENSIONS = RAW_EXTENSIONS | RENDERED_EXTENSIONS
SIDECAR_EXTENSIONS = {"xmp": "xmp", "acr": "acr"}
MIME = {
    "jpg": "image/jpeg",
    "jpeg": "image/jpeg",
    "tif": "image/tiff",
    "tiff": "image/tiff",
    "dng": "image/x-adobe-dng",
    "heic": "image/heic",
    "heif": "image/heif",
    "psd": "image/vnd.adobe.photoshop",
}
SKIP_DIRS = {".git", "node_modules", "$RECYCLE.BIN", "System Volume Information", ".lrdata", "Lightroom Settings"}


def _iso(ts: float) -> str:
    return datetime.fromtimestamp(ts, tz=UTC).isoformat(timespec="milliseconds").replace("+00:00", "Z")


def iter_files(roots: list[str]) -> Iterator[Path]:
    for root in roots:
        rp = Path(root)
        if rp.is_file():
            yield rp
            continue
        for dirpath, dirnames, filenames in os.walk(rp):
            dirnames[:] = [d for d in dirnames if d not in SKIP_DIRS and not d.endswith(".lrdata")]
            for name in filenames:
                yield Path(dirpath) / name


def scan_folders(
    roots: list[str],
    *,
    include_metadata: bool = True,
    progress: Callable[[str, int, int], None] | None = None,
    max_files: int = 250_000,
) -> dict[str, Any]:
    photos: dict[tuple[str, str], list[Path]] = {}  # (dir, stem-lower) -> candidate photo files
    sidecars: dict[tuple[str, str], list[Path]] = {}
    unsupported: list[dict[str, Any]] = []
    total_seen = 0

    for path in iter_files(roots):
        total_seen += 1
        if total_seen > max_files:
            unsupported.append({"path": str(path), "reason": f"scan stopped after {max_files} files"})
            break
        name = path.name
        if name.startswith("._") or name.startswith("."):
            continue
        ext = path.suffix.lower().lstrip(".")
        key = (str(path.parent), path.stem.lower())
        if ext in SUPPORTED_EXTENSIONS:
            photos.setdefault(key, []).append(path)
        elif ext in SIDECAR_EXTENSIONS:
            sidecars.setdefault(key, []).append(path)
        elif ext in {"lrcat", "lrcat-data", "lrdata", "db", "txt", "json", "md", "pdf", "mp4", "mov", "lock"}:
            continue
        else:
            unsupported.append(
                {"path": str(path), "reason": f"unsupported extension .{ext}" if ext else "no extension"}
            )
        if progress and total_seen % 500 == 0:
            progress("scanning", total_seen, 0)

    assets: list[dict[str, Any]] = []
    duplicate_basenames: list[dict[str, Any]] = []
    orphan_sidecars: list[str] = []
    keys = sorted(photos)
    total = len(keys)
    raw_count = 0
    with_xmp = 0
    with_acr = 0
    dng_without_sidecar = 0

    for i, key in enumerate(keys):
        candidates = photos[key]
        raws = [p for p in candidates if p.suffix.lower().lstrip(".") in RAW_EXTENSIONS]
        rendered = [p for p in candidates if p not in raws]
        side = sidecars.pop(key, [])
        if len(raws) > 1:
            duplicate_basenames.append(
                {
                    "directory": key[0],
                    "stem": key[1],
                    "files": [str(p) for p in raws],
                    "reason": "two RAW files share a basename; sidecar pairing is ambiguous",
                }
            )
        elif len(candidates) > 1:
            duplicate_basenames.append(
                {
                    "directory": key[0],
                    "stem": key[1],
                    "files": [str(p) for p in candidates],
                    "reason": "RAW and rendered file share a basename; sidecar attached to the RAW",
                }
            )
        # Sidecar goes to the first RAW; rendered files without a RAW get it themselves.
        owner_order = raws + rendered
        for j, p in enumerate(owner_order):
            ext = p.suffix.lower().lstrip(".")
            try:
                st = p.stat()
                fh = fast_hash(p)
            except OSError as e:
                unsupported.append({"path": str(p), "reason": f"unreadable: {e}"})
                continue
            entry: dict[str, Any] = {
                "sourcePath": str(p),
                "fileName": p.name,
                "extension": ext,
                "mimeType": MIME.get(ext, f"image/x-{ext}"),
                "sizeBytes": st.st_size,
                "modifiedTime": _iso(st.st_mtime),
                "fastHash": fh,
                "isRaw": ext in RAW_EXTENSIONS,
                "sidecars": [],
                "metadata": {},
            }
            if ext in RAW_EXTENSIONS:
                raw_count += 1
            if j == 0:
                for s in side:
                    kind = s.suffix.lower().lstrip(".")
                    try:
                        sst = s.stat()
                        shash = fast_hash(s)
                    except OSError:
                        continue
                    entry["sidecars"].append(
                        {"type": kind, "path": str(s), "modifiedTime": _iso(sst.st_mtime), "hash": shash}
                    )
                    if kind == "xmp":
                        with_xmp += 1
                    elif kind == "acr":
                        with_acr += 1
                if ext == "dng" and not any(sc["type"] == "xmp" for sc in entry["sidecars"]):
                    dng_without_sidecar += 1
                    entry["notes"] = [
                        "DNG without XMP sidecar: develop settings may be embedded in the DNG (not read in 0.x)"
                    ]
            if include_metadata:
                try:
                    entry["metadata"] = read_metadata(p)
                except Exception as e:
                    entry["metadata"] = {}
                    entry.setdefault("notes", []).append(f"metadata unreadable: {type(e).__name__}")
            assets.append(entry)
        if progress and (i % 50 == 0 or i + 1 == total):
            progress("pairing", i + 1, total)

    for leftover in sidecars.values():
        orphan_sidecars.extend(str(p) for p in leftover)

    return {
        "assets": assets,
        "unsupported": unsupported,
        "duplicateBasenames": duplicate_basenames,
        "orphanSidecars": sorted(orphan_sidecars),
        "stats": {
            "filesSeen": total_seen,
            "photos": len(assets),
            "rawFiles": raw_count,
            "withXmp": with_xmp,
            "withAcr": with_acr,
            "dngWithoutSidecar": dng_without_sidecar,
            "unsupported": len(unsupported),
            "orphanSidecars": len(orphan_sidecars),
        },
    }
