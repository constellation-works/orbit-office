//! The Library as a whole: scaffold, index, settings, search and
//! attachments (specs/library.md, specs/plugin-surface.md).
//! Integrator-owned: one line per tool, written by the scaffold.

pub(crate) mod attach;
pub(crate) mod config_set;
pub(crate) mod init;
pub(crate) mod private_route_check;
pub(crate) mod reindex;
pub(crate) mod search;
