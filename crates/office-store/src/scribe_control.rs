//! Per-session scribe control records, `plugin_state/scribe_control.sqlite`.
//!
//! Short-lived plugin state shared by the Scribe window and the provider
//! plugin's hooks (specs/sessions.md, "Live scribe"). It is not Library
//! evidence and not the index: nothing here is captured, rebuilt or synced.
//! The record rules live in `office_core::domain::scribe_control`; this
//! module only loads and stores them, one immediate transaction per change.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use office_core::domain::scribe_control::{Change, Control, RETENTION, from_unix, to_unix};
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};

use crate::error::{Error, Result};

/// File name under the plugin state directory.
pub const DATABASE_FILE: &str = "scribe_control.sqlite";

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS scribe_control (
    session_id TEXT PRIMARY KEY,
    switch_on INTEGER,
    now_requested_at INTEGER,
    title TEXT,
    heartbeat_at INTEGER,
    updated_at INTEGER NOT NULL
);";

/// One scribe control database. Each call opens its own connection.
#[derive(Clone, Debug)]
pub struct ScribeControlStore {
    path: PathBuf,
}

impl ScribeControlStore {
    /// Open (creating if needed) `state_dir/scribe_control.sqlite`.
    pub fn open(state_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(state_dir).map_err(|source| {
            Error::io("create scribe control state directory", state_dir, source)
        })?;
        set_owner_only(state_dir, true);
        let store = Self {
            path: state_dir.join(DATABASE_FILE),
        };
        store
            .connect()?
            .execute_batch(SCHEMA)
            .map_err(|source| Error::scribe_control_sqlite(&store.path, "create schema", source))?;
        set_owner_only(&store.path, false);
        Ok(store)
    }

    /// The database file this store writes.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Read one session's record; an unknown session reads as the default.
    pub fn get(&self, session_id: &str) -> Result<Control> {
        Ok(load(&self.connect()?, &self.path, session_id)?.unwrap_or_default())
    }

    /// Apply one change at `now` and commit it. Records untouched for the
    /// retention period are dropped in the same transaction.
    pub fn apply(
        &self,
        session_id: &str,
        change: &Change,
        now: SystemTime,
    ) -> Result<(Control, bool)> {
        let mut conn = self.connect()?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|source| Error::scribe_control_sqlite(&self.path, "begin", source))?;
        let mut control = load(&tx, &self.path, session_id)?.unwrap_or_default();
        let consumed = control.apply(change, now)?;
        let updated_at = encode(&self.path, control.updated_at, "updated_at")?
            .ok_or_else(|| corrupt(&self.path, "encode updated_at"))?;
        tx.execute(
            "INSERT INTO scribe_control
                (session_id, switch_on, now_requested_at, title, heartbeat_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(session_id) DO UPDATE SET
                switch_on = excluded.switch_on,
                now_requested_at = excluded.now_requested_at,
                title = excluded.title,
                heartbeat_at = excluded.heartbeat_at,
                updated_at = excluded.updated_at",
            params![
                session_id,
                control.on.map(i64::from),
                encode(&self.path, control.now_requested_at, "now_requested_at")?,
                control.title,
                encode(&self.path, control.heartbeat_at, "heartbeat_at")?,
                updated_at,
            ],
        )
        .map_err(|source| Error::scribe_control_sqlite(&self.path, "write record", source))?;
        if let Some(cutoff) = now.checked_sub(RETENTION).and_then(to_unix) {
            tx.execute(
                "DELETE FROM scribe_control WHERE updated_at <= ?1",
                params![cutoff],
            )
            .map_err(|source| {
                Error::scribe_control_sqlite(&self.path, "drop expired records", source)
            })?;
        }
        tx.commit()
            .map_err(|source| Error::scribe_control_sqlite(&self.path, "commit", source))?;
        Ok((control, consumed))
    }

    fn connect(&self) -> Result<Connection> {
        let conn = Connection::open_with_flags(
            &self.path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|source| Error::scribe_control_sqlite(&self.path, "open", source))?;
        office_common::storage::sqlite::apply_default_pragmas(&conn).map_err(|source| {
            Error::scribe_control_sqlite(&self.path, "apply default pragmas", source)
        })?;
        Ok(conn)
    }
}

type Row = (Option<i64>, Option<i64>, Option<String>, Option<i64>, i64);

fn load(conn: &Connection, path: &Path, session_id: &str) -> Result<Option<Control>> {
    let row: Option<Row> = conn
        .query_row(
            "SELECT switch_on, now_requested_at, title, heartbeat_at, updated_at
             FROM scribe_control WHERE session_id = ?1",
            params![session_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .optional()
        .map_err(|source| Error::scribe_control_sqlite(path, "load record", source))?;
    let Some((on, requested, title, heartbeat, updated)) = row else {
        return Ok(None);
    };
    let on = match on {
        None => None,
        Some(0) => Some(false),
        Some(1) => Some(true),
        Some(_) => return Err(corrupt(path, "stored switch")),
    };
    let decode = |secs: Option<i64>, phase: &str| {
        secs.map(|secs| from_unix(secs).ok_or_else(|| corrupt(path, phase)))
            .transpose()
    };
    Ok(Some(Control {
        on,
        now_requested_at: decode(requested, "stored now_requested_at")?,
        title,
        heartbeat_at: decode(heartbeat, "stored heartbeat_at")?,
        updated_at: decode(Some(updated), "stored updated_at")?,
    }))
}

fn encode(path: &Path, time: Option<SystemTime>, field: &str) -> Result<Option<i64>> {
    time.map(|time| to_unix(time).ok_or_else(|| corrupt(path, &format!("encode {field}"))))
        .transpose()
}

fn corrupt(path: &Path, phase: &str) -> Error {
    Error::scribe_control_sqlite(path, phase, rusqlite::Error::InvalidQuery)
}

fn set_owner_only(path: &Path, is_dir: bool) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = if is_dir { 0o700 } else { 0o600 };
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
    }
    #[cfg(not(unix))]
    let _ = (path, is_dir);
}
