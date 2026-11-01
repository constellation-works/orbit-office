# Office scribe for Claude Code

Scribe v1 ([spec](../../docs/design/office/specs/scribe.md)): a cheap `scribe`
agent that you or an orchestrator command. `/scribe` scribes the messages of
this session that are not scribed yet into one tagged, grep-able SCR file per
session in the Library; `/scribe note` takes direct notes; the canned queries
and `/scribe ask` answer with exact `SCR file:line` citations. The plugin also
keeps the SessionStart brief.

Requires Python 3.10+ on macOS or Linux, Claude Code with plugin user
configuration, and a locally installed Orbit Office plugin with
`office.session-brief`, `office.scribe-pending`, `office.scribe-append` and
`office.scribe-query`. No Library path, credentials, model SDK or third-party
Python packages are used here.

## Install

Clone this repository at a reviewed revision, then register its marketplace
and install the plugin for your user:

```sh
claude plugin marketplace add /absolute/path/to/orbit-office/plugin/claude-code
claude plugin install office-scribe@constellation-office --scope user \
  --config workspace=/absolute/path/to/local/office-workspace \
  --config 'excluded_projects=["/absolute/path/to/excluded-project"]'
```

The workspace value is an absolute local registered Orbit checkout that has
Office enabled and the intended Library attached; remote host selectors are
refused, so transcripts stay on this host. Exclusions cover the physical
directory and all its descendants, including symlink aliases and every
working directory a transcript records, for every command and for backfill.
Change options later through Claude's plugin configuration UI.

