//! `office.selection-get` and `office.selection-put` through dispatch.
//!
//! The tray is the in-memory fake, which applies the same domain rules as
//! the SQLite adapter. Expiry uses a fixed clock rather than the wall clock.

use std::time::{Duration, SystemTime};

use office_core::domain::selection::EXPIRY;
use office_core::domain::source::format_rfc3339_utc;
use office_core::ports::Ports;
use serde_json::{Value, json};

fn call(tool: &str, input: Value, ports: &Ports<'_>) -> Value {
    let raw = office_testkit::envelope::request(tool, input, json!({}));
    let request = office_core::protocol::parse(&raw).expect("request is valid");
    office_testkit::envelope::response(&crate::serve(&request, ports)).expect("response is JSON")
}

fn at(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
}

fn ports_at(
    selections: office_testkit::fakes::FakeSelections,
    now: SystemTime,
    initialized: bool,
) -> Ports<'static> {
    let library = if initialized {
        office_testkit::fakes::FakeLibrary::initialized()
    } else {
        office_testkit::fakes::FakeLibrary::default()
    };
    Ports {
        scripts: Box::new(office_testkit::fakes::FakeScripts::default()),
        library: Box::new(library),
        models: Box::new(office_testkit::fakes::FakeModels::new()),
        orbit: Box::new(office_testkit::fakes::FakeOrbit::default()),
        clock: Box::new(office_testkit::fakes::FakeClock(now)),
        selections: Box::new(selections),
    }
}

fn put(ports: &Ports<'_>, input: Value) -> Value {
    let response = call("office.selection-put", input, ports);
    assert_eq!(response["ok"], true, "{response}");
    response
}

#[test]
fn conformance_goldens_replay_offline_success_and_refusal() {
    for verb in ["selection-get", "selection-put"] {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "../../plugin/.orbit-plugin/tests/conformance/{verb}.yaml"
        ));
        let golden = std::fs::read_to_string(&path).expect("golden is readable");
        let ports = ports_at(
            office_testkit::fakes::FakeSelections::default(),
            at(1_700_000_000),
            true,
        );
        let mut input = None;
        let (mut successes, mut refusals) = (0, 0);
        for line in golden.lines() {
            if let Some(raw) = line.strip_prefix("    input: ") {
                input = Some(serde_json::from_str(raw).expect("golden input is JSON"));
            }
            for (prefix, success) in [("      output: ", true), ("      error: ", false)] {
                if let Some(raw) = line.strip_prefix(prefix) {
                    let expected: Value = serde_json::from_str(raw).expect("golden expect is JSON");
                    let response = call(
                        &format!("office.{verb}"),
                        input.take().expect("case has an input"),
                        &ports,
                    );
                    assert_eq!(response["ok"], success, "{response}");
                    if success {
                        successes += 1;
                        assert_eq!(response["output"], expected, "{response}");
                    } else {
                        refusals += 1;
                        assert_eq!(response["error"]["code"], expected["code"], "{response}");
                    }
                }
            }
        }
        assert!(successes > 0 && refusals > 0, "{verb}");
    }
}

