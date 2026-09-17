//! Canonical EditDNA: the versioned, Lightroom-version-neutral representation
//! of a photograph's develop state (docs/EDIT_DNA.md).
//!
//! The mapping between canonical controls and Lightroom keys is DATA
//! (`packages/contracts/edit_mapping_v1.json`), shared byte-for-byte with the
//! Python engine and the TypeScript contracts package. This module never
//! drops a Lightroom key: anything not covered by the mapping lands in
//! `unknownLightroomSettings`, and local/mask payloads are reported as
//! `observed` but never re-emitted as writable settings.

pub mod mapping;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

pub use mapping::{Control, Mapping, ValueType, MAPPING, MAPPING_VERSION, SCHEMA_VERSION};

/// One canonical control observation. `raw` is always the source value as
/// parsed; `value` is the normalized 0..1 scalar for numeric controls, or
/// absent for enum/string/curve/bool controls.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlValue {
    pub raw: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    /// Which Lightroom key supplied the value (e.g. `Exposure2012` vs `Exposure`).
    pub source_key: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalEdits {
    /// `unsupported` (nothing observed), `observed` (mask/local data present but
    /// not readable/writable by Mimic), or `supported` (never in 0.x).
    pub status: String,
    pub observed_keys: Vec<String>,
    pub operations: Vec<Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LightroomInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub process_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crs_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_settings: Option<bool>,
    pub heavy_edit_keys: Vec<String>,
}

/// Result of normalizing one raw Lightroom settings table.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Normalized {
    pub schema_version: String,
    pub mapping_version: String,
    /// family -> canonical short name -> value. Absent key == "not present".
    pub global: BTreeMap<String, BTreeMap<String, ControlValue>>,
    pub local: LocalEdits,
    pub lightroom: LightroomInfo,
    pub unknown: Map<String, Value>,
    pub warnings: Vec<String>,
    pub raw_settings_hash: String,
}

impl Normalized {
    /// Look up a control by its dotted canonical name, e.g. `tone.exposure`.
    pub fn get(&self, canonical: &str) -> Option<&ControlValue> {
        let (family, rest) = canonical.split_once('.')?;
        self.global.get(family)?.get(rest)
    }

    /// Number of canonical controls that are present with a non-default raw value.
    pub fn meaningful_edit_count(&self) -> usize {
        MAPPING
            .controls
            .iter()
            .filter(|c| {
                self.get(&c.canonical).is_some_and(|v| match (&c.default, &v.raw) {
                    (Some(d), raw) => !values_equal(d, raw),
                    (None, _) => true,
                })
            })
            .count()
    }
}

fn values_equal(a: &Value, b: &Value) -> bool {
    match (a.as_f64(), b.as_f64()) {
        (Some(x), Some(y)) => (x - y).abs() < 1e-9,
        _ => a == b,
    }
}

/// Normalize a raw Lightroom settings object (from XMP `crs:*` or
/// `photo:getDevelopSettings()`). Never fails: bad values become warnings and
/// the offending key is preserved in `unknown`.
pub fn normalize(raw: &Map<String, Value>) -> Normalized {
    let mapping: &Mapping = &MAPPING;
    let mut out = Normalized {
        schema_version: SCHEMA_VERSION.to_string(),
        mapping_version: MAPPING_VERSION.to_string(),
        raw_settings_hash: crate::ids::stable_json_hash(&Value::Object(raw.clone())),
        ..Default::default()
    };
    let mut consumed: std::collections::HashSet<&str> = std::collections::HashSet::new();

    // Lightroom metadata keys.
    for key in &mapping.lightroom_metadata_keys.process_version {
        if let Some(v) = raw.get(key) {
            out.lightroom.process_version = Some(value_to_string(v));
            consumed.insert(key);
        }
    }
    for key in &mapping.lightroom_metadata_keys.crs_version {
        if let Some(v) = raw.get(key) {
            out.lightroom.crs_version = Some(value_to_string(v));
            consumed.insert(key);
        }
    }
    for key in &mapping.lightroom_metadata_keys.has_settings {
        if let Some(v) = raw.get(key) {
            out.lightroom.has_settings = parse_bool(v);
            consumed.insert(key);
        }
    }
    for key in
        mapping.lightroom_metadata_keys.already_applied.iter().chain(&mapping.lightroom_metadata_keys.raw_file_name)
    {
        if raw.contains_key(key) {
            consumed.insert(key);
        }
    }

    // Canonical controls: first matching key wins (modern key listed first).
    for control in &mapping.controls {
        for key in &control.lightroom_keys {
            let Some(v) = raw.get(key) else { continue };
            consumed.insert(key);
            match parse_control_value(control, v) {
                Ok((parsed, normalized)) => {
                    let (family, short) = control.canonical.split_once('.').expect("canonical has family");
                    out.global.entry(family.to_string()).or_default().insert(
                        short.to_string(),
                        ControlValue { raw: parsed, value: normalized, source_key: key.clone() },
                    );
                    break;
                }
                Err(reason) => {
                    out.warnings.push(format!("{key}: {reason}; kept raw in unknownLightroomSettings"));
                    out.unknown.insert(key.clone(), v.clone());
                }
            }
        }
    }

    // Everything else: local/mask, heavy edits, unknown.
    for (key, v) in raw {
        if consumed.contains(key.as_str()) {
            continue;
        }
        if mapping.local_correction_key_prefixes.iter().any(|p| key.starts_with(p)) {
            out.local.observed_keys.push(key.clone());
            out.unknown.insert(key.clone(), v.clone());
        } else if mapping.heavy_edit_key_prefixes.iter().any(|p| key.starts_with(p)) {
            out.lightroom.heavy_edit_keys.push(key.clone());
            out.unknown.insert(key.clone(), v.clone());
        } else {
            out.unknown.insert(key.clone(), v.clone());
        }
    }
    out.local.status = if out.local.observed_keys.is_empty() { "unsupported" } else { "observed" }.to_string();
    out.local.observed_keys.sort();
    out.lightroom.heavy_edit_keys.sort();
    out
}

