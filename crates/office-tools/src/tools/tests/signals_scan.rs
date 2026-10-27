//! `office.signals-scan` through dispatch against the Orbit and Library
//! fakes (STD-04): fixture histories produce the expected candidates.

use office_core::domain::knowledge::{SearchFilter, SearchHit};
use office_core::domain::settings::Settings;
use office_core::ports::Library;
use office_core::{Error, Result};
use serde_json::{Map, Value, json};

use crate::registry::Call;
use crate::tools::proposals::signals_scan::run;

fn ports_with(
    tasks: Value,
    frictions: Value,
) -> (
    office_core::ports::Ports<'static>,
    office_testkit::fakes::FakeOrbit,
) {
    let orbit = office_testkit::fakes::FakeOrbit::default();
    orbit.push_response(Ok(tasks));
    orbit.push_response(Ok(frictions));
    let observer = orbit.clone();
    (
        office_core::ports::Ports {
            library: Box::new(office_testkit::fakes::FakeLibrary::initialized()),
            models: Box::new(office_testkit::fakes::FakeModels::new()),
            orbit: Box::new(orbit),
            clock: Box::new(office_testkit::fakes::FakeClock(
                std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(86_400 * 40),
            )),
        },
        observer,
    )
}

fn call_run(ports: &office_core::ports::Ports<'_>) -> Value {
    let input = Map::new();
    let context = Map::new();
    run(&Call {
        ports,
        input: &input,
        context: &context,
    })
    .expect("scan succeeds")
}

#[test]
fn scan_returns_repetition_and_friction_candidates_with_evidence() {
    let tasks = json!({"tasks": [
        {"id": "t1", "title": "weekly status report", "status": "done", "tags": ["owner:you", "report"], "completed_days_ago": 3},
        {"id": "t2", "title": "weekly status report", "status": "done", "tags": ["owner:you", "report"], "completed_days_ago": 10},
        {"id": "t3", "title": "weekly status report", "status": "done", "tags": ["owner:you", "report"], "completed_days_ago": 20},
        {"id": "t4", "title": "one off errand", "status": "done", "tags": ["owner:you"], "completed_days_ago": 2}
    ]});
    let frictions = json!({"hits": [
        {"record": {"id": "f1", "text": "deploy script fails on macos"}},
        {"record": {"id": "f2", "text": "deploy script fails again on macos"}}
    ]});
    let (ports, orbit) = ports_with(tasks, frictions);
    let output = call_run(&ports);
    assert_eq!(output["count"], json!(2));
    let signals: Vec<&str> = output["candidates"]
        .as_array()
        .expect("candidates")
        .iter()
        .map(|c| c["signal"].as_str().expect("signal"))
        .collect();
    assert_eq!(signals, vec!["friction_recurrence", "repetition"]);
    assert_eq!(
        output["candidates"][1]["evidence"],
        json!(["t1", "t2", "t3"])
    );
    assert_eq!(output["candidates"][0]["evidence"], json!(["f1", "f2"]));
    // Both reads went through the Orbit port.
    let tools: Vec<&str> = orbit.calls().iter().map(|c| c.tool).collect();
    assert_eq!(tools, vec!["orbit.task.list", "orbit.search"]);
}

#[test]
fn scan_below_threshold_returns_no_candidates() {
    let tasks = json!({"tasks": [
        {"id": "t1", "title": "weekly status report", "status": "done", "tags": ["owner:you", "report"], "completed_days_ago": 3}
    ]});
    let (ports, _) = ports_with(tasks, json!({"hits": []}));
    let output = call_run(&ports);
    assert_eq!(output["count"], json!(0));
    assert_eq!(output["candidates"], json!([]));
}

#[test]
fn scan_refuses_an_uninitialized_library() {
    let ports = office_core::ports::Ports {
        library: Box::new(office_testkit::fakes::FakeLibrary::default()),
        models: Box::new(office_testkit::fakes::FakeModels::new()),
        orbit: Box::new(office_testkit::fakes::FakeOrbit::default()),
        clock: Box::new(office_testkit::fakes::FakeClock(
            std::time::SystemTime::UNIX_EPOCH,
        )),
    };
    let input = Map::new();
    let context = Map::new();
    let error = run(&Call {
        ports: &ports,
        input: &input,
        context: &context,
    })
    .expect_err("uninitialized library refuses");
    assert_eq!(error.code(), "library_not_initialized");
}

fn clock() -> office_testkit::fakes::FakeClock {
    office_testkit::fakes::FakeClock(
        std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(86_400 * 40),
    )
}

fn ports_for(
    orbit: office_testkit::fakes::FakeOrbit,
    library: Box<dyn Library>,
) -> office_core::ports::Ports<'static> {
    office_core::ports::Ports {
        library,
        models: Box::new(office_testkit::fakes::FakeModels::new()),
        orbit: Box::new(orbit),
        clock: Box::new(clock()),
    }
}

/// `office.signals-scan` through the registered serve entry point.
fn scan(ports: &office_core::ports::Ports<'_>) -> Value {
    let raw = office_testkit::envelope::request("office.signals-scan", json!({}), json!({}));
    let request = office_core::protocol::parse(&raw).expect("request is valid");
    office_testkit::envelope::response(&crate::serve(&request, ports)).expect("response is JSON")
}

