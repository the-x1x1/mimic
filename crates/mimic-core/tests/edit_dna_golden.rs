//! Golden EditDNA normalization: the parsed crs maps in `fixtures/expected/*.raw.json`
//! (produced by the Python XMP parser) must normalize to exactly
//! `fixtures/expected/*.normalized.json`. Regenerate deliberately with
//! `MIMIC_REGEN_GOLDEN=1 cargo test -p mimic-core --test edit_dna_golden`.

use std::path::PathBuf;

use mimic_core::edit_dna::{normalize, Normalized};
use serde_json::Value;

const CASES: &[&str] = &["simple_pv2012", "modern_masks_unknown", "legacy_pv2010", "no_crs_metadata_only"];

fn expected_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/expected")
}

#[test]
fn normalization_matches_golden_files() {
    let regen = std::env::var_os("MIMIC_REGEN_GOLDEN").is_some();
    for case in CASES {
        let raw_file = expected_dir().join(format!("{case}.raw.json"));
        let raw: Value =
            serde_json::from_slice(&std::fs::read(&raw_file).unwrap_or_else(|e| panic!("{}: {e}", raw_file.display())))
                .unwrap();
        let settings = raw["rawSettings"].as_object().cloned().unwrap_or_default();
        let normalized = normalize(&settings);
        let golden_file = expected_dir().join(format!("{case}.normalized.json"));
        if regen {
            std::fs::write(&golden_file, serde_json::to_vec_pretty(&normalized).unwrap()).unwrap();
            continue;
        }
        let golden: Normalized = serde_json::from_slice(
            &std::fs::read(&golden_file)
                .unwrap_or_else(|e| panic!("{}: {e} (run with MIMIC_REGEN_GOLDEN=1)", golden_file.display())),
        )
        .unwrap();
        assert_eq!(normalized, golden, "golden mismatch for {case}");
    }
}

#[test]
fn golden_cases_cover_the_spec_scenarios() {
    let read = |case: &str| -> Normalized {
        serde_json::from_slice(&std::fs::read(expected_dir().join(format!("{case}.normalized.json"))).unwrap()).unwrap()
    };
    let simple = read("simple_pv2012");
    assert_eq!(simple.lightroom.process_version.as_deref(), Some("11.0"));
    assert_eq!(simple.get("tone.exposure").unwrap().raw, serde_json::json!(0.4));
    assert_eq!(simple.local.status, "unsupported");
    assert!(simple.meaningful_edit_count() > 20);

    let modern = read("modern_masks_unknown");
    assert_eq!(modern.local.status, "observed");
    assert_eq!(modern.local.observed_keys, vec!["MaskGroupBasedCorrections"]);
    assert!(modern.unknown.contains_key("SomeNewSlider2027"), "unknown slider preserved");
    assert!(modern.unknown.contains_key("Look"));
    assert!(modern.lightroom.heavy_edit_keys.iter().any(|k| k == "LensBlur"));
    assert!(modern.lightroom.heavy_edit_keys.iter().any(|k| k == "PointColors"));
    assert_eq!(modern.get("crop.hasCrop").unwrap().raw, serde_json::json!(true));
    assert_eq!(modern.get("crop.angle").unwrap().raw, serde_json::json!(-1.5));

    let legacy = read("legacy_pv2010");
    assert_eq!(legacy.get("tone.exposure").unwrap().source_key, "Exposure");
    assert_eq!(legacy.get("tone.shadows").unwrap().source_key, "FillLight");
    assert!(legacy.unknown.contains_key("Brightness"), "legacy-only key preserved");
    assert_eq!(legacy.get("toneCurve.point.rgb").unwrap().source_key, "ToneCurve");

    let none = read("no_crs_metadata_only");
    assert!(none.global.is_empty() && none.unknown.is_empty());
}
