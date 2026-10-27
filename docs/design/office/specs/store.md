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
| `fs/` (path_safety, lock, yaml) | `office-common::fs` (`path_safety`, `atomic`, `hash`) | Single-safe-component id validation, atomic owner-only writes (temp + fsync + publication + dir fsync; rename for replacement, hard link for exclusive creation), SHA-256 pinning/dedupe. Shared machinery lives in the foundation crate, as orbit's does in `orbit-common`. |
| `driver/file/` | `src/driver/file/library.rs` | The Library as plain Markdown: layout, frontmatter parsing, full scan for reindex |
| `driver/sqlite/` (connection, migration registry) | `src/driver/sqlite/` (`connection.rs`, `migration.rs`) | One writer connection behind a mutex; owner-only database file; the same pragmas on every connection (WAL, `busy_timeout` 5000, `foreign_keys=ON`, `synchronous=NORMAL`); ordered migrations with `user_version`, refusing a newer schema |
| `repository/` | `src/repository/library.rs` | `LibraryStore`: the one facade — open, `reindex`, `write_document` (write + index + commit under the writer lock), `read_document`, `read_text`, `get`, `backlinks`, `knowledge_pages`, `source_documents`, `templates`, `search`, `head_commit`, `stage_attachment`/`remove_staged`, `file_sha256` |

What is deliberately not carried over: orbit-store's read pool, forward-
compatible read-only open, failure-class SQL functions, task reservations
and claims, and its per-feature migration files. They return only when a
tool needs them, as new migration steps — never as edits to step 1.

## Index schema (migrations 1 and 2)

