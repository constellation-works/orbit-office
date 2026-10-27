---
title: office — Overview
owner: daniel
last_updated: 2026-10-26
last_validated: 2026-10-26
status: Draft
feature: office
doc_role: overview
type: design
summary: A personal agentic work-management system on Orbit — sources, a cited Library, proposals and metrics that move manual work to agents under approval.
tags: [office, plugin, orbit, library]
paths: ["crates/**", "plugin/**", "app/**"]
related_features: [office]
related_artifacts: [ORB-16585]
---

# office — Overview

Office work is a generic, easy-to-use, personal agentic work-management
system. It automates repeatable work systematically, tracks all work, and
keeps a store of knowledge that agents draw on and that grows more useful
the longer it is used. With prolonged use the user does barely any manual
work and acts as a **manager** who sets high-level direction: they talk to
one orchestrator agent, which runs the operation. The orchestrator is not
something this project builds — it is the user's own agent CLI, Muse
Spark by default — connected to Orbit and given the office skill. The
product is what that agent can see and do, not a new agent.

Source: the product and technical spec (polaris `projects/office-work/spec.md`,
2026-10-26), §1–§4. This folder breaks that spec into design docs; where the
spec and this folder disagree, the spec wins until a decision in
[4_decisions.md](./4_decisions.md) records otherwise.

## 1. Motivation

Cloud agent products run your routines on someone else's computer. Their
schedules, tasks and what they learned about you are invisible and hard to
keep track of, they cannot be tuned to how you work, and leaving means
starting over. Office work brings it all local: routines, schedules, tasks
and the knowledge they build up live on the user's machine, visible in an
app they own, with no vendor lock-in. Data, schedules, tasks and knowledge
are local; only inference leaves the machine.

Everything in the product serves one loop:

```
observe ─▶ propose ─▶ approve ─▶ automate ─▶ measure
   ▲                                            │
   └──────────── knowledge compounds ◀──────────┘
```

1. **Observe:** capture what happens (meetings, pasted chats, finished tasks, frictions).
2. **Propose:** the orchestrator suggests auto-tasks, routines and knowledge changes, each citing its evidence.
3. **Approve:** the user decides. Nothing becomes standing automation or leaves the machine without approval.
4. **Automate:** approved routines and auto-tasks do the work, with agents using attached knowledge.
5. **Measure:** analytics show the share of work agents now carry.

Orbit tasks and their audit trail are the history. Analytics show whether
the operation is getting more effective, above all whether work that was
once manual is now automated. The yearly performance review becomes a
by-product: complete, cited, and written by an auto-task.

### Principles

1. **Local-first and owned.** Sources, knowledge, tasks, schedules and history live on the user's machine as plain files and Orbit records. Only inference leaves the machine, and privacy-sensitive steps can stay on an open-weight model ([specs/models-and-privacy.md](./specs/models-and-privacy.md)).
2. **Propose, don't impose.** The orchestrator proposes. It does not create standing automation, edit knowledge pages or take outward-facing actions without approval.
3. **Provenance everywhere.** Every knowledge claim, summary and agent output links back to the source or task it came from, down to a timestamp or line.
4. **Plain words.** The user sees To-dos, Routines, Library, Needs you and Insights. They never see ids, crews, runs, workflows or raw statuses.
5. **Generic core, packs for verticals.** The plugin knows sources, knowledge, proposals and metrics. Meetings, receipts and bills are **routine packs**: data (routine and auto-task definitions, runbooks, skills), not code paths.
6. **Honest numbers.** Analytics count what happened. Seeded demo history is labelled simulated.

## 2. Core Concepts

