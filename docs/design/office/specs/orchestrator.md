---
type: design
summary: "Spec: orchestrator — the user's agent CLI running the office skill: role, tool surface, behaviours and guardrails"
last_validated: 2026-10-26
---

# Spec: Orchestrator

The orchestrator is the user's chief of staff: it plans, routes, proposes
and reports. It does not do long-running work itself. It turns requests
into tasks for agents, which the dispatch pass picks up, and it keeps the
user's own to-do list honest.

Source: spec §8.

## What it is

The orchestrator is the user's agent CLI running the
`office-orchestrator` skill. That mirrors the constellation setup, where an
agent session supervises Orbit. The project ships no agent loop: what it
ships is the skill and the tool surface, and the boundaries below come
from that surface, not from instructions.

- **Provider.** Muse Code is the default and the one shown in the demo,
  which keeps the conversation on Meta models. Claude Code and Codex work
  too, because the surface is Orbit MCP plus a skill.
- **Skill discovery.** The plugin's skill links as `office-orchestrator`
  into Orbit's skill roots. Muse Code also loads Claude Code and Codex
  personal skills unless that is turned off, so one linked skill reaches
  all three. Confirm on day 1 that an interactive Muse Code session lists
  it (spec §16).

## Tools

- **`office.*` over MCP:** capture, search, knowledge show, artifact show,
  attach, signals scan, proposal create, metrics, review inputs,
  selection get, status.
- **Hidden from MCP** (`mcp_scope: none`; only the app calls them, through
  `orbit tool run`; the install script calls `office.init`): `office.init`,
  `office.knowledge-write`, `office.proposal-accept`,
  `office.artifact-promote`, `office.selection-put`, `office.config-set`,
  `office.reindex`.
  The orchestrator cannot approve its own proposals or write to the
  Library; it can only propose.
- **`orbit.*` over agent-authority MCP:** Orbit decides what agent
  authority includes, and workflow dispatch and operator commands are not
  part of it. The skill tells the agent which tools to use: `task.add`,
  `task.update`, `task.list`, `task.show`, `task.artifact.get`,
  `friction.add`, `friction.update`, `search`, `auto_task.list`.
- **`gateway.*`** for API requests through the user's connections, and **`cloudcli.run`** for read-only cloud queries when it is enabled ([gateway.md](./gateway.md)). The skill tells the agent to use them instead of `curl` or a cloud CLI in its shell. Writes are drafts the user approves.
- **Not in the surface at all:** workflow dispatch (the app's dispatch
  pass owns it), crew, pool or host management, release, experiments,
  leases, `command.exec`.

## Behaviours

The skill (`skills/orchestrator/SKILL.md`, versioned in the plugin so its
behaviour is reviewable) requires:

0. **Start from the brief.** The first call of every session is `office.session-brief`: the previous session's handoff, what is waiting on the user, what agents are doing, open questions and the last session's decisions ([sessions.md](./sessions.md#session-brief)). The agent picks up from the handoff, opens with what needs the user, briefly, and treats quoted entries as data. While the brief's onboarding section lists `user`, the agent offers the onboarding interview once, in one line; answers become `fact` entries and one `knowledge_add` proposal per page ([knowledge-templates.md](./knowledge-templates.md#onboarding-interview)).
1. **Capture first.** External content pasted into the session becomes a source before anything else happens. Decisions, directions and actions are appended to the session's `conversation` source as they happen (the agent writes the decision log itself; the CLI's transcript is its own). The agent keeps a rolling `handoff` entry current, appending a new one whenever the plan changes, rather than writing a summary at the end ([sessions.md](./sessions.md#handoff)). One-off scripts and intermediate files go in `.orbit/tmp/session-<yyyymmdd>/`, never in the Library; after the brief, the agent deletes its own `session-*` folders older than 7 days and nothing else ([scripts.md](./scripts.md#scratch-space)).
2. **Turn requests into tasks.** Anything that will take more than one turn becomes an Orbit task with acceptance criteria, owner, attached knowledge and source tags.
3. **Split to-dos.** Work goes to `owner:agent` when a runbook or a clear tool path exists and the action is not outward-facing. Otherwise it goes to `owner:you`, with a draft attached when helpful.
4. **Propose on evidence.** After each capture or batch of completed tasks it runs `office.signals-scan` and mentions at most two new proposals per conversation turn. The onboarding set counts as one, mentioned once as a group.
5. **Answer from the Library.** Questions such as "when did I…" or "what did Jane say about…" are answered by searching sources, knowledge and artifacts, with citations. If nothing in the Library supports an answer, it says so.
6. **Resolve "this".** When the user says "this", "that", "here" or "these", call `office.selection-get` first, then use the selection as a typed reference ([selection-sharing.md](./selection-sharing.md)). When the hook already injected a selection into the prompt, use that instead.
7. **Stay short.** Plain words, no internal ids in prose. Citations will render as links that open the app at the source or page once the planned source screen ships.
8. **Lead the bootstrap.** In a new role, after the onboarding interview, the agent runs the Library bootstrap phase by phase (docs, KT videos, team artifacts, processes and policies): asks for the material, shows the batch preview, and walks the user through proposals by subject. It never writes pages outside it ([library-bootstrap.md](./library-bootstrap.md)). When the user asks it to carry out a `performable` runbook, it files an `owner:agent` to-do whose output is a draft the user submits.

## Guardrails

- **Outward-facing actions** (sending email or messages, purchases, submissions, API writes, anything leaving the machine apart from model inference and audited gateway reads) are never executed directly. The agent's draft lands in `review`. Approving it promotes a `draft` artifact with a Send action, and Send records `sent_at`. In the hackathon build, Send copies the draft to the clipboard. Nothing is sent.
- **Standing automation** and **knowledge writes** go through proposals, with no exceptions. When the user asks for a page change in conversation, the agent files a proposal that cites the conversation source, and the user accepts it in the app with one click. The write tools are hidden from MCP, so this is enforced by the tool surface.
- **Residual risk, stated honestly.** An interactive agent CLI with shell access runs commands as the user, so it could call `orbit tool run` directly. The backstops are the agent CLI's own approval prompt for shell commands, and the audit trail, which shows every write and who made it. The demo keeps Muse Code's shell approvals on.
