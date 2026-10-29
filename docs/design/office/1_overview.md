---
title: office — Overview
owner: daniel
last_updated: 2026-10-29
last_validated: 2026-10-26
status: Draft
stage: building
feature: office
doc_role: overview
type: design
summary: An Orbit distribution for personal work management — sources, a cited Library, proposals and metrics that move manual work to agents under approval.
tags: [office, distribution, orbit, library]
paths: ["crates/**", "plugin/**", "app/**"]
related_features: [office]
related_artifacts: [ORB-16585]
---

# office — Overview

**Next:** Coordinate the hackathon entry and submission [ORB-16585].

Orbit Office is a distribution of Orbit: an app built on Orbit as its engine.
The product combines the standalone app, the `office` and `gateway` component
plugins, optional `cloudcli`, and an office workspace preset. Orbit supplies
the runtime; plugins supply capabilities; the preset selects the workspace's
configuration and initial automation. The preset shipped in [ORB-19197]
([install spec](./specs/install.md#office-workspace-preset)).

Office work is a generic, easy-to-use, personal agentic work-management
system for anyone whose work is knowledge work: analysts, consultants,
managers, operations, and software engineers. An engineer's day has the same
shape: KT videos, design docs, runbooks, on-call procedures, API calls and
cloud queries. It automates repeatable work systematically, tracks all work, and
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

1. **Observe:** capture what happens (KT videos, pasted chats, meeting notes, finished tasks, frictions).
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
5. **Generic core, packs for verticals.** The `office` component plugin knows sources, knowledge, proposals and metrics. KT videos, meetings, receipts and bills are **routine packs**: data (routine and auto-task definitions, runbooks, skills), not code paths.
6. **Honest numbers.** Analytics count what happened. Seeded demo history is labelled simulated.

## 2. Core Concepts

| User sees | Internal name | Is | Lives in |
|---|---|---|---|
| Sources | `source` | An immutable record of something that happened: KT video, meeting, pasted chat or email, document, receipt photo, orchestrator conversation | `library/sources/` |
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
| System shape, dispatch, authority boundaries | [2_design.md](./2_design.md) | [ORB-16585]; dispatch [ORB-18725] done |
| Library data model (sources, knowledge, artifacts, frictions) | [specs/library.md](./specs/library.md) | [ORB-16585] |
| Plugin tool surface, definitions, config, permissions | [specs/plugin-surface.md](./specs/plugin-surface.md) | [ORB-16585] |
| Library persistence (`office-store`, after `orbit-store`) | [specs/store.md](./specs/store.md) | [ORB-18709] done, [ORB-18710] done |
| Exec envelope contract | [specs/output-contract.md](./specs/output-contract.md) | [ORB-16585] |
| Orchestrator skill, tools and guardrails | [specs/orchestrator.md](./specs/orchestrator.md) | [ORB-18720] done |
| Proposals and their signals | [specs/proposals.md](./specs/proposals.md) | [ORB-18721] done, [ORB-18722] done |
| Analytics and generated reviews | [specs/analytics-and-reviews.md](./specs/analytics-and-reviews.md) | [ORB-18724] done |
| Web app screens and rules | [specs/web-app.md](./specs/web-app.md) | [ORB-18727] done, [ORB-18728] done |
| Calendar: read-only sync, meeting prep, capture after the meeting | [specs/calendar.md](./specs/calendar.md) | [ORB-19191] done, [ORB-19192] done, [ORB-19193] done, [ORB-19194] done, [ORB-19195] done |
| Shared library: `shareable` documents synced with one team repository | [specs/shared-library.md](./specs/shared-library.md) | [ORB-19417] done, [ORB-19418] done, [ORB-19419] done, [ORB-19421] done |
| Selection sharing between app and agent CLI | [specs/selection-sharing.md](./specs/selection-sharing.md) | [ORB-18723] done |
| Models and privacy routing | [specs/models-and-privacy.md](./specs/models-and-privacy.md) | [ORB-18712] done |
| Office workspace preset and install | [specs/install.md](./specs/install.md) | [ORB-19197] done |
| Hackathon compliance, demo and build plan | [specs/delivery.md](./specs/delivery.md) | [ORB-16585] |
| Directions beyond the event | [3_vision.md](./3_vision.md) | [ORB-16585] |

### Hackathon scope (spec §3)

**Must:** orchestrator setup (Muse Code wired to Orbit MCP with the office
skill); sources store with paste capture and video upload; knowledge
store (domain, runbook; frictions in Orbit); artifacts store; KT pack
(cost preview → transcribe and read keyframes → chaptered index with
tangents folded → cited knowledge proposals, a runbook and questions for the
presenter; [specs/kt-videos.md](./specs/kt-videos.md));
Library bootstrap for a new role (document capture, digest per source, consolidation per topic under the page rules, batch preview, setup progress; [specs/library-bootstrap.md](./specs/library-bootstrap.md)); knowledge attached to agent tasks; proposals from three signals
(repetition, friction, staleness); Needs you; Insights (automation ratio);
review generation over seeded history.

**Should:** meetings from `.vtt` transcripts and pasted notes; receipts pack; daily digest (session brief and decision log are Must, with the orchestrator); open-weight routing for
`private` sources; selection sharing; proposal dismissal memory; the shared library (`shareable` documents synced with one team repository, so a new joiner inherits the team's knowledge; [specs/shared-library.md](./specs/shared-library.md)).

**Pitch slide only:** bills pack; email triage, social content, groceries,
lectures, photo organizer, glasses diary; encrypted sync; paid routine packs.

**Non-goals for the event:** accounts, permissions or shared writes
beyond the shared library's sync
([decision](./4_decisions.md#teams-share-through-one-shared-library-synced-from-a-shareable-field)),
mobile, sending real email or making purchases, hardware, changes to Orbit
core.

## Task References

- [ORB-16585] — files the orchestrator assignment: rules, go/no-go, coordination, demo and submission for the office-work entry.
- [ORB-18709] — built the Library store adapter with the writer lock, git and settings.
- [ORB-18710] — built `office.init`, `office.reindex` and Library health.
- [ORB-18712] — built the Meta and Baseten model adapters with privacy routing.
- [ORB-18720] — built the agent to-do activity and the orchestrator skills.
- [ORB-18721] — built `office.signals-scan`.
- [ORB-18722] — built proposal create and accept, with the signals routine.
- [ORB-18723] — built `office.selection-get` and `office.selection-put`.
- [ORB-18724] — built metrics, review inputs and the review.
- [ORB-18725] — built the app server, the Orbit client, static serving and the dispatch pass.
- [ORB-18727] — built the Today screen and its server feature.
- [ORB-19191] — built the gateway authentication schemes for calendar feeds and Microsoft Graph.
- [ORB-19192] — built calendar sync, listing and the event snapshot.
- [ORB-19193] — built meeting preparation and its routine.
- [ORB-19194] — built capture from a calendar event and the meeting inbox routine.
- [ORB-19195] — built the Calendar screen, server feature and Today card.
- [ORB-18728] — built the To-dos screen and its server feature.
- [ORB-19417] — built the `shareable` field, `office.share-set`, the shared view and the `shared` settings.
- [ORB-19418] — built `office.library-sync`, its routine and the two-workspace demo.
- [ORB-19419] — built the shared layer: index, qualified citations, reads, attach and bootstrap.
- [ORB-19421] — built the app's shared-library settings, labels, share switch and Needs you cards.
- [ORB-19197] — built the office workspace preset so a fresh workspace starts without Orbit's developer defaults.

> Resolve any task above with `orbit task show <ID>` or `git log --grep=<ID>`.
