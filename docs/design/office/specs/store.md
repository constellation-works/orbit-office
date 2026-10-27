---
type: design
summary: "Spec: office-store — Library persistence structured after orbit-store: contracts, fs machinery, file and SQLite drivers, repository facade"
last_validated: 2026-10-27
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
| `contracts/` | `src/contracts.rs` | Compatible re-exports of core record shapes: `DocumentRecord`, `DocumentKind`, `SearchFilter`, `SearchHit`, `Revision`, `Backlink`, `LinkKind` |
| `fs/` (path_safety, lock, yaml) | `office-common::fs` (`path_safety`, `atomic`, `hash`) | Single-safe-component id validation, atomic owner-only writes (temp + fsync + rename + dir fsync), SHA-256 pinning/dedupe. Shared machinery lives in the foundation crate, as orbit's does in `orbit-common`. |
| `driver/file/` | `src/driver/file/library.rs` | The Library as plain Markdown: layout, frontmatter parsing, full scan for reindex |
| `driver/sqlite/` (connection, migration registry) | `src/driver/sqlite/` (`connection.rs`, `migration.rs`) | One writer connection behind a mutex; owner-only database file; the same pragmas on every connection (WAL, `busy_timeout` 5000, `foreign_keys=ON`, `synchronous=NORMAL`); ordered migrations with `user_version`, refusing a newer schema |
| `repository/` | `src/repository/library.rs` | `LibraryStore`: the one facade — open, `reindex`, `write_document` (write + index + commit under the writer lock), `read_document`, `read_text`, `get`, `backlinks`, `knowledge_pages`, `templates`, `search`, `head_commit`, `stage_attachment`/`remove_staged`, `file_sha256` |

What is deliberately not carried over: orbit-store's read pool, forward-
compatible read-only open, failure-class SQL functions, task reservations
and claims, and its per-feature migration files. They return only when a
tool needs them, as new migration steps — never as edits to step 1.

## Index schema (migrations 1 and 2)

