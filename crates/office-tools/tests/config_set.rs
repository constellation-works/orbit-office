//! The public tool dispatch with testkit services, without host I/O.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use office_testkit::{envelope, fakes::PortsBuilder};
use serde_json::{Value, json};

fn call(ports: &office_core::ports::Ports<'_>, input: Value) -> Value {
    let raw = envelope::request("office.config-set", input, json!({}));
    let request = office_core::protocol::parse(&raw).unwrap();
    envelope::response(&office_tools::serve(&request, ports)).unwrap()
}

#[test]
fn updates_persist_through_the_library_port_and_refusals_preserve_them() {
    let ports = PortsBuilder::default().build();
    let response = call(&ports, json!({"key": "retention.meeting", "value": 30}));
    assert_eq!(response["ok"], true, "{response}");
    assert_eq!(
        response["output"],
        json!({"path": ".office/config.yaml", "key": "retention.meeting"})
    );
    let settings = ports.library.settings().unwrap();
    assert_eq!(settings.retention["meeting"], Some(30));
    for (key, value, code) in [
        (
            "meta_api_key",
            json!("fixture-secret"),
            "unknown_library_setting",
        ),
        ("retention.meeting", json!(0), "invalid_library_setting"),
    ] {
        let response = call(&ports, json!({"key": key, "value": value}));
        assert_eq!(response["ok"], false);
        assert_eq!(response["error"]["code"], code);
        assert!(!response.to_string().contains("fixture-secret"));
        assert_eq!(ports.library.settings().unwrap(), settings);
    }
    assert_eq!(
        call(
            &ports,
            json!({"key": "sensitivity.note", "value": "private"})
        )["ok"],
        true
    );
    let updated = ports.library.settings().unwrap();
    assert_eq!(updated.retention["meeting"], Some(30));
    assert_eq!(updated.sensitivity["note"].as_str(), "private");
}

#[test]
fn demo_simulated_until_writes_a_date_and_refuses_anything_else_naming_the_field() {
    let ports = PortsBuilder::default().build();
    let response = call(
        &ports,
        json!({"key": "demo.simulated_until", "value": "2026-10-12"}),
    );
    assert_eq!(response["ok"], true, "{response}");
    assert_eq!(
        ports
            .library
            .settings()
            .unwrap()
            .demo
            .simulated_until
            .as_deref(),
        Some("2026-10-12")
    );
    for value in [json!("2026-02-30"), json!(20261012)] {
        let response = call(
            &ports,
            json!({"key": "demo.simulated_until", "value": value}),
        );
        assert_eq!(response["ok"], false, "{response}");
        assert_eq!(
            response["error"]["code"], "invalid_library_setting",
            "{response}"
        );
        assert!(
            response.to_string().contains("demo.simulated_until"),
            "{response}"
        );
        assert_eq!(
            ports
                .library
                .settings()
                .unwrap()
                .demo
                .simulated_until
                .as_deref(),
            Some("2026-10-12")
        );
    }
    let response = call(
        &ports,
        json!({"key": "demo.unknown", "value": "2026-10-12"}),
    );
    assert_eq!(
        response["error"]["code"], "unknown_library_setting",
        "{response}"
    );
    assert!(
        response.to_string().contains("demo.simulated_until"),
        "{response}"
    );
}
