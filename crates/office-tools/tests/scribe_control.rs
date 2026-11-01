//! `office.scribe-control` through public exec dispatch: one shared record
//! per session, read and written by the Scribe window and the plugin.
#![allow(clippy::expect_used, clippy::unwrap_used)]
use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};

use office_core::ports::Ports;
use office_testkit::{
    envelope,
    fakes::{FakeClock, FakeScribeControls},
};
use serde_json::{Value, json};

/// 2026-11-01T00:00:00Z.
const T0: u64 = 1_793_491_200;

fn ports_at(controls: &FakeScribeControls, secs: u64) -> Ports<'static> {
    let mut ports = office_testkit::fakes::PortsBuilder::default().build();
    ports.clock = Box::new(FakeClock(UNIX_EPOCH + Duration::from_secs(secs)));
    ports.scribe_control = Box::new(controls.clone());
    ports
}

fn call(ports: &Ports<'_>, input: Value) -> Value {
    let request = office_core::protocol::parse(&envelope::request(
        "office.scribe-control",
        input,
        json!({}),
    ))
    .unwrap();
    envelope::response(&office_tools::serve(&request, ports)).unwrap()
}

fn ok(ports: &Ports<'_>, input: Value) -> Value {
    let response = call(ports, input);
    assert_eq!(response["ok"], true, "{response}");
    response["output"].clone()
}

#[test]
fn get_set_switch_request_now_set_title_and_heartbeat_round_trip() {
    let controls = FakeScribeControls::default();
    let ports = ports_at(&controls, T0);
    let before = ports.library.head_commit().unwrap();
    let sid = json!("session-one");

    let fresh = ok(&ports, json!({"op":"get","session_id":sid}));
    assert_eq!(
        fresh["on"],
        Value::Null,
        "unset switch: the plugin default applies"
    );
    assert_eq!(fresh["now_requested"], false);
    assert_eq!(fresh["window_open"], false);

    assert_eq!(
        ok(
            &ports,
            json!({"op":"set_switch","session_id":sid,"on":true})
        )["on"],
        true
    );
    let requested = ok(&ports, json!({"op":"request_now","session_id":sid}));
    assert_eq!(requested["now_requested"], true);
    assert_eq!(requested["now_requested_at"], "2026-11-01T00:00:00+00:00");
    let titled = ok(
        &ports,
        json!({"op":"set_title","session_id":sid,"title":"  Release planning  "}),
    );
    assert_eq!(titled["title"], "Release planning");
    let beat = ok(&ports, json!({"op":"heartbeat","session_id":sid}));
    assert_eq!(beat["heartbeat_at"], "2026-11-01T00:00:00+00:00");
    assert_eq!(beat["window_open"], true);

    // Another host (a later call) reads the same record.
    let later = ports_at(&controls, T0 + 119);
    let read = ok(&later, json!({"op":"get","session_id":sid}));
    assert_eq!(read["op"], "get");
    assert_eq!(read["on"], true);
    assert_eq!(read["now_requested"], true);
    assert_eq!(read["title"], "Release planning");
    assert_eq!(read["window_open"], true, "heartbeat under two minutes old");
    assert_eq!(
        ok(
            &ports_at(&controls, T0 + 120),
            json!({"op":"get","session_id":sid})
        )["window_open"],
        false
    );

    // A repeated request keeps the first request time; switching off sticks.
    let again = ok(&later, json!({"op":"request_now","session_id":sid}));
    assert_eq!(again["now_requested_at"], "2026-11-01T00:00:00+00:00");
    assert_eq!(
        ok(
            &later,
            json!({"op":"set_switch","session_id":sid,"on":false})
        )["on"],
        false
    );
    // Sessions are independent.
    assert_eq!(
        ok(&later, json!({"op":"get","session_id":"session-two"}))["on"],
        Value::Null
    );
    // Plugin state only: the Library is untouched.
    assert_eq!(ports.library.head_commit().unwrap(), before);
}

