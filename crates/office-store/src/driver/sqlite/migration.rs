//! Ordered schema migrations, referenced by the `MIGRATIONS` registry.
//! A step runs once, in a transaction that also bumps `user_version`; a
//! database newer than this build is refused, never rewritten
//! ([`Error::NewerSchema`](crate::Error)).

use rusqlite::Connection;
use std::path::Path;

use crate::error::{Error, Result};

/// The newest schema this build understands.
pub(crate) const SUPPORTED_SCHEMA: u32 = 3;

/// One migration step: SQL applied in order.
struct Migration {
    /// Schema version this step produces.
    to: u32,
    /// The step's SQL.
    sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        to: 1,
        sql: "
CREATE TABLE documents (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    title TEXT NOT NULL,
    summary TEXT NOT NULL,
    tags TEXT NOT NULL,
    series TEXT,
    sensitivity TEXT NOT NULL,
    path TEXT NOT NULL UNIQUE,
    body TEXT NOT NULL
);
CREATE INDEX documents_kind ON documents(kind);
CREATE INDEX documents_series ON documents(series);
CREATE TABLE cites (
    doc_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    target TEXT NOT NULL,
    PRIMARY KEY (doc_id, target)
);
CREATE TABLE links (
    from_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    to_id TEXT NOT NULL,
    PRIMARY KEY (from_id, to_id)
);
CREATE VIRTUAL TABLE documents_fts USING fts5(id UNINDEXED, title, summary, body, tags);
",
    },
    // The template and date search filters. Rows indexed before this step
    // read NULL until they are rescanned from their files, so migrating
    // from schema 1 records a durable backfill marker (below) and
    // `LibraryStore::open` reindexes under the writer lock before it
    // returns. The marker survives a failed or interrupted backfill:
    // `user_version` is already 2 by then, so the version alone could
    // never retrigger it.
    Migration {
        to: 2,
        sql: "
ALTER TABLE documents ADD COLUMN template TEXT;
ALTER TABLE documents ADD COLUMN date TEXT;
CREATE INDEX documents_template ON documents(template);
CREATE INDEX documents_date ON documents(date);
",
    },
    Migration {
        to: 3,
        sql: "ALTER TABLE documents ADD COLUMN shareable INTEGER NOT NULL DEFAULT 0 CHECK (shareable IN (0, 1));
CREATE TABLE IF NOT EXISTS index_backfill_pending (id INTEGER PRIMARY KEY CHECK (id = 1));
INSERT OR IGNORE INTO index_backfill_pending (id) SELECT 1 WHERE EXISTS (SELECT 1 FROM documents);",
    },
];

/// Bring `conn` up to [`SUPPORTED_SCHEMA`], refusing a newer database.
pub(crate) fn migrate(conn: &mut Connection, path: &Path) -> Result<()> {
    let found: u32 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|e| Error::sqlite(path, "read schema version", e))?;
    if found > SUPPORTED_SCHEMA {
        return Err(Error::NewerSchema {
            path: path.to_path_buf(),
            found,
            supported: SUPPORTED_SCHEMA,
        });
    }
    for migration in MIGRATIONS.iter().filter(|m| m.to > found) {
        let tx = conn
            .transaction()
            .map_err(|e| Error::sqlite(path, "begin migration", e))?;
        tx.execute_batch(migration.sql)
            .map_err(|e| Error::sqlite(path, "apply migration", e))?;
        tx.execute_batch(&format!("PRAGMA user_version = {}", migration.to))
            .map_err(|e| Error::sqlite(path, "record schema version", e))?;
        tx.commit()
            .map_err(|e| Error::sqlite(path, "commit migration", e))?;
    }
    record_backfill_marker(conn, path, found)
}

/// Record the durable migration-2 backfill marker when it is owed.
///
/// The marker table is created on every open so a database already at
/// schema 2 (migrated by a build that never backfilled) can be marked
/// the first time this build sees it: any row with a `NULL` template or
/// date may be a stale pre-upgrade row, and one rescan heals it. A fresh
/// database (schema 0) and a schema-2 database this build has already
/// marked or healed owe nothing. `rebuild` clears the marker in its
/// rebuild transaction, so a failed backfill is retried on the next open.
fn record_backfill_marker(conn: &mut Connection, path: &Path, found: u32) -> Result<()> {
    let marker_existed: bool = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master
             WHERE type = 'table' AND name = 'index_backfill_pending'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|e| Error::sqlite(path, "inspect backfill marker", e))?
        > 0;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS index_backfill_pending (
             id INTEGER PRIMARY KEY CHECK (id = 1)
         );",
    )
    .map_err(|e| Error::sqlite(path, "create backfill marker", e))?;
    let owed = if found == 1 || found == 2 {
        true
    } else if found == SUPPORTED_SCHEMA && !marker_existed {
        conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM documents
                            WHERE template IS NULL OR date IS NULL)",
            [],
            |row| row.get::<_, bool>(0),
        )
        .map_err(|e| Error::sqlite(path, "inspect stale index rows", e))?
    } else {
        false
    };
    if owed {
        conn.execute(
            "INSERT OR IGNORE INTO index_backfill_pending (id) VALUES (1)",
            [],
        )
        .map_err(|e| Error::sqlite(path, "record backfill marker", e))?;
    }
    Ok(())
}
