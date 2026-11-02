//! Replay the shipped provider golden through the public exec dispatcher.
#![allow(clippy::expect_used, clippy::unwrap_used)]
use office_testkit::{
    envelope,
    fakes::{FakeLibrary, FakeModels, FakeOrbit, PortsBuilder},
};
use serde_json::{Value, json};

#[test]
fn provider_conformance_is_model_free_and_uses_no_orbit_callbacks() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../plugin/.orbit-plugin/tests/conformance/decision-precedents.yaml");
    let mut input = None;
    let mut count = 0;
    for line in std::fs::read_to_string(path).unwrap().lines() {
        if let Some(value) = line.strip_prefix("    input: ") {
            input = Some(serde_json::from_str::<Value>(value).unwrap());
        }
        for (prefix, success) in [("      output: ", true), ("      error: ", false)] {
            if let Some(value) = line.strip_prefix(prefix) {
                let expected: Value = serde_json::from_str(value).unwrap();
                let models = FakeModels::new();
                let orbit = FakeOrbit::default();
                let mut ports = PortsBuilder::default().build();
                ports.library = Box::new(FakeLibrary::initialized());
                ports.models = Box::new(models.clone());
                ports.orbit = Box::new(orbit.clone());
                let raw = envelope::request(
                    "office.decision-precedents",
                    input.take().unwrap(),
                    json!({}),
                );
                let request = office_core::protocol::parse(&raw).unwrap();
                let response = envelope::response(&office_tools::serve(&request, &ports)).unwrap();
                assert_eq!(response["ok"], success, "{response}");
                if success {
                    assert_eq!(response["output"], expected);
                } else {
                    assert_eq!(response["error"]["code"], expected["code"]);
                }
                assert!(models.extract_requests.borrow().is_empty());
                assert!(orbit.calls().is_empty());
                count += 1;
            }
        }
    }
    assert_eq!(count, 4, "every shipped provider conformance case ran");
}
