//! The selection tray database, `plugin_state/selections.sqlite`.
//!
//! This is short-lived UI state. It is not the Library index: nothing here
//! is rebuilt from Markdown, and the index schema never sees these rows.
//! Tray rules (expiry, a limit of five, private quotes) live in
//! `office_core::domain::selection`. This module only loads and stores them.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use office_common::Sensitivity;
use office_core::domain::selection::{PutRequest, PutResult, Selection, Tray, TrayRow, View};
use rusqlite::{Connection, OpenFlags, TransactionBehavior, params};

use crate::error::{Error, Result};

/// File name under the plugin state directory.
pub const DATABASE_FILE: &str = "selections.sqlite";

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS selections (
    seq INTEGER PRIMARY KEY,
    id TEXT NOT NULL UNIQUE,
    object TEXT NOT NULL,
    anchor TEXT,
    quote TEXT,
    quote_sha256 TEXT,
    view TEXT NOT NULL,
    sensitivity TEXT NOT NULL,
    selected_at INTEGER NOT NULL,
    consumed_at INTEGER
);";

/// One selections database. Each call opens a connection, takes an
/// immediate transaction, and applies the in-memory tray rules.
#[derive(Clone, Debug)]
pub struct SelectionStore {
    path: PathBuf,
}

impl SelectionStore {
    /// Open (creating if needed) `state_dir/selections.sqlite`.
    pub fn open(state_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(state_dir)
            .map_err(|source| Error::io("create selection state directory", state_dir, source))?;
        set_owner_only(state_dir, true);
        let path = state_dir.join(DATABASE_FILE);
        let store = Self { path };
        {
            let conn = store.connect()?;
            conn.execute_batch(SCHEMA)
                .map_err(|source| Error::selection_sqlite(&store.path, "create schema", source))?;
        }
        set_owner_only(&store.path, false);
        Ok(store)
    }

    /// The database file this store writes.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Apply a put at `now` and commit it.
    pub fn put(&self, request: &PutRequest, now: SystemTime) -> Result<PutResult> {
        self.transact(|tray| tray.put(request, now))
    }

    /// Return fresh selections without consuming them.
    pub fn peek(&self, now: SystemTime) -> Result<Vec<Selection>> {
        let conn = self.connect()?;
        Ok(load(&conn, &self.path)?.peek(now))
    }

    /// Return fresh selections and mark them consumed.
    pub fn get(&self, now: SystemTime) -> Result<Vec<Selection>> {
        self.transact(|tray| tray.get(now))
    }

    fn transact<T>(&self, body: impl FnOnce(&mut Tray) -> T) -> Result<T> {
        let mut conn = self.connect()?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|source| Error::selection_sqlite(&self.path, "begin", source))?;
        let mut tray = load(&tx, &self.path)?;
        let output = body(&mut tray);
        save(&tx, &tray, &self.path)?;
        tx.commit()
            .map_err(|source| Error::selection_sqlite(&self.path, "commit", source))?;
        Ok(output)
    }

    fn connect(&self) -> Result<Connection> {
        let conn = Connection::open_with_flags(
            &self.path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|source| Error::selection_sqlite(&self.path, "open", source))?;
        office_common::storage::sqlite::apply_default_pragmas(&conn).map_err(|source| {
            Error::selection_sqlite(&self.path, "apply default pragmas", source)
        })?;
        Ok(conn)
    }
}

struct Stored {
    seq: i64,
    id: String,
    object: String,
    anchor: Option<String>,
    quote: Option<String>,
    quote_sha256: Option<String>,
    view: String,
    sensitivity: String,
    selected_at: i64,
    consumed_at: Option<i64>,
}

fn load(conn: &Connection, path: &Path) -> Result<Tray> {
    let mut statement = conn
        .prepare(
            "SELECT seq, id, object, anchor, quote, quote_sha256, view, sensitivity,
                    selected_at, consumed_at
             FROM selections
             ORDER BY seq",
        )
        .map_err(|source| Error::selection_sqlite(path, "prepare load", source))?;
    let rows = statement
        .query_map([], |row| {
            Ok(Stored {
                seq: row.get(0)?,
                id: row.get(1)?,
                object: row.get(2)?,
                anchor: row.get(3)?,
                quote: row.get(4)?,
                quote_sha256: row.get(5)?,
                view: row.get(6)?,
                sensitivity: row.get(7)?,
                selected_at: row.get(8)?,
                consumed_at: row.get(9)?,
            })
        })
        .map_err(|source| Error::selection_sqlite(path, "load selections", source))?;
    let mut tray = Tray::default();
    for row in rows {
        let stored =
            row.map_err(|source| Error::selection_sqlite(path, "read selection", source))?;
        tray.rows.push(row_from_stored(path, stored)?);
    }
    Ok(tray)
}

fn row_from_stored(path: &Path, stored: Stored) -> Result<TrayRow> {
    let seq = u64::try_from(stored.seq).map_err(|_| corrupt(path, "stored seq"))?;
    let view = View::parse(&stored.view).ok_or_else(|| corrupt(path, "stored view"))?;
    let sensitivity = Sensitivity::parse(&stored.sensitivity)
        .ok_or_else(|| corrupt(path, "stored sensitivity"))?;
    Ok(TrayRow {
        seq,
        selection: Selection {
            id: stored.id,
            object: stored.object,
            anchor: stored.anchor,
            quote: stored.quote,
            quote_sha256: stored.quote_sha256,
            view,
            sensitivity,
            selected_at: from_unix(stored.selected_at)
                .ok_or_else(|| corrupt(path, "stored selected_at"))?,
        },
        consumed_at: stored
            .consumed_at
            .map(|secs| from_unix(secs).ok_or_else(|| corrupt(path, "stored consumed_at")))
            .transpose()?,
    })
}

fn save(conn: &Connection, tray: &Tray, path: &Path) -> Result<()> {
    conn.execute("DELETE FROM selections", [])
        .map_err(|source| Error::selection_sqlite(path, "clear selections", source))?;
    for row in &tray.rows {
        let seq = i64::try_from(row.seq).map_err(|_| corrupt(path, "encode seq"))?;
        let selected_at = to_unix(row.selection.selected_at)
            .ok_or_else(|| corrupt(path, "encode selected_at"))?;
        let consumed_at = row
            .consumed_at
            .map(|time| to_unix(time).ok_or_else(|| corrupt(path, "encode consumed_at")))
            .transpose()?;
        conn.execute(
            "INSERT INTO selections
                (seq, id, object, anchor, quote, quote_sha256, view, sensitivity,
                 selected_at, consumed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                seq,
                row.selection.id,
                row.selection.object,
                row.selection.anchor,
                row.selection.quote,
                row.selection.quote_sha256,
                row.selection.view.as_str(),
                row.selection.sensitivity.as_str(),
                selected_at,
                consumed_at,
            ],
        )
        .map_err(|source| Error::selection_sqlite(path, "insert selection", source))?;
    }
    Ok(())
}

fn corrupt(path: &Path, phase: &str) -> Error {
    Error::selection_sqlite(path, phase, rusqlite::Error::InvalidQuery)
}

fn to_unix(time: SystemTime) -> Option<i64> {
    let secs = time.duration_since(UNIX_EPOCH).ok()?.as_secs();
    i64::try_from(secs).ok()
}

fn from_unix(secs: i64) -> Option<SystemTime> {
    u64::try_from(secs)
        .ok()
        .map(|secs| UNIX_EPOCH + std::time::Duration::from_secs(secs))
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
