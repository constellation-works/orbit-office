//! The domain model and its rules, one module per area, each matching a
//! spec under `docs/design/office/specs/`. Reserved until the first task
//! that needs an area fills it (ARCHITECTURE.md; the reservation is the
//! STD-02@3 §R17 deviation recorded in 4_decisions.md).

pub mod artifact;
pub mod bootstrap;
pub mod endpoint;
pub mod knowledge;
pub mod metrics;
mod proposal;
pub mod script;
pub mod selection;
mod session;
pub mod settings;
pub mod signals;
pub mod source;
pub mod template;
