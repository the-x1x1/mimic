import json
import shutil
import sqlite3
from pathlib import Path

import numpy as np
import pytest

from mimic_engine.inference.predictor import Predictor
from mimic_engine.training.dataset import load_dataset
from mimic_engine.training.split import grouped_split
from mimic_engine.training.trainer import InsufficientDataError, train
from tests.synth import build_db


@pytest.fixture(scope="module")
def synth_db(tmp_path_factory) -> tuple[Path, dict]:
    p = tmp_path_factory.mktemp("db") / "mimic.db"
    info = build_db(p, n=160, shoots=8)
    return p, info


def test_dataset_loads_and_filters(synth_db):
    db, info = synth_db
    ds = load_dataset(db, [info["libraryId"]], None)
    assert len(ds) == 160
    assert ds.excluded["noSnapshot"] == 3
    assert len(set(p.group_key for p in ds.pairs)) == 8
    x, y, m, e = ds.matrix()
    assert x.shape == (160, len(ds.feature_names)) and y.shape[1] == len(ds.control_names)
    j = ds.control_names.index("tone.exposure")
    assert m[:, j].all() and y[:, j].min() >= 0 and y[:, j].max() <= 1
    assert not m[:, ds.control_names.index("tone.blacks")].any(), "absent controls stay absent"
    assert e is None


def test_grouped_split_never_leaks_and_is_deterministic():
    keys = [f"lib|2025-01-{d:02d}" for d in range(1, 9) for _ in range(20)]
    a = grouped_split(keys, seed=1)
    b = grouped_split(keys, seed=1)
    assert (a.train, a.validation, a.holdout) == (b.train, b.validation, b.holdout)
    for bucket_a, bucket_b in ((a.train, a.validation), (a.train, a.holdout), (a.validation, a.holdout)):
        assert not ({keys[i] for i in bucket_a} & {keys[i] for i in bucket_b}), "shoot leaked across split"
    assert len(a.train) + len(a.validation) + len(a.holdout) == len(keys)
    assert a.strategy == "session_grouped" and a.holdout and a.validation
    c = grouped_split(keys, seed=2)
    assert c.holdout != a.holdout
    single = grouped_split(["lib|2025-01-01"] * 50)
    assert single.strategy == "time_ordered_single_group" and single.warnings
    two = grouped_split(["a"] * 30 + ["b"] * 10)
    assert two.strategy == "session_grouped_two_groups" and not two.validation


def test_train_is_reproducible_beats_baseline_and_predicts(synth_db, tmp_path):
    db, info = synth_db
    calls = []
    r1 = train(
        db,
        [info["libraryId"]],
        "style-1",
        "mv-1",
        tmp_path / "styles",
        None,
        {"seed": 42},
        progress=lambda *a: calls.append(a),
    )
    r2 = train(db, [info["libraryId"]], "style-1", "mv-2", tmp_path / "styles", None, {"seed": 42})
    assert calls and calls[-1][0] == "writing artifacts"
    assert r1["modelType"] == "hybrid_knn_residual"
    assert r1["trainingConfig"]["trainingDataFingerprint"] == r2["trainingConfig"]["trainingDataFingerprint"]
    assert r1["metrics"]["holdout"]["hybrid"]["overall"]["nMae"] == pytest.approx(
        r2["metrics"]["holdout"]["hybrid"]["overall"]["nMae"], abs=1e-6
    )
    hold = r1["metrics"]["holdout"]
    assert hold["n"] > 0 and r1["counts"]["train"] > r1["counts"]["holdout"]
    assert r1["split"]["strategy"] == "session_grouped"
    assert r1["beatsBaselines"]["beatsGlobalMedian"], (
        f"hybrid {hold['hybrid']['overall']['nMae']} vs median {hold['global_median']['overall']['nMae']}"
    )
    assert (
        hold["hybrid"]["perControl"]["tone.exposure"]["mae"]
        < hold["global_median"]["perControl"]["tone.exposure"]["mae"] * 0.9
    )
    assert hold["hybrid"]["perControl"]["tone.exposure"]["unit"] == "raw"
    assert hold["acceptanceProxy"]["rate"] is not None and "proxy" in hold["acceptanceProxy"]["note"]
    for a in r1["artifacts"]:
        assert Path(a["path"]).is_file() and len(a["sha256"]) == 64
    cfg = json.loads((Path(r1["artifactDir"]) / "training_config.json").read_text())
    assert cfg["seed"] == 42 and cfg["targetControls"] == r1["trainingConfig"]["targetControls"]

    # Predict on a training asset and on a synthetic out-of-distribution asset.
    model_path = next(a["path"] for a in r1["artifacts"] if a["kind"] == "model")
    pred = Predictor(model_path)
    with sqlite3.connect(db) as conn:
        conn.row_factory = sqlite3.Row
        row = conn.execute(
            "SELECT a.*, f.* FROM assets a JOIN visual_features f ON f.asset_id = a.id WHERE a.id = ?",
            (info["assetIds"][0],),
        ).fetchone()
    stats = {
        "histogram": json.loads(row["histogram_json"]),
        "luminance": json.loads(row["luminance_json"]),
        "color": json.loads(row["color_json"]),
        "clipping": json.loads(row["clipping_json"]),
        "sharpness": row["sharpness"],
        "noiseEstimate": row["noise_estimate"],
    }
    md = {
        "iso": row["iso"],
        "aperture": row["aperture"],
        "shutterSpeed": row["shutter_speed"],
        "focalLength": row["focal_length"],
    }
    out = pred.predict_one(stats, md, None, "Canon EOS R6", "50mm")
    assert 0 <= out["confidence"] <= 1 and out["ood"] is False
    assert out["global"]["tone"]["exposure"]["raw"] == pytest.approx(
        json.loads(row["histogram_json"])
        and float(np.clip(1.2 * (0.5 - json.loads(row["luminance_json"])["mean"]) + 0.0, -5, 5)),
        abs=0.6,
    )
    assert isinstance(out["global"]["tone"]["contrast"]["raw"], int)
    assert len(out["nearestExamples"]) == 8 and all(n["assetId"] for n in out["nearestExamples"])
    unknown_cam = pred.predict_one(stats, {**md, "iso": 51200}, None, "Nikon Z9", "24mm")
    assert unknown_cam["confidence"] < out["confidence"]
    assert any("Nikon Z9" in r for r in unknown_cam["reasons"]) and any(
        "ISO 51200" in r for r in unknown_cam["reasons"]
    )
    far = {
        **stats,
        "luminance": {
            **stats["luminance"],
            "mean": 0.99,
            "percentiles": {k: 0.99 for k in stats["luminance"]["percentiles"]},
            "dynamicRange": 0.0,
        },
        "color": {**stats["color"], "saturationMean": 0.99, "castRedGreen": 0.4, "castBlueYellow": -0.4},
    }
    ood = pred.predict_one(far, {**md, "iso": 100, "focalLength": 600, "aperture": 22}, None, "Canon EOS R6", "50mm")
    assert ood["confidence"] <= 0.49 or ood["ood"] or ood["confidence"] < out["confidence"]


