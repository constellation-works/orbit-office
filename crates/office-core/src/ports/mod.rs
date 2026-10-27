//! Ports through which tools reach external services and the clock.

mod clock;
mod library;
mod models;
mod orbit;

pub use clock::Clock;
pub use library::{
    CaptureSourceRequest, ConversationEntry, InitOutcome, Library, LibraryHealth, SourceOutcome,
    SourceView, not_initialized_error,
};
pub use models::{
    ExtractRequest, ImageReading, ImageRequest, Models, TranscribeRequest, Transcript,
    TranscriptSegment,
};
pub use orbit::Orbit;

/// The services available to a single tool call.
pub struct Ports<'a> {
    /// Library access, opened only when needed.
    pub library: Box<dyn Library + 'a>,
    /// Model access.
    pub models: Box<dyn Models + 'a>,
    /// Orbit callbacks.
    pub orbit: Box<dyn Orbit + 'a>,
    /// Current time.
    pub clock: Box<dyn Clock + 'a>,
}
