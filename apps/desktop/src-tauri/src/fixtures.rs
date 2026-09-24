//! The contract fixtures this crate writes: read models written with
//! `MIMIC_REGEN_FIXTURES=1`, compared by shape otherwise, and parsed by the
//! zod suite — the same arrangement as the core's `pipeline_e2e` fixtures.

use serde_json::Value;

/// Written with `MIMIC_REGEN_FIXTURES=1`, compared by shape otherwise,
/// and parsed by the zod suite — the same arrangement as the core's
/// `pipeline_e2e` fixtures.
pub(crate) fn check_fixture(name: &str, value: &Value) {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../fixtures/contracts").join(name);
    let pretty = format!("{}\n", serde_json::to_string_pretty(value).unwrap());
    if std::env::var("MIMIC_REGEN_FIXTURES").is_ok() {
        std::fs::write(&path, &pretty).unwrap();
        return;
    }
    let existing = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("missing fixture {}: {e}. Run with MIMIC_REGEN_FIXTURES=1", path.display()));
    let existing: Value = serde_json::from_str(&existing).unwrap();
    assert_eq!(shape(&existing), shape(value), "the shape of {name} changed; regenerate the fixture if intended");
}

fn shape(v: &Value) -> Value {
    match v {
        Value::Object(map) => Value::Object(map.iter().map(|(k, v)| (k.clone(), shape(v))).collect()),
        Value::Array(items) => Value::Array(items.first().map(shape).into_iter().collect()),
        Value::String(_) => Value::String("string".into()),
        Value::Number(_) => Value::String("number".into()),
        Value::Bool(_) => Value::String("boolean".into()),
        Value::Null => Value::String("null".into()),
    }
}
