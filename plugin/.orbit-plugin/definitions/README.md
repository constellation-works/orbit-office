# Seeded definitions

Definitions land only with the tools they call, each
seeded `enabled: false` with `dedupe: skip_if_open`:

- Routines: `office-kt-inbox` (every 15 minutes), `office-signals` (daily),
  `office-daily-digest` (daily).
- Auto-tasks: `office-knowledge-maintenance` (weekly), `office-review` (period end).
- Activities: `office-agent-todo`.

Every definition's `required_tools` must list every `office.*` tool its steps
call, and no definition may require an outward-facing action.

`auto_tasks/office-review.yaml` is shipped and registered in the manifest. It
mints quarterly reviews at the next quarter boundary and a yearly review in
January, using `office.review-inputs`; all outputs await user approval. The
other definitions above remain planned until their tools land.
