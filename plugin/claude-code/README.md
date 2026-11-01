# Office scribe for Claude Code

A per-user Claude Code plugin with a session brief, local live/SessionEnd
staging, `/scribe` controls and cited `/recall`. Requires Python 3.10+ on
macOS or Linux, Claude Code with plugin user configuration, and a locally
installed Orbit Office plugin with `office.capture`, `office.session-brief`
and `office.recall`. No Library path, credentials, model SDK or third-party
Python packages are used here.

## Install on the transcript-owning Mac

Clone this repository at a reviewed revision, then register its marketplace:

```sh
claude plugin marketplace add /absolute/path/to/orbit-office/plugin/claude-code
claude plugin install office-scribe@constellation-office --scope user \
  --config workspace=/absolute/path/to/local/office-workspace \
  --config default_on=true \
  --config 'excluded_projects=["/absolute/path/to/excluded-project"]'
```

The workspace value is an absolute local registered Orbit checkout. Orbit
accepts that path as a workspace selector; remote host selectors are refused.
Use the checkout that already has Office enabled and the intended Library
attached. Configure these same options through Claude's plugin configuration
UI to change them later. `default_on` defaults to false; it affects live
passes in new sessions. SessionEnd always captures eligible projects.
Exclusions apply to the physical directory and all descendants, including
symlink aliases and transcript working directories, before staging. They
apply to `now`, `title` and backfill too.

To keep extraction on this exact host, add the unique label
`office-scribe-<hostname>` to this host's existing global Orbit configuration
`[machine].labels` array. Use the value of Python's `socket.gethostname()`;
keep any existing labels, and never assign this label to another host.
The host must serve the selected workspace. Extraction tasks require that
label plus this host's OS; missing placement is refused rather than routing
transcripts elsewhere. The host's usual task admission, approval and dispatcher
policy still applies: this client creates low-complexity tasks and never
approves, dispatches or waits for inference. Enable the shipped, initially
disabled `office-conversation-extract` recovery sweep through the normal
operator workflow if desired; its manifest registration remains a separate
integrator task. Staging is retained for recovery, never uploaded as task
artifacts. Only staged paths, digests and session identifiers enter tasks.

Restart Claude after installation and check `/hooks`, `/agents` and `/mcp`.
The stable provider-qualified commands are `/office-scribe:scribe` and
`/office-scribe:recall`; these are the plugin's `/scribe` and `/recall`
commands under Claude's standard namespacing. `/recall` runs the plugin's
Haiku `recall` subagent with only the bridge's `office.recall` tool allowed.
The bridge also provides a structured `scribe` control tool to the command;
that tool is absent from the recall agent's allowlist. Permission prompts
remain governed by the user's Claude and Orbit settings.

After verifying this plugin's SessionStart brief, the operator can retire
constellation's `operations/scripts/session-brief.py` hook. Replace its old
SessionEnd stager too if installed. This repository does not modify either.
The `provider-scribe-` filename prefix prevents publication collisions with
legacy stagers; Office's session/digest idempotency prevents duplicate captures,
but leaving both hooks enabled can still create redundant extraction tasks.

## Commands

- `/scribe on` / `/scribe off`: change this session's live switch. Off keeps
  existing captured evidence and still permits SessionEnd extraction.
- `/scribe now`: stage complete records and request a live pass immediately,
  bypassing the time/turn debounce. An unchanged snapshot is a no-op; an
  outstanding live pass is reused instead of starting concurrent extraction.
- `/scribe status`: read the switch and last queued task without creating
  state or querying its lifecycle. `task_status: not_checked` is explicit;
  queued does not mean captured.
- `/scribe title <text>`: set `title_by: user` through `office.capture`.
  This performs conservative local capture of the current complete snapshot,
  ensuring a title change cannot skip unselected evidence. Later crew title
  proposals cannot overwrite it. No model call occurs in this command.
- `/scribe backfill [--since YYYY-MM-DD]`: process local top-level provider
  transcripts, oldest first by their first conversational UTC timestamp.
  Excluded projects and subagent files are omitted; incomplete or malformed
  candidates count as refused. Repeating the command queues no duplicate
  session/digest. A 30-second command budget bounds a batch; rerun to continue
  after a deadline. This is additive capture, not a deletion or cleanup.
