//! Sources: capture, transcription, video planning and frames, extraction
//! and display (specs/library.md, specs/kt-videos.md).
//! Integrator-owned: one line per tool, written by the scaffold.

pub(crate) mod capture;
pub(crate) mod extract;
pub(crate) mod frames;
pub(crate) mod set_sensitivity;
pub(crate) mod show;
pub(crate) mod transcribe;
pub(crate) mod video_plan;
