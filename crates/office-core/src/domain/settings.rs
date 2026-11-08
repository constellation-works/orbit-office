//! Typed Library settings. Parsing is pure; the adapter reads `.office/config.yaml`.
//!
//! The supported YAML subset is a two-level mapping, scalar values, and inline
//! or block string lists. Unsupported YAML constructs are refused, never guessed.

mod convert;
mod core;
mod helpers;
mod model;
mod types;
mod yaml;

pub use model::{
    Costs, DEFAULT_CAREFUL_MODEL, DEFAULT_STANDARD_MODEL, DEFAULT_TRANSCRIBE_MODEL, ModelSettings,
    OpenWeights, SignalThresholds,
};
pub use types::{
    BriefSettings, DemoSettings, HandoffMode, MeetingSettings, PrecedentSettings, Settings,
    VideoSettings,
};
