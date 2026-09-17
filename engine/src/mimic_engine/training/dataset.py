"""Training-set construction from the Mimic SQLite database (read-only).

A training pair is an asset with (a) an observed edit snapshot (xmp or
lightroom_sdk) normalized by mimic-core and (b) `features_v1` visual features.
Targets are the normalized 0..1 values of the *predictable* numeric controls
from `edit_mapping_v1.json`; a per-control presence mask distinguishes
"not present" from zero. Filters follow spec §41.
"""

from __future__ import annotations

import json
import sqlite3
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

import numpy as np

from mimic_engine.features.stats import FEATURE_VERSION, flat_vector
from mimic_engine.training.mapping import predictable_controls

MIN_PAIRS = 30
MAX_UNKNOWN_KEYS = 40


@dataclass
class Pair:
    asset_id: str
    library_id: str | None
    group_key: str
    camera: str
    lens: str
    captured_at: str | None
    features: np.ndarray
    embedding: np.ndarray | None
    target: np.ndarray
    present: np.ndarray
    source: str


@dataclass
class Dataset:
    pairs: list[Pair]
    feature_names: list[str]
    control_names: list[str]
    embedding_provider: str | None
    excluded: dict[str, int] = field(default_factory=dict)
    warnings: list[str] = field(default_factory=list)

    def __len__(self) -> int:
        return len(self.pairs)

    def matrix(self) -> tuple[np.ndarray, np.ndarray, np.ndarray, np.ndarray | None]:
        x = np.stack([p.features for p in self.pairs]) if self.pairs else np.zeros((0, len(self.feature_names)))
        y = np.stack([p.target for p in self.pairs]) if self.pairs else np.zeros((0, len(self.control_names)))
        m = np.stack([p.present for p in self.pairs]) if self.pairs else np.zeros((0, len(self.control_names)), bool)
        e = None
        if self.pairs and all(p.embedding is not None for p in self.pairs):
            e = np.stack([p.embedding for p in self.pairs])  # type: ignore[arg-type]
        return x, y, m, e

    def fingerprint(self) -> str:
        import hashlib

        h = hashlib.sha256()
        for p in sorted(self.pairs, key=lambda p: p.asset_id):
            h.update(p.asset_id.encode())
            h.update(p.target.astype(np.float32).tobytes())
        h.update(",".join(self.control_names).encode())
        return h.hexdigest()


def open_readonly(db_path: str | Path) -> sqlite3.Connection:
    uri = f"file:{Path(db_path).as_posix()}?mode=ro"
    conn = sqlite3.connect(uri, uri=True)
    conn.row_factory = sqlite3.Row
    return conn


def _group_key(row: sqlite3.Row, library_index: dict[str, int], is_correction: bool = False) -> str:
    """Shoot identity used for leakage-free splits: library + capture day (fallback: folder).

    The library is identified by its position in the requested list rather than
    its UUID so the seeded split is reproducible across databases holding the
    same photos.
    """
    lib = "C" if is_correction else f"L{library_index.get(row['library_id'], 0)}"
    day = (row["captured_at"] or "")[:10]
    if day:
        return f"{lib}|{day}"
    folder = Path(row["source_path"]).parent.name
    return f"{lib}|{folder}"


def _stats_from_row(row: sqlite3.Row) -> dict[str, Any]:
    return {
        "histogram": json.loads(row["histogram_json"]),
        "luminance": json.loads(row["luminance_json"]),
        "color": json.loads(row["color_json"]),
        "clipping": json.loads(row["clipping_json"]),
        "sharpness": row["sharpness"] if row["sharpness"] is not None else 0.0,
        "noiseEstimate": row["noise_estimate"] if row["noise_estimate"] is not None else 0.0,
    }


