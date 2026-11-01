---
type: design
summary: "Spec: proposals — kinds, mechanical signals, record and lifecycle, dismissal memory"
last_validated: 2026-10-30
---

# Spec: Proposals

Proposals are how the system gets better over time without the user
designing it. Nothing becomes standing automation, and no knowledge page
changes, without the user accepting a proposal.

Source: spec §7.

## Kinds

| Kind | Effect when accepted |
|---|---|
| `auto_task` | Creates and enables an Orbit auto-task (condition- or schedule-triggered task minting) |
| `knowledge_add` | Creates a domain or runbook page from the cited evidence, starting from its template's skeleton when it has one ([knowledge-templates.md](./knowledge-templates.md)) |
| `knowledge_update` | Applies a diff to an existing page |
| `runbook_from_friction` | Writes or edits a runbook so a recurring friction stops happening |
| `retire` | Marks a page `retired` or disables an auto-task nobody uses |
| `shared_conflict` | Filed only by `office.library-sync` when the user's copy of a shared page and the shared copy both changed. Acceptance takes `choice`: `theirs` writes the shared version (keeping the user's contact and path refs); `mine` changes no page and pushes the user's version on the next sync ([shared-library.md](./shared-library.md#officelibrary-sync)) |

## Signals

These are mechanical and computed by `office.signals-scan`. They are not
left to the model's discretion.

`office.signals-scan` returns `{candidates, count}` only when its task list
(`orbit.task.list`), friction search (`orbit.search`), and Library document
reads all succeed and the task list is not truncated. The Library reads
enumerate every knowledge page (`library.knowledge_pages`) and current source
(`library.source_documents`), without search limits or snippets, and read
page backlinks (`library.backlinks`). They preserve page aliases, tags,
verification dates and outgoing citations, and source participants, tags and
full bodies. A failed read is refused: the error code is that failure's own code, and the message
names the read (`orbit.task.list`, the friction `orbit.search`, or
the corresponding Library operation). An unreadable document or invalid
metadata is refused with its path. A task list with `truncated: true` is refused with
`task_list_truncated` and is not scanned as the complete set. These reads
are fail-closed: an empty candidate list means nothing matched, not that a
source was unread (STD-02 R29, R31).

For staleness candidates, a page must be at least `thresholds.stale_days`
past `last_verified` and have a citing source dated strictly after that
verification. Source dates use the Library document date: `occurred_at`,
then `captured_at`, then legacy `updated_at` or `last_verified`, then the
date in a dated source id. An old event captured recently does not become newer
evidence. Undated sources, same-day citations, unlinked sources and `related`
links do not establish staleness. `updated_at` does not replace a page's
verification date. The candidate includes the page and newer citing source
ids for review; before proposing an update, the model must quote evidence
that contradicts or extends the page.

Direction and rhythm entries are read from the complete body, including the
`[RFC3339] direction: text` format conversation capture writes. A direction
is already recorded when `dom-standing-directions` cites its source, with or
without an anchor. A source citing that page is incoming evidence and does
not record the direction. The scan does not load auto-task usage telemetry; unused-automation facts
remain unavailable, so the unused-automation signal is planned and does not fire yet,
and cadence coverage currently uses the listed task titles.