- `/recall <question>`: return captured evidence with `src-…#L<n>` citations,
  conversation title/project/date and a resume command. Status comes only
  from live refs; unresolved PR/commit refs stay unresolved. An empty result
  says **not captured**, citing coverage (earliest capture, private sessions,
  uncaptured providers). Office uses literal AND search, so the recall agent
  searches the question's key terms. Tool failure means unavailable, not empty.

## Runtime and recovery

Hook input is Claude's JSON on stdin. SessionStart supplies the deterministic
brief and records the session context. Stop stages only while on; it copies
through the last newline and ignores a trailing partial record. SessionEnd
requires a complete final JSONL file. Each immutable snapshot is published
`0600` in `0700` `.orbit/tmp/session-transcripts/`, then its sibling descriptor
is published last with session id, absolute transcript path, SHA-256, byte
length, completion time, project and mode. Provider input is bounded to 8 MiB.
Nothing reads a Library directory or sends raw transcripts off-host.

Per-session locks serialize publications and queue decisions, without waiting
on a busy lock. Live debounce is five minutes **or** eight new user turns;
live dedupe is session plus snapshot length. One live task may be outstanding.
A final descriptor always requests a separate digest-deduplicated final task;
when a live task is outstanding, the final task depends on it, so extraction
runs serially. This final task also closes the race between a live worker's
last descriptor scan and SessionEnd. No crew/model is pinned for extraction.
The shipped instruction follows the Office extraction definition, including
private local capture, incremental cursors, rules and opt-in handoffs.

Hooks have a one-second total Orbit budget (plus at most 100 ms termination
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

## Desktop pane

The plugin also ships a Claude desktop mod (`mod/scribe-pane.tsx`, named in
`hooks/hooks.json` under `modules`): a pane beside the chat for the session
it sits in. It opens on session start (or with the `scribe-pane` command),
shows that session's captured entries newest first with kind, time,
`src-…#L<n>` anchor and corrected/by markers, a status line (switch, title,
last queued pass, entry count), Turn on/off, **Capture now** and title
controls, inline corrections (**Not a decision**, **Remove**, **Wrong
wording…**, **Wrong kind…**), a **Missed something…** box (kind chosen with
the Missed kind selector, stored verbatim with `by: user`), an ask box and
a link to the web Scribe screen for other conversations (the app's default
local address, `http://localhost:4317/scribe`).

The pane reaches Office only through `orbit tool run office.*` run with
`$.process`: the feed is `office.recall` queried with the engine-supplied
session id (recall matches it against each source's session identity and
returns that session's entries newest first), corrections are one
`office.capture` `append_to` append per choice, and switch/Capture
now/title go through the plugin's structured pane control
(`scripts/pane.py`, the same command path as the `scribe` MCP tool). The
ask box spawns the plugin's `recall` subagent; when a surface returns no
subagent answer it renders the same cited evidence from `office.recall`
directly. It refreshes on a 30-second timer and after each turn. When
Orbit is unreachable it shows a single `scribe unavailable` line with a
Retry button — no error loop, and the session stays responsive.

While the pane is open it keeps the window-open heartbeat: every refresh
runs the pane control's `heartbeat` action, which touches
`.orbit/tmp/scribe/<session-id>.window-open`; the end-of-turn hook reads
that file and, while it is under two minutes old, tightens the live
debounce to two new turns (the time bound unchanged), as in
`docs/design/office/specs/recall.md` § "Pace while the window is open".
The web Scribe screen is expected to refresh the same heartbeat file for
its session when it is built.

## Verification

```sh
make -C plugin/claude-code test
claude plugin validate plugin/claude-code/.claude-plugin/plugin.json
claude plugin test plugin/claude-code
```

`make ci-fast` includes the Python syntax gate; `make test` includes this
plugin's behavior suite. Tests drive the registered hook commands and stdio MCP entrypoint with a
recording Orbit executable, isolated HOME/OFFICE roots and temporary provider
files under the repository's ignored `.orbit/tmp/`. They do not run inference,
contact services or inspect real transcripts. Linux verifies local installation
and runtime behavior; the operator must perform the installation and smoke
check on the transcript-owning Mac, including its native process observer.

Provider contracts: [plugin manifests and user configuration](https://code.claude.com/docs/en/plugins-reference),
[marketplaces](https://code.claude.com/docs/en/plugin-marketplaces),
[subagent tool restriction](https://code.claude.com/docs/en/sub-agents),
[isolated slash commands](https://code.claude.com/docs/en/skills#run-skills-in-a-subagent).
