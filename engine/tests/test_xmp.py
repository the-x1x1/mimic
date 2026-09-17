import json
from pathlib import Path

import pytest

from mimic_engine.xmp.parser import PARSER_VERSION, XmpParseError, parse_xmp_bytes, parse_xmp_file

EXPECTED = Path(__file__).resolve().parents[2] / "fixtures" / "expected"


def test_simple_pv2012(fixtures_dir):
    r = parse_xmp_file(fixtures_dir / "xmp" / "simple_pv2012.xmp")
    raw = r["rawSettings"]
    assert r["parserVersion"] == PARSER_VERSION
    assert r["sourceHash"].startswith("sha256:")
    assert raw["Exposure2012"] == "+0.40"
    assert raw["ProcessVersion"] == "11.0"
    assert raw["ToneCurvePV2012"] == ["0, 0", "64, 60", "192, 198", "255, 255"]
    assert raw["HasSettings"] == "True"
    assert r["metadata"]["tiff:Make"] == "Canon"
    assert r["metadata"]["aux:Lens"] == "RF50mm F1.2 L USM"
    assert r["metadata"]["exif:FNumber"] == "28/10"
    assert r["hasLocalCorrections"] is False
    assert r["warnings"] == []
    assert r["acrSidecarPresent"] is False


def test_modern_masks_unknown_preserved(fixtures_dir):
    r = parse_xmp_file(fixtures_dir / "xmp" / "modern_masks_unknown.xmp")
    raw = r["rawSettings"]
    assert raw["ProcessVersion"] == "15.4"
    assert raw["SomeNewSlider2027"] == "+42", "unknown crs keys are kept verbatim"
    assert raw["LensBlur"] == '{"Active":false}'
    masks = raw["MaskGroupBasedCorrections"]
    assert isinstance(masks, list) and masks[0]["CorrectionName"] == "Subject 1"
    assert masks[0]["CorrectionMasks"][0]["MaskName"] == "Subject 1"
    assert raw["Look"]["Name"] == "Adobe Color"
    assert raw["Look"]["Parameters"]["CameraProfile"] == "Adobe Standard"
    assert r["hasLocalCorrections"] is True
    assert "http://example.invalid/ns/future-lightroom/1.0/" in r["otherNamespaces"]
    assert r["metadata"]["exifEX:LensModel"] == "FE 85mm F1.8"


def test_legacy_pv2010(fixtures_dir):
    r = parse_xmp_file(fixtures_dir / "xmp" / "legacy_pv2010.xmp")
    raw = r["rawSettings"]
    assert raw["ProcessVersion"] == "5.7"
    assert raw["Exposure"] == "+0.75" and "Exposure2012" not in raw
    assert raw["ToneCurve"][1] == "32, 22"
    assert raw["Brightness"] == "+50", "legacy-only keys survive for mimic-core to file under unknown"


def test_no_crs(fixtures_dir):
    r = parse_xmp_file(fixtures_dir / "xmp" / "no_crs_metadata_only.xmp")
    assert r["rawSettings"] == {}
    assert r["hasCrs"] is False
    assert any("no Camera Raw" in w for w in r["warnings"])
    assert r["metadata"]["tiff:Model"] == "X-T5"


def test_malformed_raises_not_crashes(fixtures_dir):
    with pytest.raises(XmpParseError):
        parse_xmp_file(fixtures_dir / "xmp" / "malformed_truncated.xmp")
    with pytest.raises(XmpParseError):
        parse_xmp_file(fixtures_dir / "xmp" / "does_not_exist.xmp")


def test_attribute_and_element_form_ordering_independent():
    a = b"""<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
      <rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/" crs:Exposure2012="+1.0">
        <crs:Contrast2012>5</crs:Contrast2012></rdf:Description></rdf:RDF></x:xmpmeta>"""
    b = b"""<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
      <rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/">
        <crs:Contrast2012>5</crs:Contrast2012><crs:Exposure2012>+1.0</crs:Exposure2012></rdf:Description></rdf:RDF></x:xmpmeta>"""
    assert (
        parse_xmp_bytes(a)["rawSettings"]
        == parse_xmp_bytes(b)["rawSettings"]
        == {"Exposure2012": "+1.0", "Contrast2012": "5"}
    )


@pytest.mark.parametrize("name", ["simple_pv2012", "modern_masks_unknown", "legacy_pv2010", "no_crs_metadata_only"])
def test_golden_raw_settings(fixtures_dir, name):
    """Golden: parsed crs map must match fixtures/expected/<name>.raw.json exactly.

    Regenerate deliberately with `uv run python -m tests.regen_golden` after a
    reviewed parser change — never by hand-editing the expected file.
    """
    r = parse_xmp_file(fixtures_dir / "xmp" / f"{name}.xmp")
    expected = json.loads((EXPECTED / f"{name}.raw.json").read_text("utf-8"))
    assert r["rawSettings"] == expected["rawSettings"]
    assert r["metadata"] == expected["metadata"]


def test_nested_descriptions_do_not_leak(fixtures_dir):
    r = parse_xmp_file(fixtures_dir / "xmp" / "modern_masks_unknown.xmp")
    raw = r["rawSettings"]
    for leaked in ("What", "CorrectionAmount", "MaskName", "Name", "UUID", "Amount"):
        assert leaked not in raw, f"{leaked} leaked from a nested rdf:Description"
    assert raw["Look"]["Name"] == "Adobe Color"
