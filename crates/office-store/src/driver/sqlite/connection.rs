//! The index store handle and its queries.
//!
//! Open discipline follows orbit-store's SQLite machinery, re-created at
//! this crate's scale: the database file and its parent are owner-only,
//! every connection gets the same pragmas (WAL, `busy_timeout`,
//! `foreign_keys`, `synchronous=NORMAL`), and the schema migrates through
//! the ordered registry in `migration.rs` before any query runs.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use rusqlite::{Connection, OpenFlags, OptionalExtension, params};

use crate::contracts::{DocumentKind, DocumentRecord, SearchFilter, SearchHit};
use crate::driver::sqlite::migration;
use crate::error::{Error, Result};

/// SQLite index handle: one writer connection behind a mutex.
#[derive(Clone)]
pub(crate) struct IndexStore {
    conn: Arc<Mutex<Connection>>,
    path: PathBuf,
}

impl IndexStore {
    /// Open (creating if needed) the index at `path`, migrating it first.
    pub(crate) fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| Error::io("create index directory", parent, e))?;
            set_owner_only(parent, true);
        }
        // No SQLITE_OPEN_NOFOLLOW: rusqlite 0.41's bundled build refuses
        // to open with it on macOS (verified by probe). Symlink refusal for
        // caller-supplied paths lives in `fs::path_safety` instead.
        let mut conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|e| Error::sqlite(path, "open index database", e))?;
        apply_default_pragmas(&conn, path)?;
        set_owner_only(path, false);
        migration::migrate(&mut conn, path)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            path: path.to_path_buf(),
        })
    }

    /// An in-memory index (tests; a second connection would see a
    /// different empty database, so reads use the same connection).
    pub(crate) fn open_in_memory() -> Result<Self> {
        let path = PathBuf::from("<in-memory>");
        let mut conn = Connection::open_in_memory()
            .map_err(|e| Error::sqlite(&path, "open in-memory index", e))?;
        apply_default_pragmas(&conn, &path)?;
        migration::migrate(&mut conn, &path)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            path,
        })
    }

    /// The database file this handle writes.
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// Replace the whole index with `documents` in one transaction. This
    /// is what `reindex` does: the files are the truth, the index is
    /// rebuilt from them.
    pub(crate) fn rebuild(&self, documents: &[DocumentRecord]) -> Result<()> {
        let mut conn = self.lock()?;
        let tx = conn
            .transaction()
            .map_err(|e| Error::sqlite(&self.path, "begin rebuild", e))?;
        tx.execute_batch(
            "DELETE FROM cites; DELETE FROM links; DELETE FROM documents_fts; DELETE FROM documents;",
        )
        .map_err(|e| Error::sqlite(&self.path, "clear index", e))?;
        for document in documents {
            insert_document(&tx, document, &self.path)?;
        }
        tx.commit()
            .map_err(|e| Error::sqlite(&self.path, "commit rebuild", e))
    }

    /// Insert or replace one document and its index rows.
    pub(crate) fn upsert(&self, document: &DocumentRecord) -> Result<()> {
        let mut conn = self.lock()?;
        let tx = conn
            .transaction()
            .map_err(|e| Error::sqlite(&self.path, "begin upsert", e))?;
        let previous: Option<String> = tx
            .query_row(
                "SELECT id FROM documents WHERE path = ?1",
                params![document.path],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| Error::sqlite(&self.path, "find previous document at path", e))?;
        if let Some(id) = previous {
            delete_document(&tx, &id, &self.path)?;
        }
        delete_document(&tx, &document.id, &self.path)?;
        insert_document(&tx, document, &self.path)?;
        tx.commit()
            .map_err(|e| Error::sqlite(&self.path, "commit upsert", e))
    }

    /// One document by id, if indexed.
    pub(crate) fn get(&self, id: &str) -> Result<Option<DocumentRecord>> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, kind, title, summary, tags, series, sensitivity, path, body
                 FROM documents WHERE id = ?1",
            )
            .map_err(|e| Error::sqlite(&self.path, "prepare get", e))?;
        let mut record = {
            let mut rows = stmt
                .query(params![id])
                .map_err(|e| Error::sqlite(&self.path, "query get", e))?;
            match rows
                .next()
                .map_err(|e| Error::sqlite(&self.path, "read get row", e))?
            {
                Some(row) => record_from_row(row, &self.path)?,
                None => return Ok(None),
            }
        };
        record.cites = query_column(
            &conn,
            "SELECT target FROM cites WHERE doc_id = ?1 ORDER BY target",
            id,
            &self.path,
        )?;
        record.related = query_column(
            &conn,
            "SELECT to_id FROM links WHERE from_id = ?1 ORDER BY to_id",
            id,
            &self.path,
        )?;
        Ok(Some(record))
    }

    /// Number of indexed documents.
    pub(crate) fn count(&self) -> Result<u32> {
        let conn = self.lock()?;
        let count: u32 = conn
            .query_row("SELECT count(*) FROM documents", [], |row| row.get(0))
            .map_err(|e| Error::sqlite(&self.path, "count documents", e))?;
        Ok(count)
    }

    /// Indexed document counts grouped by kind token.
    pub(crate) fn counts_by_kind(&self) -> Result<Vec<(String, u32)>> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare("SELECT kind, count(*) FROM documents GROUP BY kind")
            .map_err(|e| Error::sqlite(&self.path, "prepare kind counts", e))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, u32>(1)?))
            })
            .map_err(|e| Error::sqlite(&self.path, "run kind counts", e))?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| Error::sqlite(&self.path, "read kind counts", e))?;
        Ok(rows)
    }

    /// Full-text search over title, summary, body and tags. Filters apply
    /// before the limit.
    pub(crate) fn search(&self, query: &str, filter: &SearchFilter) -> Result<Vec<SearchHit>> {
        let conn = self.lock()?;
        let limit = i64::try_from(filter.limit.unwrap_or(20).min(100)).unwrap_or(20);
        let mut stmt = conn
            .prepare(
                "SELECT d.id, d.kind, d.title, d.path,
                        snippet(documents_fts, 3, '<b>', '</b>', '…', 12)
                 FROM documents_fts
                 JOIN documents d ON d.id = documents_fts.id
                 WHERE documents_fts MATCH ?1
                   AND (?2 IS NULL OR d.kind = ?2)
                   AND (?3 IS NULL OR EXISTS (
                       SELECT 1 FROM json_each(d.tags) WHERE json_each.value = ?3))
                   AND (?4 IS NULL OR d.series = ?4)
                 ORDER BY rank
                 LIMIT ?5",
            )
            .map_err(|e| Error::sqlite(&self.path, "prepare search", e))?;
        let hits = stmt
            .query_map(
                params![
                    fts_match_expr(query),
                    filter.kind.map(|kind| kind.as_str()),
                    filter.tag,
                    filter.series,
                    limit,
                ],
                |row| {
                    Ok(SearchHit {
                        id: row.get(0)?,
                        kind: DocumentKind::parse(&row.get::<_, String>(1)?)
                            .unwrap_or(DocumentKind::Source),
                        title: row.get(2)?,
                        path: row.get(3)?,
                        snippet: row.get(4)?,
                    })
                },
            )
            .map_err(|e| Error::sqlite(&self.path, "run search", e))?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| Error::sqlite(&self.path, "read search rows", e))?;
        Ok(hits)
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>> {
        self.conn.lock().map_err(|_| Error::Sqlite {
            path: self.path.clone(),
            detail: "index connection lock poisoned".to_string(),
            source: rusqlite::Error::InvalidQuery,
        })
    }
}

