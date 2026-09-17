"""Synthetic training database for tests.

Creates a real Mimic SQLite database using the checked-in migration and fills
it with N synthetic photographs whose edits are a deterministic function of
their features plus noise, so a learner that uses the features must beat the
global median and a leaky split would be detectable.
"""

from __future__ import annotations

import json
import sqlite3
import uuid
from datetime import UTC, datetime, timedelta
from pathlib import Path

import numpy as np

from mimic_engine.features.stats import FEATURE_VERSION
from mimic_engine.training.mapping import predictable_controls

REPO = Path(__file__).resolve().parents[2]
MIGRATION = REPO / "crates" / "mimic-core" / "src" / "db" / "migrations" / "0001_init.sql"


def normalize_value(control, raw):
    r = control["range"]
    if control.get("normalize") == "log":
        import math

        return (math.log10(max(raw, r["min"])) - math.log10(r["min"])) / (math.log10(r["max"]) - math.log10(r["min"]))
    return min(1.0, max(0.0, (raw - r["min"]) / (r["max"] - r["min"])))


def build_db(
    path: Path, n: int = 160, shoots: int = 8, seed: int = 7, cameras=("Canon EOS R6", "SONY ILCE-7M4")
) -> dict:
    rng = np.random.default_rng(seed)
    conn = sqlite3.connect(path)
    conn.executescript(MIGRATION.read_text("utf-8"))
    conn.execute(
        "CREATE TABLE IF NOT EXISTS schema_migrations(version INTEGER PRIMARY KEY, name TEXT NOT NULL, applied_at TEXT NOT NULL)"
    )
    conn.execute("INSERT INTO schema_migrations VALUES (1,'init','2026-01-01T00:00:00Z')")
    now = datetime(2025, 1, 1, tzinfo=UTC)
    lib_id = str(uuid.uuid4())
    conn.execute(
        "INSERT INTO libraries(id,name,source_type,root_path,created_at,status) VALUES (?,?,?,?,?,?)",
        (lib_id, "synthetic", "folder_sidecars", "/synthetic", now.isoformat(), "scanned"),
    )
    controls = {c["canonical"]: c for c in predictable_controls()}
    exposure, contrast, temp, shadows, vib = (
        controls["tone.exposure"],
        controls["tone.contrast"],
        controls["whiteBalance.temperature"],
        controls["tone.shadows"],
        controls["presence.vibrance"],
    )
    asset_ids = []
    for i in range(n):
        shoot = i % shoots
        day = now + timedelta(days=shoot * 3)
        lum = float(rng.uniform(0.15, 0.85))
        cast = float(rng.normal(0, 0.05))
        cam = cameras[shoot % len(cameras)]
        iso = int(rng.choice([100, 200, 400, 800, 1600, 3200]))
        # Photographer's rule: brighten dark frames, cool warm casts, lift shadows in low light, shoot-level style offset.
        style_offset = (shoot % 3 - 1) * 0.15
        exp_raw = float(np.clip(1.2 * (0.5 - lum) + style_offset + rng.normal(0, 0.05), -5, 5))
        con_raw = float(np.clip(10 + 40 * (0.5 - abs(0.5 - lum)) + rng.normal(0, 3), -100, 100))
        temp_raw = float(np.clip(5500 - 8000 * cast + rng.normal(0, 60), 2000, 50000))
        sha_raw = float(np.clip(60 * (0.5 - lum) + 20 + rng.normal(0, 4), -100, 100))
        vib_raw = float(np.clip(15 + rng.normal(0, 2), -100, 100))
        aid = str(uuid.uuid4())
        asset_ids.append(aid)
        captured = (day + timedelta(minutes=i)).isoformat(timespec="seconds")
        conn.execute(
            "INSERT INTO assets(id,library_id,source_path,normalized_path,file_name,extension,size_bytes,fast_hash,camera_make,camera_model,lens,iso,aperture,shutter_speed,focal_length,captured_at,width,height,created_at,updated_at) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
            (
                aid,
                lib_id,
                f"/synthetic/s{shoot}/IMG_{i:04d}.CR3",
                f"/synthetic/s{shoot}/img_{i:04d}.cr3",
                f"IMG_{i:04d}.CR3",
                "cr3",
                1000,
                f"fh1:{i}",
                cam.split()[0],
                " ".join(cam.split()[1:]),
                "50mm",
                iso,
                2.8,
                1 / 200,
                50.0,
                captured,
                6000,
                4000,
                captured,
                captured,
            ),
        )
        hist = np.exp(-((np.linspace(0, 1, 32) - lum) ** 2) / 0.02)
        hist = (hist / hist.sum()).round(6).tolist()
        pct = {f"p{p}": float(np.clip(lum + (p - 50) / 100 * 0.6, 0, 1)) for p in (1, 5, 25, 50, 75, 95, 99)}
        stats_rows = (
            json.dumps({"bins": 32, "luminance": hist}),
            json.dumps(
                {
                    "mean": lum,
                    "std": 0.2,
                    "percentiles": pct,
                    "dynamicRange": 0.8,
                    "center": lum,
                    "border": lum,
                    "centerBorderDelta": 0.0,
                }
            ),
            json.dumps(
                {
                    "channelMeans": [lum + cast, lum, lum - cast],
                    "channelPercentiles": {},
                    "grayWorldGain": [1 + cast, 1, 1 - cast],
                    "castRedGreen": cast,
                    "castBlueYellow": -cast,
                    "saturationMean": 0.3,
                    "saturationP95": 0.6,
                    "skyLikeFraction": 0.1,
                }
            ),
            0.002,
            0.01,
            json.dumps({"highlights": 0.0, "shadows": 0.0, "channelHighlights": [0, 0, 0]}),
        )
        conn.execute(
            "INSERT INTO visual_features(asset_id,feature_version,histogram_json,luminance_json,color_json,sharpness,noise_estimate,clipping_json,scene_labels_json,computed_at) VALUES (?,?,?,?,?,?,?,?,?,?)",
            (aid, FEATURE_VERSION, *stats_rows, "{}", captured),
        )
        normalized = {
            "schemaVersion": "1.0",
            "mappingVersion": "edit_mapping_v1",
            "global": {
                "tone": {
                    "exposure": {
                        "raw": round(exp_raw, 2),
                        "value": normalize_value(exposure, exp_raw),
                        "sourceKey": "Exposure2012",
                    },
                    "contrast": {
                        "raw": int(con_raw),
                        "value": normalize_value(contrast, con_raw),
                        "sourceKey": "Contrast2012",
                    },
                    "shadows": {
                        "raw": int(sha_raw),
                        "value": normalize_value(shadows, sha_raw),
                        "sourceKey": "Shadows2012",
                    },
                },
                "whiteBalance": {
                    "temperature": {
                        "raw": int(temp_raw),
                        "value": normalize_value(temp, temp_raw),
                        "sourceKey": "Temperature",
                    }
                },
                "presence": {
                    "vibrance": {"raw": int(vib_raw), "value": normalize_value(vib, vib_raw), "sourceKey": "Vibrance"}
                },
            },
            "local": {"status": "unsupported", "observedKeys": [], "operations": []},
            "lightroom": {"processVersion": "15.4", "heavyEditKeys": []},
            "unknown": {},
            "warnings": [],
            "rawSettingsHash": "x",
        }
        conn.execute(
            "INSERT INTO edit_snapshots(id,asset_id,source,process_version,normalized_settings_json,raw_settings_json,unknown_settings_json,mapping_version,observed_at,provenance_json) VALUES (?,?,?,?,?,?,?,?,?,?)",
            (
                str(uuid.uuid4()),
                aid,
                "xmp",
                "15.4",
                json.dumps(normalized),
                "{}",
                "{}",
                "edit_mapping_v1",
                captured,
                "{}",
            ),
        )
    # A few assets without edits and one with default-only edits (must be excluded).
    for j in range(3):
        aid = str(uuid.uuid4())
        conn.execute(
            "INSERT INTO assets(id,library_id,source_path,normalized_path,file_name,extension,size_bytes,fast_hash,created_at,updated_at) VALUES (?,?,?,?,?,?,?,?,?,?)",
            (
                aid,
                lib_id,
                f"/synthetic/noedit{j}.CR3",
                f"/synthetic/noedit{j}.cr3",
                f"noedit{j}.CR3",
                "cr3",
                1,
                f"fh1:ne{j}",
                now.isoformat(),
                now.isoformat(),
            ),
        )
    conn.commit()
    conn.close()
    return {"libraryId": lib_id, "assetIds": asset_ids, "n": n}
