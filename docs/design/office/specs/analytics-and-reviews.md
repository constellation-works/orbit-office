---
type: design
summary: "Spec: analytics and reviews — the metrics office.metrics computes and how period reviews are produced"
last_validated: 2026-10-26
---

# Spec: Analytics and reviews

Analytics count what happened; reviews narrate it, cited. Both are
computed from Orbit records and the Library index, never estimated by a
model.

Source: spec §10–§11.

## Metrics

Computed by `office.metrics` over a chosen window, by week.

| Metric | Definition | Why it matters |
|---|---|---|
| **Automation ratio** (headline) | Completed `owner:agent` tasks plus routine outputs ÷ all completed work. The daily digest is housekeeping and counts as neither. Shown two ways: by count, and by estimated minutes (each task's estimate, defaulting by kind) | The "barely any manual work" trajectory |
| **Manual → automated conversions** | Work clusters that were `owner:you` and are now covered by an accepted routine or auto-task | Shows the system learning |
| **Hours returned** | Estimated minutes of automated work in the window | The number a manager cares about |
| **First-pass acceptance** | Share of agent outputs approved without edits (`approved.edited: false` on artifacts with `approved.by: user`) | Agent quality, and whether knowledge helps |
| **Knowledge coverage** | Share of agent tasks with at least one runbook attached | A leading indicator of the above |
| **Proposal acceptance rate** | Accepted ÷ (accepted + dismissed) | Whether the proposals are worth the user's attention |
| **Open load** | `owner:you` to-dos open, and their age | What still lands on the user |

The Insights screen shows the automation ratio as a weekly line, the
other metrics as small tiles, and a "what got automated" list linking each
conversion to its proposal ([web-app.md](./web-app.md)). Seeded demo
history is labelled *simulated* on screen; the metrics themselves never
distinguish seeded from real records, so the label is the honesty
mechanism.

## Reviews

- **`office-review` auto-task**, condition `period_end`: quarterly and yearly reviews (must, shown over seeded history in the demo). The day-to-day summary is the daily digest, which is a routine, not an auto-task ([sessions.md](./sessions.md#daily-digest)).
- **Inputs:** completed tasks and their outputs, accepted proposals, metrics for the period, conversation sources (decisions), the period's daily digests, and knowledge pages created or updated.
- **Production:** the auto-task mints an agent to-do. The agent calls `office.review.inputs` for the period's material and writes the review. Approving it promotes a `review` artifact in the series `review-<cadence>`, for example `review-quarterly`, so each review can start from the previous one (the compounding rule in [library.md](./library.md)).
- **Sections:**
  - **Highlights:** the top outcomes, each cited.
  - **Work by area:** grouped by project and client tags.
  - **What I automated:** the conversions, with hours returned.
  - **Decisions:** from conversation sources.
  - **Snags and fixes:** frictions and the runbooks that resolved them.
  - **Next period:** open load and proposed focus.

Every line cites a task, source or proposal, so the review can be audited.
A line the material does not support is omitted, never invented.
