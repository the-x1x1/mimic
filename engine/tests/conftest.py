from __future__ import annotations

import shutil
from pathlib import Path

import pytest

REPO = Path(__file__).resolve().parents[2]
FIXTURES = REPO / "fixtures"


@pytest.fixture(scope="session")
def fixtures_dir() -> Path:
    return FIXTURES


@pytest.fixture()
def photo_tree(tmp_path: Path) -> Path:
    """A small library tree exercising every pairing rule."""
    root = tmp_path / "lib"
    a = root / "2024-05-11 Wedding"
    b = root / "2025-06-21 Portraits"
    a.mkdir(parents=True)
    b.mkdir(parents=True)
    img = FIXTURES / "images" / "demo_landscape_sky.jpg"
    # RAW stand-ins are copies of a JPEG renamed; the scanner keys on extension only.
    shutil.copy(img, a / "IMG_1024.CR3")
    shutil.copy(FIXTURES / "xmp" / "simple_pv2012.xmp", a / "IMG_1024.xmp")
    shutil.copy(img, a / "IMG_1025.CR3")  # no sidecar
    shutil.copy(img, a / "IMG_1025.JPG")  # rendered twin
    shutil.copy(img, b / "DSC00001.ARW")
    shutil.copy(FIXTURES / "xmp" / "modern_masks_unknown.xmp", b / "DSC00001.xmp")
    (b / "DSC00001.acr").write_bytes(b"opaque")
    shutil.copy(img, b / "DSC00002.dng")  # DNG without sidecar
    shutil.copy(FIXTURES / "xmp" / "legacy_pv2010.xmp", b / "ORPHAN.xmp")
    (b / "notes.txt").write_text("ignored")
    (b / "weird.bin").write_bytes(b"\x00")
    shutil.copy(FIXTURES / "xmp" / "malformed_truncated.xmp", a / "IMG_1026.xmp")
    shutil.copy(img, a / "IMG_1026.CR3")
    return root