/// Parse and normalize one raw value for a control.
fn parse_control_value(control: &Control, v: &Value) -> Result<(Value, Option<f64>), String> {
    match control.value_type {
        ValueType::Float | ValueType::Int => {
            let n = parse_number(v).ok_or_else(|| format!("expected number, got {v}"))?;
            let raw = if control.value_type == ValueType::Int && n.fract() == 0.0 { json!(n as i64) } else { json!(n) };
            Ok((raw, control.normalize_value(n)))
        }
        ValueType::Bool => {
            let b = parse_bool(v).ok_or_else(|| format!("expected boolean, got {v}"))?;
            Ok((Value::Bool(b), Some(if b { 1.0 } else { 0.0 })))
        }
        ValueType::Enum | ValueType::String => Ok((Value::String(value_to_string(v)), None)),
        ValueType::Curve => {
            let points = parse_curve(v).ok_or_else(|| format!("expected curve points, got {v}"))?;
            Ok((json!(points), None))
        }
    }
}

pub fn parse_number(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().trim_start_matches('+').parse::<f64>().ok(),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    }
}

pub fn parse_bool(v: &Value) -> Option<bool> {
    match v {
        Value::Bool(b) => Some(*b),
        Value::Number(n) => n.as_f64().map(|f| f != 0.0),
        Value::String(s) => match s.trim().to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" => Some(true),
            "false" | "0" | "no" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

/// Curves arrive as `["0, 0", "255, 255"]` (XMP) or `[{x,y}]`/`[[x,y]]` (SDK).
pub fn parse_curve(v: &Value) -> Option<Vec<[f64; 2]>> {
    let items = v.as_array()?;
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let pt = match item {
            Value::String(s) => {
                let mut parts = s.split(',').map(|p| p.trim().parse::<f64>());
                [parts.next()?.ok()?, parts.next()?.ok()?]
            }
            Value::Array(a) if a.len() == 2 => [a[0].as_f64()?, a[1].as_f64()?],
            Value::Object(o) => [o.get("x")?.as_f64()?, o.get("y")?.as_f64()?],
            _ => return None,
        };
        out.push(pt);
    }
    Some(out)
}

fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Convert canonical global values back into a flat Lightroom settings table
/// for the given set of writable Lightroom keys (from the capability probe).
/// Controls with no writable key are returned in `skipped` with the reason.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LightroomWrite {
    pub settings: Map<String, Value>,
    pub skipped: Vec<SkippedControl>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkippedControl {
    pub canonical: String,
    pub reason: String,
}

pub fn to_lightroom_settings(
    global: &BTreeMap<String, BTreeMap<String, ControlValue>>,
    writable_keys: &std::collections::HashSet<String>,
) -> LightroomWrite {
    let mut out = LightroomWrite::default();
    for (family, controls) in global {
        for (short, cv) in controls {
            let canonical = format!("{family}.{short}");
            let Some(control) = MAPPING.control(&canonical) else {
                out.skipped.push(SkippedControl { canonical, reason: "not in mapping".into() });
                continue;
            };
            let key = control.lightroom_keys.iter().find(|k| writable_keys.contains(k.as_str())).or_else(|| {
                // Prefer the key the value came from when the probe allows it.
                if writable_keys.contains(cv.source_key.as_str()) {
                    Some(&cv.source_key)
                } else {
                    None
                }
            });
            match key {
                Some(k) => {
                    let raw = match (&control.value_type, cv.value) {
                        (ValueType::Float | ValueType::Int, Some(norm)) => json!(control.denormalize_value(norm)),
                        _ => cv.raw.clone(),
                    };
                    let raw = match control.value_type {
                        ValueType::Int => raw.as_f64().map(|f| json!(f.round() as i64)).unwrap_or(raw),
                        ValueType::Float => raw.as_f64().map(|f| json!((f * 1000.0).round() / 1000.0)).unwrap_or(raw),
                        _ => raw,
                    };
                    out.settings.insert(k.clone(), raw);
                }
                None => out
                    .skipped
                    .push(SkippedControl { canonical, reason: "no writable Lightroom key in capability set".into() }),
            }
        }
    }
    out
}

/// Compare intended vs. read-back settings for the keys we wrote. Numeric
/// values are compared with a per-control tolerance (one normalized step of
/// 0.5% of the control's range, or exact for ints).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyResult {
    pub ok: bool,
    pub mismatches: Vec<Mismatch>,
    pub compared: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mismatch {
    pub key: String,
    pub intended: Value,
    pub observed: Value,
}

pub fn verify_readback(intended: &Map<String, Value>, observed: &Map<String, Value>) -> VerifyResult {
    let mut result = VerifyResult { ok: true, mismatches: Vec::new(), compared: 0 };
    for (key, want) in intended {
        result.compared += 1;
        let got = observed.get(key);
        let matches = match (got, MAPPING.control_for_key(key)) {
            (None, _) => false,
            (Some(got), Some(control)) => match control.value_type {
                ValueType::Float => match (parse_number(want), parse_number(got)) {
                    (Some(a), Some(b)) => (a - b).abs() <= control.tolerance(),
                    _ => false,
                },
                ValueType::Int => match (parse_number(want), parse_number(got)) {
                    (Some(a), Some(b)) => (a.round() - b.round()).abs() < 0.5,
                    _ => false,
                },
                ValueType::Bool => parse_bool(want) == parse_bool(got),
                ValueType::Curve => parse_curve(want) == parse_curve(got),
                ValueType::Enum | ValueType::String => value_to_string(want) == value_to_string(got),
            },
            (Some(got), None) => got == want,
        };
        if !matches {
            result.ok = false;
            result.mismatches.push(Mismatch {
                key: key.clone(),
                intended: want.clone(),
                observed: got.cloned().unwrap_or(Value::Null),
            });
        }
    }
    result
}

/// Full EditDNA document for one training pair (spec §8).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditDna {
    pub schema_version: String,
    pub asset: Value,
    pub input: Value,
    pub lightroom: Value,
    pub target: Value,
    pub unknown_lightroom_settings: Map<String, Value>,
    pub provenance: Value,
}

