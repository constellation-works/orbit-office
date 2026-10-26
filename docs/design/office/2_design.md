---
title: office — Design
owner: daniel
last_updated: 2026-10-26
last_validated: 2026-10-26
status: Draft
feature: office
doc_role: design
type: design
summary: How the system fits together — Orbit, the Rust plugin backend, the web app as the user's console, and the user's agent CLI as orchestrator.
tags: [office, plugin, orbit, architecture]
paths: ["crates/**", "plugin/**", "app/**"]
related_features: [office]
related_artifacts: [ORB-16585]
---

# office — Design

Source: spec §5, §6.6, §8, §13 and §18. Mechanism contracts live in
`specs/`; this doc is the map between them.

## 1. System shape

```
┌──────────────────────────── user's machine ────────────────────────────┐
│                                                                        │
│  Web app (new)                      Your agent CLI = the orchestrator  │
│  Today · To-dos · Routines          Muse Code (or Claude Code, Codex)  │
│  Library · Insights · Review        + office-orchestrator skill (new)  │
│        │ local app server                 │                            │
│        │ orbit tool run (as the user)     │ Orbit MCP (agent authority)│
│        └──────────────┬───────────────────┘                            │
│                       ▼                                                │
│  Orbit (pre-existing, MIT runtime, pinned commit)                      │
│  tasks · audit · routines · auto-tasks · frictions · agent runs        │
│        │ plugin protocol (exec, JSON on stdin/stdout)                   │
│        ▼                                                               │
│  office plugin (new) ──────▶ library/  (sources, knowledge, artifacts) │
│        │                     plugin_state/index.sqlite (rebuildable)   │
└────────┼───────────────────────────────────────────────────────────────┘
         ▼
  Meta Model API (Muse Spark, Muse Voice Transcribe)   ·   Muse Spark Open (Baseten or local)
```

- **Repository:** this repo, `constellation-works/orbit-office`, public,
  created after kickoff, holding `plugin/` and `app/` plus the Rust
  workspace at its root. Orbit is a labelled dependency (README states the
  tested commit). It is never vendored, forked or added as a submodule.
- **Office plugin:** namespace `office`, `exec` backend. The spec proposed
  Python with `uv`; the shipped stack is Rust (workspace at the repo root),
  recorded in [4_decisions.md](./4_decisions.md). It owns the Library and
  the index. Surface: [specs/plugin-surface.md](./specs/plugin-surface.md);
  envelope: [specs/output-contract.md](./specs/output-contract.md).
- **Web app:** a local server and a single-page UI (TypeScript, as the
  spec proposed). The server
  is the only process the browser talks to. It is the user's console, so
  it acts **as the user**: it calls Orbit with `orbit tool run --json` for
  both `orbit.*` and `office.*` tools, including the approval tools the
  orchestrator cannot see. It never reads `.orbit/` directly. Orbit's HTTP
  API is not used, because it belongs to Orbit's own dashboard (spec §18.3).
- **Orchestrator:** the user's agent CLI, not code in this repo. Setup
  registers Orbit's MCP server with it at agent authority. For Muse Code
  this means an `mcpServers` entry in its `settings.json`, written by the
  setup script because `orbit mcp init` has no Muse target (spec §18.6).
  The plugin's `office-orchestrator` skill tells it how to run the
  operation. What this project builds for the orchestrator is the skill,
  the tool surface and its guardrails:
  [specs/orchestrator.md](./specs/orchestrator.md).
- **Implementing agents:** carry out agent to-dos as ordinary Orbit task
  runs, using Orbit's existing crews (including the pre-existing Muse
  crews). The new work is what they are given: attached knowledge and
  runbooks.
- **Orbit workspace:** one dedicated workspace for the office user, for
  example `~/office`, with the Library at `~/office/library/`. The
  workspace is a git repository — it is the Library repo — which gives
  Orbit the checkout its runs expect.

