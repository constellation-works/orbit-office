//! Fake models port: canned answers, and a record of every call's
//! sensitivity so a test can assert where material would have gone.
//! Normal calls report the `hosted` route; private calls report
//! `private:company`.

use std::cell::RefCell;

use office_common::Sensitivity;
use office_core::domain::endpoint::{EndpointKind, ModelRoute};
use office_core::ports::{
    DerivedBy, ExtractRequest, ImageReading, ImageRequest, Models, RouteCheck, Routed,
    TranscribeRequest, Transcript, TranscriptSegment,
};
use office_core::{Error, Result};
use serde_json::{Value, json};

/// A models fake with explicit canned answers and no host I/O.
#[derive(Default)]
pub struct FakeModels {
    calls: RefCell<Vec<office_common::Sensitivity>>,
    /// Transcript returned by `transcribe`.
    pub transcript: Transcript,
    /// Value returned by `extract`.
    pub extracted: Value,
    /// Reading returned by `read_image`.
    pub reading: Option<ImageReading>,
    /// What `check_private_route` returns; `None` refuses it as
    /// `private_endpoint_unconfigured`.
    pub route_check: Option<RouteCheck>,
    checks: RefCell<usize>,
}

impl FakeModels {
    /// An empty fake returning a one-segment transcript and `{}` extractions.
    #[must_use]
    pub fn new() -> Self {
        Self {
            transcript: Transcript {
                text: "fake transcript".into(),
                segments: vec![TranscriptSegment {
                    speaker: Some("Speaker 1".into()),
                    start_ms: 0,
                    end_ms: 1000,
                    text: "fake transcript".into(),
                }],
            },
            extracted: json!({}),
            ..Self::default()
        }
    }

    /// Sensitivities of every call so far, in order.
    #[must_use]
    pub fn sensitivities(&self) -> Vec<office_common::Sensitivity> {
        self.calls.borrow().clone()
    }

    /// How many times `check_private_route` ran.
    #[must_use]
    pub fn checks(&self) -> usize {
        *self.checks.borrow()
    }

    fn routed<T>(&self, sensitivity: Sensitivity, value: T) -> Routed<T> {
        self.calls.borrow_mut().push(sensitivity);
        let (route, model) = match sensitivity {
            Sensitivity::Normal => (ModelRoute::Hosted, "fake-hosted"),
            Sensitivity::Private => (ModelRoute::Private(EndpointKind::Company), "fake-private"),
        };
        Routed {
            value,
            derived_by: DerivedBy {
                route,
                model: model.into(),
            },
        }
    }
}

impl Models for FakeModels {
    fn transcribe(&self, request: &TranscribeRequest) -> Result<Routed<Transcript>> {
        Ok(self.routed(request.sensitivity, self.transcript.clone()))
    }

    fn extract(&self, request: &ExtractRequest) -> Result<Routed<Value>> {
        Ok(self.routed(request.sensitivity, self.extracted.clone()))
    }

    fn read_image(&self, request: &ImageRequest) -> Result<Routed<ImageReading>> {
        let reading = self.reading.clone().unwrap_or(ImageReading {
            text: "fake reading".into(),
            fields: json!({}),
        });
        Ok(self.routed(request.sensitivity, reading))
    }

    fn check_private_route(&self) -> Result<RouteCheck> {
        *self.checks.borrow_mut() += 1;
        self.route_check.clone().ok_or_else(|| Error::Model {
            code: "private_endpoint_unconfigured",
            message: "private material needs an open_weights endpoint in Library settings".into(),
            retryable: false,
        })
    }
}
