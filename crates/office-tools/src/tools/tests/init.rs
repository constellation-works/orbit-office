//! `office.init` through dispatch against the fake Library (STD-04).

use serde_json::{Value, json};

fn call(tool: &str, input: Value, ports: &office_core::ports::Ports<'_>) -> Value {
    let raw = office_testkit::envelope::request(tool, input, json!({}));
    let request = office_core::protocol::parse(&raw).expect("request is valid");
    office_testkit::envelope::response(&crate::serve(&request, ports)).expect("response is JSON")
}

fn uninitialized_ports() -> office_core::ports::Ports<'static> {
    office_core::ports::Ports {
        library: Box::new(office_testkit::fakes::FakeLibrary::default()),
        models: Box::new(office_testkit::fakes::FakeModels::new()),
        orbit: Box::new(office_testkit::fakes::FakeOrbit::default()),
        clock: Box::new(office_testkit::fakes::FakeClock(
            std::time::SystemTime::UNIX_EPOCH,
        )),
    }
}

#[test]
fn init_scaffolds_then_reports_already_initialized() {
    let ports = uninitialized_ports();
    let first = call("office.init", json!({}), &ports);
    assert_eq!(first["ok"], json!(true));
    assert_eq!(first["output"]["already_initialized"], json!(false));
    assert!(
        first["output"]["created"]
            .as_array()
            .is_some_and(|created| !created.is_empty())
    );
    let second = call("office.init", json!({}), &ports);
    assert_eq!(second["output"]["already_initialized"], json!(true));
    assert_eq!(second["output"]["created"], json!([]));
}

#[test]
fn init_refuses_a_library_dir_outside_the_library_and_unknown_keys() {
    let ports = uninitialized_ports();
    let outside = call(
        "office.init",
        json!({ "library_dir": "../outside" }),
        &ports,
    );
    assert_eq!(outside["ok"], json!(false));
    assert_eq!(
        outside["error"]["code"],
        json!("library_dir_outside_workspace")
    );
    let unknown = call("office.init", json!({ "verbose": true }), &ports);
    assert_eq!(unknown["error"]["code"], json!("invalid_argument"));
    let wrong_type = call("office.init", json!({ "library_dir": 42 }), &ports);
    assert_eq!(wrong_type["error"]["code"], json!("invalid_argument"));
}