### Rust layering inside the backend

See [ARCHITECTURE.md](../../../ARCHITECTURE.md): protocol domain
(`office-core`: envelope dispatch, tool table, handlers — no I/O),
composition (`office::app`: stdin → core → envelope), surface
(`office::cli`, `office::output`: argv, stdout). Persistence is its own
crate, `office-store`, structured after `orbit-store`
([specs/store.md](./specs/store.md)); the composition layer will open it
from the call context when the first Library tool lands.

## 2. Tasks, ownership and dispatch (spec §6.6)

- **Attaching knowledge.** A task gets its knowledge through
  `context_files`: the runbook and domain pages, the `source.md` files and
  any earlier artifacts it needs as examples. The orchestrator calls
  `office.attach` to pick them (index search over the task's text and
  tags, plus the `related` graph and artifact series, capped at 8 files),
  then `orbit.task.update` to set them.
- **Ownership.** Tags `owner:you` or `owner:agent`. Orbit has no
  human-owned task, and any `backlog` task is admissible to a drain
  (spec §18.1), so **the office workspace never runs an auto drain**
  (`orbit run auto`). Both kinds of to-do sit in `backlog`. Agent work
  starts only through the **dispatch pass**: every 30 seconds the app
  server lists `backlog` tasks tagged `owner:agent` and submits them by id
  with `orbit run ship <ids>`, which is review-only without `--complete`.
  `owner:you` tasks are never submitted, and the user closes them from the
  app. The app runs as a login service (launchd or a systemd user unit),
  so dispatch continues while the browser is closed. The orchestrator
  cannot dispatch: Orbit's agent-authority MCP does not include workflow
  dispatch (spec §18.6).
- **Agent to-dos are non-code work.** Each is tagged `no-diff-expected`,
  so Orbit runs no validation gates and does not hold it for a file
  footprint. Its output is a task artifact, never a file edit. Changes to
  the Library go through `office.*` tools and proposals.
- **Review is the approval step.** A finished agent to-do lands in
  `review`. That is the "Needs you" item. Approving it in the app records
  a structured verdict, promotes the output into the Library
  (`office.artifact.promote`) and moves the task to `done`. Asking for
  changes sends it back to `backlog` with a comment.
- **Provenance.** Every task created from a source carries `source:<id>`
  in its tags and the anchor in its description. Agent outputs stay task
  artifacts until approved, then become Library artifacts that name the
  task.
- **Acceptance criteria** are set on every agent task (Orbit requires
  them), generated from the runbook's `done_when` section when one is
  attached.

## 3. Authority boundaries (spec §8.2, §8.4)

Three actors, three authorities:

