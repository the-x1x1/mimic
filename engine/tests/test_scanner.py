from mimic_engine.ingest.scanner import scan_folders


def test_pairing_rules(photo_tree):
    r = scan_folders([str(photo_tree)], include_metadata=False)
    by_name = {a["fileName"]: a for a in r["assets"]}
    # RAW with sidecar
    assert [s["type"] for s in by_name["IMG_1024.CR3"]["sidecars"]] == ["xmp"]
    # RAW + rendered twin: sidecar-less, reported as duplicate basename, both kept
    assert "IMG_1025.CR3" in by_name and "IMG_1025.JPG" in by_name
    assert any(d["stem"] == "img_1025" for d in r["duplicateBasenames"])
    # RAW with XMP + ACR
    kinds = sorted(s["type"] for s in by_name["DSC00001.ARW"]["sidecars"])
    assert kinds == ["acr", "xmp"]
    # DNG without sidecar noted
    assert any("DNG without XMP" in n for n in by_name["DSC00002.dng"].get("notes", []))
    # Orphan sidecar and unsupported file reported
    assert any(p.endswith("ORPHAN.xmp") for p in r["orphanSidecars"])
    assert any(u["path"].endswith("weird.bin") for u in r["unsupported"])
    assert not any(u["path"].endswith("notes.txt") for u in r["unsupported"]), (
        "known non-photo files are silently skipped"
    )
    stats = r["stats"]
    assert stats["photos"] == 6 and stats["withXmp"] == 3 and stats["withAcr"] == 1 and stats["dngWithoutSidecar"] == 1
    for a in r["assets"]:
        assert a["fastHash"].startswith("fh1:") and a["sizeBytes"] > 0 and a["modifiedTime"].endswith("Z")


def test_metadata_extraction_does_not_fail_scan(photo_tree):
    r = scan_folders([str(photo_tree)], include_metadata=True)
    assert all("metadata" in a for a in r["assets"])
    jpg = next(a for a in r["assets"] if a["fileName"] == "IMG_1025.JPG")
    assert jpg["metadata"]["width"] == 720 and jpg["metadata"]["height"] == 480


def test_progress_callback_and_missing_root(photo_tree, tmp_path):
    calls = []
    scan_folders([str(photo_tree)], include_metadata=False, progress=lambda *a: calls.append(a))
    assert calls and calls[-1][0] == "pairing"
    r = scan_folders([str(tmp_path / "empty")], include_metadata=False)
    assert r["assets"] == [] and r["stats"]["filesSeen"] == 0
