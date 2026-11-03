//! Session continuity: the session brief and the daily digest
//! (specs/sessions.md).
//! Integrator-owned: one line per tool, written by the scaffold.

pub(crate) mod brief;
pub(crate) mod decision_ingest;
pub(crate) mod decision_precedents;
pub(crate) mod decision_resolve;
pub(crate) mod digest;