def test_insufficient_data_is_a_structured_failure(tmp_path):
    db = tmp_path / "small.db"
    info = build_db(db, n=12, shoots=2)
    with pytest.raises(InsufficientDataError) as ei:
        train(db, [info["libraryId"]], "s", "m", tmp_path / "styles", None)
    assert ei.value.details["pairs"] == 12
    assert not (tmp_path / "styles").exists(), "no artifacts on failure"


def _clone_asset_into_session_library(conn, src_asset_id: str, new_id: str, lib_id: str) -> None:
    cols = [r[1] for r in conn.execute("PRAGMA table_info(assets)").fetchall()]
    row = dict(zip(cols, conn.execute("SELECT * FROM assets WHERE id = ?", (src_asset_id,)).fetchone(), strict=True))
    row["id"] = new_id
    row["library_id"] = lib_id
    row["normalized_path"] = row["normalized_path"] + f"/{new_id}"
    conn.execute(f"INSERT INTO assets({','.join(cols)}) VALUES ({','.join('?' for _ in cols)})", [row[c] for c in cols])
    fcols = [r[1] for r in conn.execute("PRAGMA table_info(visual_features)").fetchall()]
    frow = dict(
        zip(
            fcols,
            conn.execute("SELECT * FROM visual_features WHERE asset_id = ?", (src_asset_id,)).fetchone(),
            strict=True,
        )
    )
    frow["asset_id"] = new_id
    conn.execute(
        f"INSERT INTO visual_features({','.join(fcols)}) VALUES ({','.join('?' for _ in fcols)})",
        [frow[c] for c in fcols],
    )


def test_dataset_includes_only_corrected_session_assets(synth_db, tmp_path):
    db, info = synth_db
    work = tmp_path / "corr.db"
    shutil.copy(db, work)
    with sqlite3.connect(work) as conn:
        conn.execute(
            "INSERT INTO libraries(id,name,source_type,created_at,status,purpose) VALUES ('sess','Session: x','folder_sidecars','t','scanned','session')"
        )
        src = info["assetIds"][0]
        snap = conn.execute(
            "SELECT normalized_settings_json, raw_settings_json, mapping_version FROM edit_snapshots WHERE asset_id = ? AND source = 'xmp'",
            (src,),
        ).fetchone()
        for new_id in ("corrected", "predicted-only"):
            _clone_asset_into_session_library(conn, src, new_id, "sess")
            conn.execute(
                "INSERT INTO edit_snapshots(id, asset_id, source, normalized_settings_json, raw_settings_json, mapping_version, observed_at) VALUES (?,?,?,?,?,?,?)",
                (f"p-{new_id}", new_id, "prediction", snap[0], snap[1], snap[2], "2026-01-01T00:00:00Z"),
            )
        conn.execute(
            "INSERT INTO edit_snapshots(id, asset_id, source, normalized_settings_json, raw_settings_json, mapping_version, observed_at) VALUES (?,?,?,?,?,?,?)",
            ("c-corrected", "corrected", "correction", snap[0], snap[1], snap[2], "2026-01-02T00:00:00Z"),
        )
    base = load_dataset(work, [info["libraryId"]], None)
    assert len(base) == 160, "session assets never leak into a library-only dataset"
    ds = load_dataset(work, [info["libraryId"]], None, ["corrected", "predicted-only", "missing"])
    assert len(ds) == 161
    extra = [p for p in ds.pairs if p.asset_id == "corrected"]
    assert extra and extra[0].source == "correction" and extra[0].group_key.startswith("C|")
    assert ds.excluded["notCorrected"] == 1, "a prediction-only asset is not a training pair"
    r = train(
        work, [info["libraryId"]], "s", "m", tmp_path / "styles", None, {"seed": 3, "correctionAssetIds": ["corrected"]}
    )
    assert r["counts"]["correctionPairs"] == 1
    assert r["trainingSet"]["correctionAssetIds"] == ["corrected"]
