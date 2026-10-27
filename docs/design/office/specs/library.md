---
type: design
summary: "Spec: Library — layout, sources, knowledge pages, artifacts and frictions"
last_validated: 2026-10-27
---

# Spec: Library

The Library is the user's durable memory: what came in (sources), what is
understood (knowledge), and what went out (artifacts). Markdown is the
source of truth; every claim is citable to an anchor in a source.

Source: spec §6.1–§6.5.

## Layout

```
library/
  sources/YYYY/MM/<source-id>/
    source.md            # frontmatter + normalized, anchorable text
    original.<ext>       # raw upload: m4a, wav, png, jpg, pdf, txt
    v2.md …              # corrections, as new versions (never overwrite)
  knowledge/
    domain/<slug>.md
    runbooks/<slug>.md
  artifacts/YYYY/MM/<artifact-id>/
    artifact.md          # frontmatter + summary (the body itself for text kinds)
    <files>              # report.md, draft.eml.md, script.py, data.csv, deck.pdf …
  proposals/<proposal-id>.md   # rendered evidence for each proposal (the record is the Orbit task)
  .office/config.yaml          # Library settings: retention, sensitivity, packs, thresholds
  .office/templates/<name>.md  # About and How-to page templates (knowledge-templates.md)
  .gitignore                   # original.* and frames/ (binaries are pinned by sha256, not tracked)
```

- The index in `plugin_state/index.sqlite` (FTS5 plus link tables) is
  rebuildable at any time with `office.reindex`. Reindex rebuilds only
  after a complete scan: a missing layer is empty, and an enumeration
  failure leaves the previous index unchanged.
## Git

- The Library is its own git repository at `library/`, created by
  `office.init`. The plugin commits there after each mutating tool call,
  so the history of every page is recoverable. Binaries (`original.*`)
  are ignored by the Library's own `.gitignore`, and each is pinned by the
  `sha256` in its source's frontmatter.
