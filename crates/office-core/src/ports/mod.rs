//! Ports through which tools reach external services and the clock.

mod calendar;
mod clock;
mod library;
mod models;
mod orbit;
mod scripts;
mod selection;
mod shared;

pub use calendar::{Calendar, UnavailableCalendar};
pub use clock::Clock;
pub use library::{
    CaptureSourceRequest, ConversationEntry, InitOutcome, Library, LibraryHealth, LibraryText,
    SensitivityOutcome, SourceOutcome, SourceView, not_initialized_error,
};
pub use models::{
    DerivedBy, ExtractRequest, ImageReading, ImageRequest, ModelTier, Models, RouteCheck,
    RouteProbe, Routed, TranscribeRequest, Transcript, TranscriptChunk, TranscriptSegment,
};
pub use office_common::Sensitivity;
pub use orbit::Orbit;
pub use scripts::ScriptRunner;
pub use selection::{Selections, UnavailableSelections};
pub use shared::{
    PushOutcome, PushRequest, SharedFile, SharedFileKind, SharedRepository, SharedSnapshot,
    UnavailableShared,
};

/// The services available to a single tool call.
pub struct Ports<'a> {
    /// Private calendar snapshot and offline imports.
    pub calendar: Box<dyn Calendar + 'a>,
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
    /// The selection tray in plugin state.
    pub selections: Box<dyn Selections + 'a>,
    /// The team's shared library clone in plugin state.
    pub shared: Box<dyn SharedRepository + 'a>,
}
