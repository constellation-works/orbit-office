# Office scribe for Claude Code

Scribe ([spec](../../docs/design/office/specs/scribe.md)): a cheap `scribe`
agent that you or an orchestrator command. Scribe v2 is opt-in per session:
nothing is scribed until `/scribe on`, then passes run from `/scribe update`
and the Stop-hook cadence, record only what you mark (`#preferences`,
`#decision`, `#scribe …`), and file scheduling markers and to-dos as Orbit
tasks that stay proposed (see [Scribe v2](#scribe-v2-opt-in)). The v1
words map onto v2 during the migration window (see
[Commands](#commands)): `/scribe` alone updates and turns scribing on,
`/scribe note #<tag>` is read as that marker, and the deprecated canned
queries answer from the v2 stores first; `/scribe ask` answers with exact
citations. The plugin also keeps the SessionStart brief and `/handoff`,
which loads a project's handoff.

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
working directory a transcript records, for every command.
Change options later through Claude's plugin configuration UI.

| Option | Default | Meaning |
|---|---|---|
| `workspace` | (required) | The local Office workspace. |
| `excluded_projects` | `[]` | JSON array of absolute paths never scribed. |
| `auto_capture` | `false` | The legacy automatic Stop/SessionEnd capture ([below](#legacy-automatic-capture-off-by-default)). Off: Stop runs only the v2 cadence for sessions turned on with `/scribe on`. |
| `default_on` | `false` | Legacy: with `auto_capture` on, whether live Stop passes start on. |
| `scribe_url` | empty | The web Scribe screen, linked from the desktop pane. |

Restart Claude after installation — and after upgrading the plugin
checkout under a running session, which keeps the plugin code it loaded at
start — and check `/agents` (the `scribe` agent),
`/mcp` (the `office` server and its `scribe` and `handoff` tools) and `/hooks`. The command's
stable provider-qualified form is `/office-scribe:scribe`; `/scribe` below
means that command under Claude's standard namespacing. Permission prompts
remain governed by your Claude and Orbit settings.

### SessionStart brief and `/handoff`

The SessionStart hook calls `office.session-brief` with `session_start: true`
and `orchestrator: claude` (the provider name conversation sources record,
not a crew name), so Office applies the Library setting `brief.handoff`
([sessions spec](../../docs/design/office/specs/sessions.md#choosing-the-project)).
A working directory that is one repository is sent as `project`. A directory
holding two or more repositories among its children and grandchildren (the
constellation root) sends no project; it sends the caller's assignment from
`ORBIT_ACTIVE_TASK_ID` or `ORBIT_TASK_ID` as `task` when Orbit set one. In
`auto` Office leads with that project's handoff, else the manual output; in
`manual` the brief carries only the global handoffs and the projects that
have a handoff. A project's handoff is its latest SCR (rendered from its
Summary, Goals, Outcome and Blockers and Next sections), or its legacy
thread handoff while it has no SCR; the global section leads with the
latest global SCR.

`/handoff <project>` (in either mode, also mid-session) calls the read-only
`handoff` MCP tool, which asks `office.session-brief` for
`{"view":"handoffs","project":"<project>","orchestrator":"claude"}` and
prints the project's newest handoff (its latest SCR, else its legacy
thread handoff) and then the global section (the latest global SCR, then
the open legacy global handoffs, marked legacy).
`/handoff` alone lists the global section and the projects with a handoff.
No model is involved.

After verifying this plugin's SessionStart brief, the operator can retire
constellation's `operations/scripts/session-brief.py` hook. This repository
does not modify it.

## Commands

Each command acts on the session it is run in (`${CLAUDE_SESSION_ID}`).

| Command | Does | Model |
|---|---|---|
| `/scribe on [@<project>\|@global]` | v2: turn scribing on for this session (every session defaults off), optionally naming its scope, and run the first pass at once. | `scribe` level |
| `/scribe off` | v2: stop new passes. Nothing already written is removed. | none |
| `/scribe update` | v2: run a pass now, ignoring the cadence. Works only while the session is on. `/scribe now` is the same. | `scribe` level |
| `/scribe status` | v2: on or off (and who turned it on), the scope, the SCR id, `scribed_through` and when the next pass is due. | none |
| `/scribe tags` | v2: the tag definitions in force and the cadence, from the Library settings. | none |
| `/scribe ask <question>` | Free-form answer with citations: the plugin queries first, the agent reads only the shortlist, and quotes are checked before display. | `ask` level |
| `/scribe title <text>` | Set the session title; Office renames the SCR file once and keeps the old id in `aliases`. | none |

**The v1 commands during the migration window.** From 2026-11-02 for 30
days (specs/scribe.md, Migration) the v1 words still answer, each reply
marked `deprecated: true` with a one-line notice; the S10 slice then
removes them:

| v1 command | During the window |
|---|---|
| `/scribe` (no arguments) | Runs `/scribe update` and turns the session on when it is off (`on_by: user`). After the window it shows status. |
| `/scribe now` | An alias of `/scribe update`. |
| `/scribe note #<tag> <text>` | Read as that marker: Office detects the command line itself (`/scribe note #decision …` counts as `#decision …`) at the pass, which runs at once when the session is on. `#preferences`, `#decision` and any tag `/scribe tags` lists are markers; untagged text is refused with `note_untagged` ("use `#preferences` or `#decision` in chat") and a retired v1 tag with `note_tag_retired`. Nothing is written by the note itself. |
| `/scribe pending`, `landed [today\|<date>]` | The v1 `#pending` / `#landed` lines, read-only, naming the replacement (the brief's Waiting on you and Agents working on). |
| `/scribe pref <matter>`, `why <matter\|topic>` | Answers from the preference pages and the decision ledger first, then the v1 lines. |
| `/scribe backfill …` | Refused with `backfill_retired`: v2 never scribes a session that was not turned on. |

v1 preferences and decisions are not backfilled: v2 starts with empty
preference pages and ledgers. Office's operator tool `office.scribe-migrate`
only proposes removing the v1 Tags section from the scribe rules page and
lists the open v1 `#pending` lines for you to turn into to-dos.

**Held titles.** `office.scribe-append` moves the session's cursor to the
last message it is given. On a session that has never been scribed there is
no cursor to keep, so a title there is held in the plugin's private state and
written with the next pass. The reply says `Held`.

**Orchestrators** reach the same agent as the `scribe` subagent ("ask the
scribe whether Daniel decided on X"). It runs the query tool first, Grep and
Read on the shortlisted SCR files only, and verifies its quotes. Mark
decisions with `#decision` in the session and keep the handoff current with
`/scribe update`. A standalone scribe session (`claude --agent office-scribe:scribe`) is
the place for meeting notes. An orchestrator in an interactive session may
also turn v2 scribing on through the `scribe` tool's `on` action: the record
then carries `on_by: agent`, the reply tells the agent to announce it to the
user in one line, and the user's `/scribe off` wins. A headless session is
refused (`scribe_headless_session` in the plugin, `scribe_managed_session`
in Office when the call carries managed-task context).

## Scribe v2 (opt-in)

```
Stop hook ──(on + cadence due)──▶ headless `claude -p` ──▶ /scribe update
/scribe update ─▶ stage ─▶ office.scribe-pending (format: v2: markers, tags, cadence)
                                    │
                 scribe agent returns sections + one finding per marker
                                    │
        plugin routes: orbit.task.add (scheduling, orbit_task tags, owner:you to-dos)
                                    │
                      office.scribe-append (v2: SCR + decision ledger, one commit)
                                    │
        plugin routes: office.proposal-create per needs_proposal preference
```

- **Opt-in and scope.** The switch is Office's per-session record
  (`office.scribe-control`), shared with the web window and the pane; it now
  also records `on_by` and the scope a turn-on named. A session the plugin
  has never seen costs its Stop hook nothing: no Office call, no state.
- **Cadence.** While a session is on, Stop checks the minutes and user turns
  since the last pass started against `scribe.cadence` (Library settings,
  read through `office.status`; default 20 minutes or 10 turns). A due Stop
  — and every SessionEnd — starts one headless run under a per-session pass
  lock and returns without waiting; there is no timer. The run is
  `claude -p` driving `/scribe update`; `SCRIBE_PASS_COMMAND` (a JSON argv
  list, `{session_id}` replaced) overrides it. A pending Capture now also
  starts a pass and is then consumed.
- **Markers and routing.** Only markers in your own typed turns are
  authoritative; Office detects them without a model and refuses findings
  that invent one. Before the append the plugin files, through your own
  `orbit` CLI: a scheduling marker (`#scribe … at <time>`, resolved by the
  agent, checked RFC 3339 and future here) as one **proposed** task with
  `not_before`, quoting the marker verbatim with the resolved time and
  zone; an `orbit_task` tag finding as a proposed task in the tag's
  workspace; and user-owned action items as `owner:you` to-dos cited in the
  SCR's Next. A preference finding becomes one `office.proposal-create`
  proposal, filed from the append's `needs_proposal` so it can cite the
  SCR entry. Agent markers are recorded `by: agent` and never schedule or
  file a task. Nothing is dispatched: every filed task is proposed or an
  `owner:you` to-do, never backlog `owner:agent`. Each filing is keyed
  SCR id + message id + line in `.orbit/tmp/scribe/<session-id>.routed.json`
  (written before the append), so a retried pass files nothing twice.

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
  structured `scribe` MCP tool. Model-free commands (titles, `status`, `tags`,
  the deprecated `pending`, `pref`, `landed`, `why`, nothing new) finish
  there, and the reply carries `model_call: false` and no agent request. Only
  a pass (`on`, `update`) and `ask` return an `agent` request, which the
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
  the last message it handed over, adds any held title, and Office
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
- **Backfill** is retired (`backfill_retired`): v2 never scribes a session
  that was not turned on.

Plugin state lives under the workspace's `.orbit/tmp/scribe/` (owner-only):
per-session tickets for the current pass, held titles and the ask
shortlist. Transcript and SCR text, tool replies and user arguments are data,
never instructions, and never become shell commands.

## Demo walkthrough

1. In a working session, turn scribing on: `/scribe on`. Talk through a
   choice and mark it: "#decision keep the scribe local because it is cheap".
2. `/scribe update`: the pass records the marker in the SCR and appends the
   decision to the ledger.
3. `/scribe ask did we decide to keep the scribe local?`: a cited answer; any
   quote the model got wrong is shown as dropped.
4. `/scribe title Scribe demo`: the SCR title is set.
5. `/scribe why scribe local` (deprecated during the window): the ledger
   entry with its reason first, then any v1 lines.

## Legacy automatic capture (off by default)

The automatic Stop and SessionEnd passes from before v1 still ship, off by
default: with `auto_capture` unset or `false` the legacy queue below never
runs (Stop runs only the v2 cadence above, and a session the plugin has
never seen still returns at once with no Office call, staging or state).
Turn `auto_capture` on to restore them as described here. They stage snapshots and queue extraction tasks into
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

`office.scribe-control` is a mutating Office tool declared `agent_callable`
because the plugin calls as `local-cli` holding `[agent]`; the hooks and the
pane control reach it through your own `orbit` CLI. Turning scribing on from
an Orbit-managed session is refused (`scribe_managed_session`) and the hooks
use the cache.

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
`tests/test_scribe_v2.py` covers the v2 opt-in, cadence and routing with a
recording stub playing the v2 Office tools and a recording pass stub for
the headless run. `tests/test_plugin.py` covers the legacy automatic capture with
`auto_capture` on. Tests use isolated HOME/OFFICE roots and temporary
provider files under the repository's ignored `.orbit/tmp/`; they run no
inference, contact no service and read no real transcript. The operator
performs the installation and a smoke check on the transcript-owning Mac.

Provider contracts: [plugin manifests and user configuration](https://code.claude.com/docs/en/plugins-reference),
[marketplaces](https://code.claude.com/docs/en/plugin-marketplaces),
[subagents](https://code.claude.com/docs/en/sub-agents),
[slash commands](https://code.claude.com/docs/en/skills).