#[test]
fn get_returns_each_selection_once_with_the_clock_time() {
    let selections = office_testkit::fakes::FakeSelections::default();
    let now = at(1_700_000_000);
    let ports = ports_at(selections.clone(), now, true);
    let recorded = put(
        &ports,
        json!({"object": "src-a", "view": "source", "anchor": "#L1", "quote": "alpha"}),
    );
    let selection = &recorded["output"]["selection"];
    assert_eq!(selection["id"], json!("sel-1"));
    assert_eq!(selection["object"], json!("src-a"));
    assert_eq!(selection["anchor"], json!("#L1"));
    assert_eq!(selection["quote"], json!("alpha"));
    assert!(selection["quote_sha256"].as_str().is_some());
    assert_eq!(selection["view"], json!("source"));
    assert_eq!(selection["sensitivity"], json!("normal"));
    assert_eq!(selection["selected_at"], json!(format_rfc3339_utc(now)));
    let later = ports_at(selections, now + Duration::from_secs(2), true);
    let peek = call("office.selection-get", json!({"peek": true}), &later);
    assert_eq!(peek["ok"], true, "{peek}");
    assert_eq!(peek["output"]["selections"][0]["object"], "src-a");
    let first = call("office.selection-get", json!({}), &later);
    assert_eq!(first["ok"], true, "{first}");
    assert_eq!(
        first["output"]["selections"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(first["output"]["selections"][0]["object"], json!("src-a"));
    let second = call("office.selection-get", json!({"peek": true}), &later);
    assert_eq!(second["output"]["selections"], json!([]));
}

#[test]
fn expired_selections_are_not_returned() {
    let selections = office_testkit::fakes::FakeSelections::default();
    let now = at(1_700_000_000);
    let early = ports_at(selections.clone(), now, true);
    put(
        &early,
        json!({"object": "src-old", "view": "source", "quote": "stale"}),
    );
    let late = ports_at(selections, now + EXPIRY, true);
    let expired = call("office.selection-get", json!({"peek": true}), &late);
    assert_eq!(expired["output"]["selections"], json!([]), "{expired}");
    put(
        &late,
        json!({"object": "src-new", "view": "page", "quote": "fresh"}),
    );
    let got = call("office.selection-get", json!({}), &late);
    assert_eq!(
        got["output"]["selections"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(got["output"]["selections"][0]["object"], json!("src-new"));
}

#[test]
fn sixth_put_evicts_the_oldest_and_a_second_get_is_empty() {
    let selections = office_testkit::fakes::FakeSelections::default();
    let start = at(1_700_000_000);
    for index in 1..=6 {
        let ports = ports_at(selections.clone(), start + Duration::from_secs(index), true);
        let response = put(
            &ports,
            json!({"object": format!("obj-{index}"), "view": "artifact", "quote": "q"}),
        );
        if index == 6 {
            assert_eq!(response["output"]["evicted"], json!(["sel-1"]));
        } else {
            assert_eq!(response["output"]["evicted"], json!([]));
        }
    }
    let ports = ports_at(selections, start + Duration::from_secs(10), true);
    let first = call("office.selection-get", json!({}), &ports);
    let objects: Vec<&str> = first["output"]["selections"]
        .as_array()
        .expect("selections")
        .iter()
        .map(|selection| selection["object"].as_str().expect("object"))
        .collect();
    assert_eq!(objects, ["obj-2", "obj-3", "obj-4", "obj-5", "obj-6"]);
    let second = call("office.selection-get", json!({}), &ports);
    assert_eq!(second["output"]["selections"], json!([]));
}

#[test]
fn a_private_selection_returns_no_quote() {
    let selections = office_testkit::fakes::FakeSelections::default();
    let now = at(1_700_000_000);
    let ports = ports_at(selections.clone(), now, true);
    let recorded = put(
        &ports,
        json!({
            "object": "src-private",
            "view": "task",
            "quote": "secret words",
            "quote_sha256": "not-stored",
            "sensitivity": "private"
        }),
    );
    assert_eq!(recorded["output"]["selection"]["quote"], Value::Null);
    assert_eq!(recorded["output"]["selection"]["quote_sha256"], Value::Null);
    assert_eq!(
        recorded["output"]["selection"]["sensitivity"],
        json!("private")
    );
    let ports = ports_at(selections, now, true);
    let got = call("office.selection-get", json!({}), &ports);
    assert_eq!(got["output"]["selections"][0]["quote"], Value::Null);
    assert_eq!(got["output"]["selections"][0]["quote_sha256"], Value::Null);
}

#[test]
fn clear_empties_the_tray() {
    let selections = office_testkit::fakes::FakeSelections::default();
    let now = at(1_700_000_000);
    let ports = ports_at(selections.clone(), now, true);
    put(
        &ports,
        json!({"object": "src-a", "view": "source", "quote": "alpha"}),
    );
    let cleared = put(&ports, json!({"clear": true}));
    assert_eq!(
        cleared["output"],
        json!({"cleared": true, "evicted": [], "selection": null})
    );
    let ports = ports_at(selections, now, true);
    let got = call("office.selection-get", json!({}), &ports);
    assert_eq!(got["output"]["selections"], json!([]));
}

#[test]
fn selection_tools_refuse_an_uninitialized_library_and_a_bad_put() {
    let cold = ports_at(
        office_testkit::fakes::FakeSelections::default(),
        at(1_700_000_000),
        false,
    );
    let refused = call("office.selection-get", json!({}), &cold);
    assert_eq!(refused["ok"], false, "{refused}");
    assert_eq!(refused["error"]["code"], json!("library_not_initialized"));
    let ports = ports_at(
        office_testkit::fakes::FakeSelections::default(),
        at(1_700_000_000),
        true,
    );
    for input in [
        json!({"peek": "true"}),
        json!({"peek": null}),
        json!({"other": true}),
    ] {
        let bad = call("office.selection-get", input, &ports);
        assert_eq!(bad["error"]["code"], json!("invalid_argument"));
    }
    let missing = call("office.selection-put", json!({}), &ports);
    assert_eq!(missing["error"]["code"], json!("invalid_argument"));
    let bad_view = call(
        "office.selection-put",
        json!({"object": "src-a", "view": "slide"}),
        &ports,
    );
    assert_eq!(bad_view["error"]["code"], json!("invalid_argument"));
}
