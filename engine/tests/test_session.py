from datetime import datetime, timedelta

import numpy as np

from mimic_engine.session.consistency import apply_consistency
from mimic_engine.session.grouping import SessionItem, group_session


def item(i, t, lum, cast=0.0):
    f = np.asarray([lum, 0.2, lum - 0.2, lum + 0.2, cast * 4, -cast * 4, 0.3, 0.1] + [1 / 8] * 8, dtype=np.float32)
    return SessionItem(asset_id=f"a{i}", captured_at=t, features=f)


def at(base: str, seconds: int) -> str:
    return (datetime.fromisoformat(base) + timedelta(seconds=seconds)).isoformat()


def test_grouping_splits_on_time_gaps_and_visual_difference_and_is_stable():
    items = []
    # Block 1: 12 bright frames at 10:00, one per 10 s; block 2 after 2.5 h: 12 dark frames.
    for i in range(12):
        items.append(item(i, at("2025-06-01T10:00:00", i * 10), 0.7))
    for i in range(12, 24):
        items.append(item(i, at("2025-06-01T12:30:00", (i - 12) * 10), 0.2))
    # Block 3: mixed bright/dark frames close in time → should split visually.
    for i in range(24, 40):
        items.append(item(i, at("2025-06-01T15:00:00", (i - 24) * 4), 0.75 if i % 2 else 0.15))
    a = group_session(items, seed=1)
    b = group_session(items, seed=1)
    assert a == b, "grouping must be deterministic"
    assert a["timeBlocks"] == 3
    g = a["assignments"]
    assert len({g[f"a{i}"] for i in range(12)}) == 1
    assert len({g[f"a{i}"] for i in range(12, 24)}) == 1
    assert g["a0"] != g["a12"]
    block3 = {g[f"a{i}"] for i in range(24, 40)}
    assert len(block3) == 2, block3
    assert g["a25"] == g["a27"] and g["a24"] == g["a26"] and g["a24"] != g["a25"]
    assert all(c["count"] >= 4 for c in a["clusters"])
    # Bursts: block 3 frames are 4 s apart (> 3 s) → no bursts; make a burst explicitly.
    burst_items = [
        item(100, "2025-06-01T16:00:00", 0.5),
        item(101, "2025-06-01T16:00:01", 0.5),
        item(102, "2025-06-01T16:00:02", 0.5),
        item(103, "2025-06-01T16:00:30", 0.5),
    ]
    r = group_session(burst_items, seed=1)
    assert r["bursts"].get("a100") == r["bursts"].get("a101") == r["bursts"].get("a102") and "a103" not in r["bursts"]


def test_grouping_handles_missing_times_and_tiny_sessions():
    r = group_session([item(0, None, 0.5)], seed=1)
    assert len(r["clusters"]) == 1 and r["assignments"]["a0"] == "group-1"
    # Untimed frames share one block after the timed ones; invalid timestamps count as untimed.
    mixed = [item(1, "2025-06-01T10:00:00", 0.5), item(2, None, 0.5), item(3, "not-a-date", 0.5), item(4, "", 0.5)]
    r = group_session(mixed, seed=1)
    assert r["timeBlocks"] == 2
    assert len({r["assignments"][f"a{i}"] for i in (2, 3, 4)}) == 1
    assert group_session([], seed=1)["clusters"] == []


def test_consistency_pulls_wb_but_never_exposure():
    names = ["tone.exposure", "whiteBalance.temperature", "whiteBalance.tint"]
    pred = np.asarray([[0.3, 0.50, 0.5], [0.8, 0.52, 0.5], [0.5, 0.90, 0.5], [0.6, 0.51, 0.5]], dtype=np.float32)
    out, shift = apply_consistency(pred, names, ["g", "g", "g", "g"])
    assert np.array_equal(out[:, 0], pred[:, 0]), "exposure untouched"
    assert out[2, 1] < 0.90 and out[2, 1] >= 0.90 - 0.06 - 1e-6, "bounded pull toward group median"
    assert shift[2] > 0 and shift[0] <= 0.06
    # Groups smaller than 3 are left alone.
    out2, shift2 = apply_consistency(pred[:2], names, ["g", "g"])
    assert np.array_equal(out2, pred[:2]) and not shift2.any()


def test_service_session_group_and_consistent_predict(tmp_path):
    from mimic_engine.protocol.service import EngineService
    from mimic_engine.training.trainer import train
    from tests.synth import build_db

    db = tmp_path / "mimic.db"
    info = build_db(db, n=120, shoots=6)
    svc = EngineService()
    svc.configure({"dbPath": str(db)}, lambda *a: None)
    ids = info["assetIds"][:40]
    grouped = svc.session_group({"assetIds": [*ids, "missing-asset"]}, lambda *a: None)
    assert grouped["version"] == "grouping_v1"
    assert grouped["missingFeatures"] == ["missing-asset"]
    assert set(grouped["assignments"]) == set(ids)
    assert sum(c["count"] for c in grouped["clusters"]) == 40
    again = svc.session_group({"assetIds": [*ids, "missing-asset"]}, lambda *a: None)
    assert again == grouped, "stable across calls"

    r = train(db, [info["libraryId"]], "s", "m", tmp_path / "styles", None, {"seed": 1})
    model_path = next(a["path"] for a in r["artifacts"] if a["kind"] == "model")
    plain = svc.model_predict({"modelPath": model_path, "assetIds": ids}, lambda *a: None)
    cons = svc.model_predict(
        {"modelPath": model_path, "assetIds": ids, "groups": grouped["assignments"]}, lambda *a: None
    )
    assert all("consistencyShift" not in p for p in plain["results"])
    assert all("consistencyShift" in p for p in cons["results"] if "error" not in p)
    for a, b in zip(plain["results"], cons["results"], strict=True):
        assert a["global"]["tone"]["exposure"]["value"] == b["global"]["tone"]["exposure"]["value"]
        for fam, shorts in b["global"].items():
            for short, v in shorts.items():
                assert abs(v["value"] - a["global"][fam][short]["value"]) <= 0.061
    off = svc.model_predict(
        {"modelPath": model_path, "assetIds": ids, "groups": grouped["assignments"], "consistency": False},
        lambda *a: None,
    )
    assert off["results"] == plain["results"]
