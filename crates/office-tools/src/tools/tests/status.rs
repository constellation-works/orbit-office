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
    assert_eq!(output["secrets"]["open_weights_token"], json!("unset"));
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
    assert_eq!(output["open_weights"], json!(null));
}

#[test]
fn status_reports_off_template_pages_and_names_a_broken_template() {
    let input = Map::new();
    let context = Map::new();
    let mut library = office_testkit::fakes::FakeLibrary::initialized();
    library.pages_off_template = 2;
    library.template_invalid = vec![".office/templates/notes.md".to_string()];
    let ports = office_core::ports::Ports {
        scripts: Box::new(office_testkit::fakes::FakeScripts::default()),
        library: Box::new(library),
        models: Box::new(office_testkit::fakes::FakeModels::new()),
        orbit: Box::new(office_testkit::fakes::FakeOrbit::default()),
        clock: Box::new(office_testkit::fakes::FakeClock(
            std::time::SystemTime::UNIX_EPOCH,
        )),
        calendar: Box::new(office_core::ports::UnavailableCalendar),
        selections: Box::new(office_testkit::fakes::FakeSelections::default()),
        shared: Box::new(office_testkit::fakes::FakeShared::default()),
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
    assert_eq!(output["secrets"]["open_weights_token"], json!("unset"));
}

#[test]
fn status_reports_an_uninitialized_library_without_refusing() {
    let input = Map::new();
    let context = Map::new();
    let ports = office_core::ports::Ports {
        scripts: Box::new(office_testkit::fakes::FakeScripts::default()),
        library: Box::new(office_testkit::fakes::FakeLibrary::default()),
        models: Box::new(office_testkit::fakes::FakeModels::new()),
        orbit: Box::new(office_testkit::fakes::FakeOrbit::default()),
        clock: Box::new(office_testkit::fakes::FakeClock(
            std::time::SystemTime::UNIX_EPOCH,
        )),
        calendar: Box::new(office_core::ports::UnavailableCalendar),
        selections: Box::new(office_testkit::fakes::FakeSelections::default()),
        shared: Box::new(office_testkit::fakes::FakeShared::default()),
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
    assert_eq!(output["open_weights"], json!(null));
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
        scripts: Box::new(office_testkit::fakes::FakeScripts::default()),
        library: Box::new(PendingWriteLibrary),
        models: Box::new(office_testkit::fakes::FakeModels::new()),
        orbit: Box::new(office_testkit::fakes::FakeOrbit::default()),
        clock: Box::new(office_testkit::fakes::FakeClock(
            std::time::SystemTime::UNIX_EPOCH,
        )),
        calendar: Box::new(office_core::ports::UnavailableCalendar),
        selections: Box::new(office_testkit::fakes::FakeSelections::default()),
        shared: Box::new(office_testkit::fakes::FakeShared::default()),
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
        scripts: Box::new(office_testkit::fakes::FakeScripts::default()),
        library: Box::new(NoLibrary),
        models: Box::new(office_testkit::fakes::FakeModels::new()),
        orbit: Box::new(office_testkit::fakes::FakeOrbit::default()),
        clock: Box::new(office_testkit::fakes::FakeClock(
            std::time::SystemTime::UNIX_EPOCH,
        )),
        calendar: Box::new(office_core::ports::UnavailableCalendar),
        selections: Box::new(office_testkit::fakes::FakeSelections::default()),
        shared: Box::new(office_testkit::fakes::FakeShared::default()),
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
    assert_eq!(output["open_weights"], json!(null));
}

#[test]
fn status_reports_the_private_route_token_and_never_the_retired_name() {
    let input = Map::new();
    let mut context = Map::new();
    context.insert(
        "secrets".to_string(),
        json!({ "open_weights_token": "do-not-echo-this", "baseten_token": "retired" }),
    );
    let output = run(&Call {
        ports: &office_testkit::fakes::PortsBuilder::default().build(),
        input: &input,
        context: &context,
    })
    .expect("status succeeds");
    assert_eq!(
        output["secrets"],
        json!({"meta_api_key": "unset", "open_weights_token": "set", "baseten_token": "set"})
    );
    assert!(!output.to_string().contains("do-not-echo-this"));
    assert_eq!(output["open_weights"], json!(null));
}

#[test]
fn status_accepts_the_retired_secret_name_as_the_open_weights_alias() {
    let input = Map::new();
    let mut context = Map::new();
    context.insert(
        "secrets".to_string(),
        json!({ "baseten_token": "do-not-echo-this" }),
    );
    let output = run(&Call {
        ports: &office_testkit::fakes::PortsBuilder::default().build(),
        input: &input,
        context: &context,
    })
    .expect("status succeeds");
    assert_eq!(
        output["secrets"],
        json!({"meta_api_key": "unset", "open_weights_token": "set", "baseten_token": "set"})
    );
    assert!(!output.to_string().contains("do-not-echo-this"));
}

#[test]
fn status_reports_a_configured_private_endpoint_and_never_its_token() {
    let input = Map::new();
    let mut context = Map::new();
    context.insert(
        "secrets".to_string(),
        json!({ "open_weights_token": "do-not-echo-this" }),
    );
    let mut library = office_testkit::fakes::FakeLibrary::initialized();
    library.settings = office_core::domain::settings::Settings {
        open_weights: Some(office_core::domain::settings::OpenWeights {
            endpoint: "https://muse.internal.example/v1".into(),
            model: "muse".into(),
            kind: office_core::domain::endpoint::EndpointKind::Provider,
            label: "Hosted".into(),
            capabilities: Some(vec![
                office_core::domain::endpoint::Capability::Text,
                office_core::domain::endpoint::Capability::Image,
            ]),
            verified_at: Some("2026-10-26T00:00:00Z".into()),
        }),
        ..office_core::domain::settings::Settings::default()
    };
    let ports = office_core::ports::Ports {
        scripts: Box::new(office_testkit::fakes::FakeScripts::default()),
        library: Box::new(library),
        models: Box::new(office_testkit::fakes::FakeModels::new()),
        orbit: Box::new(office_testkit::fakes::FakeOrbit::default()),
        clock: Box::new(office_testkit::fakes::FakeClock(
            std::time::SystemTime::UNIX_EPOCH,
        )),
        calendar: Box::new(office_core::ports::UnavailableCalendar),
        selections: Box::new(office_testkit::fakes::FakeSelections::default()),
        shared: Box::new(office_testkit::fakes::FakeShared::default()),
    };
    let output = run(&Call {
        ports: &ports,
        input: &input,
        context: &context,
    })
    .expect("status succeeds");
    assert_eq!(
        output["open_weights"],
        json!({
            "endpoint": "https://muse.internal.example/v1",
            "model": "muse",
            "kind": "provider",
            "label": "Hosted",
            "capabilities": ["text", "image"],
            "verified_at": "2026-10-26T00:00:00Z",
        })
    );
    assert_eq!(output["secrets"]["open_weights_token"], json!("set"));
    assert!(!output.to_string().contains("do-not-echo-this"));
}

/// Health says the Library is initialized, but reading settings is refused.
struct BrokenSettings;

impl office_core::ports::Library for BrokenSettings {
    fn health(&self) -> office_core::Result<office_core::ports::LibraryHealth> {
        Ok(office_core::ports::LibraryHealth {
            initialized: true,
            ..office_core::ports::LibraryHealth::default()
        })
    }

    fn settings(&self) -> office_core::Result<office_core::domain::settings::Settings> {
        Err(office_core::Error::Library {
            code: "invalid_library_setting",
            message: "open_weights.endpoint".to_string(),
        })
    }
}

#[test]
fn status_returns_a_settings_refusal_instead_of_a_missing_endpoint() {
    let input = Map::new();
    let context = Map::new();
    let ports = office_core::ports::Ports {
        scripts: Box::new(office_testkit::fakes::FakeScripts::default()),
        library: Box::new(BrokenSettings),
        models: Box::new(office_testkit::fakes::FakeModels::new()),
        orbit: Box::new(office_testkit::fakes::FakeOrbit::default()),
        clock: Box::new(office_testkit::fakes::FakeClock(
            std::time::SystemTime::UNIX_EPOCH,
        )),
        calendar: Box::new(office_core::ports::UnavailableCalendar),
        selections: Box::new(office_testkit::fakes::FakeSelections::default()),
        shared: Box::new(office_testkit::fakes::FakeShared::default()),
    };
    let error = run(&Call {
        ports: &ports,
        input: &input,
        context: &context,
    })
    .expect_err("invalid settings are not reported as no endpoint");
    assert_eq!(error.code(), "invalid_library_setting");
}

#[test]
fn status_reports_the_configured_shared_library_block() {
    let input = Map::new();
    let context = Map::new();
    let mut library = office_testkit::fakes::FakeLibrary::initialized();
    library.settings = office_core::domain::settings::Settings {
        shared: Some(office_core::domain::sharing::SharedSettings {
            remote: "git@github.com:acme-team/team-library.git".into(),
            branch: "main".into(),
            member: "daniel".into(),
        }),
        ..office_core::domain::settings::Settings::default()
    };
    let ports = office_core::ports::Ports {
        scripts: Box::new(office_testkit::fakes::FakeScripts::default()),
        library: Box::new(library),
        models: Box::new(office_testkit::fakes::FakeModels::new()),
        orbit: Box::new(office_testkit::fakes::FakeOrbit::default()),
        clock: Box::new(office_testkit::fakes::FakeClock(
            std::time::SystemTime::UNIX_EPOCH,
        )),
        calendar: Box::new(office_core::ports::UnavailableCalendar),
        selections: Box::new(office_testkit::fakes::FakeSelections::default()),
        shared: Box::new(office_testkit::fakes::FakeShared::default()),
    };
    let output = run(&Call {
        ports: &ports,
        input: &input,
        context: &context,
    })
    .expect("status succeeds");
    assert_eq!(
        output["shared"],
        json!({
            "remote": "git@github.com:acme-team/team-library.git",
            "branch": "main",
            "member": "daniel",
        })
    );
    // Unconfigured is null, like the private endpoint.
    let output = run(&Call {
        ports: &office_testkit::fakes::PortsBuilder::default().build(),
        input: &input,
        context: &context,
    })
    .expect("status succeeds");
    assert_eq!(output["shared"], json!(null));
}
