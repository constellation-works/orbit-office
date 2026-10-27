//! The private-route check handler against fake Library and Models ports.

use std::time::{Duration, SystemTime};

use office_core::domain::endpoint::{Capability, EndpointKind};
use office_core::domain::settings::{OpenWeights, Settings};
use office_core::ports::{Ports, RouteCheck, RouteProbe};
use office_testkit::fakes::{FakeClock, FakeLibrary, FakeModels, FakeOrbit};
use serde_json::{Map, Value, json};

use crate::registry::Call;
use crate::tools::library::private_route_check::run;

const ENDPOINT: &str = "https://muse.internal.acme.example/v1";
const MODEL: &str = "meta-models/Muse-Spark-Open-816B-A42B";

fn library(open_weights: Option<OpenWeights>) -> FakeLibrary {
    let mut library = FakeLibrary::initialized();
    library.settings = Settings {
        open_weights,
        ..Settings::default()
    };
    library
}

fn route() -> OpenWeights {
    OpenWeights {
        endpoint: ENDPOINT.into(),
        model: MODEL.into(),
        kind: EndpointKind::Company,
        label: "Acme's server".into(),
        capabilities: None,
        verified_at: None,
    }
}

fn check(endpoint: &str, accepted: &[Capability]) -> RouteCheck {
    RouteCheck {
        endpoint: endpoint.into(),
        model: MODEL.into(),
        reported_model: Some("muse-spark-open".into()),
        probes: Capability::ALL
            .into_iter()
            .map(|capability| RouteProbe {
                capability,
                accepted: accepted.contains(&capability),
                elapsed_ms: accepted.contains(&capability).then_some(7),
                error: (!accepted.contains(&capability))
                    .then(|| "model_request_rejected".to_string()),
            })
            .collect(),
    }
}

struct Run {
    ports: Ports<'static>,
}

impl Run {
    fn new(library: FakeLibrary, route_check: Option<RouteCheck>) -> Self {
        let mut models = FakeModels::new();
        models.route_check = route_check;
        Self {
            ports: Ports {
                library: Box::new(library),
                models: Box::new(models),
                orbit: Box::new(FakeOrbit::default()),
                clock: Box::new(FakeClock(
                    SystemTime::UNIX_EPOCH + Duration::from_secs(1_793_008_920),
                )),
            },
        }
    }

    fn call(&self) -> office_core::Result<Value> {
        let input = Map::new();
        let context = Map::new();
        run(&Call {
            ports: &self.ports,
            input: &input,
            context: &context,
        })
    }

    fn settings(&self) -> Settings {
        self.ports.library.settings().expect("settings")
    }
}

#[test]
fn records_exactly_the_accepted_capabilities_with_verified_at() {
    let run = Run::new(
        library(Some(route())),
        Some(check(ENDPOINT, &[Capability::Text, Capability::Image])),
    );

    let output = run.call().expect("check succeeds");

    assert_eq!(output["capabilities"], json!(["text", "image"]));
    assert_eq!(output["checks"][0]["elapsed_ms"], json!(7));
    assert_eq!(output["checks"][2]["elapsed_ms"], Value::Null);
    assert_eq!(output["route"], json!("private:company"));
    assert_eq!(output["label"], json!("Acme's server"));
    assert_eq!(output["routable"], json!(true));
    assert_eq!(output["checks"][2]["capability"], json!("audio"));
    assert_eq!(output["checks"][2]["accepted"], json!(false));
    assert!(
        output["checks"][2]["note"]
            .as_str()
            .is_some_and(|note| note.contains("doesn't take audio"))
    );
    assert_eq!(output["checks"][0]["note"], Value::Null);
    let recorded = run.settings().open_weights.expect("block kept");
    assert_eq!(
        recorded.capabilities,
        Some(vec![Capability::Text, Capability::Image])
    );
    assert_eq!(
        recorded.verified_at.as_deref(),
        Some("2026-10-26T10:02:00+00:00")
    );
}

#[test]
fn an_endpoint_failing_text_is_recorded_as_not_routable() {
    let run = Run::new(
        library(Some(route())),
        Some(check(ENDPOINT, &[Capability::Image, Capability::Audio])),
    );

    let output = run.call().expect("check succeeds");

    assert_eq!(output["routable"], json!(false));
    let recorded = run.settings().open_weights.expect("block kept");
    assert!(!recorded.accepts(Capability::Text));
}

#[test]
fn refuses_without_an_endpoint_and_sends_nothing() {
    let run = Run::new(library(None), Some(check(ENDPOINT, &Capability::ALL)));
    let error = run.call().expect_err("unconfigured");
    assert_eq!(error.code(), "private_endpoint_unconfigured");
    assert_eq!(run.settings().open_weights, None);
}

#[test]
fn refuses_to_record_a_check_made_on_another_endpoint() {
    let run = Run::new(
        library(Some(route())),
        Some(check("https://other.acme.example/v1", &Capability::ALL)),
    );
    let error = run.call().expect_err("endpoint changed");
    assert_eq!(error.code(), "private_route_changed");
    assert_eq!(run.settings().open_weights, Some(route()));
}
