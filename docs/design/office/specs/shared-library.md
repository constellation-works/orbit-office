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

The approval field, `office.share-set`, pure shared view and validated settings
are implemented. Sync, the shared read layer, routine, app and demo sections
below describe separately planned work.

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
- Changing `remote` or `member` after a sync deletes the local clone and
  its sync state on the next sync, which then starts fresh. Documents
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
  plugin's existing grant. Sync state (per path: the blob last seen in the
  shared library and the digest of the own document pushed for it) is
  `{{plugin_state}}/shared/state.json`, owner-only and written atomically
  (STD-03@2 §R5).

### The shared view of a document

`office_core::domain::sharing::shared_view` is pure: Markdown, a member name,
a current-sharing predicate and an already-shared predicate produce canonical
frontmatter and a body whose text outside citations stays verbatim. Private
input refuses `share_private_refused`. A future push writes this document's
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

Planned separately.

Mutating, `mcp_scope: workspace`: the orchestrator may run it when the user
asks, because it only moves documents the user already approved for
sharing. Input: `{}` or any of `{"direction": "both" | "pull" | "push",
"dry_run": false}`. The default is `both`.

1. **Settings.** Reads the `shared` block, else `shared_not_configured`.
2. **Clone or fetch.** Clones the branch into `repo/` when it is missing,
   else fetches and fast-forwards. The clone is the tool's own: local
   commits left by a failed push are discarded (`reset --hard` to the
   fetched branch), because the next push is recomputed from the Library.
   A repository without `.office-shared.yaml` at format 1 is refused with
   `shared_layout_invalid`, except an empty repository, which the first
   push initializes.