| Signal | Condition | Proposes |
|---|---|---|
| **Repetition** | ≥3 `owner:you` tasks done in 30 days whose titles and descriptions cluster together (index similarity above a threshold, the same tags) | `auto_task` to take that work over, plus a draft runbook from how the user did it |
| **Friction recurrence** | The same friction (by text similarity or explicit link) recorded ≥2 times | `runbook_from_friction` |
| **Staleness** | A knowledge page is at least `thresholds.stale_days` past `last_verified` and has a newer citing source; before proposing an update, the model must quote evidence that contradicts or extends it | `knowledge_update` |
| **Gap** | An agent task finished without a runbook, and the user had to correct its output | `knowledge_add` (runbook) |
| **Unused automation (planned — does not fire yet)** | An auto-task has produced nothing the user opened in 30 days (no usage data is loaded, so this never fires today) | `retire` (planned) |
| **New subject** | A participant, or a `client:`/`project:`/`topic:` tag, that no page covers (by title, `aliases` or tag), seen in ≥2 sources | `knowledge_add` from the matching template |
| **Unrecorded direction** | A `direction` entry in a conversation source that `dom-standing-directions` does not cite | `knowledge_update` adding it |
| **Scribe corrections** | ≥3 appended corrections of the same choice in `thresholds.window_days` (inclusive; future entries excluded) | `knowledge_update` candidate for `dom-scribe-rules`, quoting originals and citing original/correction anchors |
| **Declared cadence** | A `rhythm` entry with a deliverable that no auto-task covers | `auto_task` for that deliverable |

