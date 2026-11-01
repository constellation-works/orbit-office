---
type: design
summary: "Spec: recall — one scribe across every conversation, and answers about past conversations without involving an orchestrator"
last_validated: 2026-11-01
---

# Spec: Recall

Status: specified, not built.

One scribe follows the user across every agent conversation they hold, in
every project, and answers "where did we discuss X?", "what did we decide
about Y?" and "did Z land?" from what it captured. It answers with
citations and a way back to the conversation, and it never asks an
orchestrator. The scribe is the live and SessionEnd extractor of
[sessions.md](./sessions.md); this spec covers how it is installed for
the user rather than per project, how conversations are told apart, how
talk is linked to outcomes, and how questions are answered.

## Why This Exists

A user who works across several conversations at once loses track of
which one held a discussion, what was decided there and whether the
result shipped, and asks again. Asking the orchestrator of the current
conversation costs its context and its time, and it does not know what
other conversations said. The Library already holds one conversation
source per session; nothing yet lets the user query them as one memory.

## Distribution: a provider plugin

The scribe is installed once per user as a provider plugin, not wired into
each project's settings.

- **Claude Code:** a Claude Code plugin shipped from this repository at
  `plugin/claude-code/` (`.claude-plugin/plugin.json`, `hooks/`,
  `commands/`, `agents/`), installable from a marketplace. It carries:
  - the SessionStart hook (the brief), the end-of-turn hook (live passes)
    and the SessionEnd hook (final pass and staging), as specified in
    [sessions.md](./sessions.md#live-scribe-planned);
  - commands `/scribe on|off|now|status|backfill` and `/recall <question>`;
  - the `recall` subagent.
- **Codex** gets an equivalent package once its transcript adapter exists;
  until then Codex sessions are not captured and recall says so.
- **Configuration** is plugin user configuration, not Library settings:
  the office workspace selector to capture into, the default switch state
  for new sessions, and a list of excluded project paths that are never
  captured whatever the switch says.
- The plugin talks to Office only through Orbit's tool surface
  (`orbit tool run office.<tool>`); it holds no Library path and no
  credentials, and hooks fail open (log and exit 0) so a broken scribe
  never slows a conversation.

The Orbit plugin (`plugin/.orbit-plugin/`) stays the only writer of the
Library. The provider plugin is a client of it.

## Telling conversations apart

Each session is one `conversation` source keyed by `session_id`
([library.md](./library.md#sources)). Capture adds these frontmatter
fields, all nullable on existing sources:

```yaml
project: /Users/daniel/workspace/constellation   # session working directory at start
branch: agent-main                                # git branch at start, null outside a repo
started_at: 2026-11-01T01:02:03Z                  # first conversational record
last_active_at: 2026-11-01T03:10:00Z              # last conversational record captured
resume: { provider: claude, command: "claude --resume <session-id>" }
title: "Live scribe and recall"                   # short title
title_by: scribe                                  # scribe | user
```

- `project`, `branch`, `started_at`, `last_active_at` and `resume` come
  from the transcript and the hook, deterministically.
- `title` is proposed by the extraction crew from the captured entries,
  at most 8 words, and marked `title_by: scribe`. It is replaced on later
  passes until the user sets one (`/scribe title <text>`, `title_by:
  user`), which is never overwritten. With no crew (private sessions) the
  title is the first user line, truncated to 8 words.
- The provider's own conversation link (for example the desktop app's
  session link) is recorded in `resume` when the hook can read it; the
  command form is always present for Claude Code.

## Linking talk to outcomes

Captured lines that name work carry `refs`, extracted deterministically
by the adapter from the copied text, never by the model:

- Orbit task ids (`<PREFIX>-<n>`, prefixes from the registered workspaces);
- pull requests as `owner/repo#n` or GitHub PR URLs; a bare `#n` only when
  the same line or the session's `project` names its repository;
- commit SHAs of at least 7 hex characters in an `action` line.

Refs are stored on the entry. Their status is never stored: recall reads
it live (`orbit.task.show`, the PR's state and merge commit), so an answer
says what is true now, not what was true when the line was captured.

## Asking

### The tool: `office.recall`

Read-only, `mcp_scope: workspace`, agent-callable. Input:
`{"question": "<text>"}` plus optional `since`, `until`, `project` and
`limit` (1–50, default 20). It makes no model call. It returns:

- `entries`: the matching conversation entries (full-text over entry
  text, source titles and refs), newest first, each with its source id,
  line anchor, kind, text, time and refs;
- `sessions`: one item per source that contributed an entry, with its
  title, project, branch, started/last-active times and `resume`;
- `refs`: each distinct ref with its live status (task status and
  workspace; PR state, merged-at and merge commit), or `unresolved` with
  the reason;
- `coverage`: the earliest captured session date, sessions skipped as
  private, and providers not captured, so an empty result can say why.

Private sources contribute session items and refs but never entry text.

### Answering: the `recall` subagent

`/recall <question>` runs the `recall` subagent, a low-cost model with one
tool, `office.recall`. Its answer:

- cites every claim to a source anchor (`src-…#Ln`) and names the
  conversation by title, project and date with its `resume` command;
- reports work status only from the live `refs` result;
- says "not captured" when the result holds no evidence, citing
  `coverage`, and never fills the gap from its own knowledge;
- quotes user directions and decisions verbatim, as captured.

Because it runs as a subagent, the conversation it was asked from receives
only the answer, not the search. The same tool serves two more entry
points with the same rules: `office recall "<question>"` from a terminal,
and a Recall box in the web app.

## The Scribe window

The user sees what the scribe writes as it writes it, corrects it in place
and asks it questions, in one small window beside their conversations. The
window is a view over the same Library and tools; it adds no store and no
second writer.

### Two hosts, one screen

- **Web app:** a compact **Scribe** screen in the orbit-office app
  (`/scribe`), laid out for a narrow window (from 320 px wide) and usable
  as an installed PWA window. It works for every provider and for terminal
  sessions.
- **Claude desktop mod:** a pane beside the chat (`$.ui.open`), shipped
  with the Claude Code plugin. It shows the same three parts for the
  session it sits in, using the session identity the engine supplies, and
  links to the web screen for everything else. It reaches Office the same
  way the plugin's hooks do, through `orbit tool run office.*`; it holds no
  Library path or credentials.

### What it shows

- **Feed:** captured entries, newest first, as they land: kind, the copied
  line, time and a link to its anchor (and, in the mod, to the line's place
  in the conversation where the surface supports it). The current
  conversation comes first; others are grouped by title and project, the
  same fields recall uses. It refreshes by polling, like the rest of the
  app; there is no event bus.
- **Status line:** switch state for the current session, the last pass
  ("2 min ago, 3 entries"), and a pending pass if one is queued.
- **Controls:** the session's switch, **Capture now** (`/scribe now`), and
  the session title (`/scribe title`).
- **Ask box:** a question answered by recall's rules: cited, conversation
  named with its resume command, live status for refs, "not captured" with
  coverage. Answers open feed entries and conversations by link.

### Corrections

Each feed entry offers **Not a decision**, **Wrong kind…**, **Wrong
wording** and **Remove**; the window also offers **Missed something…**
with a free-text box. The log stays append-only: a correction is a new
`correction` entry in the same source that names the corrected anchor and
the user's choice, written through `office.capture` with `append_to`.

- Readers apply corrections: the brief, the digest, `office.recall` and the
  feed treat a corrected entry as its corrected kind, or as absent after
  **Remove**, and show that it was corrected.
- **Missed something…** writes the user's text as a `direction`,
  `decision` or `fact` entry chosen by the user, verbatim, marked
  `by: user`.
- Repeated corrections of the same kind are a signal for a
  `knowledge_update` proposal to `dom-scribe-rules`, quoting the corrected
  entries ([proposals.md](./proposals.md#signals)).

### Pace while the window is open

The feed is only as fresh as the last pass. While a window is open on a
session, its debounce tightens to `max_turns: 2` (the time bound unchanged),
and **Capture now** covers the rest. With no window open, the defaults of
[sessions.md](./sessions.md#trigger-and-debounce) apply.

### Answering in each host

- In Claude Code, `/recall` and the mod's ask box use the `recall` subagent.
- In the web app, the server makes one low-cost model call through the
  Models port, so the privacy routing of
  [models-and-privacy.md](./models-and-privacy.md#routing-rules) applies:
  private sources are answered by reference only unless an open-weights
  route is configured.

## Backfill

`/scribe backfill [--since <date>]` queues SessionEnd extraction for the
user's existing provider transcripts in non-excluded projects, oldest
first, through the same staging and dedupe as SessionEnd. Sessions already
captured are no-ops. Recall's `coverage` reports how far back capture
reaches.

## Invariants

- One writer: the Library is written only through `office.capture`;
  the Scribe window writes corrections through it too.
- Answers come only from captured entries and live ref status.
- Raw transcripts never leave the host that holds them; only captured
  entries reach the Library and its sync.
- Excluded projects are never staged, captured or backfilled.
