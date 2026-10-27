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
    assert_eq!(output["library"]["pages_off_template"], json!(0));
    assert_eq!(output["library"]["template_invalid"], json!([]));
}

#[test]
fn status_reports_off_template_pages_and_names_a_broken_template() {
    let input = Map::new();
    let context = Map::new();
    let mut library = office_testkit::fakes::FakeLibrary::initialized();
    library.pages_off_template = 2;
    library.template_invalid = vec![".office/templates/notes.md".to_string()];
    let ports = office_core::ports::Ports {
        library: Box::new(library),
        models: Box::new(office_testkit::fakes::FakeModels::new()),
        orbit: Box::new(office_testkit::fakes::FakeOrbit::default()),
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
    assert_eq!(output["library"]["initialized"], json!(true));
    assert_eq!(output["library"]["documents"], json!(0));
    assert_eq!(output["library"]["pages_off_template"], json!(2));
    assert_eq!(
        output["library"]["template_invalid"],
        json!([".office/templates/notes.md"])
    );
    assert_eq!(output["secrets"]["baseten_token"], json!("unset"));
}

#[test]
fn status_reports_an_uninitialized_library_without_refusing() {
    let input = Map::new();
    let context = Map::new();
    let ports = office_core::ports::Ports {
        library: Box::new(office_testkit::fakes::FakeLibrary::default()),
        models: Box::new(office_testkit::fakes::FakeModels::new()),
        orbit: Box::new(office_testkit::fakes::FakeOrbit::default()),
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
    assert_eq!(output["library"]["pages_off_template"], json!(0));
    assert_eq!(output["library"]["template_invalid"], json!([]));
}

/// A Library whose health read is refused because a write is pending.
struct PendingWriteLibrary;

impl office_core::ports::Library for PendingWriteLibrary {
    fn health(&self) -> office_core::Result<office_core::ports::LibraryHealth> {
        Err(office_core::Error::Library {
            code: "library_write_pending",
            message: "a write is pending".to_string(),
        })
    }
}

#[test]
fn status_returns_a_pending_write_as_its_typed_error_not_as_uninitialized() {
    let input = Map::new();
    let context = Map::new();
    let ports = office_core::ports::Ports {
        library: Box::new(PendingWriteLibrary),
        models: Box::new(office_testkit::fakes::FakeModels::new()),
        orbit: Box::new(office_testkit::fakes::FakeOrbit::default()),
        clock: Box::new(office_testkit::fakes::FakeClock(
            std::time::SystemTime::UNIX_EPOCH,
        )),
    };
    let error = run(&Call {
        ports: &ports,
        input: &input,
        context: &context,
    })
    .expect_err("a pending write is refused, not reported as an uninitialized Library");
    assert_eq!(error.code(), "library_write_pending");
}

/// A context with no Library adapter: the port defaults refuse every operation.
struct NoLibrary;

impl office_core::ports::Library for NoLibrary {}

#[test]
fn status_reports_an_unavailable_library_adapter_as_uninitialized_without_refusing() {
    let input = Map::new();
    let context = Map::new();
    let ports = office_core::ports::Ports {
        library: Box::new(NoLibrary),
        models: Box::new(office_testkit::fakes::FakeModels::new()),
        orbit: Box::new(office_testkit::fakes::FakeOrbit::default()),
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
    assert_eq!(output["library"]["template_invalid"], json!([]));
}
