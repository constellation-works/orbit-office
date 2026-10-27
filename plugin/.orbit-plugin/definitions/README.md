# Seeded definitions

Definitions land only with the tools they call. Routines and auto-tasks are
seeded `enabled: false`; auto-tasks use `dedupe: skip_if_open`. Activities use
Orbit's schemaVersion 2 Activity envelope and have no schedule switch or dedupe key.

- Routines: `office-kt-inbox` (every 15 minutes), `office-signals` (daily),
  `office-daily-digest` (daily), `office-bootstrap` (every 15 minutes).
- Auto-tasks: `office-knowledge-maintenance` (weekly), `office-review` (period end).
- Activities: `office-agent-todo`.

`activities/office-agent-todo.yaml` defines the agent loop for an
`owner:agent`, `no-diff-expected` to-do. It reads the attached `context/`
artifacts and writes outputs plus `artifact-manifest.json` as task artifacts
for review. A performable runbook stops at a draft or a gateway draft;
submission always waits for the user. Its subprocess allowlist is `python3`
only, so the gateway plugin is available but cloud CLIs are not.

An activity declares every tool it calls in `spec.tools`; `required_tools`
belongs to task records. No office backend tool is needed in this activity:
the orchestrator attaches Library context before the task becomes eligible.
No definition may require an outward-facing action.

Plugin definitions are loaded only when listed in `spec.definitions` in
`plugin.yaml`. The activity is registered in `spec.definitions.activities`
as `definitions/activities/office-agent-todo.yaml`. Merely placing a file
in this directory does not register it.

`office-kt-inbox` is registered under `spec.definitions.routines` and its
job under `spec.definitions.jobs`. The disabled routine (`*/15 * * * *`)
targets `job:office-kt-inbox`; the job lists confirmed, unprocessed plans
with `office.video-plan` in list mode, fans out over them with deterministic
`plugin.tool_call` steps `office.transcribe`, `office.frames` and
`office.extract` (`profile: kt`), then runs `office.consolidate`
(`batch_id: kt`) over the videos' digests instead of filing proposals
itself. Extraction files one `owner:you` to-do per question, tagged
`kt-question`. The job creates no agent loop, crew or task, calls no
outward-facing action, and uses only the routine fields Orbit accepts
(routines carry no `required_tools`).

`office-bootstrap` is registered under `spec.definitions.routines` and its
job under `spec.definitions.jobs`. The disabled routine (`*/15 * * * *`)
targets `job:office-bootstrap`; the job lists confirmed, unconsolidated
batches with `office.batch-plan` in list mode and confirmed video plans
with `office.video-plan` in list mode, fans out over them with
deterministic `plugin.tool_call` steps (`office.transcribe`,
`office.frames` and `office.extract` with `profile: kt` for videos, and
`office.extract` with `profile: digest` for a batch's documents), then
fans out `office.consolidate` over the batches instead of filing proposals
itself. Consolidation files one knowledge proposal per page. The job
creates no agent loop, crew or task, calls no outward-facing action, and
uses only the routine fields Orbit accepts (routines carry no
`required_tools`).

`office-daily-digest` is registered under `spec.definitions.routines` and
its single-step job under `spec.definitions.jobs`. The job contains an inline
deterministic activity with `action: plugin.tool_call` and
`config.tool: office.digest`. It creates no agent loop, crew or task. The
routine runs daily at `5 0 * * *`; the tool defaults to the previous complete
UTC day. `max_active_runs: 1` bounds the job. The inline activity needs no
separate activity registration or `required_tools` field.

From the repository root, run `python3 -I plugin/tests/test_activities.py`
with Orbit >=0.34 installed. The regression checks validate temporary plugin
copies through `orbit plugin validate`, including malformed activity and
missing subprocess policy cases. They neither enable a plugin nor dispatch
an agent. Run this check alongside export validation before shipping changes
to the activity or its registration.


`office-signals` is registered under routines and jobs. The disabled daily
routine (`15 0 * * *`) targets `job:office-signals`; its one deterministic
`plugin.tool_call` invokes `office.signals-scan` with empty input and returns
candidates without filing or approving proposals. The weekly
`office-knowledge-maintenance` auto-task (`30 0 * * 1`, `enabled: false`,
`dedupe: skip_if_open`) is registered under auto_tasks. It asks an agent to
review evidence and file knowledge proposals, leaving approval to the user.
These are shipped definitions, not proposals for new automation.
