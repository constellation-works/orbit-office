---
type: design
summary: "Spec: office-store — Library persistence structured after orbit-store: contracts, fs machinery, file and SQLite drivers, repository facade"
last_validated: 2026-10-26
---

# Spec: office-store

`office-store` is the persistence crate for the Library
([library.md](./library.md)). It is structured after `orbit-store` —
Orbit's persistence crate — re-created at this project's scale: Orbit is
a labelled dependency and none of its code is vendored, so the structure
and machinery are reused, the code is this project's.

## Shape (mirrors orbit-store)

| orbit-store | office-store | Owns |
|---|---|---|
| `contracts/` | `src/contracts.rs` | I/O-free record shapes: `DocumentRecord`, `DocumentKind`, `SearchFilter`, `SearchHit` |
| `fs/` (path_safety, lock, yaml) | `office-common::fs` (`path_safety`, `atomic`, `hash`) | Single-safe-component id validation, atomic owner-only writes (temp + fsync + rename + dir fsync), SHA-256 pinning/dedupe. Shared machinery lives in the foundation crate, as orbit's does in `orbit-common`. |
| `driver/file/` | `src/driver/file/library.rs` | The Library as plain Markdown: layout, frontmatter parsing, full scan for reindex |
| `driver/sqlite/` (connection, migration registry) | `src/driver/sqlite/` (`connection.rs`, `migration.rs`) | One writer connection behind a mutex; owner-only database file; the same pragmas on every connection (WAL, `busy_timeout` 5000, `foreign_keys=ON`, `synchronous=NORMAL`); ordered migrations with `user_version`, refusing a newer schema |
| `repository/` | `src/repository/library.rs` | `LibraryStore`: the one facade — open, `reindex`, `write_document` (write + index in one call), `read_document`, `get`, `search`, `file_sha256` |

What is deliberately not carried over: orbit-store's read pool, forward-
compatible read-only open, failure-class SQL functions, task reservations
and claims, and its per-feature migration files. They return only when a
tool needs them, as new migration steps — never as edits to step 1.

## Index schema (migration 1)

- `documents(id, kind, title, summary, tags JSON, series, sensitivity, path UNIQUE, body)` with kind/series indexes.
- `cites(doc_id → documents, target)` and `links(from_id → documents, to_id)`, cascading on replace.
- `documents_fts` FTS5 over `(id UNINDEXED, title, summary, body, tags)`; search builds a quoted prefix MATCH per term — caller text is never interpolated unquoted.

## Invariants

- **Files are the truth.** The index is rebuildable at any time
  (`reindex` = scan + one-transaction rebuild); a write that succeeds is
  indexed in the same call, so the index never lags a successful write.
- **Preconditions before side effects:** path components are validated
  before any write; unknown document kinds and newer index schemas are
  refused, never guessed or rewritten.
- **Secrets never enter the store.** Frontmatter conventions forbid them
  ([library.md](./library.md)); the store indexes what it is given.