- `documents(id, kind, title, summary, tags JSON, series, sensitivity, path UNIQUE, body)` with kind/series indexes.
- `cites(doc_id → documents, target)` and `links(from_id → documents, to_id)`, cascading on replace.
- `documents_fts` FTS5 over `(id UNINDEXED, title, summary, body, tags)`; search builds a quoted prefix MATCH per term — caller text is never interpolated unquoted.
- Migration 2 adds `documents.template` (a page's frontmatter `template`) and `documents.date` (`YYYY-MM-DD`: the first of `occurred_at`, `captured_at`, `updated_at` and `last_verified`, else the date in a dated `src-`/`art-` id), each indexed, for the `office.search` template and date filters. Rows indexed before it read `NULL` in those columns, so migration records a durable backfill marker in the index and `LibraryStore::open` reindexes under the writer lock before it returns; the marker is cleared by that rebuild and survives a failed one, so the next open retries. A database already migrated to schema 2 without a backfill is marked on the first open by this build when any row still has a `NULL` template or date, and healed the same way.
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
- **Scans do not follow symlinks.** `reindex`, knowledge-page reads and source-document reads
  skip a symlink file or directory instead of opening it, including a
  symlinked layer directory (`sources`, `knowledge`, `artifacts`).
  Content reached only through a link is neither indexed nor returned,
  and a directory symlink that points at an ancestor ends the walk at
  the link. `read_document` still refuses a symlink leaf with
  `unsafe_path`.
- **An incomplete scan does not rebuild the index.** A missing indexed
  layer is an empty part of a complete scan. Any other failure while
  inspecting or listing a layer directory, a nested directory, or a
  directory entry — including a regular file where a layer directory
  belongs — returns `io_error` before the index transaction. Previously
  indexed rows stay searchable. Knowledge-page reads use the same walk:
  a missing or symlinked `knowledge` directory is an empty page list,
  and any other enumeration error fails the read. `source_documents` reads
  every `sources/**/source.md` with complete frontmatter and body, in path
  order with no search cap. It uses the same missing-layer and symlink rules;
  originals, `v2.md` sidecars and other Library layers are excluded. An
  enumeration or file-read failure refuses the entire document listing.
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
commit decision for the operation. Document and settings decisions use version 2
with a `kind` of `document` or `settings`. Upload captures use version 3 with
`kind: document` and a required `original` object containing the Library-relative
`relative` path and the exact `bytes` as a JSON array of byte values. The original
is decided together with the source Markdown before either file is published;
failure to record the decision leaves no original or capture directory behind.
Version-1 document journals (with or without `kind: document`) and version-2
document/settings journals remain readable. Original payloads in older versions,
version-3 decisions without an original, malformed version-4 artifact bundles,
unknown kinds and newer versions are
refused and preserved. Settings decisions
accept only `.office/config.yaml`, validate the decided settings, replace the
config and commit it without indexing it as a document. Recovery can finish a
settings replacement even when the old config is missing or invalid, provided
the Library's own git repository remains present. Document decisions retain
the indexed-path and document-identity checks. Upload decisions additionally bind
the original to an `original.*` sibling of their `sources/**/source.md`, with
matching filename, SHA-256 and byte count in the source frontmatter.
The text, original bytes when present, and write token are replayed
idempotently under the writer lock before an interrupted store opens. A failure
after that decision leaves the journal intact. With no active writer, read
operations refuse with `library_write_pending` until recovery completes;
they never replay or clear a journal themselves. Invalid/newer journals are
preserved and refused by recovery. A token in the commit message prevents a replay
after commit from duplicating the commit. The journal is private (0600), and completion
fsyncs its removal. During a live write, reads return the available file, indexed
record/search hits or committed history, which may reflect the old or new
document. These calls do not promise a snapshot across files, index and git.

Upload recovery publishes the original before the document, index and commit,
using the shared atomic writer's exclusive-creation mode: a synced temporary
file is hard-linked into place without replacing an existing entry. Identical
existing bytes are an idempotent replay and keep the same file; different bytes,
non-file entries and symlinks refuse recovery and preserve the journal and the
existing data. Newly created original directories and their ancestors through the
Library root are synced before completion. A filesystem without hard-link support
refuses publication and leaves the decided capture recoverable. Recovery never
needs the caller's upload path to survive. Capture ID allocation considers both
existing source document IDs and every on-disk entry in the capture's month,
including empty directories, files and dangling symlinks. This preserves originals
left by older captures even when they have no `source.md`; those legacy leftovers
are reserved rather than guessed into a new source or deleted.

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

### Immutable artifact bundles

Artifact promotion holds the writer lock across series-version and id
allocation, a version-4 `artifact` journal decision, all companion bytes,
`artifact.md`, the index upsert and one commit of the exact bundle paths.
The journal stores each relative file name and its bytes. Version-1/2
single-document and settings decisions and version-3 upload decisions retain
their existing recovery semantics. Older builds refuse version 4 rather
than dropping companion files. Recovery validates the full manifest/path/file
set, stages the complete directory beside its destination under a reserved
`.office-pending-artifact-<token>` name excluded from all document scans,
then publishes with one directory rename. Existing staging or published
files are accepted only when their bytes are identical to the decision. A conflicting existing file preserves both it and
the journal for diagnosis. Replaying after commit does not duplicate the
commit. Companion Markdown files remain outside the document index.

### Video plans and derived files

Video plans and call reservations update `sources/**/source.md` under the same
writer lock. A revision comparison refuses a stale writer with `plan_changed`.
Version-5 `video` journals bind the updated source and its `frames.md` and
`frames/<index>.png` companion bytes. Recovery validates the source ledger,
companion names and physical path containment before replaying files, indexing
and committing; older versions and artifact bundles retain their existing
validation. The PNGs remain ignored and are excluded from the video commit.
Only source Markdown and `frames.md` are committed by a frame result.

The index still has one row per source ID. Reading/indexing its `source.md`
appends the bodies of sibling `frames.md` and `chapters.md` to the indexed body.
These companions do not create independent document IDs. Reindex reproduces
the same combined row; symlinked or unreadable companions refuse the scan instead
of dropping searchable content. Generic `write_document` still accepts only
primary indexed documents; video writes publish companions through their
journaled operation.

Initialization writes `original.*` and `frames/` in a new `.gitignore`. On an
existing Library, it appends a missing `frames/` line while preserving all
existing bytes (adding a separating newline when needed), commits the update,
and is a no-op on the next call. It refuses a symlink at this fixed root path.

### Bootstrap previews and phase progress

The fixed `.office/bootstrap.json` file maps phase names to complete batch
ledgers. `Library::bootstrap_snapshot` reads every current source and optional
sibling `digest.json`, knowledge page, and phase ledger without an index result
limit. Malformed or unreadable JSON and symlinked sidecars/ledgers refuse the
read. `open_for_bootstrap_read` opens no on-disk index, creates no directories,
and performs no recovery or backfill, so bootstrap status also works with a
missing disk index or plugin-state directory. Its internal unused index is
in memory. An abandoned journal refuses reads as `library_write_pending`.

`Library::mutate_batch` compares the complete snapshot/settings revision under
the writer lock and returns `plan_changed` before a decision on a stale write.
Version-6 `bootstrap` journals bind the entire phase-ledger replacement and all
source privacy updates. The ledger's fixed path, phase names, signed plans,
approvals and consumed call counts are validated before the decision and again
on recovery; bundle entries are contained `sources/**/source.md` paths only.
Recovery writes/indexes those sources, replaces the ledger and commits exactly
those paths once. Earlier journal versions retain their existing validation.

### Transcript versions and extraction results

`Library::read_media` reads video or meeting originals through the same contained
path and recorded-SHA check as `read_video`. Optional `source_sidecar` reads allow
only `frames.md`, `chapters.md` and `digest.json`, distinguishing absent files
from unreadable files and refusing symlinks.

Version-7 `source_processing` journals bind source provenance, the current
`source.md` view, immutable `vN.md` transcript snapshots and derived extraction
files. `write_source_processing` compares the source revision under the writer
lock. Version files publish without replacement; recovery accepts an existing
snapshot only when its bytes match. Sidecars are limited to `chapters.md`,
`digest.json`, `extract-result.json` and numbered `extract-window-<index>.json`
files. Physical containment and names are checked before deciding and on replay.
Every derived text file is committed with the source; frame PNGs remain ignored.
The source index is rebuilt from the updated current body and its companions.

Audio and extraction reservations use the existing video journal before each
model request. The ledger records `audio_completed` separately from consumed
calls and binds generated transcripts by content hash. An interrupted model
request cannot be silently repeated or mistaken for completed audio. A stale
result revision refuses publication instead of overwriting a newer source.