def load_dataset(
    db_path: str | Path,
    library_ids: list[str],
    embeddings_dir: str | Path | None,
    correction_asset_ids: list[str] | None = None,
) -> Dataset:
    """Load training pairs from the Style's libraries plus, optionally, assets
    outside those libraries whose latest observed snapshot is a `correction`
    (the photographer's final edit after a Mimic apply). Correction assets are
    grouped as their own shoot key (`C|<day>`) so the split treats a corrected
    session like any other shoot."""
    controls = predictable_controls()
    control_names = [c["canonical"] for c in controls]
    excluded: dict[str, int] = {
        "noSnapshot": 0,
        "noFeatures": 0,
        "noMeaningfulEdits": 0,
        "tooManyUnknownKeys": 0,
        "badFeatures": 0,
    }
    library_index = {lib: i for i, lib in enumerate(library_ids)}
    pairs: list[Pair] = []
    provider: str | None = None
    provider_conflict = False
    correction_ids = [str(a) for a in (correction_asset_ids or [])]
    correction_set = set(correction_ids)
    with open_readonly(db_path) as conn:
        placeholders = ",".join("?" for _ in library_ids)
        extra_clause = ""
        extra_args: list[str] = []
        if correction_ids:
            extra_clause = f" OR a.id IN ({','.join('?' for _ in correction_ids)})"
            extra_args = correction_ids
        rows = conn.execute(
            f"""
            SELECT a.id AS asset_id, a.library_id, a.source_path, a.camera_make, a.camera_model, a.lens, a.iso, a.aperture,
                   a.shutter_speed, a.focal_length, a.captured_at,
                   s.normalized_settings_json, s.unknown_settings_json, s.source,
                   f.histogram_json, f.luminance_json, f.color_json, f.clipping_json, f.sharpness, f.noise_estimate, f.embedding_artifact_id
            FROM assets a
            LEFT JOIN edit_snapshots s ON s.id = (
                SELECT id FROM edit_snapshots WHERE asset_id = a.id AND source IN ('xmp','lightroom_sdk','correction')
                ORDER BY observed_at DESC LIMIT 1)
            LEFT JOIN visual_features f ON f.asset_id = a.id AND f.feature_version = ?
            WHERE a.library_id IN ({placeholders}){extra_clause}
            ORDER BY a.captured_at, a.file_name
            """,
            [FEATURE_VERSION, *library_ids, *extra_args],
        ).fetchall()
    feature_names: list[str] = []
    for row in rows:
        is_correction = row["asset_id"] in correction_set and row["library_id"] not in library_index
        if is_correction and row["source"] != "correction":
            # Only the photographer's final edit counts; a bare prediction never trains itself.
            excluded["notCorrected"] = excluded.get("notCorrected", 0) + 1
            continue
        if row["normalized_settings_json"] is None:
            excluded["noSnapshot"] += 1
            continue
        if row["histogram_json"] is None:
            excluded["noFeatures"] += 1
            continue
        normalized = json.loads(row["normalized_settings_json"])
        unknown = json.loads(row["unknown_settings_json"] or "{}")
        if len(unknown) > MAX_UNKNOWN_KEYS:
            excluded["tooManyUnknownKeys"] += 1
            continue
        target = np.zeros(len(controls), dtype=np.float32)
        present = np.zeros(len(controls), dtype=bool)
        meaningful = 0
        glob = normalized.get("global", {})
        for i, c in enumerate(controls):
            family, short = c["canonical"].split(".", 1)
            cv = glob.get(family, {}).get(short)
            if cv is None or cv.get("value") is None:
                continue
            target[i] = float(cv["value"])
            present[i] = True
            default = c.get("default")
            if default is None or abs(float(cv.get("raw", 0.0) or 0.0) - float(default)) > 1e-9:
                meaningful += 1
        if meaningful == 0:
            excluded["noMeaningfulEdits"] += 1
            continue
        metadata = {
            "iso": row["iso"],
            "aperture": row["aperture"],
            "shutterSpeed": row["shutter_speed"],
            "focalLength": row["focal_length"],
        }
        try:
            names, vec = flat_vector(_stats_from_row(row), metadata)
        except (KeyError, TypeError, ValueError):
            excluded["badFeatures"] += 1
            continue
        if not feature_names:
            feature_names = names
        embedding = None
        if embeddings_dir and row["embedding_artifact_id"]:
            path = Path(embeddings_dir) / row["embedding_artifact_id"]
            meta = path.with_suffix(".json")
            if path.is_file():
                try:
                    embedding = np.load(path, allow_pickle=False).astype(np.float32)
                    prov = json.loads(meta.read_text("utf-8")).get("provider") if meta.is_file() else None
                    if provider is None:
                        provider = prov
                    elif prov != provider:
                        provider_conflict = True
                        embedding = None
                except (OSError, ValueError):
                    embedding = None
        pairs.append(
            Pair(
                asset_id=row["asset_id"],
                library_id=row["library_id"],
                group_key=_group_key(row, library_index, is_correction),
                camera=f"{row['camera_make'] or ''} {row['camera_model'] or ''}".strip() or "unknown",
                lens=row["lens"] or "unknown",
                captured_at=row["captured_at"],
                features=np.asarray(vec, dtype=np.float32),
                embedding=embedding,
                target=target,
                present=present,
                source=row["source"],
            )
        )
    warnings: list[str] = []
    if provider_conflict:
        warnings.append("embeddings from mixed providers were found; embeddings disabled for this training set")
        for p in pairs:
            p.embedding = None
        provider = None
    missing_emb = sum(1 for p in pairs if p.embedding is None)
    if pairs and 0 < missing_emb < len(pairs):
        warnings.append(f"{missing_emb} pairs lack an embedding; embeddings disabled for this training set")
        for p in pairs:
            p.embedding = None
        provider = None
    ds = Dataset(
        pairs=pairs,
        feature_names=feature_names or flat_vector_names(),
        control_names=control_names,
        embedding_provider=provider,
        excluded=excluded,
        warnings=warnings,
    )
    return ds


def flat_vector_names() -> list[str]:
    zero = {
        "histogram": {"luminance": [0.0] * 32},
        "luminance": {
            "mean": 0,
            "std": 0,
            "percentiles": {f"p{p}": 0 for p in (1, 5, 25, 50, 75, 95, 99)},
            "dynamicRange": 0,
            "centerBorderDelta": 0,
        },
        "color": {
            "channelMeans": [0, 0, 0],
            "grayWorldGain": [1, 1, 1],
            "castRedGreen": 0,
            "castBlueYellow": 0,
            "saturationMean": 0,
            "saturationP95": 0,
            "skyLikeFraction": 0,
        },
        "clipping": {"highlights": 0, "shadows": 0},
        "sharpness": 0,
        "noiseEstimate": 0,
    }
    return flat_vector(zero, {})[0]
