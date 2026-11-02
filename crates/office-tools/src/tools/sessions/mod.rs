//! Session continuity: the session brief and the daily digest
//! (specs/sessions.md).
//! Integrator-owned: one line per tool, written by the scaffold.

pub(crate) mod brief;
pub(crate) mod decision_ingest;
pub(crate) mod digest;
pub(crate) mod scribe_append;
pub(crate) mod scribe_ask;
pub(crate) mod scribe_control;
pub(crate) mod scribe_migrate;
pub(crate) mod scribe_pending;
pub(crate) mod scribe_query;
