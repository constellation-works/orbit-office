---
type: design
summary: "Spec: Library — layout, sources, knowledge pages, artifacts and frictions"
last_validated: 2026-10-26
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
  .gitignore                   # original.* (binaries are pinned by sha256, not tracked)
```

- The index in `plugin_state/index.sqlite` (FTS5 plus link tables) is
  rebuildable at any time with `office.reindex`.
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
   (`{{workspace}}/library`), or if `git` is missing.
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

It is idempotent: it creates only what is missing and never overwrites an
existing config, `.gitignore`, template or page. A Library is complete only
when its layout, repository, config, `.gitignore`, index and every bundled
template file are present. A second run on a complete Library changes
nothing and reports `already_initialized`. Copying a missing bundled
template lists that file in `created` and does not report
`already_initialized`. The result lists what was created.

Other tools open the store, which still creates the layout directories
(`LibraryStore::open`, a safety net), but they never write the config,
run `git init` or commit outside their own change. A tool that finds the
Library uninitialized (no repository or no config) refuses with
`library_not_initialized` and names `office.init`.

Demo history goes in after `office.init`, through `office.capture` and
the other tools rather than by copying files, so the seeded Library has
real commits, a real index and real provenance.

## Sources

**Id:** `src-YYYYMMDD-<slug>`, made unique with a numeric suffix.

**`source.md` frontmatter:**

```yaml
id: src-20261028-acme-qbr
kind: meeting            # meeting | chat | email | doc | receipt | bill | conversation | note
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

- **Meeting:** one paragraph per speaker turn, prefixed with `[hh:mm:ss] Speaker:`. The anchor is `#t=00:12:31`.
- **Chat:** one line per message, `[timestamp] Author: text`. The anchor is `#L14-20`.
- **Email, doc:** paragraphs. The anchor is `#L…`.
- **Receipt, bill:** extracted fields as a YAML block plus the raw OCR text.
- **Conversation:** the decision log of one orchestrator session: one timestamped line per decision, direction (verbatim), fact the user stated, action, open question, resolution or handoff, anchored `#L<n>`. It is append-only (`office.capture` with `append_to`), so nothing already recorded changes. The agent CLI's own transcript belongs to that CLI, so the skill has the agent write this log itself ([sessions.md](./sessions.md#decision-log)).

**Rules:**

- **Immutable.** A correction writes `v2.md` with `supersedes` set. Citations name a version when it matters, otherwise the latest.
- **Dedupe.** If the `sha256` of the original, or of the normalized pasted text, matches an existing source, capture returns that source instead of creating a new one.
- **Paste capture.** When the user pastes external content into the orchestrator session (a Slack thread, an email), the skill has the agent call `office.capture` *before* acting on it, then cite the new source id in its reply. Short directives typed by the user are not captured separately: they go in the `conversation` source.
- **Conversation sources.** The skill has the agent open one `conversation` source per session and append to it whenever the user makes a decision or gives a standing direction, and for each action it takes. Instead of an end-of-session summary, the agent keeps a rolling `handoff` entry current, and the next session starts from `office.session.brief`, which leads with that handoff ([sessions.md](./sessions.md)). The user's directions are decisions, and the review needs them.
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

- Claims in the body carry inline citations, for example `[src-…#t=00:12:31]`. The app renders them as links that open the source at the anchor.
- **Runbooks** add `inputs`, `steps` and `done_when` sections. These are what an implementing agent follows.
- **Templates.** A page may name the template it follows (`template: person`). Templates fix its id pattern and required sections, and decide when `office.attach` includes it. The bundled set and the onboarding interview that writes the first pages are in [knowledge-templates.md](./knowledge-templates.md).
- **Pointers** (files, links, contacts, templates, accounts) live in a domain page's `refs`, on the page about their subject, for example Jane Park's email on `dom-person-jane-park`. Each ref has one `path`, `url`, `contact`, `account` (`system:key`, such as `jira:ACME`) or `artifact` target. Never secrets. Contact values never reach a model ([knowledge-templates.md](./knowledge-templates.md#contact-details)).
- Knowledge may cite artifacts as well as sources, for example a research report's findings folded into a domain page. The artifact's own `cites` keep the chain back to evidence.
- **Writes** go through `office.knowledge.*` tools, which validate frontmatter, refuse a claim without `cites` (except user-authored pages), and commit. The write tool is hidden from MCP; see [plugin-surface.md](./plugin-surface.md).

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
3. **Promote.** Approval calls `office.artifact.promote`. It copies the task's output into `library/artifacts/…`, writes the frontmatter (task, producer, cites, whether the user edited it), indexes it and commits. Output that is rejected or never approved stays only on the Orbit task.
4. **Revise.** A re-run of the same deliverable (next week's report, a corrected draft) promotes a new artifact in the same `series` with `supersedes` set. Approved artifacts are never edited in place.

**Rules by kind:**

- **`draft`** (an email or message for someone else) records `sent_at` once the user sends it. In the hackathon build, sending copies the draft to the clipboard.
- **`script`** is never run because it was promoted. A script runs only as a step of a routine or auto-task whose accepted proposal named it. That step pins the artifact version by `sha256` and runs inside Orbit's sandbox. That step calls `office.script.run`, which runs it with the install's pinned Python in its own run directory, offline, with dependencies fetched once at approval, and files its outputs for review ([scripts.md](./scripts.md)). Changing the script means a new version and a new proposal.
- **`review`** holds the generated reviews ([analytics-and-reviews.md](./analytics-and-reviews.md)).
- **`report`** in the series `digest-daily` is the daily digest, the one artifact promoted without review (`approved.by: routine`; [sessions.md](./sessions.md#daily-digest)).
- **`data`** (csv, json) records its schema in the summary, so later tasks can use it without opening it.

**Compounding.** `office.attach` can attach earlier artifacts in the same
series, or approved artifacts of the same kind, as examples. Last quarter's
approved review, or the QBR deck the user accepted unedited, becomes the
template for the next one. This is the artifacts' part in "gets better
with time".

## Frictions

Frictions stay Orbit records (`orbit.friction.add` /
`orbit.friction.update`). The plugin adds a `cites` convention in the
friction body (source ids, task ids) and a `runbook` field when a runbook
fixes it. Re-implementing frictions in the plugin would duplicate a core
record.
