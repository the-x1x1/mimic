"""Regenerate XMP golden files. Run from engine/: `uv run python -m tests.regen_golden`."""

import json
from pathlib import Path

from mimic_engine.xmp.parser import parse_xmp_file

ROOT = Path(__file__).resolve().parents[2]

if __name__ == "__main__":
    for name in ["simple_pv2012", "modern_masks_unknown", "legacy_pv2010", "no_crs_metadata_only"]:
        r = parse_xmp_file(ROOT / "fixtures" / "xmp" / f"{name}.xmp")
        out = ROOT / "fixtures" / "expected" / f"{name}.raw.json"
        out.write_text(
            json.dumps(
                {"parserVersion": r["parserVersion"], "rawSettings": r["rawSettings"], "metadata": r["metadata"]},
                indent=2,
                ensure_ascii=False,
            )
            + "\n",
            "utf-8",
        )
        print("wrote", out)
