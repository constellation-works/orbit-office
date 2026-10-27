---
type: design
summary: "Spec: analytics and reviews — the metrics office.metrics computes and how period reviews are produced"
last_validated: 2026-10-27
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
other metrics as small tiles, and a "what got automated" list naming each
conversion and its proposal. Links are deferred until a view of accepted
proposals exists. Insights shows the most recent 84 UTC days, including today.
Seeded demo
history is labelled *simulated* on screen; the metrics themselves never
distinguish seeded from real records, so the label is the honesty
mechanism. The Library records how far that history runs as
`demo.simulated_until` (a `YYYY-MM-DD` date, set by the demo seeder and
changed with `office.config-set`). `office.metrics` returns it as
`simulated_until` when it is set and omits it otherwise. The app labels
weeks with recorded completed work dated on or before `simulated_until`
as simulated; empty weeks before the first seeded receipt and weeks after
the cutoff remain unlabelled. Individual records stay unmarked.

## Reviews

- **`office-review` auto-task**, condition `period_end`: quarterly and yearly reviews (must, shown over seeded history in the demo). The day-to-day summary is the daily digest, which is a routine, not an auto-task ([sessions.md](./sessions.md#daily-digest)).
- **Inputs:** completed tasks and their outputs, accepted proposals, metrics for the period, conversation sources (decisions), the period's daily digests, and knowledge pages created or updated.
- **Production:** the auto-task mints an agent to-do. The agent calls `office.review-inputs` for the period's material and writes the review. Approving it promotes a `review` artifact in the series `review-<cadence>`, for example `review-quarterly`, so each review can start from the previous one (the compounding rule in [library.md](./library.md)).
- **Sections:**
  - **Highlights:** the top outcomes, each cited.
  - **Work by area:** grouped by project and client tags.
  - **What I automated:** the conversions, with hours returned.
  - **Decisions:** from conversation sources.
  - **Snags and fixes:** frictions and the runbooks that resolved them.
  - **Next period:** open load and proposed focus.

Every line cites a task, source or proposal, so the review can be audited.
A line the material does not support is omitted, never invented.

## Tool and evidence contract

`office.metrics` and `office.review-inputs` accept exactly `since` and `until`,
both real `YYYY-MM-DD` dates. The window is `[since, until)` in UTC, at most
3,660 days. For example, the fourth quarter is
`{"since":"2026-10-01","until":"2027-01-01"}`. Weekly buckets start on
Monday and are clipped at both period boundaries. Ratios with no eligible
observations are `null`; their numerators and denominators are zero.

The tools read a complete Orbit task-list projection (including `tags`,
`type`, `created_at`, state-transition `history` and artifact paths), plus
all Library artifact metadata. An unavailable read, malformed relevant
record, or truncated task list refuses the request. The current callback
limit is 10,000 records; a larger corpus is explicitly refused rather than
silently sampled. Neither tool writes data or calls a model. A Library
commit change during a read refuses the snapshot for retry.

- Work is identified by `owner:agent`, `owner:you` or `owner:routine` tags.
  Proposals are decisions, not completed work. Completion and proposal
  decisions use recorded `history[].to_status` transitions into `done` or
  `rejected`, reconstructed as of the exclusive period end. `updated_at`
  is never substituted for completion. Terminal work without transition
  history is refused. Work reopened before period end is not completed.
- A task's `estimate-minutes:<integer>` tag supplies its estimated minutes
  (0–1,000,000). Otherwise the fixed defaults by Orbit type are: feature
  60, bug 30, refactor 30, chore 15. Estimates are explicitly estimates;
  the tools do not infer them from titles, bodies or a model.
- A routine output without a listed owner task is identified by a
  `routine:<name>` artifact tag. It counts once per originating task/run,
  at its first approval, so multiple output files or promoted versions
  do not inflate work. The same estimate tag overrides artifact-kind
  defaults: report/review 30, doc/script 60, draft/data/other 15 minutes.
  Listed task work is counted at task completion, not counted again on
  artifact promotion. `digest-daily` artifacts and their source tasks
  (also recognizable by `series:digest-daily`) are housekeeping and excluded.
- First-pass acceptance counts user-approved non-digest artifact outputs
  whose approval timestamp is in the period. Its numerator has
  `approved.edited: false`; its denominator includes edited user approvals.
  Routine approvals never enter this population.
- Knowledge coverage uses completed agent tasks in the period. A runbook
  is attached only when the task's artifact metadata includes an actual
  `context/knowledge/runbooks/*.md` file produced by `office.attach`.
  A runbook named in a tag alone does not claim coverage.
- A conversion needs a shared explicit `cluster:<id>` tag on prior completed
  `owner:you` work and an accepted `proposal:auto_task` (or a legacy `proposal:routine` approval).
  Only the first accepted automation of each cluster counts. The conversion
  returns its proposal and manual task ids, even when the manual examples
  predate this period. Without that link the tools report no conversion;
  they do not invent a cluster from similar titles.
- Open load is user-owned work open just before `until`, including work
  created before `since`. Age is measured from creation to that period end,
  in whole days. Future-created tasks and terminal states are excluded.

`review-inputs` returns period-completed tasks with their output paths,
period-accepted proposals, the same complete metrics report, period-approved
outputs, and daily digests separately. Daily digests use their `day:` tag
for period selection, so a report published after midnight belongs to the
day it summarizes; legacy reports without that tag use approval time.
Conversation sources contain only entries timestamped in the period, with
their original source line anchors;
an old session appended during the period is included. Knowledge changes
are selected by Library git revision timestamps, with commit ids, paths and
subjects. A later revision or a newer current page header does not hide an
earlier in-period change. Returned knowledge references identify revisions;
the current page body is not presented as historical text.

The seeded `office-review` definition is disabled and uses `skip_if_open`.
Orbit currently supports cron/interval schedules rather than a `period_end`
condition token, so its period-end implementation fires at 00:05 host-local
time on January, April, July and October 1. It requests the just-completed
UTC quarter and, in January, the previous UTC year as a second review. The
task's recorded creation date anchors the periods even if execution is late.
Reviews remain task artifacts awaiting user approval; the definition does
not promote them. The plugin manifest registers this definition explicitly.
