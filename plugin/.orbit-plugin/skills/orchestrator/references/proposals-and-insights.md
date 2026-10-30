# Proposals and insights

How to find, file and talk about proposals, record snags, and report the numbers honestly.

Nothing becomes standing automation, and no knowledge page changes, until the
user accepts a proposal in the app. You find candidates and file them. You
never accept, enable or write. For the tool list and guardrails see
[tools-and-guardrails.md](tools-and-guardrails.md). For citations see
[library.md](library.md).

## When to scan

Call `office.signals-scan` after a capture or after a batch of completed work.
It takes no input (`{}`). It returns `{candidates, count}`. Each candidate has
`signal`, `kind`, `summary` and `evidence` (task, source, page or friction ids).

The scan is mechanical and fail-closed. If a read fails or the task list is
truncated, it refuses with a named error (for example `task_list_truncated`).
Report the refusal. Do not treat it as "nothing found". An empty list means
nothing matched.

A candidate is a lead, not a proposal. Read its evidence before you file.

## The signals

Thresholds live in Library settings (`thresholds.*`). Defaults are shown.

| Signal | Fires when | Suggests |
|---|---|---|
| `repetition` | 3+ `owner:you` tasks done in 30 days that share tags or wording | `auto_task`, plus a runbook draft from how the user did it |
| `friction_recurrence` | the same friction recorded 2+ times (similar text) | `runbook_from_friction` |
| `staleness` | a page is 90+ days past `last_verified` and a citing source is dated after that | `knowledge_update` |
| `gap` | a done agent task tagged `corrected` with no runbook | `knowledge_add` (a runbook) |
| `new_subject` | a participant or `client:`/`project:`/`topic:` tag no page covers, seen in 2+ sources | `knowledge_add` from the matching template |
| `unrecorded_direction` | a `direction` entry that `dom-standing-directions` does not cite | `knowledge_update` adding it |
| `declared_cadence` | a `rhythm` entry with a deliverable no auto-task covers | `auto_task` for it |
| `unused_automation` (planned — does not fire yet) | an auto-task nobody opened output from in 30 days (no usage data is loaded) | `retire` (planned) |

Notes:

- `unused_automation` is planned and does not fire yet. The scan loads no auto-task usage
  data. Do not tell the user an automation is unused unless they say so.
- For `staleness`, quote the newer evidence that contradicts or extends the
  page before you propose an update. A newer source alone is not enough.
