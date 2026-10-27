//! Recording behavior through the same port used by tool handlers.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use office_core::ports::Orbit;
use office_core::{Error, Result};
use office_testkit::fakes::FakeOrbit;
use serde_json::{Map, Value, json};

type Method = fn(&dyn Orbit, &Map<String, Value>) -> Result<Value>;

#[test]
fn observer_sees_every_filed_payload_after_the_fake_is_boxed() {
    let fake = FakeOrbit::default();
    let port: Box<dyn Orbit> = Box::new(fake.clone());
    let callbacks: [(&str, Method); 8] = [
        ("orbit.task.add", |p, i| p.task_add(i)),
        ("orbit.task.update", |p, i| p.task_update(i)),
        ("orbit.task.list", |p, i| p.task_list(i)),
        ("orbit.task.show", |p, i| p.task_show(i)),
        ("orbit.task.artifact.get", |p, i| p.task_artifact_get(i)),
        ("orbit.task.artifact.put", |p, i| p.task_artifact_put(i)),
        ("orbit.friction.add", |p, i| p.friction_add(i)),
        ("orbit.search", |p, i| p.search(i)),
    ];
    let input = json!({"title":"filed", "metadata":{"files":["output.md"]}});
    for (name, method) in callbacks {
        fake.push_response(Ok(json!({"tool":name})));
        assert_eq!(
            method(port.as_ref(), input.as_object().unwrap()).unwrap(),
            json!({"tool":name})
        );
    }
    let calls = fake.calls();
    assert_eq!(calls.len(), 8);
    for (call, (name, _)) in calls.iter().zip(callbacks) {
        assert_eq!(call.tool, name);
        assert_eq!(&call.input, input.as_object().unwrap());
    }
}

#[test]
fn failures_are_recorded_and_missing_replies_never_report_success() {
    let fake = FakeOrbit::default();
    fake.push_response(Err(Error::OrbitRefused {
        code: "policy_denied".into(),
    }));
    assert!(matches!(
        fake.task_add(&Map::new()),
        Err(Error::OrbitRefused { .. })
    ));
    assert!(matches!(
        fake.search(&Map::new()),
        Err(Error::OrbitUnavailable)
    ));
    assert_eq!(
        fake.calls().iter().map(|c| c.tool).collect::<Vec<_>>(),
        ["orbit.task.add", "orbit.search"]
    );
}