- The workspace repository ignores `/library/`. Orbit refuses a plugin
  write grant on the workspace's `.git`, and plugin writes to tracked
  files in the primary checkout would fail running agent tasks with
  `primary_checkout_drift`, so the workspace repo cannot be the Library
  repo ([decision](../4_decisions.md#the-library-is-its-own-git-repository-and-knowledge-reaches-agents-as-task-artifacts)).

## Scaffolding: `office.init`

The [install script](./install.md) creates the Library with one explicit
call, `orbit tool run office.init`, after it has set up git and the
workspace and enabled the plugin. The user never runs it by hand.

`office.init` (mutating, `mcp_scope: none`) does, in order:

1. Refuses if `library_dir` is not inside the granted write root
   (`{{workspace}}/library`) after resolving existing path components, if the
   destination itself is a symlink, or if `git` is missing. Missing destination
   components are checked against their nearest existing ancestor before any
   Library or index is created.
2. Creates the layout directories, with a `.gitkeep` in each empty one.
3. Runs `git init` in `library/` if it is not already a repository.
4. Writes `.office/config.yaml` from the plugin's bundled default, and the
   `.gitignore`.
5. Copies each bundled page template that is missing into
   `.office/templates/` ([knowledge-templates.md](./knowledge-templates.md#templates)).
   It writes no pages: About pages come from the onboarding interview and
   later sources, each citing its evidence.
6. Creates or migrates `plugin_state/index.sqlite`, then reindexes.
7. Commits whatever it created ("Initialize the Library").

After validating the requested path and git availability, init prepares the
plugin state directory and acquires `plugin_state/library.writer.lock` before
checking completeness or writing Library files. It holds that one lock through
scaffolding, any pending-write recovery, index creation/rebuild and the commit;
recovery and reindex reuse the guard rather than acquiring it again. If another
process holds the lock past its five-second deadline, init returns
`library_lock_timeout` without creating the Library layout, config, `.gitignore`,
repository or index.

It is idempotent: it creates only what is missing and never overwrites an
existing config, `.gitignore`, template or page. A Library is complete only
when its layout, repository, config, `.gitignore`, index and every bundled
template file are present. A second run on a complete Library changes
nothing and reports `already_initialized`. Copying a missing bundled
template lists that file in `created` and does not report
`already_initialized`. The result lists what was created.

Other tools open the store only after the same grant decision `office.init`
uses. A `library_dir` from plugin config (`context.config.library_dir`) or
the call context (`context.library_dir`) that is absolute, or relative and
resolves outside `{{workspace}}/library`, is refused with
`library_dir_outside_workspace` before any layout directory, index or lock
is created. The check is the physical path against that root, shared with
init, and it runs before the uninitialized-Library check. An escaped root
is therefore not created and is not reported as `library_not_initialized`.
`office.status` returns that error too, rather than `initialized: false`.

A root inside the grant still opens the store, which creates the layout
directories (`LibraryStore::open`, a safety net), but those tools never
write the config, run `git init` or commit outside their own change. A tool
that finds the Library uninitialized (no repository or no config) refuses
with `library_not_initialized` and names `office.init`. `office.status` does
not refuse a root inside the grant: an absent Library reads as
`initialized: false` with zero counts, but an invalid config or a pending
write is returned as its typed error (`invalid_library_setting`,
`library_write_pending`, `library_journal_invalid`), never as an
uninitialized Library.

Demo history goes in after `office.init`, through `office.capture` and
the other tools rather than by copying files, so the seeded Library has
real commits, a real index and real provenance.

## Sources

**Id:** `src-YYYYMMDD-<slug>`, made unique with a numeric suffix.

**`source.md` frontmatter:**

```yaml
id: src-20261028-acme-qbr
kind: meeting            # video | meeting | chat | email | doc | receipt | bill | conversation | note
title: Acme QBR
occurred_at: 2026-10-28T14:00:00-04:00
captured_at: 2026-10-28T15:02:11-04:00
captured_by: upload      # upload | paste | routine | orchestrator
origin: "Zoom recording, uploaded"     # free text: "Slack #pricing, pasted"
participants: [Jane Park (Acme), Daniel]
sensitivity: normal      # normal | private
original: { file: original.m4a, sha256: …, bytes: 48211234, media_type: audio/mp4 }
derived_by: { transcription: muse-voice-transcribe, at: … }   # how the body was produced
version: 1
supersedes: null
retention: { original_until: 2027-01-26 }  # from config; null = keep
```

**Body: anchorable text.**

- **Video:** the transcript as for a meeting, plus `frames.md` (frame readings, `#frame=00:12:31`) and `chapters.md` (`#chapter=3`) beside it ([kt-videos.md](./kt-videos.md#library-shape)).
- **Meeting:** one paragraph per speaker turn, prefixed with `[hh:mm:ss] Speaker:`. The anchor is `#t=00:12:31`.
- **Chat:** one line per message, `[timestamp] Author: text`. The anchor is `#L14-20`.
- **Email, doc:** paragraphs. The anchor is `#L…`. Converted documents add `#slide=<n>` (PowerPoint) and `#page=<n>` (PDF); conversions are in [library-bootstrap.md](./library-bootstrap.md#document-capture).
- **Digest:** any source may carry `digest.json` beside `source.md`: the per-source summary, subjects, cited claims, procedures and terms that the bootstrap's consolidation reads ([library-bootstrap.md](./library-bootstrap.md#stage-1-a-digest-per-source-cheap)).
- **Receipt, bill:** extracted fields as a YAML block plus the raw OCR text.
- **Conversation:** the decision log of one orchestrator session: one timestamped line per decision, direction (verbatim), fact the user stated, action, open question, resolution or handoff, anchored `#L<n>`. It is append-only (`office.capture` with `append_to`), so nothing already recorded changes. The agent CLI's own transcript belongs to that CLI, so the skill has the agent write this log itself ([sessions.md](./sessions.md#decision-log)).

**Rules:**

- **Immutable.** A correction writes `v2.md` with `supersedes` set. Citations name a version when it matters, otherwise the latest.
- **Dedupe.** If the `sha256` of the original, or of the normalized pasted text, matches an existing source, capture returns that source instead of creating a new one.
- **Paste capture.** When the user pastes external content into the orchestrator session (a Slack thread, an email), the skill has the agent call `office.capture` *before* acting on it, then cite the new source id in its reply. Short directives typed by the user are not captured separately: they go in the `conversation` source.
- **Conversation sources.** The skill has the agent open one `conversation` source per session and append to it whenever the user makes a decision or gives a standing direction, and for each action it takes. Instead of an end-of-session summary, the agent keeps a rolling `handoff` entry current, and the next session starts from `office.session-brief`, which leads with that handoff ([sessions.md](./sessions.md)). The user's directions are decisions, and the review needs them.
- **Retention** is per kind and shown on the Routines screen: for example, keep meeting transcripts forever and drop the audio after 90 days. Expiry deletes only the `original.*` file and records that in the frontmatter.

## Knowledge pages

```yaml
id: dom-project-acme-renewal   # dom- | rb-
type: domain                   # domain | runbook
template: project              # optional; see knowledge-templates.md
title: Acme renewal
aliases: [Acme renewal, the renewal]
summary: Acme's contract renews in March; Jane Park decides; price sensitivity high.
tags: [project:acme-renewal, client:acme]
cites:
  - src-20261028-acme-qbr#t=00:12:31
  - src-20261024-slack-pricing#L14-20
refs:                          # pointers, domain pages only
  - { label: Master services agreement, path: ~/Documents/Acme/MSA-2025.pdf }
  - { label: Renewal tickets, account: "jira:ACME" }
  - { label: QBR deck template, artifact: art-20261021-qbr-template }
related: [rb-client-qbr-prep, dom-client-acme, dom-person-jane-park]
updated_at: 2026-10-28T15:10:00-04:00
last_verified: 2026-10-28
status: current                # current | stale | retired
```

- Claims in the body carry inline citations, for example `[src-…#t=00:12:31]`. Once the planned source screen ships, the app will render them as links that open the source at the anchor; until then the anchors are a recorded URL contract with no landing behaviour.
- **Runbooks** add `inputs`, `steps` and `done_when` sections. These are what an implementing agent follows.
- **Templates.** A page may name the template it follows (`template: person`). Templates fix its id pattern and required sections, and decide when `office.attach` includes it. The bundled set and the onboarding interview that writes the first pages are in [knowledge-templates.md](./knowledge-templates.md).
- **Pointers** (files, links, contacts, templates, accounts) live in a domain page's `refs`, on the page about their subject, for example Jane Park's email on `dom-person-jane-park`. Each ref has one `path`, `url`, `contact`, `account` (`system:key`, such as `jira:ACME`) or `artifact` target. Never secrets. Contact values never reach a model ([knowledge-templates.md](./knowledge-templates.md#contact-details)).
- Knowledge may cite artifacts as well as sources, for example a research report's findings folded into a domain page. The artifact's own `cites` keep the chain back to evidence.
- **Writes** go through `office.knowledge-write` (`{"markdown": "<the whole page>"}`), which is hidden from MCP ([plugin-surface.md](./plugin-surface.md)). It writes `knowledge/domain/<id>.md` or `knowledge/runbooks/<id>.md`, replacing an existing page with that id, and commits once. A claim's extent is not machine-readable, so "every claim cites" is enforced by resolving every citation a page makes, not by refusing uncited sentences; the skill and the proposal review keep claims cited (user-authored pages may have none). Every check runs first, so a refusal writes and commits nothing. In order:
  1. **Frontmatter** (`invalid_frontmatter`, naming the field, never its value): only the fields above plus `sensitivity`, `as_of`, `performable` and `submission`; `id` is `dom-<slug>` or `rb-<slug>` and `title` is required; `type`, when present, is `domain` or `runbook` (else the id prefix decides); `status` is `current`, `stale` or `retired`; dates are `YYYY-MM-DD`; `related` holds page ids. The supported YAML is a flat mapping of plain or quoted scalars, inline `[…]` and `{…}` collections, indented block lists (of scalars, flow mappings or `key: value` items), one level of indented mapping (`submission`) and comments.
  2. **Template**, when the page names one ([knowledge-templates.md](./knowledge-templates.md#the-template-field-on-pages)).
  3. **Base rules:** the id prefix matches the type; `refs` only on domain pages, and `performable` (`agent` | `you`) and `submission` (`system`, `via: draft | gateway`, and `connection` for a gateway) only on runbooks; each ref has a `label`, exactly one target, and optionally `authoritative: true` and `as_of` (`account` is `system:key`, `artifact` an `art-` id, a `url` carries no credentials); no email address or phone number in the body (`contact_in_body`); at most 1,200 words of body (`page_too_long`); and a page tagged `policy`, or with a section whose `##` heading ends in `[policy]`, needs an `authoritative` ref and an `as_of` on that ref or the page (`policy_without_source`; [library-bootstrap.md](./library-bootstrap.md#page-rules)).
  4. **Citations** (`unresolved_cite`): every frontmatter `cites` entry and every inline `[id#anchor]` (several may share brackets, separated by `;` or `,`) names an indexed document of the kind its prefix says (`src-`, `dom-`, `rb-`, `art-`), never the page itself. A source cite needs an anchor present in that source's body: `t=hh:mm:ss` from the first speaker turn's timestamp to the last's, `L<n>` or `L<n>-<m>` within the body's lines (counted after the frontmatter, surrounding blank lines trimmed), or `slide=<n>`/`page=<n>` naming a `Slide <n>`/`Page <n>` heading. `frame=` and `chapter=` anchors are refused until `frames.md` and `chapters.md` are readable through the Library port. Other documents are cited without an anchor. Refs are pointers and are never resolved.
- **Reads:** `office.knowledge-show` (`{"id": "<page id>"}`, `page_not_found` otherwise) returns the page's model view, since it is visible to agents ([knowledge-templates.md](./knowledge-templates.md#contact-details)), and its back-links: the indexed sources, pages and artifacts whose frontmatter `cites` (bare or anchored) or `related` name it, each with `via: [cites, related]`. The model view is `office_core::domain::knowledge::Page::model_view`; `PageView::to_markdown` renders it for task attachments.

## Artifacts

Artifacts are what the agents produce for the user: reports, docs, drafts,
scripts, data and reviews. A task's output is the record of one run. An
artifact is the durable, findable result, which later tasks build on.

**Id:** `art-YYYYMMDD-<slug>`. **`artifact.md` frontmatter:**

```yaml
id: art-20261029-acme-renewal-research
kind: report             # report | doc | draft | script | data | review | other
title: Acme renewal — competitor pricing research
summary: Three competitors undercut by 8–15%; recommends a two-year lock at current rate.
files: [report.md]       # one or more, beside artifact.md
task: <orbit-task-id>    # the task that produced it
produced_by: { crew: muse-wonder, runbook: rb-pricing-research, at: 2026-10-29T10:41:00-04:00 }
approved: { by: user, at: 2026-10-29T11:05:00-04:00, edited: false }   # edited = user changed it before approving; by: routine only for the daily digest
cites: [src-20261028-acme-qbr#t=00:12:31, dom-project-acme-renewal]
series: acme-renewal-research   # groups versions of the same deliverable
version: 1
supersedes: null
sensitivity: normal      # the strictest of what it cites
tags: [client:acme]
```

**Lifecycle:**

1. **Produce.** The implementing agent writes its output as Orbit task artifacts (`orbit.task.artifact.put`, up to 1 MiB each), together with a manifest naming the kind, title and what it drew on. The agent never writes into the Library.
2. **Review.** The task lands in `review`, and the user sees the output under "Needs you".
3. **Promote.** Approval calls `office.artifact-promote`. It copies the task's output into `library/artifacts/…`, writes the frontmatter (task, producer, cites, whether the user edited it), indexes it and commits. Output that is rejected or never approved stays only on the Orbit task.
4. **Revise.** A re-run of the same deliverable (next week's report, a corrected draft) promotes a new artifact in the same `series` with `supersedes` set. Approved artifacts are never edited in place.

**Rules by kind:**

- **`draft`** (an email or message for someone else) records `sent_at` once the user sends it. In the hackathon build, sending copies the draft to the clipboard.
- **`script`** is never run because it was promoted. A script runs only as a step of a routine or auto-task whose accepted proposal named it. That step pins the artifact version by `sha256` and runs inside Orbit's sandbox. That step calls `office.script-run`, which runs it with the install's pinned Python in its own run directory, offline, with dependencies fetched once at approval, and files its outputs for review ([scripts.md](./scripts.md)). Changing the script means a new version and a new proposal.
- **`review`** holds the generated reviews ([analytics-and-reviews.md](./analytics-and-reviews.md)).
- **`report`** in the series `digest-daily` is the daily digest, the one artifact promoted without review (`approved.by: routine`; [sessions.md](./sessions.md#daily-digest)).
- **`data`** (csv, json) records its schema in the summary, so later tasks can use it without opening it.

**Promotion contract.** `office.artifact-promote` takes
`{"task":"<id>","edited":false}` and an optional `manifest` task-artifact
path (default `artifact-manifest.json`). The MCP-hidden call is the user's
approval action; it accepts no approver, crew, version or approval timestamp
in input. The task must be in `review` or `done`. Its closed JSON manifest is:

```json
{
  "kind": "report",
  "title": "Renewal research",
  "summary": "Approved findings",
  "files": ["report.md", "data/prices.csv"],
  "series": "renewal-research",
  "cites": ["dom-project-acme-renewal"],
  "tags": ["client:acme"],
  "runbook": "rb-pricing-research",
  "sensitivity": "normal"
}
```

`kind`, `title`, `files` and `series` are required. The remaining fields
are optional; unknown fields are refused. Files name existing task artifacts
and keep their relative names inside the new artifact directory. Each file,
including the manifest, is at most 1 MiB. Text and base64-encoded binary
payloads are copied losslessly. Absolute paths, traversal, hidden components,
backslashes, duplicate names, file/directory conflicts and any companion
named `artifact.md` are refused. The manifest is metadata, not an output
file. A task with no artifacts returns `task_has_no_artifacts`; invalid
metadata returns `invalid_artifact`, oversized files `artifact_too_large`,
invalid payloads `artifact_payload_invalid`, and an unreviewable task
`task_not_reviewable`. All task files are read before publication, so a
callback failure writes nothing to the Library.

Promotion stamps `approved.by: user`, the current approval time, and `edited`
(default false). `produced_by.crew` and `produced_by.at` preserve the source
task's recorded crew and update timestamp; unknown values stay null. Its
runbook comes from the manifest. Cited documents must exist, and their
sensitivity raises the artifact's sensitivity when necessary. Version and
collision-free id allocation happen under the writer lock. A journal covers
the metadata and every file's exact bytes; recovery finishes that same
version and commit after an interruption. Older artifacts stay unchanged.
The shared domain refuses routine approval with
`artifact_routine_approval_refused` unless both kind `report` and series
`digest-daily` match, including when loading artifact frontmatter.

`office.artifact-show` takes `{"id":"art-YYYYMMDD-<slug>"}` and returns the
complete artifact metadata, its Library-relative `path`, file `name`/`path`
pairs, `series_history` newest version first, and indexed `backlinks` with
the same shape as knowledge-show. A missing artifact returns
`artifact_not_found`. Companion files are listed, never executed or sent to
a model. Series enumeration is not limited by the search result cap.

The artifact conformance goldens include successful and refused requests.
Their source task and fixed clock are fixtures: run
`cargo test -p office-tools --test artifacts` to replay both files through
public dispatch with the Orbit and Library fakes, offline and without
secrets. They require that fixture setup when used in another harness.

**Compounding.** `office.attach` can attach earlier artifacts in the same
series, or approved artifacts of the same kind, as examples. Last quarter's
approved review, or the QBR deck the user accepted unedited, becomes the
template for the next one. This is the artifacts' part in "gets better
with time".

## Search and attaching to a task

**`office.search`** (read-only, workspace) is full-text search over the
index: `{"query": "<terms>"}` plus any of `kind` (`source`, `domain`,
`runbook`, `artifact`), `template` (pages that name it), `tag`, `series`,
`since` and `until` (`YYYY-MM-DD`, inclusive), and `limit` (1–100, default
20). Each term must match, as a prefix, in a document's title, summary,
body or tags. Every filter narrows the results before the limit. A
document's date is its `occurred_at`, else `captured_at`, else
`updated_at`, else `last_verified`, else the date in a dated id
(`src-YYYYMMDD-…`, `art-YYYYMMDD-…`); a date filter excludes undated
documents. An empty or blank query, an unknown kind, a malformed date or
`since` after `until` is refused with `invalid_argument`. Hits carry `id`,
`kind`, `title`, `path` and a `snippet`.

**`office.attach`** (mutating, workspace) takes `{"task": "<id>"}` and
copies at most 8 Library files into that task as task artifacts at
`context/<library path>`. The task is read first with `orbit.task.show`;
an unknown task is refused with `task_not_found` before anything is read or
copied. Its title, description and tags drive the selection, and `text` and
`tags` in the input add to them. Picks fill the 8 slots in this order,
skipping repeats:

1. Pages by template attach policy, in the order of
   [knowledge-templates.md](./knowledge-templates.md#attach-policies):
   `always` (current pages only), `participants` (person pages whose title
   or an alias equals a participant of a `source:<id>` the task carries,
   ignoring a trailing `(Organization)`), `tagged` (pages carrying one of
   the task's tags) and `matched` (the glossary, when a `### Term` heading
   appears as a whole word in the source or the task text). Retired pages
   are never picked.
2. Runbooks the task names: `runbook:<id>` or `rb-…` tags, or `rb-…` ids in
   its text.
3. The task's sources (`source:<id>` tags).
4. Example artifacts in each `series:<name>` the task carries, newest first.
5. Index search hits for the task's title, then documents carrying each of
   its tags.
6. Pages `related` to a page already picked.

Search hits and related pages exclude domain pages whose template attaches
`never` (goals, rhythm). A knowledge page is copied as its model view
(`PageView::to_markdown`), so no `contact` ref value is attached; sources
and artifacts are copied as their `source.md` or `artifact.md`. `private`
documents are not attached and are listed in `omitted` with reason
`private`, as are candidates past the limit (`limit`). Page files that do
not parse are listed in `skipped`. A template that cannot be read or parsed
refuses the call with `template_invalid`.

Every file is read before the first copy, at one Library commit: attach
reads the repository's `HEAD` before and after, and refuses with
`library_changed`, copying nothing, if a write landed in between. Each copy
is staged in the Library's `.office/attach/` (never indexed or committed),
put with `orbit.task.artifact.put`, and its staging file removed
([decision](../4_decisions.md#office-attach-stages-its-copies-inside-the-library)).
A last artifact, `office-attach.json`, records the task, `library_commit`
and the attached files; the response carries the same, plus `omitted` and
`skipped`. With `dry_run: true` the call returns the selection and copies
nothing; without `task`, a dry run with `text` or `tags` previews a
selection without reading any task.

## Frictions

Frictions stay Orbit records (`orbit.friction.add` /
`orbit.friction.update`). The plugin adds a `cites` convention in the
friction body (source ids, task ids) and a `runbook` field when a runbook
fixes it. Re-implementing frictions in the plugin would duplicate a core
record.
