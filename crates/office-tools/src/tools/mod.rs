//! One module per tool, grouped by spec area (ARCHITECTURE.md, "Tools").
//! Integrator-owned: every family was declared by the scaffold.

pub(crate) mod artifacts;
pub(crate) mod insights;
pub(crate) mod knowledge;
pub(crate) mod library;
pub(crate) mod proposals;
pub(crate) mod selection;
pub(crate) mod sources;
pub(crate) mod status;

#[cfg(test)]
mod tests;
