---
title: Claude Code scribe — Decisions
owner: codex
last_updated: 2026-11-01
last_validated: 2026-11-01
status: Active
feature: claude-code-scribe
doc_role: decisions
type: design
summary: Local provider staging, serialized extraction tasks and a recall-only subagent bridge
tags: [claude-code-scribe]
paths: ["plugin/claude-code/**"]
related_features: [office]
related_artifacts: [ORB-20689]
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

## Task References

- [ORB-20689] — built the Claude Code provider scribe and recall client.

> Resolve any task above with `orbit task show <ID>` or `git log --grep=<ID>`.
