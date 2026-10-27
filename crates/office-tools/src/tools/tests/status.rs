//! The status handler: secret presence is reported, never a value.

use crate::registry::Call;
use crate::tools::status::run;
use serde_json::{Map, json};

#[test]
fn status_reports_set_and_unset_secrets_without_values() {
    let input = Map::new();
    let mut context = Map::new();
    context.insert(
        "secrets".to_string(),
        json!({ "meta_api_key": "do-not-echo-this" }),
    );
    let output = run(&Call {
        ports: &office_testkit::fakes::PortsBuilder::default().build(),
        input: &input,
        context: &context,
    })
    .expect("status succeeds");
    assert_eq!(output["secrets"]["meta_api_key"], json!("set"));
    assert_eq!(output["secrets"]["baseten_token"], json!("unset"));
    assert!(
        !output.to_string().contains("do-not-echo-this"),
        "a secret value must never appear in output"
    );
}

#[test]
fn status_reports_library_health_and_counts_by_kind() {
    let input = Map::new();
    let context = Map::new();
    // The builder's fake Library is initialized and empty.
    let output = run(&Call {
        ports: &office_testkit::fakes::PortsBuilder::default().build(),
        input: &input,
        context: &context,
    })
    .expect("status succeeds");
    assert_eq!(output["library"]["initialized"], json!(true));
    assert_eq!(output["library"]["documents"], json!(0));
    assert_eq!(output["library"]["documents_by_kind"]["source"], json!(0));
    assert_eq!(output["library"]["documents_by_kind"]["domain"], json!(0));
    assert_eq!(output["library"]["documents_by_kind"]["runbook"], json!(0));
    assert_eq!(output["library"]["documents_by_kind"]["artifact"], json!(0));
}

#[test]
fn status_reports_an_uninitialized_library_without_refusing() {
    let input = Map::new();
    let context = Map::new();
    let ports = office_core::ports::Ports {
        library: Box::new(office_testkit::fakes::FakeLibrary::default()),
        models: Box::new(office_testkit::fakes::FakeModels::new()),
        orbit: Box::new(office_testkit::fakes::FakeOrbit),
        clock: Box::new(office_testkit::fakes::FakeClock(
            std::time::SystemTime::UNIX_EPOCH,
        )),
    };
    let output = run(&Call {
        ports: &ports,
        input: &input,
        context: &context,
    })
    .expect("status succeeds");
    assert_eq!(output["library"]["initialized"], json!(false));
    assert_eq!(output["library"]["documents"], json!(0));
}