#[test]
fn consume_now_clears_a_pending_request_once_and_never_a_newer_one() {
    let controls = FakeScribeControls::default();
    let ports = ports_at(&controls, T0);
    let sid = json!("session-one");
    let none = ok(&ports, json!({"op":"consume_now","session_id":sid}));
    assert_eq!(none["consumed"], false, "nothing pending");

    let first =
        ok(&ports, json!({"op":"request_now","session_id":sid}))["now_requested_at"].clone();
    let consumed = ok(
        &ports,
        json!({"op":"consume_now","session_id":sid,"requested_at":first}),
    );
    assert_eq!(consumed["consumed"], true);
    assert_eq!(consumed["now_requested"], false);
    let twice = ok(
        &ports,
        json!({"op":"consume_now","session_id":sid,"requested_at":first}),
    );
    assert_eq!(twice["consumed"], false, "consumed once");

    // A newer request made after the hook read the old one survives.
    let later = ports_at(&controls, T0 + 5);
    ok(&later, json!({"op":"request_now","session_id":sid}));
    let stale = ok(
        &later,
        json!({"op":"consume_now","session_id":sid,"requested_at":first}),
    );
    assert_eq!(stale["consumed"], false);
    assert_eq!(stale["now_requested"], true);
    let any = ok(&later, json!({"op":"consume_now","session_id":sid}));
    assert_eq!(any["consumed"], true);
    assert!(
        ok(&later, json!({"op":"get","session_id":sid}))
            .get("consumed")
            .is_none()
    );
}

#[test]
fn invalid_inputs_are_refused_and_change_nothing() {
    let controls = FakeScribeControls::default();
    let ports = ports_at(&controls, T0);
    for input in [
        json!({"session_id":"session-one"}),
        json!({"op":"get"}),
        json!({"op":"get","session_id":"../escape"}),
        json!({"op":"get","session_id":"x".repeat(129)}),
        json!({"op":"set_switch","session_id":"session-one"}),
        json!({"op":"set_switch","session_id":"session-one","on":"yes"}),
        json!({"op":"set_title","session_id":"session-one","title":" "}),
        json!({"op":"set_title","session_id":"session-one","title":"two\nlines"}),
        json!({"op":"heartbeat","session_id":"session-one","on":true}),
        json!({"op":"consume_now","session_id":"session-one","requested_at":"yesterday"}),
        json!({"op":"erase","session_id":"session-one"}),
        json!({"op":"get","session_id":"session-one","invent":1}),
    ] {
        assert_eq!(
            call(&ports, input.clone())["error"]["code"],
            "invalid_argument",
            "{input}"
        );
    }
    let state = ok(&ports, json!({"op":"get","session_id":"session-one"}));
    assert_eq!(state["updated_at"], Value::Null, "refusals wrote nothing");
}

#[test]
fn records_untouched_for_thirty_days_are_dropped_on_a_later_write() {
    let controls = FakeScribeControls::default();
    ok(
        &ports_at(&controls, T0),
        json!({"op":"set_switch","session_id":"old","on":true}),
    );
    let month = T0 + 30 * 24 * 60 * 60;
    ok(
        &ports_at(&controls, month),
        json!({"op":"heartbeat","session_id":"new"}),
    );
    assert_eq!(
        ok(
            &ports_at(&controls, month),
            json!({"op":"get","session_id":"old"})
        )["on"],
        Value::Null
    );
}

#[test]
fn conformance_golden_runs_through_public_dispatch() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../plugin/.orbit-plugin/tests/conformance/scribe-control.yaml");
    let mut input = None;
    let mut count = 0;
    for line in std::fs::read_to_string(path).unwrap().lines() {
        if let Some(value) = line.strip_prefix("    input: ") {
            input = Some(serde_json::from_str::<Value>(value).unwrap());
        }
        for (prefix, success) in [("      output: ", true), ("      error: ", false)] {
            if let Some(value) = line.strip_prefix(prefix) {
                let expected: Value = serde_json::from_str(value).unwrap();
                let ports = ports_at(&FakeScribeControls::default(), T0);
                let response = call(&ports, input.take().unwrap());
                assert_eq!(response["ok"], success, "{response}");
                if success {
                    assert_eq!(response["output"], expected);
                } else {
                    assert_eq!(response["error"]["code"], expected["code"]);
                }
                count += 1;
            }
        }
    }
    assert_eq!(count, 5, "all golden cases executed");
}