- You can also propose from conversation ("you've asked for this three
  times; want me to set it up?"). It goes through the same tool and approval.

## Shipped routines and auto-tasks

These come with the plugin. You do not propose them; they already exist.
Every definition is seeded disabled. On a new office workspace,
`orbit-office init` applies the office preset, which turns on three routines:
`office-daily-digest`, `office-kt-inbox` and `office-signals`. The user can
change any switch on the Routines screen. Do not toggle them yourself; send
the user to Routines.

| Name | Type | Schedule | What it does | On in a new workspace |
|---|---|---|---|---|
| `office-signals` | routine | daily 00:15 | runs `office.signals-scan`; files nothing, approves nothing | yes |
| `office-daily-digest` | routine | daily 00:05 | writes the previous UTC day's cited digest (one model call) | yes |
| `office-kt-inbox` | routine | every 15 min | processes confirmed KT video plans ([packs.md](packs.md)) | yes |
| `office-bootstrap` | routine | every 15 min | runs confirmed Library bootstrap batches into proposals | no |
| `office-meeting-inbox` | routine | every 15 min | summarizes captured meetings ([packs.md](packs.md)) | no |
| `office-calendar-sync` | routine | every 15 min | refreshes the read-only calendar snapshot | no |
| `office-meeting-prep` | routine | every 15 min | writes cited briefs for upcoming meetings | no |
| `office-shared-sync` | routine | hourly | pulls and pushes the team's shared library | no |
| `office-knowledge-maintenance` | auto-task | Mondays 00:30 | mints an agent to-do to review signals and file evidenced knowledge proposals | no |
| `office-review` | auto-task | 1 Jan/Apr/Jul/Oct 00:05 | mints an agent to-do to write the quarterly (and in January, yearly) review | no |

Routines run fixed tool steps with no agent. Auto-tasks mint a to-do that an
agent does; both auto-tasks use `dedupe: skip_if_open` and leave all output
waiting for the user.

## Proposal kinds

`office.proposal-create` accepts exactly these payload kinds:

- `auto_task`: a new auto-task. `definition` needs `name` (lowercase, `-`/`_`),
  `schedule` (`{cron}` or `{every_minutes}`) and `template` (at least `title`).
  Optional `description`, `dedupe` (`skip_if_open` or `always`) and `steps`
  (`{kind: "plugin.tool_call", tool, input}`). Auto-tasks mint tasks; steps
  become the minted task's instructions and required tools.
- `knowledge_add`: a new page. Give a `page` draft (`id`, `title`, optional
  `template`, `cites`, and `body` or template `sections`) or full `markdown`.
  An existing page id is refused; use an update.
- `knowledge_update`: complete replacement `markdown` plus `base_sha256` of the
  current page you read. A changed page is refused at acceptance.
- `runbook_from_friction`: a runbook page (`page` or `markdown`) that stops a
  recurring snag. It needs a runbook id.
- `retire`: `target` is `{type: "auto_task", name}` or
  `{type: "page", id, base_sha256}`.

There is no `routine` kind. Orbit cannot create routines from a proposal, so
recurring work is always proposed as an `auto_task`. `shared_conflict`
proposals are filed only by the shared-library sync; you cannot file one.

`knowledge_add` and `knowledge_update` take an optional `shareable` flag. Set
it only when the user asked for the page to be shared.

Build page content with the `office-library-conventions` skill: templates,
page rules, the one-new-page-per-three-sources limit, contact refs. Do not
restate those rules here.

## Evidence and filing

`evidence` is required and must be non-empty: task ids, source anchors
(`<source-id>#L<n>`, `#t=hh:mm:ss`), page ids or friction ids. Cite what the
payload claims. Source citations in a page draft must resolve.

```json
{"title":"Weekly numbers email as an auto-task","description":"You sent the Friday numbers by hand three weeks running. This drafts it every Friday at 09:00 for your review.","evidence":["<task-id-1>","<task-id-2>","<task-id-3>"],"payload":{"kind":"auto_task","definition":{"name":"weekly-numbers","schedule":{"cron":"0 9 * * 5"},"template":{"title":"Draft the weekly numbers email","tags":["owner:agent","no-diff-expected"]}}}}
```

The tool files an Orbit task in `proposed`, tagged `proposal`,
`proposal:<kind>` and an evidence-cluster tag, plus any tags you pass. It
attaches `proposal.json` and writes `library/proposals/open/<id>.md`. It returns
`{created, id, suppressed, evidence_page, payload_sha256}`.

Tag onboarding proposals `onboarding`. The app shows them as one
"Set up your Library" card.

## Mention at most two per turn

Mention at most **two new** proposals per conversation turn. Pick the two with
the strongest evidence. Keep the rest for later turns. The onboarding set is
the exception: mention it once, as one group.

Say what changes and why, in plain words, with one citation. For example:
"I've put a suggestion in Needs you: draft the weekly numbers every Friday at
9, because you did it by hand three times this month." Do not say "proposal
task", "auto-task", ids or statuses in prose. The user sees Needs you and
Routines.

## Dismissal memory

If the user dismissed a proposal of the same kind on the same evidence in the
last 30 days, `proposal-create` returns `created: false`, `suppressed: true`
and the earlier task id. Nothing is filed. Do not reword or reshuffle the
evidence to get around it. If the user asks, say when they dismissed it: read
that task with `orbit.task.show` ("you dismissed this on 29 October"). The
window restarts if the dismissed task is edited.

A refusal `proposal_history_incomplete` means the dismissal history could not
be read in full. Report it; do not retry in a loop.

## Acceptance belongs to the user

`office.proposal-accept` is hidden from you (`mcp_scope: none`). Only the app
calls it, when the user presses Accept in Needs you. Acceptance pins the exact
`proposal.json` digest the user saw. Dismiss rejects the task. Never move a
proposal's status yourself, and never mark it done.

When the user asks you to change a page or set up something recurring, file
the proposal and tell them it is waiting in Needs you. That is the whole path.

## Recording snags (frictions)

Record a friction with `orbit.friction.add` when something went wrong or
slowed the work: a missing input that kept coming back, a refused tool, a
runbook step that did not match reality, a correction the user had to make.
Do not record routine questions or the user's preferences; those are `fact`
or `direction` log entries ([sessions.md](sessions.md)).

- Give a one-line `title` that names the surface and the failure, and a
  `body` saying what happened. Pass `model` and the `workspace` selector;
  pass `during_task` when it happened on a to-do.
- Put a `cites:` line in the body with the source and task ids involved.
  When a runbook later fixes it, add a `runbook:` line naming that runbook
  (with `orbit.friction.update`).
- Describe a repeat snag in the same words as last time. Recurrence is found
  by text similarity, so consistent wording lets the signal fire.

## Insights: `office.metrics`

`office.metrics` takes exactly `since` and `until` (`YYYY-MM-DD`, UTC,
`until` exclusive, at most 3,660 days).

```json
{"since":"2026-10-01","until":"2027-01-01"}
```

It returns `totals` and Monday-start `weeks`, each with:

- `automation`: `automated_count`, `manual_count`, `total_count`, the same in
  minutes, and `count_ratio` / `minutes_ratio`. This is the headline: agent
  and routine work as a share of all completed work. The daily digest counts
  as neither.
- `hours_returned`: estimated hours of automated work.
- `conversions`: work clusters that moved from manual to automated, each with
  `cluster`, `proposal` and `manual_tasks`. A conversion needs an explicit
  shared `cluster:<id>` tag; similar titles do not count.
- `first_pass_acceptance`, `knowledge_coverage`, `proposal_acceptance`:
  numerator, denominator and `ratio`.
- `open_load`: the user's open to-dos with `age_days`.
- `simulated_until`: present only when the Library holds seeded demo history.

Report numbers honestly:

- Quote figures exactly as returned. Never estimate counts, minutes or ratios.
- A `null` ratio means there was nothing to measure. Say "no data yet", not 0%.
- Minutes are estimates, from `estimate-minutes:<n>` tags or fixed defaults by
  task type. Say "about" and "estimated".
- When `simulated_until` is set, say that weeks up to that date are simulated
  demo history. Never present them as real work.
- A refusal (unreadable record, truncated list, over 10,000 records) means no
  numbers. Say so and do not fill in.

## Reviews

The `office-review` auto-task, when enabled, mints an agent to-do at the start
of each quarter. The agent calls `office.review-inputs` with the same
`since`/`until` shape. It returns `completed_tasks`, `accepted_proposals`,
`metrics`, `outputs`, `conversation_sources`, `daily_digests` and
`knowledge_changes`.

The review has these sections: Highlights, Work by area, What I automated,
Decisions, Snags and fixes, Next period. Every factual line cites a task,
output, proposal, source anchor or knowledge revision; unsupported lines and
empty sections are left out. It is attached as `review.md` with a manifest
(kind `review`, series `review-quarterly` or `review-yearly`). In January a
second, yearly review is written. It waits in Needs you; approving it is the
user's action.

If the user asks for a review now, you may call `office.review-inputs` and
answer from it with citations, or file an agent to-do that follows the same
rules ([tasks.md](tasks.md)). Either way, never publish or send it.
