//! Private persistence drivers. A driver implements one storage
//! technology and never coordinates another driver; coordination lives in
//! `crate::repository`. (After orbit-store's driver boundary.)

pub(crate) mod file;
pub(crate) mod sqlite;

pub(crate) mod git;
