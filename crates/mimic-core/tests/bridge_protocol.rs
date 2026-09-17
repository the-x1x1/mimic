//! Golden fixture tests: every checked-in bridge fixture must deserialize into
//! the typed protocol, and the command list must match the spec.

use mimic_core::bridge::{
    ApplyBatchResult, CommandEnvelope, CommandResultBody, CommandType, EventsBody, HandshakeRequest, HandshakeResponse,
};
use mimic_core::capability::CapabilityMatrix;

macro_rules! fixture {
    ($name:literal) => {
        include_str!(concat!("../../../fixtures/bridge/", $name))
    };
}

#[test]
fn fixtures_match_types() {
    let hs: HandshakeRequest = serde_json::from_str(fixture!("handshake.request.json")).unwrap();
    assert_eq!(hs.protocol_version, mimic_core::BRIDGE_PROTOCOL_VERSION);
    let matrix = CapabilityMatrix::from_probe(&hs.capabilities);
    assert!(matrix.can_apply && matrix.can_snapshot);
    let _: HandshakeResponse = serde_json::from_str(fixture!("handshake.response.json")).unwrap();
    let cmd: CommandEnvelope = serde_json::from_str(fixture!("get_develop_settings.command.json")).unwrap();
    assert_eq!(cmd.command_type, CommandType::GetDevelopSettings);
    let res: CommandResultBody = serde_json::from_str(fixture!("get_develop_settings.result.json")).unwrap();
    assert!(res.ok);
    let apply_cmd: CommandEnvelope =
        serde_json::from_str(fixture!("apply_settings_as_plugin_preset.command.json")).unwrap();
    assert!(apply_cmd.command_type.is_mutating());
    for f in [
        fixture!("apply_settings_as_plugin_preset.result.success.json"),
        fixture!("apply_settings_as_plugin_preset.result.partial_failure.json"),
    ] {
        let body: CommandResultBody = serde_json::from_str(f).unwrap();
        let batch: ApplyBatchResult = serde_json::from_value(body.result.unwrap()).unwrap();
        assert_eq!(batch.items.len(), 2);
    }
    let err: CommandResultBody = serde_json::from_str(fixture!("command_error.result.json")).unwrap();
    assert!(!err.ok);
    assert_eq!(err.error.unwrap().code, "catalog_write_denied");
    let ev: EventsBody = serde_json::from_str(fixture!("event.selection_changed.json")).unwrap();
    assert_eq!(ev.events[0].kind, "selection_changed");
}

#[test]
fn command_set_is_the_spec_list() {
    let names: Vec<&str> = CommandType::ALL.iter().map(|c| c.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "ping",
            "get_catalog_info",
            "get_selected_photos",
            "get_photo_metadata",
            "get_develop_settings",
            "create_before_snapshot",
            "apply_settings_as_plugin_preset",
            "read_back_develop_settings",
            "collect_correction_state",
            "get_capabilities"
        ]
    );
    for c in CommandType::ALL {
        let round: CommandType = serde_json::from_value(serde_json::json!(c.as_str())).unwrap();
        assert_eq!(&round, c);
    }
}
