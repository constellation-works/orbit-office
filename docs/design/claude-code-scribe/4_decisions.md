---
title: Claude Code scribe — Decisions
owner: codex
last_updated: 2026-11-01
last_validated: 2026-11-01
status: Active
feature: claude-code-scribe
doc_role: decisions
type: design
summary: Local provider staging, serialized extraction tasks, and the Scribe v1 command surface (structured router, scribe subagent, quote check)
tags: [claude-code-scribe]
paths: ["plugin/claude-code/**"]
related_features: [office]
related_artifacts: [ORB-20689, ORB-20799]
---

# Claude Code scribe — Decisions

## Local workspace selector and host placement

**Recorded:** 2026-11 · [ORB-20689]
**Code anchors:** `plugin/claude-code/scripts/scribe.py::Scribe`, `Scribe.queue`

Office calls use only `orbit tool run office.*`. Queueing itself is an Orbit
operation through `orbit.task.add`, not Library access. A local checkout path
is an existing Orbit workspace selector and also locates granted staging;
remote selectors cannot keep raw transcripts on their owning host. Tasks
require the unique operator-configured `office-scribe-<hostname>` capability
label as well as the host OS. This does not introduce a new tool, grant, crate
or dispatcher. Tasks still follow normal host approval and dispatch policy.

A final task depends on an outstanding live task, rather than merely leaving
the final descriptor for a live task that may already have completed its last
scan. Thus live passes are single-flight and the final pass is guaranteed a
queue intent without concurrent extraction. Files use a distinct immutable
prefix to avoid legacy stager races. Descriptor enumeration remains compatible
with the existing disabled-by-default recovery sweep.

Cost: installation requires the host label and existing Office deployment;
missing placement fails open. Legacy hooks must be retired by the operator.

## Raw staging and fail-open hooks

**Recorded:** 2026-11 · [ORB-20689]
**Code anchors:** `plugin/claude-code/scripts/scribe.py::Scribe.snapshot`, `scripts/process.py::run`

The task and sessions spec require raw digest-bound local JSONL staging before
Office's scrubber. This is an explicit narrow deviation from **STD-05@1 §R13**:
raw staging alone bypasses text redaction so the adapter can validate bytes
and provenance. Those files remain owner-only, never enter task artifacts or
the Library, and are never sent to inference. Backend previews scrub before
any crew sees evidence and withhold private text. Logs contain only fixed
codes and omit upstream text. Paths and session identifiers are local metadata.

Hook waits are bounded to one second of Orbit time and never wait for a busy
session lock or for inference. The supervisor observes exit without reaping,
then sweeps the owned process group and reaps it; macOS Python builds without
waitid use kqueue. Missing exit-observation capability refuses before spawn.
Lost mutation replies preserve a durable unknown marker rather than retrying,
following STD-03@2 §R31. The task.add schema's guarded request_id form cannot
carry the required low complexity, tags or placement constraints, so it cannot
be substituted for ordinary creation.

Cost: a timed-out hook may miss immediate queueing; the staged descriptor is
recoverable. An unknown creation requires operator reconciliation, not an
unsafe automatic retry. Hook checks are same-user accident guards per
STD-05@1 §R5, not a security boundary against that account owner.

## Structured commands and isolated recall

**Recorded:** 2026-11 · [ORB-20689]
**Code anchors:** `plugin/claude-code/scripts/mcp.py`, `commands/`, `agents/recall.md`

The plugin supplies a small standard-library stdio MCP bridge. Its recall
operation calls only `office.recall`; the recall agent allows only that MCP
tool and uses Haiku. The separate scribe operation accepts structured action
arguments, so titles and questions never become interpolated shell commands.
A forked recall command returns only its cited answer to the parent session.
A deterministic evidence rendering preserves source anchors, session metadata,
live refs and empty-result coverage for the answering agent.

Claude plugin slash commands retain the provider's namespace. The documented
canonical forms are `/office-scribe:scribe` and `/office-scribe:recall`; short
names describe these same commands, not globally installed command aliases.
No extra global files are written to claim unqualified names.

Cost: users see the provider namespace, and generated answers still depend on
the agent following its source-only instructions. Offline tests verify the
bridge's actual evidence output without making a hosted inference call.

## Scribe v1 command surface

**Recorded:** 2026-11-01 · [ORB-20799]
**Code anchors:** `plugin/claude-code/scripts/scribe_v1.py::V1`, `scripts/mcp.py`,
`commands/scribe.md`, `agents/scribe.md`

### Context

Scribe v1 ([specs/scribe.md](../office/specs/scribe.md)) makes `/scribe` the
only trigger, wants canned queries and tagged notes to cost no model, and
wants every quote in an answer checked before display. A Claude Code slash
command always runs through the session's model, and the Office tools
(`office.scribe-pending`, `office.scribe-append`) read transcripts only inside
the granted workspace and need a `through` cursor on every append.

### Decision

- **One structured router.** The command hands its arguments, as one data
  string, to the plugin's `scribe` MCP tool, which parses them in code.
  Model-free commands finish in the tool and return a `display` with
  `model_call: false`; the command prints it. A catch-up, an untagged note,
  `ask` and backfill return an `agent` request (subagent, model, prompt) that
  the command passes to the `scribe` subagent. The agent works through the
  same tool's steps (`pending`, `append`, `query`, `verify`), so transcript
  text never passes through the parent session.