- `documents(id, kind, title, summary, tags JSON, series, sensitivity, path UNIQUE, body)` with kind/series indexes.
- `cites(doc_id → documents, target)` and `links(from_id → documents, to_id)`, cascading on replace.
- `documents_fts` FTS5 over `(id UNINDEXED, title, summary, body, tags)`; search builds a quoted prefix MATCH per term — caller text is never interpolated unquoted.
- Migration 2 adds `documents.template` (a page's frontmatter `template`) and `documents.date` (`YYYY-MM-DD`: the first of `occurred_at`, `captured_at`, `updated_at` and `last_verified`, else the date in a dated `src-`/`art-` id), each indexed, for the `office.search` template and date filters. Rows indexed before it read `NULL` until the next reindex, which `office.init` runs on every upgrade.
- Search applies every filter (kind, tag, series, template, `since`/`until` on `date`, inclusive, excluding undated rows) before its limit. An empty query lists the filtered documents in id order instead of matching; `office.search` refuses one, and `office.attach` uses it to list a series.

## Invariants

- **Files are the truth.** The index is rebuildable at any time
  (`reindex` = scan + one-transaction rebuild); a write that succeeds is
  indexed and committed in the same call, so the index never lags a successful write.
- **Indexed paths are the rebuild set.** `reindex` scans only
  `sources/**/source.md`, `knowledge/**/*.md` and
  `artifacts/**/artifact.md`. `write_document` refuses every other
  Markdown path with `unsafe_path` before it writes, so `get` and
  `search` for a document that was written are unchanged by `reindex`.
  `read_document` and `history` use that same path rule. Source
  versions such as `v2.md`, artifact companions and `proposals/` are
  not documents of this call. `original.*` stays refused on every
  path, including under `knowledge/`.
- **Scans do not follow symlinks.** `reindex` and knowledge-page reads
  skip a symlink file or directory instead of opening it, including a
  symlinked layer directory (`sources`, `knowledge`, `artifacts`).
  Content reached only through a link is neither indexed nor returned,
  and a directory symlink that points at an ancestor ends the walk at
  the link. `read_document` still refuses a symlink leaf with
  `unsafe_path`.
- **Preconditions before side effects:** path components are validated
  before any write; unknown document kinds and newer index schemas are
  refused, never guessed or rewritten.
- **Secrets never enter the store.** Frontmatter conventions forbid them
  ([library.md](./library.md)); the store indexes what it is given.

## Library port and writes

`office_core::ports::Library` exposes current document reads, document writes,
lookup by id, back-links (documents whose `cites` or `related` name an id, from
the `cites` and `links` tables), the knowledge page and template files as
Markdown, filtered search, newest-first path history, a document's raw
Markdown (`read_text`), the repository's current commit (`head_commit`),
attachment staging, readiness, settings and single-key `set_setting` updates.
`stage_attachment` writes one `office.attach` copy under
`.office/attach/<call>/<library path>` and returns its absolute path for
`orbit.task.artifact.put`; `remove_staged` deletes only such a file and its
emptied directories. The staging tree is outside the indexed layout, and
document commits use `--only`, so a staged file is never indexed or committed
([decision](../4_decisions.md#office-attach-stages-its-copies-inside-the-library)).
`LibraryStore`
implements it; `office-testkit::fakes::FakeLibrary` implements the same operations
in memory. The store's existing record exports are re-exports of
`office_core::domain::knowledge`, so callers keep the same types. The shared
frontmatter parser is pure and is also used by the fake.

`open` creates layout/index state, but never runs `git init` or writes settings.
The readiness check verifies the Library's own `.git` directory and valid
`.office/config.yaml`; a missing repository or config requires `office.init`.
Document reads/search/history/settings refuse uninitialized Libraries. Reindex
remains available as an adapter operation; its tool is a separate deliverable.

A document write holds `plugin_state/library.writer.lock` from before recovery
and validation through file replacement, SQLite upsert and git commit. Reindex
uses the same lock. The standard-library advisory file lock is acquired with
`File::try_lock` (the bounded form of `File::lock`), with a five-second deadline;
`library.writer.owner.json` records the PID, acquisition time and label for timeout
diagnostics. Closing the file releases the lock. SQLite's mutex/transaction is
short-lived and is never held across git. Reads never wait for the writer lock
or acquire it exclusively. When a journal exists, they open the existing lock
file read-only and try a shared lock once, without creating lock/holder state.
An exclusive holder means a live write/recovery is in flight, so reads proceed.
An idle shared probe rechecks the journal while held: if it has disappeared,
the write completed; if it remains (or no lock file exists), recovery is required.
Production calls to one Library must use the same plugin state directory.
`open_in_memory` uses `.office/state` for writer state and a process-local index;
it is for tests/scans, not concurrent production indexing.

A versioned, atomically written `library.pending-write.json` is the durable
commit decision for the operation. New decisions use version 2 with a `kind`
of `document` or `settings`; version-1 document journals without a kind remain
readable. Unknown kinds and newer versions are refused. Settings decisions
accept only `.office/config.yaml`, validate the decided settings, replace the
config and commit it without indexing it as a document. Recovery can finish a
settings replacement even when the old config is missing or invalid, provided
the Library's own git repository remains present. Document decisions retain
the indexed-path and document-identity checks. The text and write token are replayed
idempotently under the writer lock before an interrupted store opens. A failure
after that decision leaves the journal intact. With no active writer, read
operations refuse with `library_write_pending` until recovery completes;
they never replay or clear a journal themselves. Invalid/newer journals are
preserved and refused by recovery. A token in the commit message prevents a replay
after commit from duplicating the commit. The journal is private (0600), and completion
fsyncs its removal. During a live write, reads return the available file, indexed
record/search hits or committed history, which may reflect the old or new
document. These calls do not promise a snapshot across files, index and git.

`set_setting` holds the writer lock from before reading the latest config
through the typed `Settings::with_setting` update, config replacement and commit,
so updates to different keys do not overwrite one another. Unknown setting keys
and invalid values are refused before the write decision. Settings use the same
symlink refusal and atomic owner-only writer as documents.

Each write stages only its document or config path and commits with `--only --allow-empty`,
using the per-commit author and committer `orbit-office <office@orbit.local>`.
Unrelated staged files remain staged and do not enter that commit. `original.*`
paths are refused, including `original.md`; originals are never staged by this
adapter even when `.gitignore` is missing. The initialization tool still owns
writing the Library's `.gitignore`. Repeated identical writes create empty write
event commits; path history shows the commits that actually changed the file.
Changing a document's id at the same path replaces its old index row; using an
id already indexed at a different path is refused before the write decision.

Git commands and test git fixtures share `office_common::process::run`. They use argument vectors, an empty stdin, a clean environment, disabled
hooks/fsmonitor/signing/auto-maintenance, a 30-second deadline and concurrent
stdout/stderr drains capped at 512 KiB per stream (1 MiB total). Linux/macOS supervision uses an owned
process group and `waitid(WNOWAIT)` to observe exit without releasing the PID;
it sweeps the group before reaping. Timeout/error cleanup sends TERM, waits up to
200 ms, then KILLs and reaps. Pipe completion waits are bounded. Git diagnostics
are not embedded in errors because they can contain document text or credentials.
The small POSIX FFI boundary has documented Linux/macOS ABI assumptions and
safety comments; there is no new external dependency. CI validates both supported
platforms; this task's local execution can validate only its host platform.
