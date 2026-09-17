import io
import json
import shutil

from mimic_engine import PROTOCOL_VERSION
from mimic_engine.protocol.service import build_server


def call(server, out, method, params, rid="r"):
    out.seek(0)
    out.truncate()
    server.handle_line(
        json.dumps({"protocolVersion": PROTOCOL_VERSION, "requestId": rid, "method": method, "params": params})
    )
    lines = [json.loads(line) for line in out.getvalue().splitlines() if line]
    resp = [line for line in lines if line.get("requestId") == rid][-1]
    events = [line for line in lines if "event" in line]
    return resp, events


def test_full_ingest_methods(photo_tree, tmp_path, fixtures_dir):
    out = io.StringIO()
    server = build_server()
    server._out = out
    resp, _ = call(
        server,
        out,
        "engine.configure",
        {
            "dbPath": str(tmp_path / "m.db"),
            "previewsDir": str(tmp_path / "p"),
            "embeddingsDir": str(tmp_path / "e"),
            "encodersDir": str(tmp_path / "enc"),
            "manifestsDir": str(fixtures_dir.parent / "models" / "manifests"),
        },
    )
    assert resp["ok"] and resp["result"]["encoder"]["provider"] == "stats_v1"

    resp, events = call(server, out, "scan.folder", {"roots": [str(photo_tree)], "jobId": "job-scan"})
    assert resp["ok"]
    scan = resp["result"]
    assert scan["stats"]["photos"] == 6
    assert any(e["event"] == "job.progress" and e["jobId"] == "job-scan" for e in events)

    xmp_path = next(s["path"] for a in scan["assets"] for s in a["sidecars"] if a["fileName"] == "IMG_1024.CR3")
    resp, _ = call(server, out, "xmp.parse", {"path": xmp_path})
    assert resp["ok"] and resp["result"]["rawSettings"]["Exposure2012"] == "+0.40"

    bad = next(s["path"] for a in scan["assets"] for s in a["sidecars"] if a["fileName"] == "IMG_1026.CR3")
    resp, _ = call(server, out, "xmp.parse", {"path": bad})
    assert not resp["ok"] and resp["error"]["code"] == "xmp_parse_error"

    resp, _ = call(server, out, "image.metadata", {"path": str(photo_tree / "2024-05-11 Wedding" / "IMG_1025.JPG")})
    assert resp["ok"] and resp["result"]["width"] == 720
    resp, _ = call(server, out, "image.metadata", {"path": str(tmp_path / "nope.jpg")})
    assert resp["error"]["code"] == "not_found"

    items = [
        {"assetId": f"a{i}", "path": a["sourcePath"], "fastHash": a["fastHash"]} for i, a in enumerate(scan["assets"])
    ]
    shutil.copy(fixtures_dir / "images" / "corrupt_not_an_image.jpg", photo_tree / "bad.jpg")
    items.append({"assetId": "bad", "path": str(photo_tree / "bad.jpg")})
    items.append({"assetId": "missing", "path": str(photo_tree / "missing.jpg")})
    resp, events = call(
        server, out, "image.analyze_batch", {"items": items, "featureVersion": "features_v1", "jobId": "job-an"}
    )
    assert resp["ok"]
    results = resp["result"]["results"]
    assert len(results) == len(items)
    ok = [r for r in results if "error" not in r]
    assert len(ok) == 6, [r for r in results if "error" in r]
    assert all(r["embeddingArtifactId"] for r in ok)
    errs = {r["assetId"]: r["error"]["code"] for r in results if "error" in r}
    assert errs == {"bad": "decode_error", "missing": "not_found"}
    assert events[-1]["current"] == len(items) == events[-1]["total"]

    resp, _ = call(server, out, "scan.folder", {"roots": [str(tmp_path / "missing-root")]})
    assert resp["error"]["code"] == "not_found"
    resp, _ = call(server, out, "scan.folder", {"roots": "not-a-list"})
    assert resp["error"]["code"] == "invalid_params"
    resp, _ = call(server, out, "engine.health", {})
    assert resp["result"]["configured"] is True
