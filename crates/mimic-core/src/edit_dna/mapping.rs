//! Typed view over `packages/contracts/edit_mapping_v1.json`.

use std::collections::HashMap;
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const MAPPING_JSON: &str = include_str!("../../../../packages/contracts/edit_mapping_v1.json");

pub static MAPPING: LazyLock<Mapping> = LazyLock::new(|| {
    let m: Mapping = serde_json::from_str(MAPPING_JSON).expect("edit_mapping_v1.json is valid");
    m.validate().expect("edit_mapping_v1.json is internally consistent");
    m
});

pub static MAPPING_VERSION: LazyLock<&'static str> =
    LazyLock::new(|| Box::leak(MAPPING.mapping_version.clone().into_boxed_str()));
pub static SCHEMA_VERSION: LazyLock<&'static str> =
    LazyLock::new(|| Box::leak(MAPPING.edit_dna_schema_version.clone().into_boxed_str()));

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ValueType {
    Float,
    Int,
    Bool,
    String,
    Enum,
    Curve,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Range {
    pub min: f64,
    pub max: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Control {
    pub canonical: String,
    pub family: String,
    pub lightroom_keys: Vec<String>,
    pub value_type: ValueType,
    #[serde(default)]
    pub range: Option<Range>,
    #[serde(default)]
    pub default: Option<Value>,
    pub normalize: String,
    #[serde(default)]
    pub process_versions: Vec<String>,
    #[serde(default)]
    pub note: Option<String>,
}

impl Control {
    /// Normalize a raw numeric value into 0..1 (clamped). `None` for controls
    /// without a range.
    pub fn normalize_value(&self, raw: f64) -> Option<f64> {
        let r = self.range.as_ref()?;
        let v = match self.normalize.as_str() {
            "log" if raw > 0.0 && r.min > 0.0 => (raw.log10() - r.min.log10()) / (r.max.log10() - r.min.log10()),
            "log" => return Some(0.0),
            _ => (raw - r.min) / (r.max - r.min),
        };
        Some(v.clamp(0.0, 1.0))
    }

    pub fn denormalize_value(&self, norm: f64) -> f64 {
        let Some(r) = self.range.as_ref() else { return norm };
        let n = norm.clamp(0.0, 1.0);
        match self.normalize.as_str() {
            "log" => 10f64.powf(r.min.log10() + n * (r.max.log10() - r.min.log10())),
            _ => r.min + n * (r.max - r.min),
        }
    }

    /// Absolute tolerance used for read-back verification: 0.5% of the range.
    pub fn tolerance(&self) -> f64 {
        match &self.range {
            Some(r) => (r.max - r.min) * 0.005,
            None => 0.0,
        }
    }

    pub fn family_predictable(&self) -> bool {
        MAPPING.families.get(&self.family).map(|f| f.predictable).unwrap_or(false)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Family {
    pub label: String,
    pub predictable: bool,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataKeys {
    #[serde(default)]
    pub process_version: Vec<String>,
    #[serde(default)]
    pub crs_version: Vec<String>,
    #[serde(default)]
    pub has_settings: Vec<String>,
    #[serde(default)]
    pub already_applied: Vec<String>,
    #[serde(default)]
    pub raw_file_name: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mapping {
    pub mapping_version: String,
    pub edit_dna_schema_version: String,
    pub families: HashMap<String, Family>,
    pub lightroom_metadata_keys: MetadataKeys,
    #[serde(default)]
    pub local_correction_key_prefixes: Vec<String>,
    #[serde(default)]
    pub heavy_edit_key_prefixes: Vec<String>,
    pub controls: Vec<Control>,
}

impl Mapping {
    fn validate(&self) -> Result<(), String> {
        let mut seen_canonical = std::collections::HashSet::new();
        let mut seen_key = std::collections::HashSet::new();
        for c in &self.controls {
            if !self.families.contains_key(&c.family) {
                return Err(format!("{}: unknown family {}", c.canonical, c.family));
            }
            if !c.canonical.starts_with(&format!("{}.", c.family)) {
                return Err(format!("{}: canonical must start with family", c.canonical));
            }
            if !seen_canonical.insert(&c.canonical) {
                return Err(format!("duplicate canonical {}", c.canonical));
            }
            for k in &c.lightroom_keys {
                if !seen_key.insert(k) {
                    return Err(format!("Lightroom key {k} mapped twice"));
                }
            }
            if matches!(c.value_type, ValueType::Float | ValueType::Int) && c.range.is_none() {
                return Err(format!("{}: numeric control without range", c.canonical));
            }
            if let Some(r) = &c.range {
                if r.min >= r.max {
                    return Err(format!("{}: bad range", c.canonical));
                }
            }
        }
        Ok(())
    }

    pub fn control(&self, canonical: &str) -> Option<&Control> {
        let idx = INDEX.0.get(canonical)?;
        self.controls.get(*idx)
    }

    pub fn control_for_key(&self, lightroom_key: &str) -> Option<&Control> {
        let idx = INDEX.1.get(lightroom_key)?;
        self.controls.get(*idx)
    }

    pub fn predictable_controls(&self) -> impl Iterator<Item = &Control> {
        self.controls
            .iter()
            .filter(|c| c.family_predictable() && matches!(c.value_type, ValueType::Float | ValueType::Int))
    }
}

static INDEX: LazyLock<(HashMap<String, usize>, HashMap<String, usize>)> = LazyLock::new(|| {
    let mut by_canonical = HashMap::new();
    let mut by_key = HashMap::new();
    for (i, c) in MAPPING.controls.iter().enumerate() {
        by_canonical.insert(c.canonical.clone(), i);
        for k in &c.lightroom_keys {
            by_key.insert(k.clone(), i);
        }
    }
    (by_canonical, by_key)
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapping_loads_and_is_consistent() {
        assert_eq!(MAPPING.mapping_version, "edit_mapping_v1");
        assert!(MAPPING.controls.len() > 90);
        assert!(MAPPING.control("tone.exposure").is_some());
        assert_eq!(MAPPING.control_for_key("Exposure").unwrap().canonical, "tone.exposure");
        assert!(MAPPING.predictable_controls().count() > 50);
        assert!(!MAPPING.control("crop.angle").unwrap().family_predictable());
    }

    #[test]
    fn normalization_is_invertible() {
        let c = MAPPING.control("tone.exposure").unwrap();
        for raw in [-5.0, -1.25, 0.0, 0.33, 5.0] {
            let n = c.normalize_value(raw).unwrap();
            assert!((c.denormalize_value(n) - raw).abs() < 1e-9);
        }
        assert_eq!(c.normalize_value(9.0), Some(1.0), "clamped");
        let t = MAPPING.control("whiteBalance.temperature").unwrap();
        let n = t.normalize_value(5500.0).unwrap();
        assert!((t.denormalize_value(n) - 5500.0).abs() < 1e-6);
        assert_eq!(t.normalize_value(0.0), Some(0.0));
    }
}
