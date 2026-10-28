---
type: design
summary: "Spec: shared library — a `shareable` field on Library documents, one team git repository, and `office.library-sync`, which pushes the shareable documents and pulls colleagues' work into a read-only shared layer, so a new joiner inherits the team's knowledge"
last_validated: 2026-10-28
---

# Spec: Shared library

Everyone keeps their own Library. A team also keeps one **shared library**:
a git repository that every member pushes their shareable documents to and
pulls colleagues' documents from. A new joiner connects to it and starts
with the pages, runbooks and KT readings the team has already built,
instead of rebuilding them from the same documents and videos
([decision](../4_decisions.md#teams-share-through-one-shared-library-synced-from-a-shareable-field)).

This is how Daniel already works: one shared knowledge base, his own, and a
sync that pushes and pulls on a schedule. Here the `shareable` field makes
the sync mechanical, so it is a deterministic tool, not an agent loop.

The approval field, `office.share-set`, pure shared view, validated settings,
the read-only shared layer, `office.library-sync`, its routine, the demo and
the app section below are implemented.

## The loop

1. **Mark.** The user marks a source, page or artifact shareable, or
   approves a proposal that suggests it. Marking is the approval for that
   document to leave the machine.
2. **Sync.** The `office-shared-sync` routine (or **Sync now**) runs
   `office.library-sync`: pull the team's changes, then push this member's
   shareable documents.
3. **Use.** Shared documents are searchable, attachable and citable beside
   the user's own, labelled with the member who shared them.
4. **Improve.** Editing a shared page goes through the usual proposal. The
   accepted page becomes the user's own copy, which is shareable and is
   pushed back on the next sync.

## The `shareable` field

Sources (`source.md`), knowledge pages and artifacts (`artifact.md`) take
`shareable: true | false`. Absent means `false`.

**`office.share-set`** (mutating, `mcp_scope: none`) takes
`{"id": "<document id>", "shareable": true}` or `false`, and returns `id`,
`path` and `shareable`. It is the user's approval action, so agents can't
call it. Under the writer lock it rewrites the one field, preserves the
rest of the document, reindexes and commits once. Repeating the current
value changes nothing and makes no commit. Clearing an absent field is already
false and is a no-op. Existing scalar whitespace, comments and line endings
are retained; an absent true field is inserted before the closing delimiter.
Malformed explicit citations refuse `invalid_frontmatter` rather than escaping
approval. The SQLite index records approval; migration 3 backfills existing rows from
Markdown with a durable rescan marker. It refuses:

- `share_private_refused`, naming `sensitivity`, for a `private` document;
- `share_cites_unshared`, listing the cited document ids that are not
  shareable (sorted, distinct own ids without anchors; missing targets fail
  closed), for a document whose `cites`, inline citations or
  `artifact` refs name an own document that is not shareable. The app
  offers **Share these too**, which calls `share-set` for each, sources
  first;
- `document_not_found` and unknown input keys, as other tools do.

Unsharing (`false`) removes the document from the shared library on the
next push. Before confirming, the app says that colleagues who already
synced keep their copy and that the repository's history keeps it too.

**Interactions:**

- `office.source-set-sensitivity` to `private` also sets `shareable: false`
  in the same commit when approval was true; its result reports `unshared: true`
  exactly when this call revoked sharing, otherwise false.
- A `knowledge_add` or `knowledge_update` proposal may carry
  `"shareable": true` when every own document the page cites is shareable. The proposal shows "Will be shared with
  the team", and acceptance writes the field with the page. The skill has
  the orchestrator suggest it for pages about the team's systems, clients
  and processes, never for the user's own pages (the `user`, `goals`,
  `rhythm` and `standing-directions` templates).
- `office.artifact-promote` takes an optional `shareable` in the manifest
  under the same citation rule.

`knowledge_add` and `knowledge_update` payloads accept optional boolean
`shareable`; when supplied it sets the field in their reviewed Markdown.
Draft pages also accept it. Creation and acceptance validate approval, and
publication checks current cited files again under the writer lock.
`office.knowledge-write` accepts the boolean field and applies the same rule.
An existing sharing flag never bypasses the citation check.

## Settings

A closed `shared` block in `library/.office/config.yaml`, validated on
every read and set through `office.config-set`:

```yaml
shared:
  remote: git@github.com:acme-team/team-library.git   # or ssh://…, https://…, or an absolute path to a bare repository
  branch: main
  member: daniel        # lowercase kebab-case, 1–32 characters; recorded as shared_by
```

- `remote` must be an `ssh://` URL, an scp-style `user@host:path`, an
  `https://` URL without user information, or an absolute local path. Any
  other form, or a URL that carries credentials, is refused at load with
  `invalid_library_setting` naming `shared.remote`, never its value.
- The block requires all three fields; null also means unconfigured.
  `office.config-set` accepts the whole block, or `shared.remote`,
  `shared.branch` and `shared.member` after a complete block exists.
  Unknown block keys refuse `invalid_library_setting`. The branch must be
  a safe git branch name. Member names have single internal hyphens and
  ASCII lowercase letters or digits.
- No block means no sharing: `office.library-sync` refuses with
  `shared_not_configured`, and nothing else changes.
- Changing `remote`, `branch` or `member` after a sync deletes the local
  clone and its sync state on the next sync, which then starts fresh. Documents
  already pushed under the old member name stay in the shared library.

## The shared repository

```
<shared library>/
  .office-shared.yaml          # format: 1
  knowledge/domain/<id>.md     # one page per subject, edited by any member
  knowledge/runbooks/<id>.md
  members/<member>/sources/YYYY/MM/<source-id>/   # source.md, vN.md, frames.md, chapters.md, digest.json, summary.md
  members/<member>/artifacts/YYYY/MM/<artifact-id>/
```

- **Pages are shared by subject.** There is one Acme client page for the
  team, and anyone may improve it.
- **Sources and artifacts belong to the member who shared them.** They are
  immutable evidence, and two members can give different sources the same
  id (both capture the same meeting on the same day), so they sit under
  `members/<member>/`. In the shared layer they are cited as
  `<member>/<id>`, for example `alice/src-20261028-acme-qbr#t=00:12:31`.
- **No originals.** `original.*` and `frames/` stay out, as they stay out
  of the Library's git. A new joiner gets transcripts, frame readings,
  chapters, digests and summaries, but not the media files.
- The local clone lives at `{{plugin_state}}/shared/repo/`, inside the
  plugin's existing grant. Sync state is
  `{{plugin_state}}/shared/state.json`, owner-only and written atomically
  (STD-03@2 §R5; [Sync state](#sync-state)).

### The shared view of a document

`office_core::domain::sharing::shared_view` is pure: Markdown, a member name,
a current-sharing predicate and an already-shared predicate produce canonical
frontmatter and a body whose text outside citations stays verbatim. Private
input refuses `share_private_refused`. A push writes this document's
**shared view**, never the file as it is:

- `contact` refs and `path` refs are dropped. Colleagues keep their own
  contacts, and a local path means nothing on another machine.
- `artifact` refs to artifacts that are not shareable are dropped.
- Own source and artifact citations are qualified with the member
  (`src-…` → `daniel/src-…`), in frontmatter and inline. Citations that are
  already qualified are unchanged. Retained artifact refs are also qualified.
- `related` keeps only pages that are shareable or already shared.
- `sensitivity` and `shareable` are dropped (everything shared is `normal`).
  `shared_by` is set to the member, and `contributors` gains the member if
  absent.

## `office.library-sync`

Mutating, `mcp_scope: workspace`: the orchestrator may run it when the user
asks, because it only moves documents the user already approved for
sharing. Input: `{}` or any of `{"direction": "both" | "pull" | "push",
"dry_run": false}`. The default is `both`. Unknown keys refuse
`invalid_argument`.

1. **Settings and lock.** Reads the `shared` block, else
   `shared_not_configured`. One sync at a time holds
   `{{plugin_state}}/shared/sync.lock`; a second refuses
   `library_writer_busy`. Sync state recorded for another remote, branch or
   member is discarded with the clone (not in a dry run).
2. **Fetch.** Creates the clone when it is missing and fetches the branch
   into its remote-tracking ref without touching the working tree, so a dry
   run reads every file from git objects. A reachable remote without the
   branch is an empty shared library. A branch without a regular
   `.office-shared.yaml` that is exactly `format: 1` is refused with
   `shared_layout_invalid`; the first push to an empty branch writes it.
3. **Pull** (directions `both` and `pull`). Every path whose blob differs
   from the one last seen is listed in `pulled`, or in `skipped` when it is
   a symlink (`shared_file_linked`), a submodule or other special entry
   (`shared_file_special`), not UTF-8 text (`shared_file_unreadable`), outside the layout or an artifact file its
   `artifact.md` does not list (`shared_path_outside_layout`), a companion
   without a valid document (`shared_document_invalid`), or a page or
   document the Library's own frontmatter, page, template and privacy rules
   refuse (that refusal's code, for example `share_private_refused`). Then:
   - The working tree is checked out at the fetched head (local changes and
     files discarded; the clone is the tool's own) and both index layers
     are rebuilt. The shared layer's own validation keeps skipped files out
     of the index.
   - **Incoming update.** When the user's own copy of a changed page is
     shareable and unchanged since this member last pushed or matched it,
     it files a `knowledge_update` proposal that applies the shared version
     ("bob updated *Acme* in the shared library"). The proposed Markdown is
     the shared copy localized: the user's own `contact` and `path` refs
     are kept, citations of the user's own member become plain ids,
     `shared_by` is dropped, and the own copy's `shareable` and
     `sensitivity` are retained.
   - **Conflict.** When both copies changed, it files one `shared_conflict`
     proposal carrying the page, the shared path and blob, the localized
     shared version and the own copy's digest. Nothing is pushed for that
     page until it is resolved. `office.proposal-accept` takes a required
     `choice`: `theirs` writes the shared version, as above; `mine` changes
     no page and records the shared blob as this member's base, so the next
     sync pushes the user's version over theirs (theirs stays in the
     history). `choice` is refused for every other kind, and
     `office.proposal-create` refuses `shared_conflict`.
   - Both use the proposal record
     ([proposals.md](./proposals.md#record-and-lifecycle)), tagged
     `shared-library`, with the page id and `shared:<path>@<blob>` as
     evidence. An open proposal for the same page and blob is reported with
     `created: false` instead of being filed again, and dismissal memory
     applies per blob.
   - An own copy of a changed page that is no longer shareable stops being
     this member's: no proposal is filed and nothing is pushed for it.
   - When the shared copy already equals the user's shared view (ignoring
     `shared_by` and `contributors`), the page is recorded as matched; this
     is how an accepted incoming update or **Use theirs** settles.
4. **Push** (directions `both` and `push`). It reads the Library at one
   commit (`HEAD` before and after, retrying once, then `library_changed`).
   For every shareable own document:
   - It checks the document again: `private`, or citing an own document
     that is no longer shareable, puts it in `skipped` with phase `push`
     and `share_private_refused` or `share_cites_unshared`.
   - A page's shared view goes to `knowledge/…/<id>.md`; a source or
     artifact's shared view goes under `members/<member>/` at its Library
     path with its regular companion files (source sidecars `vN.md`,
     `frames.md`, `chapters.md`, `digest.json`, `summary.md`; the files an
     artifact lists). `original.*` and `frames/` never leave.
   - It writes a file when it differs from the shared copy. A page is
     written only while the shared copy is still the blob this member last
     pushed, matched or chose **Keep mine** over; otherwise the pending
     incoming update or conflict decides. Member files are always this
     member's to overwrite.
   - It removes paths this member pushed whose document is no longer
     shareable or no longer exists. A page a colleague has changed since is
     left to them and forgotten instead.
   - It commits once ("<member>: share 4 documents, remove 1") with the
     user's own git identity (`shared_identity_missing` without one) and
     pushes. A rejected push fetches, recomputes and retries once, then
     returns `shared_push_rejected`. A push with nothing to write or remove
     makes no commit.
5. **Result.** `direction`, `dry_run`, `pulled`, `pushed` and `removed`
   (shared paths), `incoming` and `conflicts` (`{page, proposal, created}`),
   `skipped` (`{phase, path, id, reason}`), `commit` (the shared head) and
   `synced_at`. `dry_run: true` fetches and computes the same lists but
   changes no index, sync state, proposal, working tree, commit or push;
   its `synced_at` is null and its proposals are null.

Errors: `shared_not_configured`, `shared_remote_unreachable` (a failed
fetch or push; names the remote's host, `local` for a path, never
credentials, paths or git's stderr), `shared_push_rejected`,
`shared_layout_invalid`, `shared_identity_missing`, `shared_storage_failed`
(a local clone or state failure, naming only the operation),
`library_changed`, `library_writer_busy`. The pull's state is saved before
the push starts, so a push failure keeps the completed pull.

### Sync state

`{{plugin_state}}/shared/state.json` (owner-only, atomic, STD-03@2 §R5)
records `format: 1`, the `remote`, `branch` and `member` it belongs to,
the shared `head`, `synced_at`, and per clone-relative path: `seen` (the
blob the last pull saw), `base` (the blob this member last pushed, matched
or chose **Keep mine** over) and `own` (the SHA-256 of the own document or
file when it was pushed or matched; present only for paths this member
shares). Unknown fields refuse to load.

### Transport

- Git runs through the shared subprocess supervisor with a 120-second
  deadline per command and bounded output, from the plugin's required
  `git`. Office's git adapter is `office_store::SharedStore` behind the
  `office_core::ports::SharedRepository` port; tools never run git.
- The child environment is an allowlist (STD-05@1): `PATH`, `HOME`,
  `LANG`, `SSH_AUTH_SOCK`, `GIT_TERMINAL_PROMPT=0`. Credentials are the
  user's own git setup (SSH agent or credential helper). Office stores no
  git secret and never sees one.
- Every git command runs with `-c protocol.allow=never` plus
  `protocol.ssh.allow`, `protocol.https.allow` and `protocol.file.allow`
  set to `always`, and `-c core.hooksPath=/dev/null`, so the shared
  repository can't select another transport or run hooks. Git drops `-c`
  settings for a local transport, so a push to a path or `file://` remote
  also runs that repository's `receive-pack` with `core.hooksPath=/dev/null`.
  The clone also disables symlink checkout, fsmonitor, commit signing and
  automatic gc.
- This is the plugin's one outward path besides model inference and the
  gateway. It reaches only the configured `remote`, and it carries only
  documents the user marked shareable.

## The shared layer

Implemented independently of sync against `{{plugin_state}}/shared/repo/`.
`office.reindex` rebuilds both layers in one index transaction. A missing clone
contributes no rows and is never created by a read. Invalid frontmatter, invalid
page rules, private documents, symlinks, special files and paths outside the
shared repository layout are skipped. Each document requires a valid `shared_by`;
evidence paths must agree with that member. Shared pages with contact or local
path refs are refused, since the shared view removes those pointers. Source
rows include valid regular frames, chapters, summary and digest sidecars in
their searchable body; invalid or linked companions contribute nothing.

The index (`plugin_state/index.sqlite`) holds a second, read-only layer for
the clone. Every row records `layer` and `shared_by`; own rows have `layer: own`
and `shared_by: null`. Shared rows coexist with own copies of the same page.
Response paths use the virtual prefix `shared/` followed by the clone-relative
path; reads route it to the clone, and ordinary document writes refuse it.

- **Ids.** A plain page id (`dom-client-acme`) resolves to the user's own
  page, else the shared one. A qualified `<member>/<id>` resolves to the
  shared layer, or to the user's own document when `<member>` is the
  user's own `member`.
- **Citations.** `office.knowledge-write` accepts qualified citations and
  checks them against the shared layer the same way (`unresolved_cite`
  otherwise).
- **Reads.** `office.search` gains `layer` (`own` | `shared`; both by
  default), and hits carry `layer` and `shared_by`. `office.knowledge-list`
  and `office.artifact-list` rows and `office.source-show` also carry
  `shareable` (artifact-show and knowledge-show already return the document's
  own field), and `office.status` reports the configured block as `shared`
  (`{remote, branch, member}`, or null when unconfigured or the Library is
  not initialized), so the app can label and connect without reading files. `office.knowledge-show`,
  `office.knowledge-list`, `office.source-show` and `office.artifact-show`
  read shared documents and add the same two fields (`page.layer` and
  `page.shared_by` in knowledge-show; top-level fields in source-show and
  artifact-show). Shared source/artifact ids include the member; page ids
  remain subject ids. Artifact series history stays within the same layer
  and sharing member. Shared pages are
  already in their shared view, so no contact value is ever returned.
- **Attach.** `office.attach` picks from both layers, preferring the user's
  own copy of a page, and records each file's layer in `office-attach.json`.
- **Bootstrap.** Consolidation treats shared pages as existing pages, so it
  proposes updates to them rather than new duplicates, and setup progress
  counts them toward the phases they cover
  ([library-bootstrap.md](./library-bootstrap.md)). Each phase reports a
  `shared` count of pages matching its digest subjects or tagged
  `bootstrap:<phase>`. Own copies take precedence, so each subject counts once.
  Accepting an update against a shared-only page checks the reviewed shared
  digest and an absent own destination under the writer lock, then writes an
  own copy with `shareable: true`; the clone remains unchanged.
- **Never run.** A shared `script` artifact is never run. `office.script-run`
  accepts only artifacts in the user's own Library, so a colleague's script
  runs only after the user copies it in through a new proposal.
- **Data, not instructions.** Shared documents are colleagues' text and
  enter model inputs quoted as data, like any source.
- `office.reindex` rebuilds both layers. A missing clone is an empty layer.

## Routine

`office-shared-sync` (`definitions/routines/office-shared-sync.yaml`):
hourly at `0 * * * *`, seeded `enabled: false`, targeting
`job:office-shared-sync`. The job has one deterministic step that inlines
`plugin.tool_call` with `office.library-sync` and `input: {}`, like
`office-calendar-sync`, with `timeout_seconds: 360` and
`max_active_runs: 1`. Both are registered in `spec.definitions`. When the
user saves the `shared` block, the app offers to turn it on (planned with
the app).

## App

- **Settings › Shared library:** remote, branch and member name; **Sync
  now**; last synced time with the counts in words ("Shared 4, received 12,
  1 needs you"), or the failure in plain words.
- **Library:** shared documents carry a "Shared · from Alice" label, own
  shareable documents a "Shared" label. A page, source or artifact view has
  a **Share with the team** switch that calls `office.share-set`, with the
  refusals above in plain words.
- **Library setup:** a **Connect a shared library** step, shown first to a
  new Library, that opens Settings › Shared library.
- **Needs you:** incoming updates, conflicts and skipped documents, grouped
  as one card per sync. The sync files proposals for the first two; the app
  builds the skipped card from the result's `skipped` list, since the tool
  files no separate task for it.

Built as follows ([web-app.md](./web-app.md#shared-library-implementation)):

- Settings saves the whole block with `office.config-set` (`key: "shared"`).
  A refused field is named (the address, the branch or the member name) and
  its value is never repeated. After a save, when the `office-shared-sync`
  routine is off, the section offers **Turn on hourly sync**, which toggles it
  through `orbit.routine.control`.
- **Sync now** runs `office.library-sync` with `{}` under a six-minute
  deadline, one at a time from the app. Counts are documents, not files
  ("Shared 2, received 3, 3 need you"); a failure is its refusal code in plain
  words. The app keeps the latest result it ran in memory only, so a sync run
  by the routine is reflected in Needs you through its proposals, not in the
  counts, and the counts are gone after an app restart.
- The switch refuses in plain words: private documents can't be shared, and
  `share_cites_unshared` lists the cited documents by title with **Share
  these too**, which shares them (sources, then outputs, then pages) before
  the document. Stopping shows the copies-and-history warning first.
- Proposals carry no sync id, so Needs you shows one **Changes from the
  shared library** card for every open proposal tagged `shared-library`
  (incoming updates with Accept and Dismiss; conflicts with **Use theirs** and
  **Keep mine**, which call `office.proposal-accept` with that `choice`), plus
  the skipped documents of the app's latest sync until **Got it**. A proposal
  with `shareable: true` shows "Will be shared with the team".
- The Library setup step **Connect a shared library** is shown first while
  no block is configured and opens `?settings=shared-library#settings`.

## Demo

`demo/synthetic/shared_library.py --root <new dir>` builds two workspaces and
a bare repository under one new root, all synthetic, through the built
binary. Alice captures a time-coded Lantern KT transcript, writes a topic
page and a runbook citing it (plus a scratch page she never shares), marks
the transcript and the two pages shareable, previews with `dry_run` and
syncs; a second sync pushes nothing. A new joiner's empty Library connects,
syncs, and answers "What do I do when the Lantern spool stalls?" from
Alice's runbook, whose citations (`alice/src-…#t=00:01:10`) open Alice's
transcript at the second. The script checks each step and writes
`shared-demo.json`. Frame readings come from the macOS video pipeline, so
the demo shares a transcript only; pushing and pulling `frames.md` is
covered by the end-to-end test.

## Not built

- More than one shared library per workspace, and sharing with only some
  members.
- Accounts or permissions. Who may push is whoever the git host lets push.
- Erasing a document from the shared history.
- Sharing originals (video and audio files).
- Line-by-line merging. A page is replaced whole.
- **Ask an agent to merge** on a conflict. A conflict offers **Use theirs**
  and **Keep mine**; a merged page can still be filed as an ordinary
  `knowledge_update` proposal.

## Task References

- [ORB-19417] — builds the `shareable` field, `office.share-set`, the shared view and the `shared` settings.
- [ORB-19418] — builds `office.library-sync`, its routine and the demo repository.
- [ORB-19419] — builds the shared layer: index, qualified citations, reads, attach and bootstrap.
- [ORB-19421] — builds the app: Settings › Shared library, labels, the share switch and Needs you cards.