The orchestrator can also propose things in conversation ("you've asked me
this three times; want an auto-task?"). These go through the same record and
approval path. The must-have signals for the event are repetition,
friction, staleness and unrecorded direction; new subject and declared
cadence are should-have.

## Record and lifecycle

- Each proposal is an Orbit task tagged `proposal` and `proposal:<kind>`, in `proposed` state. The description says what changes. Its evidence (task ids, source anchors, friction ids) goes in the body and in its Library evidence page (below). The concrete payload (an auto-task definition, a page diff) is a task artifact.
- **Evidence page by status.** The page sits in the folder for its proposal's status, named in the app's words and dated by closing time once closed, so no folder grows without bound:

  | Folder | Orbit task status | When |
  |---|---|---|
  | `library/proposals/open/<id>.md` | `proposed` | Created here by `office.proposal-create` |
  | `library/proposals/accepted/YYYY/MM/<id>.md` | `done` | Moved by `office.proposal-accept` (or reconcile), in the closing month |
  | `library/proposals/dismissed/YYYY/MM/<id>.md` | `rejected` | Moved by `office.proposal-reconcile` after a dismissal, in the rejection month |

  A task in any other status (an acceptance in progress or review, `backlog`, `archived`) keeps its page where it is. The record is the Orbit task, so the page can move: nothing links to it by path, and lookups resolve by proposal id in any status folder and in the legacy flat `library/proposals/<id>.md` layout. A closed page gains an `## Outcome` section. Each move writes the new path and removes the old one in one Library commit.
- **Onboarding set.** The proposals filed from the onboarding interview also carry the tag `onboarding`. The app shows them as one "Set up your Library" card with *Accept all* and per-page review; *Accept all* accepts each one in turn, and a refusal on one leaves the others accepted and names it.
- **Script payload contract:** `proposal.json` carries `{kind, definition}`; the definition contains the concrete `plugin.tool_call` steps. `office.script-run` verifies the complete pinned step input against a done proposal. Acceptance calls `office_tools::prepare_script` before enabling any script step; see [scripts.md](./scripts.md#implementation-contracts).
- **Accept:** the app calls `office.proposal-accept`, which applies the payload (creates and enables the auto-task, writes the page; for an auto-task that runs a script, it first fetches the script's dependencies and refuses the acceptance if they do not resolve, [scripts.md](./scripts.md#dependencies-are-fetched-once-at-approval)), closes the task as done with the change recorded, and commits the Library. Its evidence page moves to `accepted/YYYY/MM/` with the acceptance summary as its outcome, in the commit that records the acceptance. `proposal-accept` is `mcp_scope: none` and not `agent_callable`, so no agent can call it: it is Operator/Runner only ([plugin-surface.md](./plugin-surface.md#agent-authority-inventory)).
- **App authority:** the app is meant to be the one non-operator caller, acting on the user's click. Orbit now provides a scoped, audited application identity for exactly this ([ORB-20362] done), but the app is not wired to it yet: the wiring task is blocked because the shipped consent contract cannot carry the app's per-proposal calls, so a decision is needed first ([ORB-20363] blocked). Until that wiring lands, the installed app runs with no `ORBIT_OPERATOR` and holds only Orbit's default capability, so Orbit still refuses its accept, accept-all, dismiss, send, approve and send-back calls with `capability_denied` (or `policy_denied`), and nothing changes. The server reports that refusal as the reason `not_permitted` on every approval route, and Today says plainly that the app isn't allowed to do this yet and that trying again won't help, rather than "Please try again". It will not grant itself Operator, nor mark the tool `agent_callable` (that would admit agents). An operator accepts from the office workspace with `ORBIT_OPERATOR=1 orbit tool run office.proposal-accept --input '{"id":"<id>","sha256":"<digest>"}'`; see the [decision](../4_decisions.md#the-app-reports-missing-approval-authority-instead-of-granting-itself-operator).
- **Dismiss:** closes it as rejected with an optional reason. The app then calls `office.proposal-reconcile`, which moves the page to `dismissed/YYYY/MM/` in one commit; the reason stays in the task's history. A failed move leaves the dismissal standing until the next reconcile.
- **Reconcile:** `office.proposal-reconcile` (`{}`, mutating, `mcp_scope: none`) lists the proposal tasks (`orbit.task.list`, tag `proposal`, up to 1000, refused as `proposal_history_incomplete` when truncated) and moves every page whose folder disagrees with its task's status, so a proposal closed outside the app (an operator's CLI) catches up and legacy flat pages migrate: to `open/` while waiting (or in another non-closed status), to `accepted/` or `dismissed/` under the month of the task's `updated_at`. A legacy acceptance receipt `<id>-accepted.md` is folded into the accepted page's outcome. A page already in its folder is left alone, so a re-run moves nothing and commits nothing. Pages with no proposal task are reported as `unknown` and left in place. It returns `{moved: [{id, from, to}], unknown}`. The app runs it after a dismissal; an operator runs `orbit tool run office.proposal-reconcile --input '{}'` to migrate a Library or repair it after closing proposals outside the app.
- **Never auto-approved.** Orbit only approves a `proposed` task automatically when a drain is started with `--approve-proposed`. The office workspace runs no drain, and the dispatch pass submits `backlog` tasks only, so a proposal changes state only when the user accepts or dismisses it.

## Dismissal memory

A dismissed proposal suppresses another of the same kind on the same
evidence cluster for 30 days. The orchestrator says so when asked ("you
dismissed this on Oct 29").


## Tool payloads and review safety

`office.proposal-create` takes `title`, `description`, nonempty `evidence`,
`payload`, and optional `tags`. It uses Orbit task creation's `proposed`
default (there is no status override), preserves additional tags such as
`onboarding`, and attaches `proposal.json`. Its response is
`{created, id, suppressed, evidence_page, payload_sha256}`, where `evidence_page` is `proposals/open/<id>.md`; a recently rejected equivalent
returns `created: false`, `suppressed: true`, its prior task id and a null
page and digest. Dismissal history must be complete and readable. Equivalence is the
kind plus the sorted, deduplicated evidence-reference set; rejection's
`updated_at` supplies the dismissal time, and suppression expires at exactly
30 elapsed days. A later edit to a rejected task restarts this window.

Payloads are closed tagged objects:

- `auto_task`: `definition` has `name`, `schedule` (five-field numeric `cron`
  or positive `every_minutes`), the Orbit `template`, optional `description`,
  `dedupe` and `steps`. Each step is `{kind: "plugin.tool_call", tool, input}`.
  Auto-tasks mint tasks; they do not execute a job. Office puts the exact
  steps into the minted task's instructions and required tools. Hidden tools
  such as `office.script-run` still require a host-authorized deterministic
  step; this payload does not grant agent MCP access to them.
- `knowledge_add` and `runbook_from_friction`: exactly one of `page` or
  `markdown`. A draft `page` has `id`, `title`, optional `template`, `cites`,
  and either free-form `body` or template `sections` keyed by heading.
  Creation preserves the template skeleton, replaces supplied sections,
  incorporates Library evidence citations and materializes the result into
  `markdown` in the artifact. Source citations require resolvable anchors.
  Acceptance uses those reviewed bytes, then checks the current page rules.
  Runbooks require a runbook id. Existing pages are refused for additions.
- `knowledge_update`: complete replacement `markdown` and `base_sha256` of
  the reviewed current page. Replacement is refused if that revision changed.
- `retire`: `target` is `{type: "auto_task", name}` or
  `{type: "page", id, base_sha256}`. It disables that auto-task or marks the
  reviewed page retired while preserving its content.

`office.proposal-accept` takes `id` and `sha256`, the SHA-256 of the exact
`proposal.json` bytes the user reviewed, plus `choice` (`theirs` or `mine`),
which a `shared_conflict` requires and every other kind refuses
(`invalid_argument`). A `shared_conflict` payload is `{kind, page, path, blob,
markdown, base_sha256}`: the page id, its shared path and blob, the localized
shared version and the digest of the user's copy it was computed against.
**Use theirs** checks that digest and writes `markdown`; **Keep mine** checks
only that the user's own copy exists, records the blob in the sync state and
returns `{page, kept: "mine"}`. `office.proposal-create` refuses the kind. Creation returns this digest; a changed
payload is refused before preparation or mutation. The digest is recorded in
the acceptance summary, and script execution verifies it before trusting a
new approval. Legacy approvals without a digest remain readable. Only a task still `proposed` with
matching proposal tags can be accepted. Payload/page checks and every pinned
script's `prepare_script` run before the atomic Orbit start transition reserves
acceptance. This transition goes directly to `in-progress`, avoiding a
scheduler-visible backlog slot. A second start is refused. Auto-task acceptance
uses `orbit.auto_task.add` (which creates an enabled definition) followed by
an explicit `orbit.auto_task.update` with `enabled: true`; preparation precedes
both. Retirement uses only update with `enabled: false`. No callback is retried
on an unknown outcome. Knowledge publication compares the reviewed revision
under the Library writer lock, including an absence check for a new page.
The Library moves the evidence page to `proposals/accepted/YYYY/MM/<id>.md`
with the acceptance summary (the receipt: payload digest and applied change)
as its `## Outcome`, in one commit, and the task records the change through
`review` to `done` with an execution summary. Before pages moved, the receipt
was a separate `proposals/<id>-accepted.md`; reconcile folds it into the page.

A failed callback after reservation can leave partial effects. The proposal
stays in progress or review and acceptance refuses it; inspect its payload,
receipt, task history and auto-task before any manual repair. Creation can also
leave a proposed task with a missing artifact or evidence page after a callback
failure. It is not reported as a successful proposal. Staged artifact bytes
are retained on failure. The cross-store limitation is recorded in
[the decisions](../4_decisions.md#proposal-acceptance-and-orbit-auto-tasks).

## Shipped maintenance definitions

The disabled daily `office-signals` routine targets the shipped
`job:office-signals`, whose single deterministic `plugin.tool_call` invokes
`office.signals-scan` with `{}`. The job reports candidates; it creates or
approves no proposals. The disabled weekly `office-knowledge-maintenance`
auto-task mints agent review work that may file knowledge proposals and leaves
them proposed. All three are registered under `spec.definitions`. Installing
these definitions does not enable either schedule.

## Rolling verification (`office-knowledge-verify`)

The staleness signal above is passive: it needs a newer citing source, so a
page whose world changed without one arriving is never flagged. The disabled
daily `office-knowledge-verify` auto-task
(`plugin/.orbit-plugin/definitions/auto_tasks/office-knowledge-verify.yaml`,
`enabled: false`, template complexity `low`) closes that gap by checking
pages against what a run can actually observe, whether or not a new source
exists. Its definition file ships with this spec; registering it in
`plugin.yaml` under `spec.definitions.auto_tasks` is integrator-owned and a
separate task, so the file alone loads and enables nothing.

**Batch selection.** Each run verifies a bounded batch of 6 pages. The agent
lists the corpus with `office.knowledge-list`, reads each page with
`office.knowledge-show` for its `last_verified`, `status` and `sensitivity`,
excludes retired pages, and orders the rest with pages that have no
`last_verified` first, then by oldest `last_verified`, breaking ties by page
id. The first 6 form the batch, so never-verified pages are checked before
any page is re-checked.

**Checks and verdicts.** For each page the agent extracts its checkable
claims — file paths, commands, tool names, config keys, Orbit task or crew
state, and statements its cited sources make — and checks each against what
the run can observe (the workspace, `orbit.task.show`/`orbit.task.list`/
`orbit.search`, and `office.source-show`/`office.knowledge-show` for cited
sources), recording the evidence (the command and its outcome, or the cited
anchor) behind every check. The per-page verdict is `drifted` when any
checked claim is contradicted, else `unverifiable` when any checkable claim
could not be checked or the page has no checkable claim, else `current` —
only when every checkable claim was checked and agreed. A private page,
whose body `office.knowledge-show` withholds (`content_withheld`), is always
`unverifiable` and is never proposed against; its `sha256` is still returned,
since a digest names the revision without its text.

**Drifted pages.** A drifted page gets one `knowledge_update` proposal whose
`base_sha256` pins the page's `sha256` from `office.knowledge-show` — the
SHA-256 of the page file's exact bytes that `office.proposal-create` checks,
so a page that moved since it was shown refuses creation with
`proposal_revision_changed`. The proposal's replacement markdown corrects
each contradicted claim to match the evidence and marks a superseded
decision superseded rather than deleting it; its description and evidence
quote the contradicting evidence verbatim. Dismissal memory applies as for
any proposal: a suppressed creation is recorded, not refiled.

**How the verification stamp lands.** A `current` page's verification is
recorded as a stamp-only `knowledge_update` proposal, pinned to the same
`sha256`: the replacement markdown is the shown page unchanged except
`last_verified` set to the run date and `status: current`. It is reviewed
like any other proposal and lands only when the user accepts it, applying
the stamp in the acceptance commit — agents never write a Library page
directly, and an unearned stamp is impossible because `current` requires
every checkable claim to have been checked. Until acceptance the page keeps
its old `last_verified` and stays in the rotation. An `unverifiable` page
gets no proposal and no stamp; it stays in the rotation.

**Run report.** Every run attaches `knowledge-verify-report.json` to its
task: one entry per page in the batch, in batch order, with the page id, its
verdict, the evidence or reason behind it, and the proposal id (or the
suppressed or refused outcome) when one was filed. What was left
unverifiable, and why, is in the report and nowhere else.

## Decision — 2026-10-27

Daniel decided that recurring proposals become auto-task proposals. Orbit has
no routine-create callback, and routine control only changes existing
operator-owned definitions. The proposal kind set therefore has no `routine`;
repetition proposes `auto_task`, acceptance creates an auto-task, and unused
automation retirement disables an auto-task. Shipped disabled routine/job
pairs are plugin definitions rather than proposal kinds.

### Sharing approval on knowledge proposals

`knowledge_add` and `knowledge_update` payloads accept optional boolean `shareable`, applied to the materialized Markdown at creation and acceptance. A `knowledge_add` page draft also accepts optional boolean `shareable` (absent false); creation materializes it into reviewed Markdown. Add and update Markdown may carry the same field. Acceptance rechecks the private-document and own-citation sharing rules before reserving the proposal; publication rechecks current cited files under the writer lock. A refusal writes no page or acceptance receipt. See [sharing approval](./shared-library.md#the-shareable-field).

## Task References

- [ORB-20362] — landed Orbit's scoped, audited application authority for user-approved tool calls.
- [ORB-20363] — wires the office app's approval actions to that scoped application authority.

> Resolve any task above with `orbit task show <ID>` or `git log --grep=<ID>`.
