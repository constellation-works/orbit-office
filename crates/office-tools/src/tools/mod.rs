//! One module per tool, grouped by spec area (ARCHITECTURE.md, "Tools").
//! Integrator-owned: every family was declared by the scaffold.

pub(crate) mod artifacts;
pub(crate) mod bootstrap;
pub(crate) mod calendar;
pub(crate) mod insights;
pub(crate) mod knowledge;
pub(crate) mod library;
pub(crate) mod proposals;
pub(crate) mod scripts;
pub(crate) mod selection;
pub(crate) mod sessions;
pub(crate) mod sharing;
pub(crate) mod sources;
pub(crate) mod status;

#[cfg(test)]
mod tests;