| Actor | Reaches Orbit through | Can approve / write the Library? |
|---|---|---|
| User, in the web app | `orbit tool run` as the user | Yes — the app is the approval surface |
| Orchestrator (user's agent CLI + skill) | Orbit MCP at agent authority | No — approval tools are `mcp_scope: none` |
| Implementing agents (Orbit crews) | Ordinary task runs | No — output is a task artifact until approved |

- **Hidden from MCP** (`mcp_scope: none`; only the app calls them):
  `office.knowledge.write`, `office.proposal.accept`,
  `office.artifact.promote`, `office.selection.put`, `office.reindex`.
  The orchestrator cannot approve its own proposals or write to the
  Library; it can only propose. This is enforced by the tool surface, not
  by instructions.
- **Outward-facing actions** (sending email or messages, purchases,
  submissions, anything leaving the machine apart from model inference)
  are never executed directly. The agent's draft lands in `review`.
  Approving it promotes a `draft` artifact with a Send action, and Send
  records `sent_at`. In the hackathon build, Send copies the draft to the
  clipboard. Nothing is sent.
- **Standing automation** and **knowledge writes** go through proposals
  with no exceptions. When the user asks for a page change in
  conversation, the agent files a proposal that cites the conversation
  source, and the user accepts it in the app with one click.

## 4. The meeting loop (hero loop)

1. A meeting recording lands in a watched folder or is uploaded.
2. A routine transcribes it (Muse Voice Transcribe), and Muse Spark
   produces a summary and the decisions.
3. Actionable to-dos are split into "yours" and "agents'".
4. Agents carry out theirs, for example drafting the follow-up email or
   researching an open question. Anything outward-facing waits for the
   user's approval.
5. The Library is updated, every page citing its source meeting and
   timestamp.

What sets it apart from meeting tools: agents act on the to-dos instead of
listing them, and the knowledge compounds locally.

## 5. Contracts

| Contract | Spec |
|---|---|
| Library files, frontmatter, lifecycles | [specs/library.md](./specs/library.md) |
| Tool list, kinds, `mcp_scope`, definitions, config, permissions | [specs/plugin-surface.md](./specs/plugin-surface.md) |
| One exec call, end to end | [specs/output-contract.md](./specs/output-contract.md) |
| Orchestrator behaviours and guardrails | [specs/orchestrator.md](./specs/orchestrator.md) |
| Proposal kinds, signals, lifecycle | [specs/proposals.md](./specs/proposals.md) |
| Metrics definitions and review production | [specs/analytics-and-reviews.md](./specs/analytics-and-reviews.md) |
| Screens, app rules, citation links | [specs/web-app.md](./specs/web-app.md) |
| Selection record, tray, pull/push, safety | [specs/selection-sharing.md](./specs/selection-sharing.md) |
| Model routing by sensitivity | [specs/models-and-privacy.md](./specs/models-and-privacy.md) |

## 6. Gates

`make ci` runs fmt, the structure scripts, the vendored standards check,
clippy, cargo-deny, tests and goldens — the rust-cli-template gates, kept.
Plugin exports are validated with `orbit plugin validate` and
`orbit plugin test` (conformance goldens, no network, no credentials).

## 7. Concerns & Honest Limitations

- **Residual orchestrator risk, stated honestly.** An interactive agent
  CLI with shell access runs commands as the user, so it could call
  `orbit tool run` directly. The backstops are the agent CLI's own
  approval prompt for shell commands, and the audit trail, which shows
  every write and who made it. The demo keeps Muse Code's shell approvals on.
- **No human-owned tasks in Orbit** (spec §18.1). The no-drain dispatch
  pass is a workaround, not a model. A generic "human-owned" task flag in
  Orbit core would retire it; that is a core change and stays out of the
  entry ([3_vision.md](./3_vision.md)).
- **Proposal approval is opt-in only because the workspace runs no
  drain** (spec §18.2). Any future drain in this workspace must not pass
  `--approve-proposed`.
- **Hiding approval tools is what keeps approval in the app** (spec
  §18.6): Orbit's task lifecycle, not the caller's authority, decides
  status moves through `orbit.task.update`, so an agent that edits Orbit
  task status directly is possible — and visible in the audit trail.
- **Muse Spark image input is unverified** (spec §18.5). Confirm the
  request shape in the API reference on day 1; the fallback is local OCR,
  then Muse Spark on the text.
- **Muse Code skill discovery and prompt hooks are day-1 checks**
  (spec §8.1, §13.3, §16): that an interactive Muse Code session lists
  `office-orchestrator`, and whether Muse Code has a `UserPromptSubmit`
  hook. Without a hook, the selection pull rule covers it.
- **Onboarding gap.** Orbit is a developer CLI. The demo shows no
  terminal, and the pitch needs an install story an office worker would
  accept; setup is one script ([specs/delivery.md](./specs/delivery.md)).

## Task References

- [ORB-16585] — files the orchestrator assignment: rules, go/no-go, coordination, demo and submission for the office-work entry.

> Resolve any task above with `orbit task show <ID>` or `git log --grep=<ID>`.
