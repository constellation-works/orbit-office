---
type: design
summary: "Spec: recall — one scribe across every conversation, and answers about past conversations without involving an orchestrator"
last_validated: 2026-11-01
---

# Spec: Recall

Status: the Orbit backend is built (ORB-20688): session context and title
ownership, deterministic stored refs, read-only `office.recall`, and append-only
corrections with repeated-correction signals (ORB-20695). The Claude Code
provider plugin, commands/subagent and backfill are built (ORB-20689); see the
[installation guide](../../../../plugin/claude-code/README.md). The Scribe UI
remains specified.
PR refs resolve live through a `gh` source (ORB-20710, below).

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
    [sessions.md](./sessions.md#live-scribe);
  - commands `/scribe on|off|now|status|title|backfill` and `/recall <question>`;
  - the `recall` subagent.
- **Codex** gets an equivalent package once its transcript adapter exists;
  until then Codex sessions are not captured and recall says so.
- **Configuration** is plugin user configuration, not Library settings:
  the office workspace selector to capture into, the default switch state
  for new sessions, and a list of excluded project paths that are never
  captured whatever the switch says.
- The plugin talks to Office only through Orbit's tool surface
  (`orbit tool run office.<tool>`); it holds no Library path and no
  credentials, and hooks fail open (fixed diagnostic and exit 0) with a
  one-second Orbit budget so a broken scribe cannot block a conversation.
  The workspace selector is an absolute local checkout path, which Orbit
  accepts as a selector; remote selectors are refused. The marketplace and
  per-user configuration ship inside `plugin/claude-code/`. Claude namespaces
  the commands as `/office-scribe:scribe` and `/office-scribe:recall`.

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

### Capture contract (built)

Session-mode `office.capture` accepts optional `project`, `branch`, `resume`,
`repository`, `title` and `title_by` alongside its existing transcript fields.
The first conversational record's `cwd` and `gitBranch` take precedence over
hook fallbacks; start context is retained across later passes. Activity time
advances to the latest conversational record, even when the crew selects no
entries. `resume` defaults to a safely quoted `claude --resume <session-id>`;
a later hook can supply its provider link. No resume value is executed.

The crew passes its proposed title with `title_by: scribe` (the default).
With no proposal, the first user line supplies at most eight words. Private
capture ignores scribe proposals and uses that local fallback; an explicit
user title is retained. A hook can set or change a user title with `title`
and `title_by: user`, including on an unchanged transcript. Later scribe
passes cannot replace it. No model call is added to capture.

`repository` is a known GitHub `owner/repo` supplied by the hook, for projects
whose filesystem path does not identify their repository. A project that is
itself `owner/repo` or a GitHub repository URL also supplies this context.
Ambiguous lines naming multiple repositories never bind a bare `#n`.

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

### Stored refs (built)

`entry_refs` in source frontmatter maps body-relative anchors (`L1`, `L2`, …)
to arrays of `{kind: task|pr|commit, id: <canonical identity>}`. This keeps
entry text verbatim and existing body citations stable. Normal conversation
capture, append and transcript passes index new entries; existing stored
identities are retained. Old sources without this field remain readable,
with empty refs until a later capture indexes them. Status is never written.

Task candidates use the general uppercase workspace-prefix/id syntax rather
than a hardcoded ORB/DANI/ORBA list. Recall sends each distinct identity to
Orbit's registered task router (`orbit.task.show`), which determines whether
it exists and is readable across workspaces; unregistered/unreadable ids are
unresolved. PR URLs normalize to `owner/repo#n`; action hashes normalize to
lowercase (7–64 hex characters). Other entry kinds never infer commit refs.

### Live PR state (built)

Task refs resolve through `orbit.task.show`, which the plugin already holds.
PR refs resolve live through `gh` (ORB-20710): a `PullRequests` port in
`office-core` (`ports/pr.rs`), implemented by the supervised `gh` adapter in
`office-store` (`pr.rs`, `GhPullRequests`), wired in the binary's `ports()`.
For each distinct PR ref the adapter runs, once and never retried,
`gh pr view <owner/repo#n> --json state,mergedAt,mergeCommit` under the
shared bounded supervisor (15-second deadline, capped output, cleared
environment forwarding only what `gh` needs to authenticate). A resolved
ref reports `status` as `open`, `closed` or `merged`, with `merged_at` and
`merge_commit` (each null when `gh` reports none).

On any failure the ref reads `unresolved` and the call still succeeds, with
a reason naming the cause: `pr_source_unavailable` when `gh` is missing or
not granted (it cannot be started), `pr_lookup_failed` when `gh` runs but
fails, times out or exceeds its bound, `invalid_pr_response` when its reply
does not match the expected shape, and `invalid_pr_ref` when the stored id
is not a canonical `owner/repo#n`. Program output is never echoed.

`gh` is declared in `spec.requires.programs` in
`plugin/.orbit-plugin/plugin.yaml`. **Operator step:** because that grant
changed, upgrade the office plugin and re-enable it so `gh` is granted to
the backend; until then PR refs read `unresolved` /
`pr_source_unavailable` while task refs keep resolving.

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

### Query behavior (built)

The question is a literal case-insensitive search: all whitespace-separated
terms must occur across the entry text, source title and stored ref ids.
There is no natural-language inference or model call. `since` and `until`
are inclusive RFC 3339 timestamps or UTC dates (a date's entire day is
included); `project` is an exact match. Ordering compares instants, including
offsets and fractional seconds, with stable source/anchor tie breaks. `limit`
bounds matching public and private entries together. Each contributing source
appears once in `sessions`; private matches contribute metadata and refs only.

`coverage.earliest_session_at` spans the whole Library and falls back to
`occurred_at`/`captured_at` for old sources. `private_sessions_skipped` counts
all private conversation sources, independently of filters; the
`providers_not_captured` list currently contains `codex`.

Every distinct task ref gets a fresh `orbit.task.show` read. Task workspace
is returned when Orbit supplies a workspace selector/id/path, otherwise null
(the current task projection has no workspace field). Callback failures and
invalid replies are `unresolved` with fixed reasons, without upstream error
text. PR refs resolve live through `gh` (see Live PR state above) and fall
back to `unresolved` with a cause-naming reason; commit refs use
`no_commit_source`. The call
writes nothing, and refuses a Library change during its reads so the caller
can retry. Backend contract and offline conformance checks live in
`schemas/recall.{request,response}.json` and `tests/conformance/recall.yaml`
under `plugin/.orbit-plugin/`.

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
  Library path or credentials. Built: `plugin/claude-code/mod/scribe-pane.tsx`,
  validated with `claude plugin validate` and
  `claude plugin test`. Its feed is `office.recall` queried with the
  engine-supplied session id (recall matches the id against each source's
  session identity and returns that session's entries newest first);
  corrections are one `office.capture` `append_to` append per choice; the
  switch, **Capture now** and title controls go through the plugin's
  structured pane control (`scripts/pane.py`); the ask box spawns the
  `recall` subagent and falls back to rendering `office.recall`'s cited
  evidence when a surface returns no subagent answer. When Orbit is
  unreachable the pane shows a single `scribe unavailable` line.

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

The correction backend is built; the desktop pane's controls are built
(the web Scribe screen's controls remain planned).
Append through `office.capture`, for example:

```json
{"append_to":"<source-id>","entries":[{"kind":"correction","correction":{"anchor":"#L1","choice":"not_decision"}}]}
```

Choices are `not_decision` (effective kind `open`), `kind` with a `kind`
field, `wording` with nonempty verbatim `text`, and `remove`. A kind change
accepts any ordinary log kind, never `correction`. Only local anchors of
original entries already in the source are accepted; corrections cannot target
other corrections, prose, future lines or entries in the same append batch.
All entries in a refused batch remain unwritten. Corrections compose in append
order; removal stays final. The original anchor and timestamp stay attached to
the effective entry. Brief and recall entries add `corrected: true`; digest
lines retain the effective kind and `(corrected)` marker even if its model
selects a plain excerpt. Multiline wording contributes complete individual
lines to digest evidence, each with the original entry anchor. Removed entries
and correction audit records are not
returned as ordinary reader evidence. A digest already published is immutable;
these rules apply when creating a new digest.

A missed entry is `{ "kind": "fact", "text": "<verbatim>", "by": "user" }`
(or `decision` / `direction`). Attribution is a caller claim, not authenticated
identity. Its single physical log line stores
`[<time>] fact (by: user): "<JSON-escaped verbatim text>"`, preserving spaces
and newlines as JSON escapes. This extends the kind delimiter, so legacy text
that happens to contain JSON or attribution words keeps its original meaning.
Credential-bearing user additions and replacement wording are refused before
writing, so accepted text stays verbatim through later scrubbed transcript passes.
Readers decode the exact text and expose `by: user`. Correction lines store the
JSON correction payload after `correction:`. Legacy plain entry lines keep
their format; a `text` containing a canonical JSON correction is also accepted.
The stored log body stays append-only, including through later transcript passes.

At least three corrections with the same **choice** within
`thresholds.window_days` produce a `scribe_corrections` candidate. Its kind is
`knowledge_update`, its target evidence includes `dom-scribe-rules`, and its
summary quotes the original entries with original and correction anchors. The
scan clock excludes future corrections; the window includes its exact lower
boundary. This proposes a change for review and never edits the rules page.

### Pace while the window is open

The feed is only as fresh as the last pass. While a window is open on a
session, its debounce tightens to `max_turns: 2` (the time bound unchanged),
and **Capture now** covers the rest. With no window open, the defaults of
[sessions.md](./sessions.md#trigger-and-debounce) apply.

The window-open heartbeat is a file the window refreshes while it is open:
`.orbit/tmp/scribe/<session-id>.window-open` in the Office workspace
(touched by the desktop pane on every refresh, through the plugin's pane
control). The end-of-turn hook treats a heartbeat under two minutes old as
"window open" and applies the tighter turn bound. The web Scribe screen
refreshes the same file for its session when it is built.

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
