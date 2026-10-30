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
| **Decision log** | transcript extractor, through `office.capture` | SessionEnd; sweep recovers misses | configured low-complexity pool |
| **Handoff** | the agent, as `handoff` entries in that log | refreshed whenever the plan changes | none |
| **Session brief** | `office.session-brief`, computed on demand | at session start | none |
| **Daily digest** | `office.digest`, from the `office-daily-digest` routine | once a day | one Muse Spark call |

The handoff is never left to the end of a session, which often ends with
the terminal simply closing. The agent rewrites it whenever the plan
changes, so the latest one is always current. A session that ends abruptly
retains its last handoff. The recovery sweep extracts its available transcript;
until extraction finishes, the brief has the existing evidence, not a promise
that the last session has already been processed.

## Decision log

One `conversation` source per orchestrator session ([library.md](./library.md#sources)).

- **Opened** by `office.capture` with `session_transcript_path` and `session_id`.
  The session-end extractor adds evidence to that same source. The orchestrator
  may open it earlier when writing a rolling handoff, then append handoffs with
  `append_to: <source-id>`. Legacy manual sources still work unchanged.
- **Append-only.** Entries are added, never edited or removed. Each append
  is one Library commit. Changed transcripts append new evidence and preserve
  prior lines and anchors; they also bump version and snapshot the previous
  source, as described below.
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

## Transcript extraction

The built `office.capture` tool has a session mode separate from uploads and
WebVTT's `transcript_path`. No new Orbit-core service is needed. Its inputs
cannot be mixed with other capture fields:

```json
{"session_transcript_path":".orbit/tmp/session-transcripts/session.jsonl","session_id":"<session-id>","transcript_preview":true}
```

Use an absolute staged path in actual calls. Preview returns `session_id`,
`orchestrator`, `model`, `transcript_sha256`, `sensitivity`, `content_withheld`
and numbered `lines` (`line`, `role`, `timestamp`, `text`). It creates no source
and takes only a shared read lock. A cheap crew selects relevant evidence:

```json
{"session_transcript_path":".orbit/tmp/session-transcripts/session.jsonl","session_id":"<session-id>","transcript_sha256":"<preview-sha256>","selections":[{"line":0,"kind":"direction"},{"line":2,"kind":"open"}]}
```

The adapter copies the selected text, never model-authored replacement words.
A stale digest, duplicate/out-of-range line or unsupported kind refuses the
whole write. Selection order is normalized to transcript order. Without
`selections`, capture conservatively classifies user lines and explicit
assistant `Action:` lines locally. This also permits private sessions to be
captured without exposing their text to an agent crew. It does not call a
hosted inference API. Empty selections are valid when no evidence is relevant.

### Adapter and provenance

The first adapter accepts complete UTF-8 Claude Code JSONL up to 8 MiB.
`type: user/assistant`, matching `message.role`, `sessionId` and RFC 3339
`timestamp` are required for conversational records. Text strings and text
content blocks are read; tool input/results, thinking, sidechains, metadata,
compaction summaries, fenced code and blockquotes are excluded. Other record
types are ignored. Mixed-session files, unsupported provider formats, truncated
records and missing timestamps refuse without writing. Stage the final file
atomically after the provider closes it; a growing file may require a retry.

The adapter records `orchestrator: claude`, the last non-synthetic assistant
`message.model` (exact spelling, null if absent), and `session_id` in source
frontmatter. The provider comes from the validated record format; these fields
are transcript claims, not authentication. `captured_by: transcript-extractor`
identifies the writer and is separate from who held the conversation.
Legacy sources may omit these three fields or use null; the plugin validates
nonnull values as nonempty single-line strings, bounded to 256 bytes.

Codex rollout JSONL and other providers need their own adapter mapping their
session metadata, model context and message records to the same numbered text
line representation. They are refused today, rather than guessed as Claude.
Adding an adapter must keep tool blocks, synthetic context and provider
instructions out of user evidence. Sessions that change model record the last
reported conversational model; the transcript digest identifies the input.

### Decision guard and split of work

The cheap crew chooses `direction`, `decision`, `fact`, `action` and `open`.
Only user lines can be directions/decisions. The deterministic guard also
requires an explicit cue: `Decision:`, `I decided`, `We decided`, `I choose`
for decisions; `Direction:`, `Always`, `Never`, `Please`, `We need to`,
`I want you to` for directions (case insensitive). Questions and uncertainty
such as maybe, perhaps, thinking, might, could, consider or conditional `if`
become `open` even if the model calls them decisions. Other formulations stay
open until the user clarifies; this intentionally favors false negatives over
invented consent. Facts must be unambiguous user statements. Assistant action
lines are reported actions, not independent verification of their outcomes.
Quoted external material is not a user's decision; the crew must exclude
pasted documents too, including unmarked pastes that an adapter cannot detect.

Directions and facts retain the exact selected line, apart from credential
redaction and outer whitespace. Multiline text contributes separate lines.
The extractor cannot write `handoff` or `resolved`. The orchestrator still
writes the rolling handoff and explicit resolutions via ordinary `append_to`.
When the first handoff is needed before extraction, call session capture once,
keep its returned source id, then append the handoff. An existing manual log
without session provenance is never silently merged into an extracted source;
continue using it until the host hook is installed, or reconcile it explicitly.

### Host, trigger and cheap crew

**Run on the transcript-owning host.** Constellation needs a `SessionEnd` hook
beside `operations/scripts/session-brief.py` (the SessionStart hook), e.g.
`operations/scripts/session-extract.py`, with provider hook registration.
It stages the completed `~/.claude/projects/**/<session>.jsonl` under the
workspace's `.orbit/tmp/session-transcripts/`, together with its session id,
using owner-only directories/files and atomic publication. Each completed input
has a sibling JSON descriptor `{session_id, transcript_path, transcript_sha256,
completed_at}`; paths are absolute and the descriptor is published last. The
host sweep and extraction task enumerate these descriptors, not growing files.
The path passed to
office must physically resolve inside the granted workspace or Library;
symlink escapes and special files are refused. The plugin does not widen its
grants to read the user's provider home or transfer raw transcripts to the
owner. Local staging is an operator-authorized hook, outside this repository.

The hook queues one extraction task/job for that session on that host, using
the instruction in `definitions/auto_tasks/office-conversation-extract.yaml`
and the path/session id as task context. Set task complexity `low`, omit `crew`
and let the host's configured low-complexity pool choose the cheap crew; do not
pin a provider or model. The hook must not wait for inference. Queue dedupe uses
session id plus raw transcript SHA-256. If a newer session starts before the
job finishes, its brief may lag; show the pending extraction rather than claim
it has completed. A hard crash that misses SessionEnd is covered by a host
sweep: the hook's staging companion enumerates completed, unstaged sessions,
and the shipped half-hour auto-task processes staged sessions idempotently.
The sweep is recovery, not the primary trigger. Retain staged files until
capture succeeds; host retention owns their later removal.

The auto-task is shipped **disabled** with `dedupe: skip_if_open` and
`complexity: low`. Registration under `spec.definitions.auto_tasks` in
`plugin.yaml` is a separate integrator task under ARCHITECTURE.md. This feature
ships the definition but does not register or enable it. The constellation
hook and host sweep are also deployment prerequisites, not changes made here.

### Secrets, privacy and retries

Before preview and persistence, the pure scrubber masks high-confidence
provider credentials, bearer values, secret assignments, URL passwords and
PEM blocks. It excludes tool blocks entirely. Copied handoffs and previous-version snapshots pass through the same scrubber
before a new extraction write. No raw transcript copy, tool result or thinking
is stored in the Library. Only the raw SHA-256 is retained
as `transcript_sha256`. The scrubber is a defense against known credential
shapes, not a claim to recognize arbitrary secrets embedded in prose; staging
remains private and operators should use the private conversation default
for sensitive sessions. Credential detection anywhere in the raw input makes
the result private. The strictest of this, the Library's conversation default
and the source's existing privacy wins. Private previews return no lines, and
the sweep uses local conservative classification for them. Normal previews
can be processed by the configured cheap crew. New transcript evidence revokes
sharing approval; extraction never marks a source shareable.

The writer lock serializes session lookup and publication. Session id selects
one conversation across both source layouts. Its stable new path is
`sources/conversation/YYYY/MM/src-<date>-session-<session-id-digest>/source.md`;
the date comes from the first text turn. Same id plus same raw digest returns
the existing source and makes no commit, regardless of a second model's
choices. A changed digest increments `version`, sets `supersedes: vN.md`, and
journals the previous complete source as immutable `vN.md` with the new current
view and index update in one Library commit. Existing entries/handoffs stay
anchorable; changed or deleted transcript text does not retract old evidence.
Multiple sources claiming a session id refuse for explicit reconciliation.
The model selection digest prevents classifying different bytes from those
previewed. A retry after a committed write is a no-op; journal recovery covers
interrupted publication. Historical snapshots remain subject to the source's
privacy; a source changed to private stays private on later extraction.

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

The Library setting `brief.task_workspaces` in the Library's `.office/config.yaml`
lists extra Orbit workspace selectors to merge into *Waiting on you* and
*Agents working on*. The user edits that file directly to change it (for
example `brief:` with `task_workspaces: [...]`); the app's Settings screen
has no field for it, and the agent never sets it (`office.config-set` is
`mcp_scope: none`, so it is not exposed to the orchestrator). Each entry is an Orbit workspace selector: a
registered name, a logical `ws_…` id, an absolute local checkout path, or a
host-qualified federated selector (`hm_…/<workspace>`). At most 16
distinct selectors, each at most 256 bytes without whitespace. Absent or
empty means today's behavior exactly: no extra reads, no new fields.
Local and same-host merging was built in [ORB-19702]; cross-host routing
was built in [ORB-19733].

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
`workspace_unavailable`. A sleeping host therefore shows as one
named line, not a blank session start.

Cross-host selectors (`hm_…/<workspace>`) naming a registered remote host
are routed by Orbit to that host: on 2026-10-28 `orbit tool run
orbit.task.list` with `hm_9ca6004473492f06/ws_orbit` from the Mac returned
box tasks (see ORB-19706 for the plugin callback contract). Every failure
yields the same single `workspace_unavailable` item, including an unlisted
host, an unreachable route (sleeping or otherwise unreachable host,
refused callback), or a missing program grant (`ssh` must be listed in
`spec.requires.programs` and granted at `orbit plugin enable` so the
callback can reach the remote host; adding it requires upgrading and
re-enabling the plugin).

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

## Task References

- [ORB-19702] — built session-brief merging of tasks from configured extra workspaces.
- [ORB-19706] — built federated workspace reads for plugin callbacks.
- [ORB-19733] — built cross-host session-brief routing to federated workspaces.

> Resolve any task above with `orbit task show <ID>` or `git log --grep=<ID>`.
