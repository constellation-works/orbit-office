# Seeded definitions

Definitions land only with the tools they call. Routines and auto-tasks are
seeded `enabled: false`; auto-tasks use `dedupe: skip_if_open`. Activities use
Orbit's schemaVersion 2 Activity envelope and have no schedule switch or dedupe key.

- Routines: `office-kt-inbox` (every 15 minutes), `office-signals` (daily),
  `office-daily-digest` (daily), `office-bootstrap` (every 15 minutes),
  `office-shared-sync` (hourly).
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

`office-meeting-inbox` is registered under routines and jobs. Its disabled
15-minute routine targets `job:office-meeting-inbox`; the single deterministic
`plugin.tool_call` invokes `office.extract` with `{profile: meeting}` and no id.
The handler walks all unsummarized meetings, transcribes empty audio captures,
extracts cited decisions and to-dos, then publishes the summary completion
marker. No agent loop or crew is involved. The job has `max_active_runs: 1`.

`office-shared-sync` is registered under routines and jobs. The disabled
hourly routine (`0 * * * *`) targets `job:office-shared-sync`; its one
deterministic `plugin.tool_call` invokes `office.library-sync` with empty
input, which pulls the team's shared library and pushes only documents the
user marked shareable. Incoming updates and conflicts become proposals for
the user; the job approves nothing. It has `max_active_runs: 1` and creates
no agent loop, crew or task of its own.


`auto_tasks/office-conversation-extract.yaml` ships a disabled half-hour missed-session
sweep. Its task complexity is `low`, with no pinned crew/model, so dispatch chooses
from the host's configured low-complexity pool. It previews staged transcripts with
`office.capture`, selects numbered evidence, then captures against the preview digest.
Private previews withhold content and use local conservative extraction instead.
The orchestrator remains responsible for rolling handoffs.

This definition is **not registered yet**: `plugin.yaml` outside reserved tool blocks
is integrator-owned. A separate integration adds
`definitions/auto_tasks/office-conversation-extract.yaml` under `spec.definitions.auto_tasks`;
enabling it remains an operator decision. Constellation's SessionEnd hook must stage
completed transcripts and queue one extraction task/job immediately, with this same
instruction, complexity and required tools; the sweep only catches missed sessions.
See the host placement, staging and privacy contract in
`docs/design/office/specs/sessions.md` (in the repository).
