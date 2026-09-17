"""Read-only XMP sidecar parser (spec §39).

Returns the Camera Raw (`crs:`) settings as a flat map of raw values exactly
as written (strings stay strings; lists become lists; structured masks become
nested dicts), plus a metadata summary from other namespaces, warnings, the
file hash and the parser version. It never writes, never normalizes, and never
drops a key it does not understand — normalization is mimic-core's job via
`packages/contracts/edit_mapping_v1.json`.
"""

from __future__ import annotations

import hashlib
import xml.etree.ElementTree as ET
from pathlib import Path
from typing import Any

PARSER_VERSION = "xmp_parser_v1"

NS = {
    "x": "adobe:ns:meta/",
    "rdf": "http://www.w3.org/1999/02/22-rdf-syntax-ns#",
    "crs": "http://ns.adobe.com/camera-raw-settings/1.0/",
    "xmp": "http://ns.adobe.com/xap/1.0/",
    "tiff": "http://ns.adobe.com/tiff/1.0/",
    "exif": "http://ns.adobe.com/exif/1.0/",
    "aux": "http://ns.adobe.com/exif/1.0/aux/",
    "dc": "http://purl.org/dc/elements/1.1/",
    "photoshop": "http://ns.adobe.com/photoshop/1.0/",
    "xmpMM": "http://ns.adobe.com/xap/1.0/mm/",
    "lr": "http://ns.adobe.com/lightroom/1.0/",
    "exifEX": "http://cipa.jp/exif/1.0/",
    "stEvt": "http://ns.adobe.com/xap/1.0/sType/ResourceEvent#",
}
URI_TO_PREFIX = {v: k for k, v in NS.items()}
RDF = "{" + NS["rdf"] + "}"
CRS = NS["crs"]

# Metadata keys surfaced for the asset row. Values are kept as strings.
METADATA_KEYS = {
    "tiff": ["Make", "Model", "Orientation", "ImageWidth", "ImageLength"],
    "exif": [
        "FNumber",
        "ExposureTime",
        "FocalLength",
        "DateTimeOriginal",
        "ISOSpeedRatings",
        "ExposureBiasValue",
        "Flash",
        "PixelXDimension",
        "PixelYDimension",
    ],
    "aux": ["Lens", "LensModel", "LensID", "SerialNumber"],
    "exifEX": ["LensModel"],
    "xmp": ["CreatorTool", "ModifyDate", "Rating"],
    "crs": ["RawFileName"],
}


class XmpParseError(Exception):
    pass


def _split(tag: str) -> tuple[str, str]:
    if tag.startswith("{"):
        uri, local = tag[1:].split("}", 1)
        return uri, local
    return "", tag


def _prefixed(uri: str, local: str) -> str:
    return f"{URI_TO_PREFIX.get(uri, uri)}:{local}"


def _element_value(el: ET.Element, warnings: list[str], depth: int = 0) -> Any:
    """Convert an rdf value element into JSON-like data (string, list or dict)."""
    if depth > 12:
        warnings.append(f"{el.tag}: nesting too deep; truncated")
        return None
    children = list(el)
    if not children:
        has_props = any(_split(a)[0] != NS["rdf"] for a in el.attrib)
        if has_props:
            return _description_to_dict(el, warnings, depth + 1)
        return (el.text or "").strip()
    first = children[0]
    if first.tag in (RDF + "Seq", RDF + "Bag", RDF + "Alt"):
        return [_element_value(li, warnings, depth + 1) for li in first.findall(RDF + "li")]
    if first.tag == RDF + "Description":
        return _description_to_dict(first, warnings, depth + 1)
    # parseType="Resource" form: properties are direct children / attributes.
    return _description_to_dict(el, warnings, depth + 1)


def _description_to_dict(desc: ET.Element, warnings: list[str], depth: int) -> dict[str, Any]:
    out: dict[str, Any] = {}
    for attr, value in desc.attrib.items():
        uri, local = _split(attr)
        if uri == NS["rdf"]:
            continue
        out[local] = value
    for child in desc:
        uri, local = _split(child.tag)
        if uri == NS["rdf"]:
            continue
        out[local] = _element_value(child, warnings, depth + 1)
    return out


def parse_xmp_bytes(data: bytes, *, source: str = "<bytes>") -> dict[str, Any]:
    warnings: list[str] = []
    try:
        root = ET.fromstring(data)
    except ET.ParseError as e:
        raise XmpParseError(f"{source}: not well-formed XML: {e}") from e

    # Only top-level descriptions (direct children of rdf:RDF). Nested
    # rdf:Description elements belong to structured values such as masks and
    # must not leak their crs: properties into the flat settings map.
    rdf_roots = [root] if root.tag == RDF + "RDF" else list(root.iter(RDF + "RDF"))
    descriptions = [d for r in rdf_roots for d in r.findall(RDF + "Description")]
    raw_settings: dict[str, Any] = {}
    metadata: dict[str, Any] = {}
    other_namespaces: set[str] = set()
    found_description = False

    for desc in descriptions:
        found_description = True
        # Attribute form: <rdf:Description crs:Exposure2012="+0.50" …>
        for attr, value in desc.attrib.items():
            uri, local = _split(attr)
            if uri == CRS:
                raw_settings[local] = value
            elif uri in URI_TO_PREFIX:
                prefix = URI_TO_PREFIX[uri]
                if local in METADATA_KEYS.get(prefix, []):
                    metadata[_prefixed(uri, local)] = value
            elif uri and uri != NS["rdf"]:
                other_namespaces.add(uri)
        # Element form: <crs:ToneCurvePV2012><rdf:Seq>…</rdf:Seq></crs:ToneCurvePV2012>
        for child in desc:
            uri, local = _split(child.tag)
            if uri == CRS:
                value = _element_value(child, warnings)
                if local in raw_settings and raw_settings[local] != value:
                    warnings.append(f"crs:{local} appears twice with different values; keeping element form")
                raw_settings[local] = value
            elif uri in URI_TO_PREFIX:
                prefix = URI_TO_PREFIX[uri]
                if local in METADATA_KEYS.get(prefix, []):
                    metadata[_prefixed(uri, local)] = _element_value(child, warnings)
            elif uri and uri != NS["rdf"]:
                other_namespaces.add(uri)

    if not found_description:
        warnings.append("no rdf:Description found; not an XMP packet?")
    if not raw_settings:
        warnings.append("no Camera Raw (crs:) settings present")

    return {
        "parserVersion": PARSER_VERSION,
        "sourceHash": "sha256:" + hashlib.sha256(data).hexdigest(),
        "rawSettings": raw_settings,
        "metadata": metadata,
        "otherNamespaces": sorted(other_namespaces),
        "warnings": warnings,
        "hasCrs": bool(raw_settings),
        "processVersion": raw_settings.get("ProcessVersion"),
        "hasLocalCorrections": any(
            k.startswith(p)
            for k in raw_settings
            for p in (
                "MaskGroupBasedCorrections",
                "GradientBasedCorrections",
                "CircularGradientBasedCorrections",
                "PaintBasedCorrections",
                "RetouchAreas",
            )
        ),
    }


def parse_xmp_file(path: str | Path) -> dict[str, Any]:
    p = Path(path)
    try:
        data = p.read_bytes()
    except OSError as e:
        raise XmpParseError(f"{p}: cannot read: {e}") from e
    out = parse_xmp_bytes(data, source=str(p))
    out["path"] = str(p)
    acr = p.with_suffix(".acr")
    out["acrSidecarPresent"] = acr.is_file()
    return out
