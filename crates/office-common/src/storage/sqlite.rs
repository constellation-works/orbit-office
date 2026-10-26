//! Shared SQLite connection defaults for every office SQLite store.
//!
//! Historically each store would hand-roll its own pragma setup and the
//! copies drift (missing `foreign_keys` here, missing `busy_timeout`
//! there). [`apply_default_pragmas`] is the single source of truth: call
//! it on every freshly opened connection, then layer any store-specific
//! overrides on top. (After orbit-common's `storage::sqlite`.)

use rusqlite::Connection;

/// Default `busy_timeout` applied to every office SQLite connection, in
/// milliseconds. Writers under WAL still serialize; this bounds how long a
/// contending connection spins before surfacing `database is locked`.
pub const DEFAULT_BUSY_TIMEOUT_MS: u32 = 5_000;

/// Apply the default pragmas to a freshly opened connection:
///
/// - `busy_timeout` = [`DEFAULT_BUSY_TIMEOUT_MS`];
/// - `journal_mode=WAL` (in-memory databases keep their `memory` mode);
/// - `foreign_keys=ON`;
/// - `synchronous=NORMAL` — the recommended WAL durability level.
pub fn apply_default_pragmas(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.busy_timeout(std::time::Duration::from_millis(u64::from(
        DEFAULT_BUSY_TIMEOUT_MS,
    )))?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA foreign_keys = ON;
         PRAGMA synchronous = NORMAL;",
    )
}