fn assert_named_refusal(response: &Value, code: &str, read: &str, cause: &str) {
    assert_eq!(response["ok"], json!(false), "{response}");
    assert!(
        response.get("output").is_none(),
        "a failed or partial read must not return a candidate count: {response}"
    );
    assert_eq!(response["error"]["code"], json!(code), "{response}");
    let message = response["error"]["message"]
        .as_str()
        .expect("error message");
    assert!(
        message.contains(read),
        "response should name {read}, got {message}"
    );
    assert!(
        message.contains(cause),
        "response should keep the underlying cause {cause}, got {message}"
    );
}

fn repetition_tasks(truncated: bool, total: u64) -> Value {
    json!({"tasks": [
        {"id": "t1", "title": "weekly status report", "status": "done", "tags": ["owner:you", "report"], "completed_days_ago": 3},
        {"id": "t2", "title": "weekly status report", "status": "done", "tags": ["owner:you", "report"], "completed_days_ago": 10},
        {"id": "t3", "title": "weekly status report", "status": "done", "tags": ["owner:you", "report"], "completed_days_ago": 20}
    ], "total": total, "truncated": truncated})
}

#[test]
fn failed_task_list_is_named_and_not_a_candidate_count() {
    let orbit = office_testkit::fakes::FakeOrbit::default();
    orbit.push_response(Err(Error::OrbitUnavailable));
    let ports = ports_for(
        orbit,
        Box::new(office_testkit::fakes::FakeLibrary::initialized()),
    );
    let response = scan(&ports);
    assert_named_refusal(
        &response,
        "orbit_unavailable",
        "orbit.task.list",
        "orbit_unavailable",
    );
}

#[test]
fn refused_task_list_keeps_its_refusal_code() {
    let orbit = office_testkit::fakes::FakeOrbit::default();
    orbit.push_response(Err(Error::OrbitRefused {
        code: "rate_limited".to_string(),
    }));
    let ports = ports_for(
        orbit,
        Box::new(office_testkit::fakes::FakeLibrary::initialized()),
    );
    let response = scan(&ports);
    assert_named_refusal(
        &response,
        "orbit_refused",
        "orbit.task.list",
        "orbit_refused",
    );
    let message = response["error"]["message"].as_str().expect("message");
    assert!(
        message.contains("rate_limited"),
        "response should keep Orbit's refusal code, got {message}"
    );
}

#[test]
fn failed_friction_search_is_named_and_not_a_candidate_count() {
    let orbit = office_testkit::fakes::FakeOrbit::default();
    orbit.push_response(Ok(json!({"tasks": [], "total": 0, "truncated": false})));
    orbit.push_response(Err(Error::Orbit {
        code: "orbit_command_failed",
        message: "callback exited unsuccessfully".to_string(),
    }));
    let ports = ports_for(
        orbit,
        Box::new(office_testkit::fakes::FakeLibrary::initialized()),
    );
    let response = scan(&ports);
    assert_named_refusal(
        &response,
        "orbit_command_failed",
        "orbit.search",
        "orbit_command_failed",
    );
    let message = response["error"]["message"].as_str().expect("message");
    assert!(
        message.contains("friction"),
        "response should name the friction search, got {message}"
    );
}

/// Library search fails after both Orbit reads succeed.
struct LibrarySearchFails;

impl Library for LibrarySearchFails {
    fn is_initialized(&self) -> Result<bool> {
        Ok(true)
    }

    fn settings(&self) -> Result<Settings> {
        Ok(Settings::default())
    }

    fn search(&self, _query: &str, _filter: &SearchFilter) -> Result<Vec<SearchHit>> {
        Err(Error::Library {
            code: "io_error",
            message: "index unreadable".to_string(),
        })
    }
}

#[test]
fn failed_library_search_is_named_and_not_a_candidate_count() {
    let orbit = office_testkit::fakes::FakeOrbit::default();
    orbit.push_response(Ok(json!({"tasks": [], "total": 0, "truncated": false})));
    orbit.push_response(Ok(json!({"hits": []})));
    let ports = ports_for(orbit, Box::new(LibrarySearchFails));
    let response = scan(&ports);
    assert_named_refusal(&response, "io_error", "library.search", "io_error");
}

#[test]
fn truncated_task_list_is_not_presented_as_complete() {
    let orbit = office_testkit::fakes::FakeOrbit::default();
    // Three clustered tasks would be a repetition candidate if this page
    // were treated as the whole set. `total` is larger, and `truncated` says so.
    orbit.push_response(Ok(repetition_tasks(true, 5)));
    orbit.push_response(Ok(json!({"hits": []})));
    let ports = ports_for(
        orbit,
        Box::new(office_testkit::fakes::FakeLibrary::initialized()),
    );
    let response = scan(&ports);
    assert_named_refusal(
        &response,
        "task_list_truncated",
        "orbit.task.list",
        "truncated",
    );
    let message = response["error"]["message"].as_str().expect("message");
    assert!(
        message.contains("complete"),
        "response should say the page is not the complete task set, got {message}"
    );
}

#[test]
fn complete_task_list_with_truncated_false_still_returns_candidates() {
    let orbit = office_testkit::fakes::FakeOrbit::default();
    orbit.push_response(Ok(repetition_tasks(false, 3)));
    orbit.push_response(Ok(json!({"hits": []})));
    let ports = ports_for(
        orbit,
        Box::new(office_testkit::fakes::FakeLibrary::initialized()),
    );
    let response = scan(&ports);
    assert_eq!(response["ok"], json!(true), "{response}");
    assert_eq!(response["output"]["count"], json!(1), "{response}");
    assert_eq!(
        response["output"]["candidates"][0]["signal"],
        json!("repetition")
    );
}
