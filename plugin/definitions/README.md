# Seeded definitions

Empty in the scaffold. Definitions land only with the tools they call, each
seeded `enabled: false` with `dedupe: skip_if_open`:

- Routines: `office-meeting-inbox` (every 15 minutes), `office-signals` (daily).
- Auto-tasks: `office-knowledge-maintenance` (weekly), `office-review` (period end).
- Activities: `office-agent-todo`.

Every definition's `required_tools` must list every `office.*` tool its steps
call, and no definition may require an outward-facing action.