| User sees | Internal name | Is | Lives in |
|---|---|---|---|
| Sources | `source` | An immutable record of something that happened: meeting, pasted chat or email, document, receipt photo, orchestrator conversation | `library/sources/` |
| About | `domain` | Facts about the user, people, clients, projects and topics, each page following a template, plus pointers to related files, links, contacts and accounts (never secrets) | `library/knowledge/domain/` |
| How-to | `runbook` | Steps for a repeatable piece of work | `library/knowledge/runbooks/` |
| Outputs | `artifact` | Agent-generated work the user approved: reports, docs, drafts, scripts, data, reviews | `library/artifacts/` |
| Snags | `friction` | Something that went wrong or slowed work | Orbit frictions (`orbit.friction.*`) |
| To-dos | task | A unit of work, owned by You or by Agents | Orbit tasks |
| Routines | routine / auto-task | Standing automation on a schedule or condition | Orbit routines and auto-tasks |
| Needs you | proposal, decision | Something waiting for the user | Orbit tasks tagged `proposal`; outward-action holds |
| Insights | metrics | Measures of the operation | Computed from Orbit records and the Library index |

The Library has three layers, for what comes in, what is understood and
what goes out:

- **Sources are evidence.** They are captured once and never edited.
- **Knowledge is derived.** It is rewritten freely, and every revision cites its sources.
- **Artifacts are outputs.** They are produced by tasks, enter the Library only once approved, and are versioned, never edited in place. Each one names the task that made it and what it drew on.

Full contracts: [specs/library.md](./specs/library.md). Terms:
[references/glossary.md](./references/glossary.md).

## 3. At a Glance

| Concern | File | Task |
|---|---|---|
| System shape, dispatch, authority boundaries | [2_design.md](./2_design.md) | [ORB-16585] |
| Library data model (sources, knowledge, artifacts, frictions) | [specs/library.md](./specs/library.md) | [ORB-16585] |
| Plugin tool surface, definitions, config, permissions | [specs/plugin-surface.md](./specs/plugin-surface.md) | [ORB-16585] |
| Library persistence (`office-store`, after `orbit-store`) | [specs/store.md](./specs/store.md) | [ORB-16585] |
| Exec envelope contract | [specs/output-contract.md](./specs/output-contract.md) | [ORB-16585] |
| Orchestrator skill, tools and guardrails | [specs/orchestrator.md](./specs/orchestrator.md) | [ORB-16585] |
| Proposals and their signals | [specs/proposals.md](./specs/proposals.md) | [ORB-16585] |
| Analytics and generated reviews | [specs/analytics-and-reviews.md](./specs/analytics-and-reviews.md) | [ORB-16585] |
| Web app screens and rules | [specs/web-app.md](./specs/web-app.md) | [ORB-16585] |
| Selection sharing between app and agent CLI | [specs/selection-sharing.md](./specs/selection-sharing.md) | [ORB-16585] |
| Models and privacy routing | [specs/models-and-privacy.md](./specs/models-and-privacy.md) | [ORB-16585] |
| Hackathon compliance, demo and build plan | [specs/delivery.md](./specs/delivery.md) | [ORB-16585] |
| Directions beyond the event | [3_vision.md](./3_vision.md) | [ORB-16585] |

### Hackathon scope (spec §3)

**Must:** orchestrator setup (Muse Code wired to Orbit MCP with the office
skill); sources store with paste capture and meeting upload; knowledge
store (domain, runbook; frictions in Orbit); artifacts store; meeting pack
(transcribe → summary, decisions → to-dos You/Agents → Library updates);
knowledge attached to agent tasks; proposals from three signals
(repetition, friction, staleness); Needs you; Insights (automation ratio);
review generation over seeded history.

**Should:** receipts pack; daily digest (session brief and decision log are Must, with the orchestrator); open-weight routing for
`private` sources; selection sharing; proposal dismissal memory.

**Pitch slide only:** bills pack; email triage, social content, groceries,
lectures, photo organizer, glasses diary; encrypted sync; paid routine packs.

**Non-goals for the event:** multi-user or teams, mobile, sending real
email or making purchases, hardware, changes to Orbit core.

## Task References

- [ORB-16585] — files the orchestrator assignment: rules, go/no-go, coordination, demo and submission for the office-work entry.

> Resolve any task above with `orbit task show <ID>` or `git log --grep=<ID>`.
