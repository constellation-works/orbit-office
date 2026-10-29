---
type: design
summary: "Spec: session continuity — the deterministic session brief, the append-only decision log, and the daily digest routine"
last_validated: 2026-10-27
---

# Spec: Session continuity

The orchestrator is the user's agent CLI, and its sessions end whenever the
user closes the terminal. The next session must pick up where the last one
left off without anyone writing a summary. Three pieces do that, and each
is the cheapest mechanism that can do its job
([decision](../4_decisions.md#session-continuity-from-a-brief-a-decision-log-and-a-digest-routine)):

| Piece | Written by | When | Model |
|---|---|---|---|
| **Decision log** | the agent, through `office.capture` | as decisions happen | none |
| **Handoff** | the agent, as `handoff` entries in that log | refreshed whenever the plan changes | none |
| **Session brief** | `office.session-brief`, computed on demand | at session start | none |
| **Daily digest** | `office.digest`, from the `office-daily-digest` routine | once a day | one Muse Spark call |

The handoff is never left to the end of a session, which often ends with
the terminal simply closing. The agent rewrites it whenever the plan
changes, so the latest one is always current. A session that ends abruptly
loses nothing: the log and the handoff were written as it went, and the
brief is computed from state that already exists.

## Decision log

One `conversation` source per orchestrator session ([library.md](./library.md#sources)).

- **Opened** by the first `office.capture` of a session with `kind: conversation`.
  Later entries are added with `append_to: <source-id>`.
- **Append-only.** Entries are added, never edited or removed. Each append
  is one Library commit. This is how a conversation source stays immutable
  while it is still being written: nothing already recorded changes.
- **Entry format,** one line each, anchorable as `#L<n>`:

  ```
  [2026-10-28T15:04:11-04:00] decision: Renewal deck goes to Jane by Friday.
  [2026-10-28T15:05:02-04:00] direction: "Always draft client emails, never send." (verbatim)
[2026-10-28T15:05:40-04:00] fact: "Jane Park runs procurement at Acme." (verbatim)
  [2026-10-28T15:06:40-04:00] action: Filed agent to-do <task-id> with the client-follow-up runbook.
  [2026-10-28T15:09:13-04:00] open: Which pricing tier does Acme get?
  [2026-10-28T16:20:55-04:00] resolved: #L4 — Acme stays on Pro.
  [2026-10-28T16:22:30-04:00] handoff: Next: review the renewal deck draft when the agent finishes; Jane's pricing question is answered (#L5).
  ```

  The kinds are `decision`, `direction` (the user's words, verbatim),
  `fact` (something the user stated about themselves, their people or
  their work, verbatim; what onboarding pages cite), `action` (one per thing the agent did), `open` (a question left
  unanswered), `resolved` (closes an earlier `open` by its anchor) and
  `handoff`.
- **What goes in:** the user's decisions and standing directions, and one
  line per action. Pasted external content does not: it is captured as
  its own source first, and the log cites it.

### Handoff

The handoff is the agent's note to its next session: what it was about to
do, what the user is waiting on it for, and the context the structured state
cannot show (why something is paused, what the user prefers next). It is a
`handoff` entry in the decision log, one line, citing entries and tasks by
anchor and id.

- **Rolling.** The agent appends a new `handoff` entry whenever the plan
  changes: after a decision that changes next steps, after filing or
  finishing work, and when the user signals they are done. The newest
  `handoff` entry supersedes earlier ones; nothing is edited.
- **Read first.** The next session's brief puts the newest handoff at the top
  (below), so the agent picks up the thread in its own words before reading
  the computed state.
- **Never the only record.** The brief does not depend on it: with no
  handoff, it still returns the computed sections.

## Session brief

`office.session-brief` (read_only, `mcp_scope: workspace`). The
orchestrator skill calls it first in every session. Its output is
assembled from Orbit and the Library, with no model call, so it is free,
always current and cannot invent anything:

- **Handoff:** the newest `handoff` entry of the previous session, verbatim,
  with its time. Absent when that session wrote none.
- **Since:** the time the last conversation source was appended to.
- **Waiting on you:** proposals awaiting approval, agent to-dos in `review`,
  and open `owner:you` to-dos, oldest first.
- **Agents working on:** `owner:agent` to-dos in progress, and those that
  finished or failed since then.
- **Open questions:** `open` entries in recent conversation sources that no
  `resolved` entry closes.
- **Last session:** that source's id and its last few `decision` and
  `direction` entries.
- **Latest digest:** a link to the newest `digest-daily` artifact, when one exists.
- **Onboarding:** each one-per-Library template with no page yet
  ([knowledge-templates.md](./knowledge-templates.md#onboarding-interview)).
  Absent once they all exist.

Every item carries its id and a link, so the agent cites rather than
restates. The brief is returned as data. Quoted entries are never treated
as instructions.

The built tool accepts `{}` and returns `sections` in the order above, with
stable `kind` tokens (`handoff`, `since`, `waiting_on_you`,
`agents_working_on`, `workspace_unavailable`, `open_questions`,
`last_session`, `latest_digest`, `onboarding`). Each section has `items`,
`total` and `truncated`. Handoff, Latest digest, Workspace unavailable and
Onboarding are omitted when empty; the other sections are always present.
Each item carries `id`, `link`, `text`, `time`, `entry_kind`, `status` and
`template`; fields that do not apply are null. Task items read from an
extra workspace additionally carry `workspace` (additive; absent for the
office workspace). Entry text and timestamps keep their original spelling.
Library links are relative paths with body-relative `#L<n>` anchors; task
links are `orbit://task/<id>` references resolved through
`orbit.task.show` (with the item's `workspace` selector, when present).

Call the brief before creating the current session's conversation source.
The previous session is the conversation source with the latest recorded
entry time (or `captured_at` for an empty log). Timestamp comparisons use
instants, including offsets and fractional seconds. Open questions cover
the ten most recently appended conversation sources, newest source first;
resolutions close earlier local anchors only. Last session includes the
source reference followed by its last five decisions/directions in log
order, with `total` counting the source and all matching entries and
`truncated` indicating an excerpt. The newest daily digest is selected by
approval time. Onboarding items name each missing singleton's template and
page id and link to the installed template.

The task read projects ownership tags, history and run identity explicitly
and refuses a truncated inventory. Finished work uses its transition to
`done`; failed work uses its associated run's typed delivery state and
finish time, not task update times or error prose. In-progress work is
always included. With no previous session, all recorded completions and
failures are eligible. The brief is an all-or-nothing read for the office
workspace: malformed, unreadable or unavailable evidence from it, or a
Library commit changing during assembly, refuses the call so an unknown
state cannot appear empty.

### Extra workspaces (`brief.task_workspaces`)

The Library setting `brief.task_workspaces` (in `.office/config.yaml`, or
set with `office.config-set` as `{"key":"brief.task_workspaces","value":[…]}`)
lists extra Orbit workspace selectors to merge into *Waiting on you* and
*Agents working on*. Each entry is an Orbit workspace selector: a
registered name, a logical `ws_…` id, an absolute local checkout path, or a
host-qualified federated selector (`hm_…/<workspace>`). At most 16
distinct selectors, each at most 256 bytes without whitespace. Absent or
empty means today's behavior exactly: no extra reads, no new fields.

For each selector the brief calls `orbit.task.list` (and, for failed-run
evidence, `orbit.task.show` with `field: delivery`) with `workspace` set
to that selector — the same `workspace` argument those Orbit tools accept
today for local and same-host selectors. Every merged task item carries
an additive `workspace` field naming its selector (office-workspace items
omit it), and its `orbit://task/<id>` link resolves through
`orbit.task.show` with that selector. A task id already seen (office
workspace first) is not repeated.

Extra workspaces do not use office ownership tags, so their rule is:
*Waiting on you* takes proposed proposals, any task in `review`, and
active `owner:you` tasks; *Agents working on* takes any non-proposal task
in progress, plus non-proposal tasks that finished (`done` transition) or
whose run failed since the previous session, by the same timestamps as
the office workspace.

Failure rule: an extra workspace that cannot be read — unreachable,
refused, truncated or malformed — never refuses the brief. It contributes
exactly one item to an additive `workspace_unavailable` section (present
only when non-empty, after *Agents working on*): the item's `id`, `text`
and `workspace` name the selector, its link is
`orbit://workspace/<selector>`, and its `status` is
`workspace_unavailable`. A sleeping or foreign host therefore shows as one
named line, not a blank session start. Cross-host federated selectors
depend on Orbit routing the callback to that host; where Orbit refuses
them, the same `workspace_unavailable` item reports it.

## Daily digest

The `office-daily-digest` routine (daily, seeded disabled) runs one
job (`job:office-daily-digest`) containing one inline deterministic
`plugin.tool_call` activity: `office.digest` (mutating, `mcp_scope: none`).
The routine and job are registered in `spec.definitions`; the activity names
its tool in `config.tool`, without a routine `required_tools` field. The daily
cron is `5 0 * * *`; an omitted `day` selects the previous complete UTC day
from the Clock port. An explicit `day` must be a real `YYYY-MM-DD` UTC date.
No agent loop, no crew and no Orbit task is involved.

- **Input:** the day's timestamped conversation entries (including entries
  appended to older sessions), Office tasks completed, proposals
  accepted or dismissed, and knowledge and artifacts changed.
- **One model call:** Muse Spark through the Models port, with a JSON
  Schema for the output. The call goes through the privacy routing
  ([models-and-privacy.md](./models-and-privacy.md#routing-rules)): a day
  that includes `private` material is summarized by the open-weights model,
  or, when none is configured, its private items are listed by reference
  only.
- **Output:** a `report` artifact in the series `digest-daily`, one per day,
  every line citing a task, source or proposal. A line the material does not
  support is omitted, never invented. The model selects single-line verbatim
  complete entries by evidence index; deterministic validation drops unknown
  evidence and partial excerpts that could remove negation or context.
  Knowledge pages cross only as model views, with contact values withheld. A day with no activity writes nothing.
- **Idempotency:** the artifact records `day:<YYYY-MM-DD>` and the provenance
  key `routine:office-daily-digest/<YYYY-MM-DD>` in its `task` field; that key
  references the routine, without creating an Orbit task. A rerun returns
  `already_exists` without a model call or write. The Library also refuses
  duplicate provenance keys under its writer lock. The report is `digest.md`
  beside `artifact.md`; its nonempty lines carry the evidence citations.
  The response names the artifact id and Library-relative path, or a no-write
  reason (`no_activity`, `already_exists`, `no_supported_lines`).
- **Promotion without review.** The digest is promoted directly, with
  `approved: { by: routine, at, edited: false }`. It is the one artifact that
  skips the approval step: it is internal, never leaves the machine, and
  asking the user to approve a diary entry every day would be friction for
  nothing. `by: routine` keeps it out of first-pass acceptance and out of
  the automation ratio ([analytics-and-reviews.md](./analytics-and-reviews.md)).
- **Compounding:** reviews select these reports by their `day:` tag, so a
  digest published just after midnight belongs to the day it summarizes.
  Older digests without the tag retain approval-date selection. A quarterly
  review starts from about ninety cited summaries rather than raw records.
