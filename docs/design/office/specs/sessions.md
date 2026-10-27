---
type: design
summary: "Spec: session continuity — the deterministic session brief, the append-only decision log, and the daily digest routine"
last_validated: 2026-10-26
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
| **Session brief** | `office.session.brief`, computed on demand | at session start | none |
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
  [2026-10-28T15:06:40-04:00] action: Filed agent to-do <task-id> with the client-follow-up runbook.
  [2026-10-28T15:09:13-04:00] open: Which pricing tier does Acme get?
  [2026-10-28T16:20:55-04:00] resolved: #L4 — Acme stays on Pro.
  [2026-10-28T16:22:30-04:00] handoff: Next: review the renewal deck draft when the agent finishes; Jane's pricing question is answered (#L5).
  ```

  The kinds are `decision`, `direction` (the user's words, verbatim),
  `action` (one per thing the agent did), `open` (a question left
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

`office.session.brief` (read_only, `mcp_scope: workspace`). The
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

Every item carries its id and a link, so the agent cites rather than
restates. The brief is returned as data. Quoted entries are never treated
as instructions.

## Daily digest

The `office-daily-digest` routine (daily, seeded disabled) runs one
deterministic `plugin.tool_call` step: `office.digest` (mutating,
`mcp_scope: none`). No agent loop, no crew and no Orbit task is involved.

- **Input:** the day's conversation sources, tasks completed, proposals
  accepted or dismissed, and knowledge and artifacts changed.
- **One model call:** Muse Spark through the Models port, with a JSON
  Schema for the output. The call goes through the privacy routing
  ([models-and-privacy.md](./models-and-privacy.md#routing-rules)): a day
  that includes `private` material is summarized by the open-weights model,
  or, when none is configured, its private items are listed by reference
  only.
- **Output:** a `report` artifact in the series `digest-daily`, one per day,
  every line citing a task, source or proposal. A line the material does not
  support is omitted, never invented. A day with no activity writes nothing.
- **Promotion without review.** The digest is promoted directly, with
  `approved: { by: routine, at, edited: false }`. It is the one artifact that
  skips the approval step: it is internal, never leaves the machine, and
  asking the user to approve a diary entry every day would be friction for
  nothing. `by: routine` keeps it out of first-pass acceptance and out of
  the automation ratio ([analytics-and-reviews.md](./analytics-and-reviews.md)).
- **Compounding:** reviews draw on the digests of their period, so a
  quarterly review starts from about ninety cited summaries rather than raw
  records.
