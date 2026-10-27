//! Ports through which tools reach external services and the clock.

mod clock;
mod library;
mod models;
mod orbit;
mod scripts;

pub use clock::Clock;
pub use library::{
    CaptureSourceRequest, ConversationEntry, InitOutcome, Library, LibraryHealth, LibraryText,
    SourceOutcome, SourceView, not_initialized_error,
};
pub use models::{
    DerivedBy, ExtractRequest, ImageReading, ImageRequest, Models, RouteCheck, RouteProbe, Routed,
    TranscribeRequest, Transcript, TranscriptChunk, TranscriptSegment,
};
pub use orbit::Orbit;
pub use scripts::ScriptRunner;

/// The services available to a single tool call.
pub struct Ports<'a> {
    /// Script dependency preparation and execution.
    pub scripts: Box<dyn ScriptRunner + 'a>,
    /// Library access, opened only when needed.
    pub library: Box<dyn Library + 'a>,
    /// Model access.
    pub models: Box<dyn Models + 'a>,
    /// Orbit callbacks.
    pub orbit: Box<dyn Orbit + 'a>,
    /// Current time.
    pub clock: Box<dyn Clock + 'a>,
}
