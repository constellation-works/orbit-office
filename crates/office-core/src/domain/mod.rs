//! The domain model and its rules, one module per area, each matching a
//! spec under `docs/design/office/specs/`. Reserved until the first task
//! that needs an area fills it (ARCHITECTURE.md; the reservation is the
//! STD-02@3 §R17 deviation recorded in 4_decisions.md).

mod artifact;
pub mod knowledge;
mod metrics;
mod proposal;
mod script;
mod selection;
mod session;
pub mod settings;
pub mod signals;
mod source;
pub mod template;