pub fn build_edit_dna(
    asset: &crate::db::Asset,
    features: Option<&crate::db::VisualFeatures>,
    snapshot: &crate::db::EditSnapshot,
) -> EditDna {
    let normalized: Normalized = serde_json::from_value(snapshot.normalized_settings.clone()).unwrap_or_default();
    EditDna {
        schema_version: SCHEMA_VERSION.to_string(),
        asset: json!({
            "id": asset.id,
            "camera": {"make": asset.camera_make, "model": asset.camera_model, "lens": asset.lens},
            "capture": {"focalLength": asset.focal_length, "iso": asset.iso, "aperture": asset.aperture, "shutterSpeed": asset.shutter_speed, "capturedAt": asset.captured_at},
            "dimensions": {"width": asset.width, "height": asset.height, "orientation": asset.orientation},
        }),
        input: match features {
            Some(f) => json!({
                "sceneFeaturesVersion": f.feature_version,
                "visualEmbeddingRef": f.embedding_artifact_id,
                "histogram": f.histogram, "luminance": f.luminance, "color": f.color,
                "noise": {"estimate": f.noise_estimate}, "sharpness": {"estimate": f.sharpness},
                "clipping": f.clipping, "scene": f.scene_labels, "sessionContext": {},
            }),
            None => json!({"sceneFeaturesVersion": null}),
        },
        lightroom: json!({
            "processVersion": normalized.lightroom.process_version,
            "crsVersion": normalized.lightroom.crs_version,
            "source": snapshot.source,
            "capabilitySchemaVersion": snapshot.capability_schema_version,
            "heavyEditKeys": normalized.lightroom.heavy_edit_keys,
        }),
        target: json!({"global": normalized.global, "local": normalized.local}),
        unknown_lightroom_settings: normalized.unknown,
        provenance: json!({
            "sourceSnapshotId": snapshot.id,
            "capturedAt": snapshot.observed_at,
            "mappingVersion": snapshot.mapping_version,
            "rawSettingsHash": normalized.raw_settings_hash,
            "extra": snapshot.provenance,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(pairs: &[(&str, Value)]) -> Map<String, Value> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
    }

    #[test]
    fn normalizes_modern_and_legacy_keys() {
        let n = normalize(&raw(&[
            ("ProcessVersion", json!("15.4")),
            ("Exposure2012", json!("+0.50")),
            ("Contrast2012", json!("-10")),
            ("Temperature", json!("5500")),
            ("WhiteBalance", json!("Custom")),
            ("LensProfileEnable", json!("1")),
            ("ToneCurvePV2012", json!(["0, 0", "128, 140", "255, 255"])),
        ]));
        assert_eq!(n.lightroom.process_version.as_deref(), Some("15.4"));
        let exp = n.get("tone.exposure").unwrap();
        assert_eq!(exp.raw, json!(0.5));
        assert!((exp.value.unwrap() - 0.55).abs() < 1e-9);
        assert_eq!(exp.source_key, "Exposure2012");
        assert_eq!(n.get("tone.contrast").unwrap().raw, json!(-10));
        assert_eq!(n.get("whiteBalance.mode").unwrap().raw, json!("Custom"));
        assert_eq!(n.get("lens.profileEnabled").unwrap().raw, json!(true));
        assert_eq!(n.get("toneCurve.point.rgb").unwrap().raw, json!([[0.0, 0.0], [128.0, 140.0], [255.0, 255.0]]));
        let temp = n.get("whiteBalance.temperature").unwrap().value.unwrap();
        assert!(temp > 0.0 && temp < 1.0);
        assert!(n.unknown.is_empty());
        assert!(n.warnings.is_empty());

        let legacy =
            normalize(&raw(&[("Exposure", json!("+1.00")), ("Shadows", json!("5")), ("FillLight", json!("20"))]));
        assert_eq!(legacy.get("tone.exposure").unwrap().source_key, "Exposure");
        assert_eq!(legacy.get("tone.blacks").unwrap().raw, json!(5));
        assert_eq!(legacy.get("tone.shadows").unwrap().raw, json!(20));
    }

    #[test]
    fn unknown_keys_are_preserved_and_local_edits_observed() {
        let n = normalize(&raw(&[
            ("Exposure2012", json!(0.0)),
            ("FutureLightroomThing", json!("x")),
            ("MaskGroupBasedCorrections", json!([{"What": "Mask/Group"}])),
            ("LookTable", json!("abc")),
            ("Contrast2012", json!("not-a-number")),
        ]));
        assert_eq!(n.unknown["FutureLightroomThing"], "x");
        assert!(n.unknown.contains_key("MaskGroupBasedCorrections"));
        assert_eq!(n.local.status, "observed");
        assert_eq!(n.local.observed_keys, vec!["MaskGroupBasedCorrections"]);
        assert_eq!(n.lightroom.heavy_edit_keys, vec!["LookTable"]);
        assert!(n.unknown.contains_key("Contrast2012"), "unparsable value kept raw");
        assert_eq!(n.warnings.len(), 1);
        assert!(n.get("tone.contrast").is_none());
        // zero is "present with value 0", not "absent".
        assert_eq!(n.get("tone.exposure").unwrap().raw.as_f64(), Some(0.0));
        assert_eq!(n.meaningful_edit_count(), 0);
    }

    #[test]
    fn roundtrip_to_lightroom_respects_capability() {
        let n = normalize(&raw(&[("Exposure2012", json!("+0.50")), ("Texture", json!("15")), ("Dehaze", json!("7"))]));
        let mut writable: std::collections::HashSet<String> =
            ["Exposure2012", "Texture"].iter().map(|s| s.to_string()).collect();
        let w = to_lightroom_settings(&n.global, &writable);
        assert_eq!(w.settings["Exposure2012"], json!(0.5));
        assert_eq!(w.settings["Texture"], json!(15));
        assert_eq!(w.skipped.len(), 1);
        assert_eq!(w.skipped[0].canonical, "presence.dehaze");
        writable.insert("Dehaze".into());
        assert!(to_lightroom_settings(&n.global, &writable).skipped.is_empty());
    }

    #[test]
    fn verify_readback_uses_tolerances() {
        let intended =
            raw(&[("Exposure2012", json!(0.5)), ("Contrast2012", json!(10)), ("WhiteBalance", json!("Custom"))]);
        let ok = raw(&[
            ("Exposure2012", json!(0.5004)),
            ("Contrast2012", json!(10.0)),
            ("WhiteBalance", json!("Custom")),
            ("Extra", json!(1)),
        ]);
        assert!(verify_readback(&intended, &ok).ok);
        let bad = raw(&[("Exposure2012", json!(0.9)), ("Contrast2012", json!(11)), ("WhiteBalance", json!("Custom"))]);
        let v = verify_readback(&intended, &bad);
        assert!(!v.ok);
        assert_eq!(v.mismatches.len(), 2);
        let missing = raw(&[("Exposure2012", json!(0.5))]);
        assert_eq!(verify_readback(&intended, &missing).mismatches.len(), 2);
    }
}
