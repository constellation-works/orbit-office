//! The Orbit port refuses every callback until an adapter is composed.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use office_core::Error;
use office_core::ports::Orbit;
use serde_json::{Map, Value};

struct Unconfigured;

impl Orbit for Unconfigured {}

type Method = fn(&dyn Orbit, &Map<String, Value>) -> office_core::Result<Value>;

#[test]
fn every_default_method_refuses_as_unavailable() {
    let methods: [Method; 11] = [
        |p, i| p.task_add(i),
        |p, i| p.task_update(i),
        |p, i| p.task_list(i),
        |p, i| p.task_show(i),
        |p, i| p.task_artifact_get(i),
        |p, i| p.task_artifact_put(i),
        |p, i| p.friction_add(i),
        |p, i| p.search(i),
        |p, i| p.auto_task_add(i),
        |p, i| p.auto_task_update(i),
        |p, i| p.auto_task_list(i),
    ];
    for method in methods {
        assert!(matches!(
            method(&Unconfigured, &Map::new()),
            Err(Error::OrbitUnavailable)
        ));
    }
}
