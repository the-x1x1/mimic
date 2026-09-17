//! Lightroom capability model (docs/LIGHTROOM_CAPABILITY_MATRIX.md).
//!
//! Adobe documents that develop-settings tables may change between Lightroom
//! versions, so nothing here is assumed: the plugin reports what the installed
//! Lightroom actually returned and supports (`CapabilityProbe`), and this module
//! derives a per-control matrix from that probe plus a conservative static
//! write policy for the 0.x line. The installed version's observed behaviour
//! always wins over the static list.

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::edit_dna::{ValueType, MAPPING};

/// What the Lightroom plugin reports on handshake / `get_capabilities`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityProbe {
    pub lightroom_version: String,
    #[serde(default)]
    pub sdk_version: Option<String>,
    pub plugin_version: String,
    /// Keys returned by `photo:getDevelopSettings()` on a probe photo (empty if
    /// no photo was available at probe time).
    #[serde(default)]
    pub develop_setting_keys: Vec<String>,
    #[serde(default)]
    pub supports: SupportFlags,
    /// Free-form notes from the plugin (e.g. which probe photo was used).
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SupportFlags {
    #[serde(default)]
    pub get_develop_settings: bool,
    #[serde(default)]
    pub apply_develop_preset: bool,
    #[serde(default)]
    pub add_develop_preset_for_plugin: bool,
    #[serde(default)]
    pub create_develop_snapshot: bool,
    #[serde(default)]
    pub develop_controller: bool,
    #[serde(default)]
    pub catalog_write_access: bool,
    #[serde(default)]
    pub lr_http: bool,
}