- **The plugin owns the cursor.** It sets `through` to the last message it
  handed the agent (at most about 40,000 characters of new text per pass),
  never the model. A note or title on a session with no `scribed_through` is
  held in private plugin state and written with the first catch-up: moving
  the cursor to anchor a note would mark unread messages scribed.
- **Staging.** Each run copies the provider transcript's complete records to
  `.orbit/tmp/session-transcripts/scribe-<session-id>.jsonl` (`0600`); that
  path is what Office reads and records. It carries no descriptor, so the
  legacy extraction sweep ignores it.
- **Quote check in code.** The `verify` step keeps a double-quoted span only
  when the line its `[<SCR id>:<line>]` citation names, from this question's
  query results or that line of a shortlisted file, contains it. Both the
  agent and the command run it, so an orchestrator calling the agent
  directly gets the same check.
- **Ask shortlist.** `office.scribe-query` has no free-text filter, so the
  plugin queries once (limit 500) and keeps the lines sharing the question's
  key terms (at most 40). An empty shortlist answers "not captured" with no
  model.
- **Automatic passes off.** The `auto_capture` option (default false) gates
  the Stop and SessionEnd hooks before any Office call. SessionStart keeps
  the brief and records the session context the commands use.
- **Recall retired from the plugin surface.** `/recall`, the `recall`
  subagent and the bridge's `office.recall` tool are removed; the desktop
  pane's ask box answers in the pane from `office.recall` under
  `mod/recall-rules.md`.

### Deviations and limits

- **Models.** The rules returned by `office.scribe-pending` have no Models
  section yet, so the defaults apply (`scribe: haiku@medium`,
  `ask: haiku@low`); the plugin reads `rules.models` when Office returns it.
  Claude Code takes a model per subagent call but effort only from the agent
  definition, so both levels run at the agent's `effort: medium`.
- **Query contract.** `office.scribe-query` was built in parallel
  (ORB-20798). The plugin sends the filters that task names (`mode`,
  `matter`, `topic`, `since`, `until`, `limit`) and reads result lines from
  `results` (or `lines`, `matches`) with `text`, `path`, `line`, `scr_id` or
  `id`, `msg`, `by`, `counterpart` and `resume`. `why <x>` asks by matter,
  then by topic. Plugin tests use fixture responses.
- **`by: <crew>`.** An orchestrator's note takes its crew from `--by <crew>`,
  else `ORBIT_CREW`, else `ORBIT_AGENT_NAME` in an Orbit agent session.

Cost: one more MCP round trip per agent step, plugin tickets under
`.orbit/tmp/scribe/`, and a held note is not in the SCR file until the first
catch-up. The command's routing still depends on the session model following
`commands/scribe.md`; the deterministic parts are tested through the MCP
entrypoint without inference.

## Scribe v1 staging strips tool payloads

**Recorded:** 2026-11-01 · [ORB-20849]
**Code anchors:** `plugin/claude-code/scripts/scribe_v1.py::V1.stage`,
`_strip_record`, `_credential_fragments`;
[specs/scribe.md](../office/specs/scribe.md#staging-tool-payloads-are-not-staged)

### Context

`office.scribe-pending` refuses a staged transcript over 8 MiB
(`session_transcript_too_large`), and long orchestration sessions — mostly
tool output — could never be scribed. Office's parser keeps only `text`
content blocks and string content, so the staged tool payloads were bytes
the scribe never reads. Lifting the cap blindly would admit arbitrarily
large raw transcripts; a windowed parse for a conversation whose text alone
is over the cap would need a Rust change in `office-store` and is not done
here.

### Decision

This supersedes the "complete records" staging bullet in
[Scribe v1 command surface](#scribe-v1-command-surface). `V1.stage` stages,
per record, only the envelope and message fields the parser reads plus text
verbatim; non-text blocks become minimal stubs with payloads dropped, and
other large record fields are dropped. Stripping is a pure per-record
function, so a growing transcript's unchanged staged prefix is byte-stable.
Credential-shaped fragments in dropped payloads are preserved verbatim in a
parser-ignored `_preserved` field, mirroring `office-core::scrub`, so a
session whose only credential sat in a tool result is still marked private
and withheld. If the kept text alone exceeds 8 MiB the call still refuses
with `session_transcript_too_large`, as documented in the spec.

### Consequences

- Tool-heavy sessions well over 8 MiB as provider files stage under the cap
  and scribe with the same `m-<n>` ids as an unstripped parse.
- Cost: the staged file is no longer a byte copy of the provider transcript,
  so its digest and per-line prefix digests describe the staged bytes, not
  the provider's; anyone needing raw tool output must read the provider
  file, and the credential-fragment mirror must track `scrub`'s shapes.

## Task References

- [ORB-20689] — built the Claude Code provider scribe and recall client.
- [ORB-20799] — Scribe v1 agent and `/scribe` commands in the plugin.
- [ORB-20849] — made long, tool-heavy sessions scribable by stripping tool payloads at staging.

> Resolve any task above with `orbit task show <ID>` or `git log --grep=<ID>`.
