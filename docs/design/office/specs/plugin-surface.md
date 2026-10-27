---
type: design
summary: "Spec: plugin surface — the office tools, their MCP visibility, shipped definitions, config and permissions"
last_validated: 2026-10-26
---

# Spec: Plugin surface

The `office` plugin is a component of the Orbit Office distribution: one tool
table, each tool with a kind, an MCP visibility, JSON Schemas, and a
handler. The manifest, schemas, goldens and code are held in agreement by
tests.

Source: spec §12. The one-call envelope is
[output-contract.md](./output-contract.md).

## Tools

| Tool | Kind | MCP | Purpose |
|---|---|---|---|
| `office.capture` | mutating | workspace | Create a source from an upload path or pasted text; dedupe; return the id. An optional `event_key` links a `meeting` source to its calendar event ([calendar.md](./calendar.md#capture-after-the-meeting)) |
| `office.transcribe` | mutating | workspace | Transcribe a video or meeting source's audio into its body in chunks (Muse Voice Transcribe); a no-op when a `.vtt` was imported |
| `office.video-plan` | mutating | **none** | Plan a video: duration, deduplicated keyframes, model-call counts, cost estimate; `confirm` with the digest the user saw ([kt-videos.md](./kt-videos.md)) |
| `office.frames` | mutating | **none** | Extract a confirmed video's keyframes and read them with Muse Spark image input into `frames.md` |
| `office.extract` | mutating | workspace | From a source, produce structured JSON plus a task artifact: `profile: kt` gives chapters, gotchas, knowledge candidates, runbook steps and questions; `profile: meeting` gives a summary, decisions and to-dos; `profile: digest` writes a source's `digest.json` |
| `office.calendar-sync` | mutating | **none** | Read the calendar through a gateway connection (or a workspace `.ics` file) and atomically replace the event snapshot in plugin state ([calendar.md](./calendar.md#officecalendar-sync)) |
| `office.calendar-list` | read_only | workspace | Events in a range, each with its prep artifact, captured sources and matched person pages; attendee emails never returned |
| `office.meeting-prep` | mutating | **none** | Write a cited prep brief artifact for each meeting starting within the lead time; run only by the `office-meeting-prep` routine ([calendar.md](./calendar.md#prep-officemeeting-prep)) |
| `office.source-show` | read_only | workspace | Return source text, provenance, citing documents/tasks and chapters; optional anchor, history and capped original/frame media ([library.md](./library.md#sources)) |
| `office.source-set-sensitivity` | mutating | **none** | Set source sensitivity under the writer lock; refuse private-to-normal |
| `office.search` | read_only | workspace | FTS over sources, knowledge and artifacts, with filters by kind, template, tag, series and date |
| `office.knowledge-show` | read_only | workspace | Read a page and its back-links |
| `office.knowledge-write` | mutating | **none** | Create or update a page; validate frontmatter, cites and its template ([knowledge-templates.md](./knowledge-templates.md#the-template-field-on-pages)); commit |
| `office.artifact-list` | read_only | workspace | List artifact summaries by kind and series, newest first, bounded with total and truncated |
| `office.artifact-show` | read_only | workspace | Read an artifact, its files, its series history and what cites it |
| `office.artifact-promote` | mutating | **none** | Copy an approved task's output into the Library as an artifact; write provenance; commit |
| `office.attach` | mutating | workspace | Pick the knowledge, source and example artifact files for a task, by template attach policy first ([knowledge-templates.md](./knowledge-templates.md#attach-policies)), and copy their model views into it as task artifacts under `context/` |
| `office.signals-scan` | read_only | workspace | Run the proposal signals and return candidate proposals with evidence |
| `office.proposal-create` | mutating | workspace | File a proposal task, its artifact and its evidence page |
| `office.proposal-accept` | mutating | **none** | Apply a proposal's payload; close the task |
| `office.metrics` | read_only | workspace | Compute the analytics metrics for a window |
| `office.selection-get` | mutating | workspace | Return the user's fresh, unused selections from the app; consumes them unless `peek: true`. Mutating because the default consumes |
| `office.selection-put` | mutating | **none** | Record a selection from the app; `clear: true` empties the tray |
| `office.session-brief` | read_only | workspace | Assemble the session-start brief from Orbit and the Library, with no model call ([sessions.md](./sessions.md#session-brief)) |
| `office.digest` | mutating | **none** | Write the day's digest artifact with one Muse Spark call; run only by the `office-daily-digest` routine ([sessions.md](./sessions.md#daily-digest)) |
| `office.script-run` | mutating | **none** | Run an approved `script` artifact as a routine step: pinned version, run directory under plugin state, offline dependencies, bounds; outputs filed as a `script-output` task for review ([scripts.md](./scripts.md)) |
| `office.batch-plan` | mutating | **none** | Plan a bootstrap phase's batch: calls and cost; confirm with the digest the user saw ([library-bootstrap.md](./library-bootstrap.md#batch-preview-and-confirmation)) |
| `office.consolidate` | mutating | **none** | Group a batch's digests by subject, apply the page rules, one careful call per passing group, file one proposal per page ([library-bootstrap.md](./library-bootstrap.md#stage-2-consolidate-per-topic-careful)) |
| `office.bootstrap-status` | read_only | workspace | Setup progress per phase, with computed gaps |
| `office.private-route-check` | mutating | **none** | Check the configured open-weights endpoint with a tiny text, image and audio request; record its capabilities ([models-and-privacy.md](./models-and-privacy.md#checking-an-endpoint-officeprivate-route-check)) |
| `office.review-inputs` | read_only | workspace | Gather a period's completed work, conversions, metrics, decisions and knowledge changes for a review |
| `office.init` | mutating | **none** | Scaffold the Library: layout, git repository, config, `.gitignore`, page templates (no pages), index; idempotent ([library.md](./library.md#scaffolding-officeinit)) |
| `office.config-set` | mutating | **none** | Validate and write one Library setting in `.office/config.yaml`; commit |
| `office.reindex` | mutating | **none** | Rebuild the SQLite index from the Library |
| `office.status` | read_only | workspace | Library and index health, and the private endpoint as `open_weights` or null (also a panel) |

Rules:

- `mcp_scope: none` tools are callable only by the app, through
  `orbit tool run`, and by the install script and the app acting as the user for `office.init`. The
  orchestrator cannot scaffold the Library, approve its own proposals,
  write knowledge, promote artifacts, record selections, change source sensitivity or settings,
  write digests, run scripts or reindex
  ([orchestrator.md](./orchestrator.md)).
- `execution_kind` is about consequence: anything that changes state or
  costs money is `mutating`, including `selection-get` (it consumes).
- Unknown input keys are refused, never ignored; identity and secrets come
  from the call context, never from input.
- Secrets are declared in the manifest (`meta_api_key`, `open_weights_token`)
  and set with `orbit plugin secret set office <name>`; handlers report
  presence only.

`orbit_tools` callbacks requested: `orbit.task.add`, `orbit.task.update`,
`orbit.task.list`, `orbit.task.show`, `orbit.task.artifact.get`,
`orbit.task.artifact.put`, `orbit.friction.add`, `orbit.search`,
`orbit.auto_task.add`, `orbit.auto_task.update`, `orbit.auto_task.list`.
The auto-task callbacks serve accepted `auto_task` proposals
([proposals.md](./proposals.md)); Orbit has no routine-create callback.

The synchronous `office-core::ports::Orbit` port has one method per callback.
It forwards JSON objects intact to Orbit's schemas (unknown keys are not dropped)
and returns the full JSON result, including list totals, truncation metadata and
task field projections. `office-orbit::OrbitCallbacks` implements the port using
`orbit tool run <name> --json --full --input <json>`; every method crosses the same
exact-name allowlist before starting a process. This local allowlist prevents
accidental calls; Orbit's host-owned session remains the authority boundary.

Composition supplies the workspace root, `ORBIT_BIN` (or `orbit` on `PATH`),
and the adapter's explicit `CALLBACK_ENV_NAMES` environment allowlist. Each child
starts with a cleared environment. The list carries non-secret CLI roots, caller
provenance, managed-run context and the plugin callback markers; operator overrides,
claim tokens and ambient provider keys are excluded. The already-open descriptor
named by `ORBIT_PLUGIN_CALLBACK_FD` is inherited unchanged, never read by Office or
copied to argv/input. The adapter always sets the non-secret
`ORBIT_PLUGIN_CALLBACK=descriptor-required` marker, so a missing descriptor is
refused by Orbit rather than treated as an ordinary user invocation.
This is Orbit's plugin callback mechanism (plugin scope §4.2);
Orbit authenticates the descriptor and enforces manifest ∩ grants ∩ caller ceiling
on each call. Office never reads an Orbit store directly.

The shared subprocess supervisor enforces a 30-second deadline, a 512 KiB limit per
stream, concurrent draining, and process-group cleanup/reaping. Success is the
CLI's JSON on stdout; nonzero exits with structured `{error, code}` JSON on stderr
become typed refusals carrying only a bounded machine code. Raw diagnostics and
details are discarded. Missing executables, malformed responses, unstructured
nonzero exits and supervisor failures are distinct typed adapter errors. Calls
are never automatically retried. A mutating callback that started but lost its
reply reports `orbit_outcome_unknown`; a failed spawn is a transport failure,
since no write could have been sent. Confirm the task, artifact or auto-task
state in Orbit before retrying an unknown outcome.
An absent callback context refuses with `orbit_unavailable`. The recording testkit
fake queues explicit results and exposes full calls through a shared clone; an
unconfigured reply also refuses rather than inventing a successful write.

## Definitions shipped

Routines and auto-tasks are seeded disabled and enabled during onboarding.
Activities have no schedule switch. Definitions load only when listed in
`spec.definitions` in the plugin manifest.

- **Routines:**
  - `office-kt-inbox`: every 15 minutes, seeded disabled, process confirmed video plans: transcribe, frames, extract (`kt`), then `office.consolidate` over the videos' digests; extraction files question to-dos ([kt-videos.md](./kt-videos.md#3-process-the-office-kt-inbox-routine)).
  - `office-bootstrap`: every 15 minutes, run a confirmed bootstrap batch: digests (and the KT steps for videos), then `office.consolidate` ([library-bootstrap.md](./library-bootstrap.md#batch-preview-and-confirmation)).
  - `office-calendar-sync`: every 15 minutes, seeded disabled; `office.calendar-sync` ([calendar.md](./calendar.md)).
  - `office-meeting-prep`: every 15 minutes, seeded disabled; `office.meeting-prep` for meetings within the lead time.
  - `office-meeting-inbox`: every 15 minutes, with the `meetings` pack: transcribe, extract (`meeting`) and save `summary.md` for each captured meeting without one.
  - `office-signals`: daily.
  - `office-daily-digest`: daily at `5 0 * * *`, seeded disabled. Targets `job:office-daily-digest`, whose sole step inlines a deterministic `plugin.tool_call` activity (`config.tool: office.digest`). The routine and job are registered in `spec.definitions`; no agent, crew or task ([sessions.md](./sessions.md#daily-digest)).
- **Auto-tasks:**
  - `office-knowledge-maintenance`: weekly. Re-verifies stale pages against newer sources and frictions, and files `knowledge_update` proposals.
  - `office-review`: on period end.
- **Activities:** `office-agent-todo`, an agent loop that executes an `owner:agent` to-do with its attached runbook and example artifacts, and writes its output as task artifacts with the manifest. Its `proc.spawn` allowlist has `python3` and no cloud CLI, so its agents can use the `gateway` plugin and Orbit refuses them `cloudcli` ([gateway.md](./gateway.md)).
- **Skills:** `orchestrator`, `library-conventions` (how to cite, write pages and package outputs), and one skill per pack (`kt`, `meetings`, later `receipts`).

The digest activity names its tool in `config.tool`; the routine uses only
Orbit's accepted routine fields and has no `required_tools` key.

A definition lands only with the tools it calls. An activity lists its
callable tools in `spec.tools`; `required_tools` belongs to task records.
The shipped activity is registered in `spec.definitions.activities` as
`definitions/activities/office-agent-todo.yaml`. No definition requires an
outward-facing action.

## Config and permissions

- **Plugin config** (`[plugins.office]`, schema `schemas/config.json`)
  holds only `library_dir`, the Library directory relative to the
  workspace (default `library`; it must stay inside the write grant).
- **Library settings** live in `library/.office/config.yaml`: `retention`
  per kind, default `sensitivity` per kind, enabled packs, signal
  thresholds, the demo `simulated_until` date and the private route's
  `open_weights` block. `office.init` writes the defaults, the app changes
  them through `office.config-set`, and the plugin validates the file on
  every read. Secrets are never in it.
- **Permissions** (`spec.permissions`, granted at `orbit plugin enable`):
  - fs read and write on `{{workspace}}/library` and `{{plugin_state}}`.
  - network `any`, the only setting that reaches remote hosts. The plugin calls only `api.meta.ai` and the one private endpoint the user configured ([models-and-privacy.md](./models-and-privacy.md#the-private-route)).
  - `orbit_tools`: the callbacks listed above.
  - Secrets: `meta_api_key` and `open_weights_token` (`spec.secrets`).
- **Required programs:** `spec.requires.programs: [git, ffmpeg, ffprobe, uv, pdftotext]` (`pdftotext` is optional: without it PDFs are refused with `converter_unavailable`), plus
  `cargo` under `spec.build` for source-built installs. Programs are
  resolved against the operator's `PATH` when the plugin is enabled, and
  that path is the one granted. `ffmpeg` splits long audio for
  transcription; `uv` runs approved scripts ([scripts.md](./scripts.md)) and is the
  install's own copy, on the `PATH` the install enables the plugin with.
- Transcription runs as a deterministic `plugin.tool_call` job step from
  the KT routine. That step is bounded by the operator's grant, so it
  can spawn `ffmpeg` and `ffprobe`. An agent calling `office.transcribe` would also need
  `ffmpeg` on its activity's `proc.spawn` allowlist, so agents don't call
  it.

## Build and install

Orbit builds the Rust backend at install time for `git+` pinned sources
(`spec.build`: `cargo build --release -p office` → `bin/office-bin`). For
a local directory install, `make -C plugin export` prebuilds the same
binary into the export. The launcher (`bin/office`) execs `bin/office-bin`
and builds nothing at call time.

### Library settings shape

The pure `Settings::from_yaml` loader supports two-level YAML mappings, scalars,
and inline or two-space-indented block lists of pack names (JSON mappings are
also accepted). Quotes and comments are supported. Unsupported YAML constructs,
duplicate YAML keys, unknown keys/kinds/packs and invalid types/ranges are refused
with `invalid_library_setting` naming the dotted key; raw values are never echoed.
The adapter validates `.office/config.yaml` on every Library read. A missing
file requires `office.init`; missing keys within a present file use these defaults:

```yaml
retention:
  video: 90
  meeting: 90
  chat: null
  email: null
  doc: null
  receipt: null
  bill: null
  conversation: null
  note: null
sensitivity:
  video: normal
  meeting: normal
  chat: normal
  email: normal
  doc: normal
  receipt: normal
  bill: normal
  conversation: normal
  note: normal
packs: [kt, meetings]
video:
  max_frames_per_hour: 120
  scene_threshold: 0.3
costs:
  currency: null
  audio_minute: null
  image: null
  text_1k_tokens: null
thresholds:
  repetition_count: 3
  window_days: 30
  stale_days: 90
open_weights: null
```

Retention is a positive integer up to 36,500 days or `null` (keep indefinitely).
Sensitivity is `normal` or `private`. Packs are distinct names from `kt`,
`meetings` and `receipts`; an empty list disables all packs. Repetition count is 1–10,000;
window/stale days are 1–36,500. The optional `open_weights` block names the
private route ([models-and-privacy.md](./models-and-privacy.md#the-private-route)):
`endpoint` (an OpenAI-compatible base URL over HTTPS, or HTTP(S) on loopback,
with no credentials, query or fragment, and never `api.meta.ai`), `model`, `kind`
(`company`, `provider` or `own_hardware`) and `label`, all required.
`capabilities` (a distinct list of `text`, `image`, `audio`) and `verified_at`
(RFC 3339) are written together by `office.private-route-check`; either one
alone, an unknown kind, an unknown block key or a refused endpoint fails with
`invalid_library_setting` naming the dotted key (`open_weights.kind`,
`open_weights.capabilities`, …); the raw URL is not echoed. No timestamp is
synthesized by defaults. `office.status` reports that block as `open_weights`
(or null when none is configured). `capabilities` and `verified_at` are null
until a check records them. The token is not in the object;
`secrets.open_weights_token` is `set` or `unset`.
These are Library policy settings, independent of plugin installation config.

### Private-route field limits

The `open_weights` `endpoint`, `model`, `kind` and `label` values must each be
nonempty, contain no control characters, and fit these limits (Unicode scalar
values):

| Field | Maximum | Additional rule |
| --- | ---: | --- |
| `endpoint` | 2,048 characters | Must also satisfy the URL restrictions above. |
| `model` | 200 characters | Must not contain whitespace. |
| `kind` | 32 characters | Must be `company`, `provider` or `own_hardware`. |
| `label` | 80 characters | — |

### Updating one Library setting

`office.config-set` takes exactly `{"key":"retention.meeting","value":30}`.
The value is typed JSON, not a YAML string. Supported keys are
`retention.<kind>`, `sensitivity.<kind>`, `thresholds.<name>`, `scripts.<timeout_seconds|log_bytes|output_files|output_file_bytes>`, `packs`, `demo.simulated_until`,
`open_weights.<endpoint|model|kind|label>` and the whole `open_weights` block
(a mapping of those four keys, or null), using the kinds, threshold names and
bounds above. `demo.simulated_until` is a `YYYY-MM-DD` date, or null to clear it.
A leaf needs the block to exist. Changing the endpoint or model
clears the recorded capabilities. A whole-block update follows the same rule:
when both endpoint and model are unchanged, it preserves the recorded
capabilities and verification time; changing either clears both. The
`open_weights.capabilities` and `open_weights.verified_at` fields are refused
with `invalid_library_setting`, because only `office.private-route-check`
writes them.
The additional thresholds `friction_count` and `new_subject_count` default to
2 (range 1–10,000); `unused_days` defaults to 30 (range 1–36,500).
Whole retention/sensitivity/threshold mappings cannot be replaced in one call.
Unknown keys, including secret names, return `unknown_library_setting`;
invalid values return `invalid_library_setting`. Missing arguments or unknown
input fields return `invalid_argument`. Refusals do not change config or HEAD.
Raw values are never echoed in errors or responses.

The Library port reads the latest settings, applies the typed update, writes
`.office/config.yaml` atomically and adds exactly one Library commit while
holding the same writer lock as document writes. Other settings and unrelated
staged files are preserved. The response is
`{"path":".office/config.yaml","key":"retention.meeting"}`, relative to
the Library. The file is normalized to a formatted JSON mapping (valid YAML),
including defaults; hand-edited comments and formatting are not retained.
Repeated identical updates still add one write-event commit. No network or
secrets are needed. Secrets remain managed by Orbit's plugin secret surface.
The KT pack adds the `video` kind, the `kt` pack, and the `video` and `costs` keys
([kt-videos.md](./kt-videos.md#settings)). The default pack list is `[kt, meetings]`.
`video.max_frames_per_hour` is an integer from 1 through 600, and
`video.scene_threshold` is a finite number from 0.05 through 0.95. The cost
rates are finite, nonnegative numbers or null, and `costs.currency` is a
three-letter uppercase currency code or null. Missing keys receive these
defaults; existing explicit pack lists keep their values.

## Task References

- [ORB-18838] — renamed the scaffold README's KT routine from `office-meeting-inbox` to `office-kt-inbox`.

> Resolve any task above with `orbit task show <ID>` or `git log --grep=<ID>`.

### Artifact callback reply budget

The default Orbit callback capture remains 512 KiB per stream with a
30-second deadline. `orbit.task.artifact.get` alone allows stdout up to
6 MiB plus 64 KiB: a 1 MiB file can expand sixfold when JSON escapes control
bytes, plus the reply envelope. Its stderr stays at 512 KiB. The shared
supervisor still bounds both streams and reaps the whole process group.
Promotion separately enforces the decoded 1 MiB per-file limit before a
Library write; increasing the transport allowance does not raise that limit.

Script execution bounds are validated Library settings under `scripts`; defaults, caps and approval/preparation contracts are specified in [scripts.md](./scripts.md#implementation-contracts).