3. **Pull.** For every path that changed upstream since the last sync:
   - It validates the file with the Library's own frontmatter and page
     rules. A file that fails, a symlink or special file, a path outside
     the layout, or a document marked `private` is listed in `skipped` and
     never indexed.
   - It reindexes the shared layer (below).
   - **Incoming update.** When the user has an own copy of a changed page
     that is unchanged since the last push, it files a `knowledge_update`
     proposal that applies the shared version ("Alice updated *Acme* in the
     shared library"). Accepting it keeps the user's own `contact` and
     `path` refs and turns citations of the user's own member back into
     plain ids.
   - **Conflict.** When both copies changed, it files one conflict
     proposal showing both versions, with **Use theirs**, **Keep mine** and
     **Ask an agent to merge** (an agent to-do that drafts a merged page as
     an ordinary `knowledge_update` proposal). Nothing is pushed for that
     page until the proposal is resolved. **Keep mine** pushes the user's
     version over theirs on the next sync; theirs stays in the history.
   - Both are filed on the proposal record
     ([proposals.md](./proposals.md#record-and-lifecycle)), the conflict as
     kind `shared_conflict`, with the page and the shared blob as evidence.
     Equivalence and dismissal memory therefore apply: a repeated sync
     files nothing new, and a dismissed incoming update stays dismissed for
     that blob.
4. **Push.** It reads the Library at one commit (`HEAD` before and after,
   retrying once on `library_changed`). For every shareable document:
   - It checks the document again: `private`, or citing an own document
     that is no longer shareable, puts it in `skipped` with
     `share_private_refused` or `share_cites_unshared`, and files one
     Needs you item for the batch.
   - It writes the shared view when it differs from the shared copy and the
     shared copy has not changed since this member last saw it.
   - It removes paths this member pushed whose document is no longer
     shareable or no longer exists.
   - It commits once ("<member>: share 4 documents, remove 1") with the
     user's own git identity and pushes. A rejected push fetches, recomputes
     and retries once, then returns `shared_push_rejected`.
5. **Result.** `pulled`, `pushed`, `removed`, `incoming` and `conflicts` (the
   proposals filed), `skipped` (id and reason), `commit` (the shared head)
   and `synced_at`. `dry_run: true` fetches, computes the same lists and
   changes nothing: no index, state, proposal, commit or push.

Errors: `shared_not_configured`, `shared_remote_unreachable` (a failed
clone, fetch or push; names the remote's host, never credentials or git's
stderr), `shared_push_rejected`, `shared_layout_invalid`,
`library_writer_busy`. A failure in one phase leaves the other phase's
completed work and the previous sync state intact.

### Transport

- Git runs through the shared subprocess supervisor with a 120-second
  deadline per command, from the plugin's required `git`.
- The child environment is an allowlist (STD-05@1): `PATH`, `HOME`,
  `LANG`, `SSH_AUTH_SOCK`, `GIT_TERMINAL_PROMPT=0`. Credentials are the
  user's own git setup (SSH agent or credential helper). Office stores no
  git secret and never sees one.
- Every git command runs with `-c protocol.allow=never` plus
  `protocol.ssh.allow`, `protocol.https.allow` and `protocol.file.allow`
  set to `always`, and `-c core.hooksPath=/dev/null`, so the shared
  repository can't select another transport or run hooks.
- This is the plugin's one outward path besides model inference and the
  gateway. It reaches only the configured `remote`, and it carries only
  documents the user marked shareable.

## The shared layer

Planned separately; qualified citations parse in this increment but need this
layer to resolve.

The index (`plugin_state/index.sqlite`) holds a second, read-only layer for
the clone. Every row records `layer: shared` and `shared_by`.

- **Ids.** A plain page id (`dom-client-acme`) resolves to the user's own
  page, else the shared one. A qualified `<member>/<id>` resolves to the
  shared layer, or to the user's own document when `<member>` is the
  user's own `member`.
- **Citations.** `office.knowledge-write` accepts qualified citations and
  checks them against the shared layer the same way (`unresolved_cite`
  otherwise).
- **Reads.** `office.search` gains `layer` (`own` | `shared`; both by
  default), and hits carry `layer` and `shared_by`. `office.knowledge-show`,
  `office.knowledge-list`, `office.source-show` and `office.artifact-show`
  read shared documents and add the same two fields. Shared pages are
  already in their shared view, so no contact value is ever returned.
- **Attach.** `office.attach` picks from both layers, preferring the user's
  own copy of a page, and records each file's layer in `office-attach.json`.
- **Bootstrap.** Consolidation treats shared pages as existing pages, so it
  proposes updates to them rather than new duplicates, and setup progress
  counts them toward the phases they cover
  ([library-bootstrap.md](./library-bootstrap.md)).
- **Never run.** A shared `script` artifact is never run. `office.script-run`
  accepts only artifacts in the user's own Library, so a colleague's script
  runs only after the user copies it in through a new proposal.
- **Data, not instructions.** Shared documents are colleagues' text and
  enter model inputs quoted as data, like any source.
- `office.reindex` rebuilds both layers. A missing clone is an empty layer.

## Routine

`office-shared-sync`: every 60 minutes, seeded disabled; a one-step job
that inlines `plugin.tool_call` with `office.library-sync`, like
`office-calendar-sync`. When the user saves the `shared` block, the app
offers to turn it on.

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
  as one card per sync.

## Demo

Two workspaces on one machine and a bare repository in the demo directory,
all synthetic. Alice's Library shares its KT pages and the KT video's
transcript and frame readings. A new joiner's empty Library connects, syncs,
and answers a question about the system from Alice's pages, with citations
that open Alice's transcript at the second.

## Not built

- More than one shared library per workspace, and sharing with only some
  members.
- Accounts or permissions. Who may push is whoever the git host lets push.
- Erasing a document from the shared history.
- Sharing originals (video and audio files).
- Line-by-line merging. A page is replaced whole; **Ask an agent to merge**
  covers the rest.

## Task References

- [ORB-19417] — builds the `shareable` field, `office.share-set`, the shared view and the `shared` settings.
- [ORB-19418] — builds `office.library-sync`, its routine and the demo repository.
- [ORB-19419] — builds the shared layer: index, qualified citations, reads, attach and bootstrap.
- [ORB-19421] — builds the app: Settings › Shared library, labels, the share switch and Needs you cards.
