//! Fake models port: canned answers, and a record of every call's
//! sensitivity so a test can assert where material would have gone.
//! Normal calls report the `hosted` route; private calls report
//! `private:company`.

use std::cell::RefCell;
use std::rc::Rc;

use office_common::Sensitivity;
use office_core::domain::endpoint::{EndpointKind, ModelRoute};
use office_core::ports::{
    DerivedBy, ExtractRequest, ImageReading, ImageRequest, Models, RouteCheck, Routed,
    TranscribeRequest, Transcript, TranscriptSegment,
};
use office_core::{Error, Result};
use serde_json::{Value, json};

/// A models fake with explicit canned answers and no host I/O.
#[derive(Clone, Default)]
pub struct FakeModels {
    /// Successive window/merge replies; an empty queue uses `extracted`.
    pub extract_replies: Rc<RefCell<std::collections::VecDeque<Value>>>,
    /// Transcription requests, including deterministic failures.
    pub transcribe_requests: Rc<RefCell<Vec<TranscribeRequest>>>,
    /// Optional zero-based transcription request to fail after recording it.
    pub transcribe_failure: Option<usize>,
    /// Optional zero-based extraction request to fail after recording it.
    pub extract_failure: Option<usize>,
    /// Extraction inputs in call order, shared across clones.
    pub extract_requests: Rc<RefCell<Vec<ExtractRequest>>>,
    /// Local audio windows requested; these spend no model calls.
    pub audio_windows: Rc<RefCell<Vec<(u64, u64)>>>,
    /// Local media metadata returned by analysis; never records an inference.
    pub video_analysis: Option<office_core::domain::source::video::VideoAnalysis>,
    /// PNG bytes returned by local extraction.
    pub video_png: Vec<u8>,
    calls: Rc<RefCell<Vec<office_common::Sensitivity>>>,
    /// Transcript returned by `transcribe`.
    pub transcript: Transcript,
    /// Value returned by `extract`.
    pub extracted: Value,
    /// Reading returned by `read_image`.
    pub reading: Option<ImageReading>,
    /// What `check_private_route` returns; `None` refuses it as
    /// `private_endpoint_unconfigured`.
    pub route_check: Option<RouteCheck>,
    checks: Rc<RefCell<usize>>,
    image_failure: Rc<RefCell<Option<usize>>>,
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
                chunks: Vec::new(),
            },
            extracted: json!({}),
            ..Self::default()
        }
    }

    /// Refuse the image request at this zero-based call-log index after recording it.
    /// Clones share this setting, allowing deterministic partial-failure tests.
    pub fn fail_image_call_at(&self, index: usize) {
        *self.image_failure.borrow_mut() = Some(index);
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
    fn media_audio(&self, bytes: &[u8], start: u64, length: u64) -> Result<Vec<u8>> {
        self.audio_windows.borrow_mut().push((start, length));
        Ok(bytes.to_vec())
    }
    fn analyze_video(
        &self,
        _bytes: &[u8],
        _threshold: &serde_json::Number,
    ) -> Result<office_core::domain::source::video::VideoAnalysis> {
        self.video_analysis.clone().ok_or_else(|| {
            office_core::domain::source::video::refusal(
                "media_unavailable",
                "fixture media metadata absent",
            )
        })
    }
    fn video_frame(&self, _bytes: &[u8], _time_ms: u64) -> Result<Vec<u8>> {
        Ok(self.video_png.clone())
    }

    fn transcribe(&self, request: &TranscribeRequest) -> Result<Routed<Transcript>> {
        self.transcribe_requests.borrow_mut().push(request.clone());
        let reply = self.routed(request.sensitivity, self.transcript.clone());
        if self.transcribe_failure == Some(self.transcribe_requests.borrow().len() - 1) {
            return Err(Error::Model {
                code: "model_unavailable",
                message: "fixture transcription failed".into(),
                retryable: false,
            });
        }
        Ok(reply)
    }

    fn extract(&self, request: &ExtractRequest) -> Result<Routed<Value>> {
        self.extract_requests.borrow_mut().push(request.clone());
        let value = self
            .extract_replies
            .borrow_mut()
            .pop_front()
            .unwrap_or_else(|| self.extracted.clone());
        let reply = self.routed(request.sensitivity, value);
        if self.extract_failure == Some(self.extract_requests.borrow().len() - 1) {
            return Err(Error::Model {
                code: "model_unavailable",
                message: "fixture extraction failed".into(),
                retryable: false,
            });
        }
        Ok(reply)
    }

    fn read_planned_image(
        &self,
        request: &ImageRequest,
        _settings_digest: &str,
    ) -> Result<Routed<ImageReading>> {
        self.read_image(request)
    }
    fn read_image(&self, request: &ImageRequest) -> Result<Routed<ImageReading>> {
        let reading = self.reading.clone().unwrap_or(ImageReading {
            text: "fake reading".into(),
            fields: json!({}),
        });
        let routed = self.routed(request.sensitivity, reading);
        if *self.image_failure.borrow() == Some(self.calls.borrow().len() - 1) {
            return Err(Error::Model {
                code: "model_unavailable",
                message: "fixture image request failed".into(),
                retryable: false,
            });
        }
        Ok(routed)
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
