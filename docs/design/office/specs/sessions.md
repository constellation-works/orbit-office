---
type: design
summary: "Spec: session continuity — the deterministic session brief, the append-only decision log, and the daily digest routine"
last_validated: 2026-11-01
---

# Spec: Session continuity

The orchestrator is the user's agent CLI, and its sessions end whenever the
user closes the terminal. The next session must pick up where the last one
left off without anyone writing a summary. Three pieces do that, and each
is the cheapest mechanism that can do its job
([decision](../4_decisions.md#session-continuity-from-a-brief-a-decision-log-and-a-digest-routine)):

| Piece | Written by | When | Model |
|---|---|---|---|
| **Decision log** | transcript extractor, through `office.capture` | SessionEnd; while the [live scribe](#live-scribe) is on, every few turns; sweep recovers misses | configured low-complexity pool |
| **Handoff** | the agent, as `handoff` entries in that log; optionally the live scribe (planned) | refreshed whenever the plan changes | none; the scribe's handoff uses the low-complexity pool |
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
WebVTT's `transcript_path` ([ORB-20242]). No new Orbit-core service is needed. Its inputs
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

Harness wrapper text injected into a user message is not the user's words
and is excluded the same way: `<system-reminder>` blocks and
`<local-command-*>` output (stdout, stderr and caveats) are dropped whole,
for string and block content alike, before lines are taken. A slash-command
invocation (`<command-message>`, `<command-name>` and `<command-args>`)
renders as one `/<name> <args>` line instead of the tags, with exactly one
leading slash on the name. A user record left with no text after this
stripping contributes no line. Pasted content is kept: it is the user's own
material, however it was framed.

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

Manual Scribe-window feedback uses the same append path: a `correction`
entry targets an earlier original anchor, and a missed entry keeps its chosen
`direction`, `decision` or `fact` kind with explicit `by: user` attribution.
See [Corrections](./recall.md#corrections) for the built backend contract.

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
The live scribe adds two narrow ways a user line becomes a decision
or direction without these cues: an explicit `Note:` command (built,
[ORB-20685]) and an answer
to a concrete question ([below](#answers-to-questions), built, [ORB-20686]).

Directions and facts retain the exact selected line, apart from credential
redaction and outer whitespace. Multiline text contributes separate lines.
The extractor cannot write `resolved`, and writes a `handoff` only as the
opt-in [scribe handoff](#scribe-handoff). Otherwise the orchestrator still
writes the rolling handoff and explicit resolutions via ordinary `append_to`.
When the first handoff is needed before extraction, call session capture once,
keep its returned source id, then append the handoff. An existing manual log
without session provenance is never silently merged into an extracted source;
continue using it until the host hook is installed, or reconcile it explicitly.

### Host, trigger and cheap crew

**Run on the transcript-owning host.** The Claude Code provider plugin at
`plugin/claude-code/` supplies SessionStart, Stop and SessionEnd hooks
([installation](../../../../plugin/claude-code/README.md), ORB-20689).
After installing it, the operator can retire constellation
`operations/scripts/session-brief.py` and its legacy SessionEnd stager.
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
owner. Local staging is the operator-authorized provider hook shipped in this
repository; it never reads a Library path or transfers transcripts. Its
`provider-scribe-` immutable filenames avoid legacy stager publication races.

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
`complexity: low`, and is **registered** under `spec.definitions.auto_tasks`
in `plugin.yaml`. This feature ships the definition registered but not
enabled. Installing the provider plugin on the transcript-owning host and
enabling any desired recovery sweep remain operator deployment steps.

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

## Live scribe

> Scribe v1 ([scribe.md](./scribe.md)) replaces automatic live passes with
> the `/scribe` command; the passes below are off by default from v1.

Status: partly built. The incremental passes and the user `Note:`/`Scribe:`
commands below are built ([ORB-20685]); the rules page, answers to
questions and scribe handoff are built ([ORB-20686]); the Claude Code
switch, trigger and debounce are built ([ORB-20689]), and the switch is
Office-owned state shared with the Scribe window ([ORB-20756]). The feedback section
remains specified, not built. Everything above still applies; this section
changes when extraction runs, how much of the transcript each pass reads,
and three narrow rules about what it may write.

SessionEnd extraction leaves the orchestrator writing captures by hand
during the session, and a session that runs all day has an empty log until
it ends. The live scribe is the same extractor run while the conversation is
still going, switched on per session. It reads the conversation only — the
adapter already drops tool calls, tool results, thinking and sidechains —
and writes through `office.capture` as before. It is not a participant: it
never posts into the conversation.

### Switch

The switch is Office-owned per-session state, kept in plugin state
(`plugin_state/scribe_control.sqlite`) behind `office.scribe-control`
(built, [ORB-20756]); it is not Library evidence. One record per session
holds the switch, a pending **Capture now** request, the user's title and
the last Scribe window heartbeat. The web Scribe window and the provider
plugin read and write the same record, so both hosts share one switch.
A provider command (`/scribe on|off|now|status|title` for Claude Code)
writes it; the default for new sessions is a setting of the provider
plugin, not of the Library. Turning the scribe off stops new passes; it
never removes evidence already captured. The hooks and commands ship as a
per-user provider plugin from this repository
([recall.md](./recall.md#distribution-a-provider-plugin)), so one scribe
covers every project; answering questions across conversations is
[recall.md](./recall.md).

The plugin's per-session flag file, `.orbit/tmp/scribe/<session-id>`,
owner-only, in the office workspace beside the transcript staging
directory (present means on), is its local cache of that record. Each
end-of-turn hook reads the record once and mirrors the switch into the
flag file; on the first turn Office sees for a session (switch unset) the
hook publishes its own switch (the plugin default) so the window shows the
same state. `/scribe on|off` writes Office first, then the flag file. When
Orbit cannot answer (unreachable, an older office plugin, the one-second
hook budget spent) the hook fails open on the cached flag file.

`office.scribe-control` takes `op` and `session_id` (1–128 ASCII letters,
digits, `-` or `_`) and returns the record after the op:

| op | Extra input | Effect |
|---|---|---|
| `get` | — | Read. An unknown session reads as `on: null` (unset), nothing pending. |
| `set_switch` | `on` | Set the switch. |
| `request_now` | — | Record a pending Capture-now request; an existing one keeps its time. |
| `consume_now` | optional `requested_at` | Clear the pending request and report `consumed`. With `requested_at` (the `now_requested_at` read earlier), clear only that request, so a newer one survives. |
| `set_title` | `title` | Store a single-line user title (at most 4096 bytes, trimmed). |
| `heartbeat` | — | Record that a Scribe window is open now. |

The record reports `on`, `now_requested`, `now_requested_at`, `title`,
`heartbeat_at`, `window_open` (heartbeat under two minutes old by the
Office clock) and `updated_at`. Records untouched for 30 days are dropped
on a later write. The tool is `mutating`, `mcp_scope: workspace` and not
`agent_callable`, like every Office mutation outside the four consented
agent verbs ([plugin-surface.md](./plugin-surface.md#agent-authority-inventory));
the plugin's hooks and commands call it through the user's own `orbit`
CLI. A hook running inside an Orbit-managed agent session is therefore
refused and falls back to the flag file.

### Trigger and debounce

The provider's end-of-turn hook (`Stop` in Claude Code) reads the shared
record (above), checks the switch and, when it is on, stages a snapshot and
may queue a pass. It never waits for inference.

- **Snapshot.** The hook copies the live transcript up to its last complete
  record (the last newline), never a partial record, and publishes it
  atomically with a descriptor carrying `mode: live`, `session_id`, the
  snapshot's byte length and SHA-256. The adapter's refusal of truncated
  records therefore never fires on a live snapshot.
- **Debounce.** At most one pass is queued or running per session. A new
  pass is queued only when the previous one was queued at least
  `min_interval` ago (default 5 minutes) or at least `max_turns` new user
  turns (default 8) have arrived since it; `max_turns` is 2 while the
  record's window heartbeat is fresh ([recall.md](./recall.md#pace-while-the-window-is-open)).
  `/scribe now` records a Capture-now request in Office and runs it
  immediately, ignoring the debounce, while reusing an outstanding live task.
  A request made in the window runs at the session's next end-of-turn hook,
  even with the switch off. Either way the request is consumed only after
  its pass is queued (or found already queued), naming the request it read;
  while an earlier pass is still pending it stays pending for a later turn.
  SessionEnd always requests a digest-deduplicated final task; it depends on
  an outstanding live task so no extraction runs concurrently for the session.
  This closes the race with a live worker finishing its last descriptor scan.
- **Dedupe** for live passes uses session id plus snapshot length, so a
  burst of hooks on an unchanged transcript queues nothing.

Each pass is the existing low-complexity extraction task: no crew pinned,
the configured pool chooses.

### Incremental passes

Built ([ORB-20685]): `from_line` previews with one read-only assistant
`context` line, the source's `extracted_through`/`extracted_prefix_sha256`
cursor, append-instead-of-version for a snapshot that only grew past the
recorded prefix (a pass without `from_line` keeps the SessionEnd
changed-digest behavior above), and a cursor that advances even when a
pass selects nothing.

A pass reads only what earlier passes have not.

- `office.capture` session preview accepts `from_line` (zero-based,
  inclusive). It returns the lines from there on, plus the nearest earlier
  assistant text line as read-only context (`context: true`) so a reply can
  be read with the question it answers. Context lines cannot be selected.
- The conversation source records `extracted_through` (the last line a pass
  considered) and `extracted_prefix_sha256` (the SHA-256 of the transcript
  bytes up to that line). The next pass starts at `extracted_through + 1`.
- **Append, not version.** When the new snapshot's leading bytes hash to
  `extracted_prefix_sha256`, the transcript only grew: the pass appends its
  entries to the current source with no version bump and no `vN.md`
  snapshot. Only a non-prefix change (compaction rewrite, edited history)
  takes the existing changed-digest path, with a version and a snapshot.
  Without this, an all-day session would journal a full copy every pass.
- A pass that selects nothing still advances `extracted_through`, in one
  commit.

### Rules page

Built ([ORB-20686]).

What the scribe notes is the user's to define, on a Library page with the
id `dom-scribe-rules` (a one-per-Library template, `scribe-rules`). Its
body has three sections: **Note** (what to capture, for example standing
directions, decisions with their reason, rejected options and why, changes
to crews, pools and configuration, facts about hosts and people), **Skip**
(for example status chatter, restated plans, anything about a named
private topic) and **Kinds** (guidance for borderline lines). The page is
passed to the crew with each preview, as data: the session preview returns
`rules` (`id`, `path`, `handoff`, and `body`, which is null for a `private`
page so it never reaches the crew) or null. It changes only through
proposals, so the user holds the pen. The preview also returns `source`,
the session's conversation source (`id`, `path`, `extracted_through` and
`next_anchor`, the `#L<n>` the pass's first new entry takes), or null
before its first capture, when the first entry takes `#L1`.

Rules can narrow what is captured and steer the choice between kinds. They
cannot widen authority: the deterministic guard still runs after the crew,
an assistant line never becomes a decision or direction, and a rule that
says otherwise is ignored. With no page, the built-in defaults in the
extraction task apply.

### Commands

- **User (built, [ORB-20685]):** a user line that begins with `Note:` (or `Scribe:`) is captured
  without model judgment, verbatim apart from credential redaction. Its kind
  is `direction` when the text after the prefix carries a direction cue,
  `decision` when it carries a decision cue, and `fact` otherwise. The local
  conservative classifier honours it too, so it works in private sessions.
  The prefix is a cue for the guard, not an instruction to any model.
- **Orchestrator:** `office.capture` with `append_to` is unchanged, for the
  rare entry the conversation does not contain (an `action` with its task
  id, a `resolved`). `/scribe now` flushes a pass. The orchestrator stops
  capturing decisions and directions itself while the scribe is on.

### Answers to questions

Built ([ORB-20686]).

Most decisions are a short reply to a concrete question ("Yes, file it").
The cue guard keeps those `open`. The scribe may record such a reply as a
`decision` when all of these hold, each checked deterministically:

- the crew's selection carries `answers: <line>`, naming an assistant line;
- that line is the nearest assistant text before the user line, with no
  other user line between them, and its text ends in `?`;
- the user line has no uncertainty marker from the guard's list, is not
  itself a question, and is either an affirmative (yes, do it, go ahead,
  ok, sounds good, ship it and their close variants) or, when the question
  offered options, one of the option labels;
- a negative ("no", "not now", "not yet") records a `decision` only when
  the question was a yes/no question, and the entry says so.

The adapter copies both lines: `decision: "Yes, file it" (answering #L41:
"File the claim-verification sweep as an Orbit task?")`. Neither text is
model-authored. A negative reads `(declining #L<n>: …)`. The raw
transcript is not persisted, so `#L<n>` here names the question's
transcript line, the same zero-based preview number `answers` gave, not a
body anchor; the quote carries its text. A question asked through a provider's question tool rather
than in message text (a structured ask with buttons) counts as assistant
text only when its adapter maps that tool's question field explicitly; tool
input is otherwise excluded as today. No adapter maps one yet: the Claude
Code adapter reads only `text` content blocks, and no recorded transcript
in this repository shows where its question tool's text lands, so such
asks are a known gap.

As built, the checks are deliberately narrower than the list above:

- "nearest assistant line with no user line between" means the question is
  the line just before the reply; a reply after a later line of the same
  message, or after another user line, stays `open`;
- the reply must lead with an affirmative or negative phrase; an
  affirmative that also carries a negation, or any reply with `but`,
  `except`, `unless`, `instead` or `wait`, stays `open`;
- a yes/no question offers no options, does not start with a question word
  (what, which, where, when, who, whom, whose, why, how) and has no `or`;
  affirmatives as well as negatives answer only such questions;
- options are labels written in the question line itself, `(a)`, `a)`,
  `(1)`, `1)` or `option a`; the reply must be the label alone, optionally
  as `(a)` or `option a`;
- `answers` counts only on a `decision` selection, may name the read-only
  context line of a `from_line` preview, and is ignored in private
  sessions.

### Scribe handoff

Built ([ORB-20686]).

Optional, off by default: `scribe.handoff: true` in the rules page's
frontmatter, written as the one-level mapping `scribe:` with
`handoff: true`. When on, a pass whose new lines change the plan (a new
decision, direction or `action`) also writes one `handoff` entry, at most
60 words. The crew sends it as the capture field `handoff` beside its
`selections`. It is written as kind `handoff` with the marker leading the
text, `[<time>] handoff: (scribe) <text>`, stamped with the capture time:
the log grammar has one kind token per entry, so a `handoff (scribe)`
kind would be refused by every log reader. Unlike every other entry it is
model-authored, so:

- the `(scribe)` marker is part of the line, and the session brief shows it;
- it must cite at least one entry anchor (`#L<n>` or `#L<n>-<m>`) of the
  current source after this pass's entries, and every anchor it cites must
  name a log entry, or the handoff is dropped (the pass's other entries
  still land);
- the newest handoff wins, whoever wrote it, so an orchestrator handoff
  written later replaces it in the brief; nothing is edited or removed.

A handoff offered while the setting is off, in a private session, or by a
pass that adds no decision, direction or action is dropped too. The
capture response then carries `handoff`: `written`, or the reason it was
dropped (`disabled`, `private`, `invalid`, `too_long`, `no_plan_change`,
`no_anchor`, `unresolved_anchor`).

This is what lets the orchestrator stop maintaining the handoff. Without
it, the orchestrator keeps doing so as today.

### Feedback to the session

Optional and constellation-owned: a `UserPromptSubmit` hook may add one
line of context — "scribe captured N entries since <time> in <source id>"
— so the orchestrator neither repeats a capture nor assumes one happened.
It reads the source's entry count; it never quotes entries back.

### Providers and limits

- Claude Code only, like extraction. Codex rollout JSONL (Codex CLI, T3
  Code) needs its adapter first; live mode refuses other formats exactly as
  SessionEnd extraction does.
- A pass's preview is bounded by the existing 8 MiB transcript limit and by
  `max_turns`; a long backlog after the scribe is switched on mid-session is
  read in successive passes, oldest first.
- Privacy is unchanged: private sessions use the local conservative path,
  which honours `Note:` commands but not answers to questions or the scribe
  handoff, since both need the crew.

## Session brief

`office.session-brief` (read_only, `mcp_scope: workspace`). The
orchestrator skill calls it first in every session. Its output is
assembled from Orbit and the Library, with no model call, so it is free,
always current and cannot invent anything:

- **Handoff:** the newest `handoff` entry of the previous session, verbatim,
  with its time; a [scribe handoff](#scribe-handoff) keeps its leading
  `(scribe)` marker. Absent when that session wrote none.
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
office workspace). Corrected entries add `corrected: true`, use the effective
kind/wording, and keep their original anchors/timestamps; removed entries are
omitted. Explicit user additions expose `by: user`. See
[the correction contract](./recall.md#corrections). Entry text and timestamps
otherwise keep their original spelling.
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
- [ORB-20242] — built conversation transcript extraction through the `office.capture` session mode and the SessionEnd sweep.
- [ORB-20685] — built the live scribe's incremental passes and `Note:`/`Scribe:` commands.
- [ORB-20686] — built the live scribe's rules page, answers to questions and scribe handoff.
- [ORB-20689] — built the per-user Claude Code hooks, commands, debounce and backfill.
- [ORB-20755] — reserved the planned `office.recall-answer` and `office.scribe-control` scaffold (registry rows, reserved manifest blocks, this section).
- [ORB-20756] — built `office.scribe-control` (Office-owned switch, Capture-now request, title and heartbeat) and the plugin hooks that share it.

> Resolve any task above with `orbit task show <ID>` or `git log --grep=<ID>`.