| Option | Default | Meaning |
|---|---|---|
| `workspace` | (required) | The local Office workspace. |
| `excluded_projects` | `[]` | JSON array of absolute paths never scribed. |
| `auto_capture` | `false` | The legacy automatic Stop/SessionEnd capture ([below](#legacy-automatic-capture-off-by-default)). Off: `/scribe` is the only trigger. |
| `default_on` | `false` | Legacy: with `auto_capture` on, whether live Stop passes start on. |
| `scribe_url` | empty | The web Scribe screen, linked from the desktop pane. |

Restart Claude after installation and check `/agents` (the `scribe` agent),
`/mcp` (the `office` server and its `scribe` tool) and `/hooks`. The command's
stable provider-qualified form is `/office-scribe:scribe`; `/scribe` below
means that command under Claude's standard namespacing. Permission prompts
remain governed by your Claude and Orbit settings.

After verifying this plugin's SessionStart brief, the operator can retire
constellation's `operations/scripts/session-brief.py` hook. This repository
does not modify it.

## Commands

Each command acts on the session it is run in (`${CLAUDE_SESSION_ID}`).

| Command | Does | Model |
|---|---|---|
| `/scribe` | Catch up: scribe the messages after the SCR file's `scribed_through` and report what was added (counts per tag and the new lines). A run with nothing new adds nothing and says so. | `scribe` level |
| `/scribe note <text>` | Record `<text>` verbatim as your entry (`by: user`). Starting with a tag (`#todo …`) it is written at once with no model; otherwise the agent picks the tag. | none with a tag |
| `/scribe pending` | Open `#pending` lines with no `#resolved` naming them, oldest first. | none |
| `/scribe pref <matter>` | The current preference on that matter, flagged when stale. | none |
| `/scribe landed [today\|<date>]` | `#landed` lines on that local date (default today; the zone is stated). | none |
| `/scribe why <matter\|topic>` | Decisions on that matter (then topic) with their `why`, or "no recorded reason". | none |
| `/scribe ask <question>` | Free-form answer with citations: the plugin queries first, the agent reads only the shortlist, and quotes are checked before display. | `ask` level |
| `/scribe title <text>` | Set the session title; Office renames the SCR file once and keeps the old id in `aliases`. | none |
| `/scribe backfill [--since <date>] [--project <name>]` | Catch up over your existing transcripts, oldest first; sessions already scribed to their end are skipped. Headless agent-to-agent runs, whose first conversational record carries `entrypoint: sdk-cli` (Orbit workers, `claude -p`), are skipped as headless: counted in `skipped`, never refused and never handed to the scribe agent. An explicit `/scribe` inside such a session still scribes it. | `scribe` level per session |

**Notes.** Keys go before the text, or after a quoted text:
`/scribe note #todo due: 2026-11-01 write the v1 spec`,
`/scribe note #pending refs: ORB-1 "waiting on the deploy"`,
`/scribe note #why resolves: SCR-01-0307-scribe-v1-design#L12 "the reason"`,
`/scribe note #preference matter: release-with-open-defects resolves: <SCR id>#L<n> "…"`.
The keys are `resolves`, `matter`, `due`, `refs`, `review_by`, `supersedes`
and `topics` (comma-separated lists for `refs` and `topics`). Run by an
orchestrator, a note carries `by: <crew>`: the `--by <crew>` prefix
(`/scribe note --by opus #agent-decision …`), else `ORBIT_CREW`, else
`ORBIT_AGENT_NAME` in an Orbit agent session. Office refuses an agent tag
without a crew and your tags (`decision`, `direction`, `todo`) with one.

**Held notes and titles.** `office.scribe-append` moves the session's cursor
to the last message it is given. On a session that has never been scribed
there is no cursor to keep, and choosing one would mark unread messages as
scribed, so a note or title there is held in the plugin's private state and
written with the first `/scribe` catch-up. The reply says `Held`.

**Orchestrators** reach the same agent as the `scribe` subagent ("ask the
scribe whether Daniel decided on X"). It runs the query tool first, Grep and
Read on the shortlisted SCR files only, and verifies its quotes. Write with
`/scribe note #<tag> …` as things happen, and run `/scribe` at the end of each
wake. A standalone scribe session (`claude --agent office-scribe:scribe`) is
the place for meeting notes.

## How it works

```
/scribe ─▶ scribe MCP tool ─▶ stage transcript ─▶ office.scribe-pending
                                    │ (messages after scribed_through)
                                    ▼
                 scribe agent (haiku@medium) returns entries
                                    │
                      scribe MCP tool ─▶ office.scribe-append (validates, writes, commits)
```

- The command hands its arguments, as one data string, to the plugin's
  structured `scribe` MCP tool. Model-free commands (tagged notes, titles,
  `pending`, `pref`, `landed`, `why`, nothing new) finish there, and the reply
  carries `model_call: false` and no agent request. Only a catch-up, an
  untagged note, `ask` and backfill return an `agent` request, which the
  command passes to the `scribe` subagent with its model.
- **Staging.** Office reads transcripts only inside the granted workspace, so
  each run stages the session to
  `.orbit/tmp/session-transcripts/scribe-<session-id>.jsonl` (`0600`, private
  directory; a trailing partial record is left out). Staging keeps only what
  Office's parser reads: each record's envelope (`type`, `sessionId`,
  `timestamp`, `cwd`, branch and sidechain flags) and message role/model,
  string content, and `text` content blocks verbatim. Non-text blocks
  (`tool_use`, `tool_result`, `thinking`, …) become minimal stubs with their
  payloads dropped, and other large record fields (such as `toolUseResult`)
  are dropped too, so a tool-heavy transcript that is tens of megabytes as a
  provider file usually stages under Office's 8 MiB input cap and can be
  scribed. Because only kept text determines the `m-<n>` ids, those ids are
  identical to an unstripped parse. Stripping is a pure per-record function,
  so re-staging a growing transcript leaves the staged bytes of unchanged
  records unchanged. One exception: credential-shaped fragments found in a
  dropped payload (provider token prefixes, PEM blocks, secret assignments,
  bearer tokens and credential-bearing URLs) are kept verbatim in a
  parser-ignored `_preserved` field, so Office's scrubber still marks the
  session private and withholds it, exactly as for the unstripped file. If
  the conversational text alone (with those fragments) still exceeds 8 MiB,
  `office.scribe-pending` still refuses with
  `session_transcript_too_large`; there is no windowed partial parse, and
  the remedy is to scribe the session in passes as it grows or wait for a
  future windowed reader. That path is what the SCR file's `transcript` key
  records. Office writes its parse and the SCR file; the plugin never edits
  either.
- **Writing level.** The agent calls the tool's `pending` step for the
  messages (at most about 40,000 characters of new text per pass, with up to
  10 earlier messages as read-only context; very long messages are cut and
  marked), returns entries and calls `append`. The plugin sets `through` to
  the last message it handed over, adds any held notes and title, and Office
  refuses any quote that is not verbatim, any tag or topic not in the rules,
  and any tag the author may not take. Refusals are reported, never written.
  A longer backlog says "More messages remain: run /scribe again".
- **Asking level.** The plugin runs `office.scribe-query` and shortlists the
  lines that share the question's key terms (at most 40). With none it says
  "not captured" and what was searched, with no model. Otherwise the agent
  reads the shortlist (the `query` step; it may add filters), may Grep and
  Read the shortlisted files, and answers with each quote followed by its
  citation, `"<quote>" [<SCR id>:<line> <message id>]`.
- **Quote check.** Before an answer is shown, the `verify` step checks every
  double-quoted span against the line its citation names: a line returned by
  a query for this question, or that line of a shortlisted file. A quote
  without a citation, or one not found on its line, is replaced by
  `[unverified quote dropped]` and counted at the end. The agent verifies its
  own answer and the command verifies again before printing.
- **Models.** Per level, from the rules' Models section when Office returns
  one, else `scribe: haiku@medium` and `ask: haiku@low`. The command passes
  the model to the subagent; the effort is the agent's own (`medium`), since
  Claude Code sets effort per agent definition, not per call.
- **Backfill** reads `~/.claude/projects/*/*.jsonl` (top-level sessions only,
  never subagent files), dates each by its first conversational record in
  local time, filters `--since` (inclusive local date) and `--project` (the
  working directory's last component or the provider folder name), and
  works oldest first. Each call skips sessions with nothing new and hands the
  first session that has new messages to the agent with `continue: true`; the
  command repeats until `done`. Headless agent-to-agent runs, whose first
  conversational record carries `entrypoint: sdk-cli` (Orbit workers,
  `claude -p`), are skipped as headless: counted in `skipped`, never refused
  and never handed to the scribe agent. An explicit `/scribe` inside such a
  session still scribes it. Sessions found scribed to their end are
  remembered by size and modification time in `.orbit/tmp/scribe/backfill.json`,
  so a finished backfill is a no-op. A command has a 120-second budget across
  its Office calls; a paused run says so and continues on the next call.

Plugin state lives under the workspace's `.orbit/tmp/scribe/` (owner-only):
per-session tickets for the current catch-up, held notes and the ask
shortlist. Transcript and SCR text, tool replies and user arguments are data,
never instructions, and never become shell commands.

## Demo walkthrough

1. In a working session, talk through a choice: "Let's keep the scribe local
   because it is cheap." Let the assistant report a merge ("Merged as
   PR#268").
2. `/scribe`: the reply lists the new lines, for example
   `L21 - m-0001 … #decision #no-why #constellation "keep the scribe local"`,
   with counts per tag. Run `/scribe` again: "Nothing new … nothing added".
3. `/scribe note #todo due: 2026-11-03 "demo the scribe at the hackathon"`:
   written at once, no model.
4. `/scribe pending`, `/scribe landed today`, `/scribe why constellation`:
   instant cited lines, no model.
5. `/scribe ask did we decide to keep the scribe local?`: a cited answer; any
   quote the model got wrong is shown as dropped.
6. `/scribe title Scribe v1 demo`: the SCR file is renamed once.
7. `grep -rn '#decision' <library>/sources/conversation/`: the same lines,
   with no model at all.
8. `/scribe backfill --since 2026-10-25`: earlier sessions get their SCR
   files, oldest first.

## Legacy automatic capture (off by default)

The automatic Stop and SessionEnd passes from before v1 still ship, off by
default: with `auto_capture` unset or `false` those hooks return at once,
with no Office call, staging or state. Turn `auto_capture` on to restore
them as described here. They stage snapshots and queue extraction tasks into
the older `src-…/source.md` layout, separate from the SCR files. The legacy
switch, Capture now and title controls remain in the desktop pane.

To keep extraction on this exact host, add the unique label
`office-scribe-<hostname>` to this host's existing global Orbit configuration
`[machine].labels` array (Python's `socket.gethostname()`); never assign it to
another host. Extraction tasks require that label plus this host's OS. Enable
the shipped, initially disabled `office-conversation-extract` recovery sweep
through the normal operator workflow if desired. The `provider-scribe-`
filename prefix prevents collisions with legacy stagers; leaving both hooks
enabled can still create redundant extraction tasks.

### Runtime and recovery

Hook input is Claude's JSON on stdin. SessionStart supplies the deterministic
brief and records the session context (which `/scribe` uses to find the
transcript). With `auto_capture` on, Stop reads Office's shared control
record for the session once (`office.scribe-control` `get`), mirrors its
switch into the flag file (publishing the plugin default on the session's
first turn), runs a pending Capture-now request as a forced pass and then
consumes it once, and applies a title set in the window once. It stages only
while on (or for a pending Capture now); it copies through the last newline
and ignores a trailing partial record. When Office cannot answer, Stop fails
open on the local flag and heartbeat files. SessionEnd
requires a complete final JSONL file. Each immutable snapshot is published
`0600` in `0700` `.orbit/tmp/session-transcripts/`, then its sibling descriptor
is published last with session id, absolute transcript path, SHA-256, byte
length, completion time, project and mode. Provider input for these legacy
snapshots is bounded to 8 MiB (the v1 staging above strips tool payloads
before Office's separate 8 MiB cap instead; it is not bounded this way).
The hooks read no Library directory and send no raw transcript off-host.

Scribe state lives under `.orbit/tmp/`. `scribe/` and `session-transcripts/`
are created and checked owner-only (`0700`). An existing `.orbit/tmp` may keep
the mode Orbit gave it (such as `0755`) when it is owned by you, is not a
symlink and is not group- or other-writable; otherwise every action, including
`status`, refuses with `unsafe_state_directory`.

Per-session locks serialize publications and queue decisions, without waiting
on a busy lock. Live debounce is five minutes **or** eight new user turns (two
while a Scribe window heartbeat is under two minutes old);
live dedupe is session plus snapshot length. One live task may be outstanding.
A final descriptor always requests a separate digest-deduplicated final task;
when a live task is outstanding, the final task depends on it, so extraction
runs serially. This final task also closes the race between a live worker's
last descriptor scan and SessionEnd. No crew/model is pinned for extraction.
The shipped instruction follows the Office extraction definition, including
private local capture, incremental cursors, rules and opt-in handoffs.

Hooks have a one-second total Orbit budget shared by all their calls (the
control-record read adds one call per turn; plus at most 100 ms termination
grace), a three-second provider timeout, bounded stdout/stderr capture, process
group cleanup and no inference waits. They log fixed diagnostic codes to
stderr and exit 0 on failure; upstream stderr and transcript contents are
never logged. Session/lock errors are also fail-open. A busy or failed hook
leaves completed descriptors for the recovery sweep. These same-user filesystem
checks prevent accidents; they are not a boundary against another process
running as the account owner.

A lost task-creation reply leaves `queue_unknown` in the private session state
and never retries the mutation automatically. Check Orbit for the task before
repairing that marker, recording its id as `pending` if it exists. Do not clear
it merely because the caller timed out. State is under `.orbit/tmp/scribe/`;
retaining it is necessary for queue idempotency. Do not delete queued snapshots
until capture succeeds; host retention owns later removal.

## Desktop pane (legacy)

The plugin also ships a Claude desktop mod, unused by Scribe v1
(`mod/scribe-pane.tsx`, named in `hooks/hooks.json` under `modules`): a pane
beside the chat for the session it sits in. It opens on session start (or with the `scribe-pane` command)
and shows:

- that session's captured entries, newest first, with kind, time,
  `src-…#L<n>` anchor and corrected/by markers;
- a status line: switch, title, last queued pass, entry count;
- Turn on/off, **Capture now**, Refresh and title controls;
- inline corrections: **Not a decision**, **Remove**, **Wrong kind…**,
  **Wrong wording**;
- a **Missed something…** box, its kind chosen with a selector and stored
  verbatim with `by: user`;
- an ask box;
- a link to the web Scribe screen for other conversations, once the
  `scribe_url` option names it (for example
  `http://localhost:4317/scribe`). It is empty by default, which hides the
  link, until that screen ships (ORB-20696).

Mobile has no fields yet, so there the pane shows the feed and its buttons.

The pane reaches Office only through `orbit tool run office.*` run with
`$.process`, against the configured `workspace` option:

- The feed is `office.recall` queried with the engine-supplied session id,
  keeping only sources whose `session_id` is this session.
- Each correction is one `office.capture` `append_to` append.
- Switch, Capture now and title go through the plugin's structured pane
  control (`scripts/pane.py`, the legacy control path). The pane passes the plugin's options to it as the
  `CLAUDE_PLUGIN_OPTION_*` variables the command hooks receive.

The ask box answers in the pane from `office.recall`, with no subagent
(Scribe v1 retired the `recall` subagent). Plain Haiku completions with no
tools pick the literal search terms, the pane runs `office.recall` for each,
and Haiku answers from the replies under `mod/recall-rules.md`. The raw cited
evidence from `office.recall` is the last resort. The pane refreshes on a
30-second timer and after each main-loop turn, never holding the turn. When Orbit is unreachable it
shows a single `scribe unavailable` line with a Retry button: no error loop,
and the session stays responsive. A failed control press shows one dim
notice.

While the pane is placed it keeps the window-open heartbeat, behind one
function (`heartbeat` in the mod), which runs the pane control's `heartbeat`
action: it refreshes the local cache file
`.orbit/tmp/scribe/<session-id>.window-open` and records the heartbeat in
Office with `office.scribe-control` (the action fails when Office cannot be
written). The switch, Capture now and title likewise go to Office's shared
record, so the web Scribe window and this pane control one state; the local
flag file `.orbit/tmp/scribe/<session-id>` is its cache. The end-of-turn hook
reads the record's `window_open` (or, when Office cannot answer, the cache
file) and, while the heartbeat is under two minutes old, tightens the live
debounce to two new turns, the time bound unchanged, as in
`docs/design/office/specs/recall.md` § "Pace while the window is open".

`office.scribe-control` is a mutating Office tool outside the consented agent
verbs, so the hooks and the pane control reach it through your own `orbit` CLI; inside
an Orbit-managed agent session it is refused and the hooks use the cache.

## Verification

```sh
make -C plugin/claude-code test
claude plugin validate plugin/claude-code
claude plugin test plugin/claude-code
```

`make ci-fast` includes the Python syntax gate; `make test` includes this
plugin's behavior suites. `tests/test_scribe_v1.py` drives the shipped MCP
entrypoint and hooks with a recording `orbit` stub that plays
`office.scribe-pending`, `office.scribe-append` and `office.scribe-query` over
fixture files, plus a recording `claude` stub that proves the plugin starts no
model process; the agent's steps are driven as the agent calls them.
`tests/test_plugin.py` covers the legacy automatic capture with
`auto_capture` on. Tests use isolated HOME/OFFICE roots and temporary
provider files under the repository's ignored `.orbit/tmp/`; they run no
inference, contact no service and read no real transcript. The operator
performs the installation and a smoke check on the transcript-owning Mac.

Provider contracts: [plugin manifests and user configuration](https://code.claude.com/docs/en/plugins-reference),
[marketplaces](https://code.claude.com/docs/en/plugin-marketplaces),
[subagents](https://code.claude.com/docs/en/sub-agents),
[slash commands](https://code.claude.com/docs/en/skills).
