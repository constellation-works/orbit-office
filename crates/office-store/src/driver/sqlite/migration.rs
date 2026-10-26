//! Ordered schema migrations, referenced by the `MIGRATIONS` registry.
//! A step runs once, in a transaction that also bumps `user_version`; a
//! database newer than this build is refused, never rewritten
//! ([`Error::NewerSchema`](crate::Error)).

use rusqlite::Connection;
use std::path::Path;

use crate::error::{Error, Result};

/// The newest schema this build understands.
pub(crate) const SUPPORTED_SCHEMA: u32 = 1;

/// One migration step: SQL applied in order.
struct Migration {
    /// Schema version this step produces.
    to: u32,
    /// The step's SQL.
    sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[Migration {
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
}];

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
    Ok(())
}
