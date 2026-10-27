//! `office.signals-scan` through dispatch against the Orbit and Library
//! fakes (STD-04): fixture histories produce the expected candidates.

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