/// One text column of rows for a single-id query, in row order.
fn query_column(conn: &Connection, sql: &str, id: &str, path: &Path) -> Result<Vec<String>> {
    let mut stmt = conn
        .prepare(sql)
        .map_err(|e| Error::sqlite(path, "prepare column query", e))?;
    let values = stmt
        .query_map(params![id], |row| row.get::<_, String>(0))
        .map_err(|e| Error::sqlite(path, "run column query", e))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| Error::sqlite(path, "read column rows", e))?;
    Ok(values)
}

/// The pragma set every index connection gets comes from
/// `office-common`, the one source of truth for SQLite defaults.
fn apply_default_pragmas(conn: &Connection, path: &Path) -> Result<()> {
    office_common::storage::sqlite::apply_default_pragmas(conn)
        .map_err(|e| Error::sqlite(path, "apply default pragmas", e))
}

/// Owner-only permissions (`0700` directories, `0600` files), best effort
/// on non-Unix hosts.
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

fn insert_document(conn: &Connection, document: &DocumentRecord, path: &Path) -> Result<()> {
    let tags = serde_json::to_string(&document.tags).map_err(|e| {
        Error::sqlite(
            path,
            "encode tags",
            rusqlite::Error::ToSqlConversionFailure(Box::new(e)),
        )
    })?;
    conn.execute(
        "INSERT INTO documents (id, kind, title, summary, tags, series, sensitivity, path, body)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            document.id,
            document.kind.as_str(),
            document.title,
            document.summary,
            tags,
            document.series,
            document.sensitivity,
            document.path,
            document.body,
        ],
    )
    .map_err(|e| Error::sqlite(path, "insert document", e))?;
    conn.execute(
        "INSERT INTO documents_fts (id, title, summary, body, tags) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            document.id,
            document.title,
            document.summary,
            document.body,
            document.tags.join(" "),
        ],
    )
    .map_err(|e| Error::sqlite(path, "insert search row", e))?;
    for target in &document.cites {
        conn.execute(
            "INSERT OR IGNORE INTO cites (doc_id, target) VALUES (?1, ?2)",
            params![document.id, target],
        )
        .map_err(|e| Error::sqlite(path, "insert citation", e))?;
    }
    for target in &document.related {
        conn.execute(
            "INSERT OR IGNORE INTO links (from_id, to_id) VALUES (?1, ?2)",
            params![document.id, target],
        )
        .map_err(|e| Error::sqlite(path, "insert link", e))?;
    }
    Ok(())
}