/// Per-control support status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlStatus {
    /// Key observed AND in the write policy AND preset application works.
    Supported,
    /// Key observed on this Lightroom but Mimic will not write it in 0.x.
    ObservedNotWritable,
    /// Not returned by this Lightroom — cannot be read or written.
    Unsupported,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlCapability {
    pub canonical: String,
    pub family: String,
    pub status: ControlStatus,
    pub lightroom_key: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityMatrix {
    /// Stable identifier of this capability set; stored on predictions so an
    /// apply can detect that Lightroom changed since prediction time.
    pub schema_version: String,
    pub lightroom_version: String,
    pub plugin_version: String,
    pub probe_had_photo: bool,
    pub can_apply: bool,
    pub can_snapshot: bool,
    pub can_read: bool,
    pub controls: Vec<ControlCapability>,
    pub family_summary: BTreeMap<String, FamilySummary>,
    pub local_edits: String,
    pub masks: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FamilySummary {
    pub label: String,
    pub supported: usize,
    pub observed_not_writable: usize,
    pub unsupported: usize,
}

/// Families Mimic is willing to write through a plugin develop preset in 0.x.
pub const WRITABLE_FAMILIES_0X: &[&str] =
    &["whiteBalance", "tone", "presence", "hsl", "toneCurve", "colorGrading", "detail", "effects", "calibration"];

fn static_write_policy(control: &crate::edit_dna::Control) -> Result<(), &'static str> {
    if !WRITABLE_FAMILIES_0X.contains(&control.family.as_str()) {
        return Err("family is read-only in Mimic 0.x");
    }
    match control.value_type {
        ValueType::Curve => Err("point curves are stored but not written in 0.x"),
        ValueType::String => Err("free-text settings (profile names) are not written"),
        _ => Ok(()),
    }
}

impl CapabilityMatrix {
    pub fn from_probe(probe: &CapabilityProbe) -> Self {
        let observed: HashSet<&str> = probe.develop_setting_keys.iter().map(String::as_str).collect();
        let probe_had_photo = !observed.is_empty();
        let can_read = probe.supports.get_develop_settings;
        let can_apply = probe.supports.apply_develop_preset
            && probe.supports.add_develop_preset_for_plugin
            && probe.supports.catalog_write_access;
        let can_snapshot = probe.supports.create_develop_snapshot && probe.supports.catalog_write_access;

        let mut controls = Vec::with_capacity(MAPPING.controls.len());
        let mut family_summary: BTreeMap<String, FamilySummary> = BTreeMap::new();
        for (name, fam) in &MAPPING.families {
            family_summary.insert(name.clone(), FamilySummary { label: fam.label.clone(), ..Default::default() });
        }
        for control in &MAPPING.controls {
            let observed_key = control.lightroom_keys.iter().find(|k| observed.contains(k.as_str())).cloned();
            let (status, reason) = match (&observed_key, static_write_policy(control), can_apply) {
                (None, _, _) if !probe_had_photo => (
                    ControlStatus::ObservedNotWritable,
                    "no probe photo yet — select a photo in Lightroom and test the connection".to_string(),
                ),
                (None, _, _) => (ControlStatus::Unsupported, "not returned by this Lightroom version".to_string()),
                (Some(_), Err(why), _) => (ControlStatus::ObservedNotWritable, why.to_string()),
                (Some(_), Ok(()), false) => {
                    (ControlStatus::ObservedNotWritable, "plugin preset application not available".to_string())
                }
                (Some(_), Ok(()), true) => {
                    (ControlStatus::Supported, "observed and writable via plugin preset".to_string())
                }
            };
            let summary = family_summary.entry(control.family.clone()).or_default();
            match status {
                ControlStatus::Supported => summary.supported += 1,
                ControlStatus::ObservedNotWritable => summary.observed_not_writable += 1,
                ControlStatus::Unsupported => summary.unsupported += 1,
            }
            controls.push(ControlCapability {
                canonical: control.canonical.clone(),
                family: control.family.clone(),
                status,
                lightroom_key: observed_key,
                reason,
            });
        }

        let mut writable_keys: Vec<&str> = controls
            .iter()
            .filter(|c| c.status == ControlStatus::Supported)
            .filter_map(|c| c.lightroom_key.as_deref())
            .collect();
        writable_keys.sort_unstable();
        let fingerprint = crate::ids::sha256_hex(
            format!("{}|{}|{}|{}", probe.lightroom_version, can_apply, can_snapshot, writable_keys.join(","))
                .as_bytes(),
        );

        CapabilityMatrix {
            schema_version: format!("cap_{}", &fingerprint[..16]),
            lightroom_version: probe.lightroom_version.clone(),
            plugin_version: probe.plugin_version.clone(),
            probe_had_photo,
            can_apply,
            can_snapshot,
            can_read,
            controls,
            family_summary,
            local_edits: "unsupported".into(),
            masks: "unsupported — mask/AI data is not readable or writable by Mimic 0.x".into(),
        }
    }

    /// Lightroom keys Mimic may write on this connection.
    pub fn writable_keys(&self) -> HashSet<String> {
        self.controls
            .iter()
            .filter(|c| c.status == ControlStatus::Supported)
            .filter_map(|c| c.lightroom_key.clone())
            .collect()
    }

    pub fn supported_count(&self) -> usize {
        self.controls.iter().filter(|c| c.status == ControlStatus::Supported).count()
    }
}

/// Keys a modern Lightroom Classic (PV 2012+) returns from `getDevelopSettings`.
/// Used ONLY by the fake bridge and demo mode — never as a runtime assumption.
pub fn fixture_develop_setting_keys() -> Vec<String> {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../../../fixtures/bridge/get_develop_settings.result.json"))
            .expect("fixture parses");
    fixture["result"]["settings"].as_object().map(|m| m.keys().cloned().collect()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full_probe() -> CapabilityProbe {
        CapabilityProbe {
            lightroom_version: "14.3".into(),
            sdk_version: Some("14.0".into()),
            plugin_version: crate::APP_VERSION.into(),
            develop_setting_keys: fixture_develop_setting_keys(),
            supports: SupportFlags {
                get_develop_settings: true,
                apply_develop_preset: true,
                add_develop_preset_for_plugin: true,
                create_develop_snapshot: true,
                develop_controller: false,
                catalog_write_access: true,
                lr_http: true,
            },
            notes: vec![],
        }
    }

    #[test]
    fn full_probe_supports_global_families_only() {
        let m = CapabilityMatrix::from_probe(&full_probe());
        assert!(m.can_apply && m.can_snapshot && m.can_read && m.probe_had_photo);
        assert!(m.supported_count() > 40, "{}", m.supported_count());
        let exposure = m.controls.iter().find(|c| c.canonical == "tone.exposure").unwrap();
        assert_eq!(exposure.status, ControlStatus::Supported);
        assert_eq!(exposure.lightroom_key.as_deref(), Some("Exposure2012"));
        let crop = m.controls.iter().find(|c| c.canonical == "crop.angle").unwrap();
        assert_eq!(crop.status, ControlStatus::ObservedNotWritable);
        let curve = m.controls.iter().find(|c| c.canonical == "toneCurve.point.rgb").unwrap();
        assert_eq!(curve.status, ControlStatus::ObservedNotWritable);
        assert!(m.writable_keys().contains("Exposure2012"));
        assert!(!m.writable_keys().contains("CropAngle"));
        assert!(m.schema_version.starts_with("cap_"));
    }

    #[test]
    fn missing_write_support_disables_apply() {
        let mut p = full_probe();
        p.supports.add_develop_preset_for_plugin = false;
        let m = CapabilityMatrix::from_probe(&p);
        assert!(!m.can_apply);
        assert_eq!(m.supported_count(), 0);
        assert!(m.writable_keys().is_empty());
        assert_ne!(m.schema_version, CapabilityMatrix::from_probe(&full_probe()).schema_version);
    }

    #[test]
    fn probe_without_photo_is_not_marked_unsupported() {
        let mut p = full_probe();
        p.develop_setting_keys.clear();
        let m = CapabilityMatrix::from_probe(&p);
        assert!(!m.probe_had_photo);
        assert!(m.controls.iter().all(|c| c.status == ControlStatus::ObservedNotWritable));
        assert_eq!(m.supported_count(), 0);
    }

    #[test]
    fn unknown_key_on_old_lightroom_is_unsupported() {
        let mut p = full_probe();
        p.develop_setting_keys.retain(|k| k != "Texture");
        let m = CapabilityMatrix::from_probe(&p);
        let t = m.controls.iter().find(|c| c.canonical == "presence.texture").unwrap();
        assert_eq!(t.status, ControlStatus::Unsupported);
    }
}
