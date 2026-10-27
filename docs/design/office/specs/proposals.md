---
type: design
summary: "Spec: proposals — kinds, mechanical signals, record and lifecycle, dismissal memory"
last_validated: 2026-10-26
---

# Spec: Proposals

Proposals are how the system gets better over time without the user
designing it. Nothing becomes standing automation, and no knowledge page
changes, without the user accepting a proposal.

Source: spec §7.

## Kinds

| Kind | Effect when accepted |
|---|---|
| `auto_task` | Creates and enables an Orbit auto-task (condition- or schedule-triggered task minting) |
| `routine` | Creates and enables an Orbit routine (scheduled job) |
| `knowledge_add` | Creates a domain or runbook page from the cited evidence, starting from its template's skeleton when it has one ([knowledge-templates.md](./knowledge-templates.md)) |
| `knowledge_update` | Applies a diff to an existing page |
| `runbook_from_friction` | Writes or edits a runbook so a recurring friction stops happening |
| `retire` | Marks a page `retired` or disables a routine nobody uses |

## Signals

These are mechanical and computed by `office.signals-scan`. They are not
left to the model's discretion.

| Signal | Condition | Proposes |
|---|---|---|
| **Repetition** | ≥3 `owner:you` tasks done in 30 days whose titles and descriptions cluster together (index similarity above a threshold, the same tags) | `auto_task` or `routine` to take that work over, plus a draft runbook from how the user did it |
| **Friction recurrence** | The same friction (by text similarity or explicit link) recorded ≥2 times | `runbook_from_friction` |
| **Staleness** | A knowledge page is cited by tasks newer than its `last_verified`, and a newer source contradicts or extends it (checked by the model, which must quote the evidence) | `knowledge_update` |
| **Gap** | An agent task finished without a runbook, and the user had to correct its output | `knowledge_add` (runbook) |
| **Unused automation** | A routine has produced nothing the user opened in 30 days | `retire` |
| **New subject** | A participant, or a `client:`/`project:`/`topic:` tag, that no page covers (by title, `aliases` or tag), seen in ≥2 sources | `knowledge_add` from the matching template |
| **Unrecorded direction** | A `direction` entry in a conversation source that `dom-standing-directions` does not cite | `knowledge_update` adding it |
| **Declared cadence** | A `rhythm` entry with a deliverable that no routine or auto-task covers | `auto_task` for that deliverable |

The orchestrator can also propose things in conversation ("you've asked me
this three times; want a routine?"). These go through the same record and
approval path. The must-have signals for the event are repetition,
friction, staleness and unrecorded direction; new subject and declared
cadence are should-have.

## Record and lifecycle

- Each proposal is an Orbit task tagged `proposal` and `proposal:<kind>`, in `proposed` state. The description says what changes. Its evidence (task ids, source anchors, friction ids) goes in the body and in `library/proposals/<id>.md`. The concrete payload (an auto-task definition, a page diff) is a task artifact.
- **Onboarding set.** The proposals filed from the onboarding interview also carry the tag `onboarding`. The app shows them as one "Set up your Library" card with *Accept all* and per-page review; *Accept all* accepts each one in turn, and a refusal on one leaves the others accepted and names it.
- **Accept:** the app calls `office.proposal-accept`, which applies the payload (creates the auto-task or routine, writes the page; for a routine or auto-task that runs a script, it first fetches the script's dependencies and refuses the acceptance if they do not resolve, [scripts.md](./scripts.md#dependencies-are-fetched-once-at-approval)), closes the task as done with the change recorded, and commits the Library. `proposal-accept` is `mcp_scope: none`: only the app can call it.
- **Dismiss:** closes it as rejected with an optional reason.
- **Never auto-approved.** Orbit only approves a `proposed` task automatically when a drain is started with `--approve-proposed`. The office workspace runs no drain, and the dispatch pass submits `backlog` tasks only, so a proposal changes state only when the user accepts or dismisses it.

## Dismissal memory (should-have)

A dismissed proposal suppresses another of the same kind on the same
evidence cluster for 30 days. The orchestrator says so when asked ("you
dismissed this on Oct 29").