fn delete_document(conn: &Connection, id: &str, path: &Path) -> Result<()> {
    conn.execute("DELETE FROM documents WHERE id = ?1", params![id])
        .map_err(|e| Error::sqlite(path, "delete document", e))?;
    conn.execute("DELETE FROM documents_fts WHERE id = ?1", params![id])
        .map_err(|e| Error::sqlite(path, "delete search row", e))?;
    // cites/links rows cascade from the documents delete (foreign_keys=ON).
    Ok(())
}

fn record_from_row(row: &rusqlite::Row<'_>, path: &Path) -> Result<DocumentRecord> {
    let tags_text: String = row
        .get(4)
        .map_err(|e| Error::sqlite(path, "read tags column", e))?;
    let tags: Vec<String> = serde_json::from_str(&tags_text).map_err(|e| {
        Error::sqlite(
            path,
            "decode tags column",
            rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(e)),
        )
    })?;
    Ok(DocumentRecord {
        id: row.get(0).map_err(|e| Error::sqlite(path, "read id", e))?,
        kind: DocumentKind::parse(
            &row.get::<_, String>(1)
                .map_err(|e| Error::sqlite(path, "read kind", e))?,
        )
        .unwrap_or(DocumentKind::Source),
        title: row
            .get(2)
            .map_err(|e| Error::sqlite(path, "read title", e))?,
        summary: row
            .get(3)
            .map_err(|e| Error::sqlite(path, "read summary", e))?,
        tags,
        series: row
            .get(5)
            .map_err(|e| Error::sqlite(path, "read series", e))?,
        sensitivity: row
            .get(6)
            .map_err(|e| Error::sqlite(path, "read sensitivity", e))?,
        path: row
            .get(7)
            .map_err(|e| Error::sqlite(path, "read path", e))?,
        body: row
            .get(8)
            .map_err(|e| Error::sqlite(path, "read body", e))?,
        cites: Vec::new(),
        related: Vec::new(),
    })
}

/// Build an FTS5 MATCH expression: each whitespace term becomes a quoted
/// prefix term, ANDed. Caller text is never interpolated unquoted.
fn fts_match_expr(query: &str) -> String {
    let terms: Vec<String> = query
        .split_whitespace()
        .map(|term| format!("\"{}\"*", term.replace('"', "\"\"")))
        .collect();
    if terms.is_empty() {
        return "\"\"".to_string();
    }
    terms.join(" AND ")
}
