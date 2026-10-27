---
type: design
summary: "Spec: plugin surface — the office tools, their MCP visibility, shipped definitions, config and permissions"
last_validated: 2026-10-26
---

# Spec: Plugin surface

The `office` plugin is the deterministic half of the product: one tool
table, each tool with a kind, an MCP visibility, JSON Schemas, and a
handler. The manifest, schemas, goldens and code are held in agreement by
tests.

Source: spec §12. The one-call envelope is
[output-contract.md](./output-contract.md).

## Tools

| Tool | Kind | MCP | Purpose |
|---|---|---|---|
| `office.capture` | mutating | workspace | Create a source from an upload path or pasted text; dedupe; return the id |
| `office.transcribe` | mutating | workspace | Transcribe a meeting source's original into its body (Muse Voice Transcribe) |
| `office.extract` | mutating | workspace | From a source, produce a summary, decisions and to-dos as structured JSON plus a task artifact |
| `office.source.show` | read_only | workspace | Return a source, optionally at an anchor |
| `office.search` | read_only | workspace | FTS over sources, knowledge and artifacts, with filters by kind, template, tag, series and date |
| `office.knowledge.show` | read_only | workspace | Read a page and its back-links |
| `office.knowledge.write` | mutating | **none** | Create or update a page; validate frontmatter, cites and its template ([knowledge-templates.md](./knowledge-templates.md#the-template-field-on-pages)); commit |
| `office.artifact.show` | read_only | workspace | Read an artifact, its files, its series history and what cites it |
| `office.artifact.promote` | mutating | **none** | Copy an approved task's output into the Library as an artifact; write provenance; commit |
| `office.attach` | mutating | workspace | Pick the knowledge, source and example artifact files for a task, by template attach policy first ([knowledge-templates.md](./knowledge-templates.md#attach-policies)), and copy their model views into it as task artifacts under `context/` |
| `office.signals.scan` | read_only | workspace | Run the proposal signals and return candidate proposals with evidence |
| `office.proposal.create` | mutating | workspace | File a proposal task, its artifact and its evidence page |
| `office.proposal.accept` | mutating | **none** | Apply a proposal's payload; close the task |
| `office.metrics` | read_only | workspace | Compute the analytics metrics for a window |
| `office.selection.get` | mutating | workspace | Return the user's fresh, unused selections from the app and mark them consumed. Mutating because it consumes |
| `office.selection.put` | mutating | **none** | Record a selection from the app; `clear: true` empties the tray |
| `office.session.brief` | read_only | workspace | Assemble the session-start brief from Orbit and the Library, with no model call ([sessions.md](./sessions.md#session-brief)) |
| `office.digest` | mutating | **none** | Write the day's digest artifact with one Muse Spark call; run only by the `office-daily-digest` routine ([sessions.md](./sessions.md#daily-digest)) |
| `office.script.run` | mutating | **none** | Run an approved `script` artifact as a routine step: pinned version, run directory under plugin state, offline dependencies, bounds; outputs filed as a `script-output` task for review ([scripts.md](./scripts.md)) |
| `office.review.inputs` | read_only | workspace | Gather a period's completed work, conversions, metrics, decisions and knowledge changes for a review |
| `office.init` | mutating | **none** | Scaffold the Library: layout, git repository, config, `.gitignore`, page templates (no pages), index; idempotent ([library.md](./library.md#scaffolding-officeinit)) |
| `office.config.set` | mutating | **none** | Validate and write one Library setting in `.office/config.yaml`; commit |
| `office.reindex` | mutating | **none** | Rebuild the SQLite index from the Library |
| `office.status` | read_only | workspace | Library and index health (also a panel) |

Rules:

- `mcp_scope: none` tools are callable only by the app, through
  `orbit tool run`, and by the install script for `office.init`. The
  orchestrator cannot scaffold the Library, approve its own proposals,
  write knowledge, promote artifacts, record selections, change settings,
  write digests, run scripts or reindex
  ([orchestrator.md](./orchestrator.md)).
- `execution_kind` is about consequence: anything that changes state or
  costs money is `mutating`, including `selection.get` (it consumes).
- Unknown input keys are refused, never ignored; identity and secrets come
  from the call context, never from input.
- Secrets are declared in the manifest (`meta_api_key`, `baseten_token`)
  and set with `orbit plugin secret set office <name>`; handlers report
  presence only.

`orbit_tools` callbacks requested: `orbit.task.add`, `orbit.task.update`,
`orbit.task.list`, `orbit.task.show`, `orbit.task.artifact.get`,
`orbit.task.artifact.put`, `orbit.friction.add`, `orbit.search`.

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
since no write could have been sent. Confirm the task or artifact state in Orbit
before retrying an unknown outcome.
An absent callback context refuses with `orbit_unavailable`. The recording testkit
fake queues explicit results and exposes full calls through a shared clone; an
unconfigured reply also refuses rather than inventing a successful write.

## Definitions shipped

Seeded disabled, enabled during onboarding:

- **Routines:**
  - `office-meeting-inbox`: every 15 minutes, process new meeting uploads, then capture, transcribe, extract, file to-dos and propose knowledge updates.
  - `office-signals`: daily.
  - `office-daily-digest`: daily. One deterministic `plugin.tool_call` step running `office.digest`; no agent, crew or task ([sessions.md](./sessions.md#daily-digest)).
- **Auto-tasks:**
  - `office-knowledge-maintenance`: weekly. Re-verifies stale pages against newer sources and frictions, and files `knowledge_update` proposals.
  - `office-review`: on period end.
- **Activities:** `office-agent-todo`, an agent loop that executes an `owner:agent` to-do with its attached runbook and example artifacts, and writes its output as task artifacts with the manifest. Its `proc.spawn` allowlist has `python3` and no cloud CLI, so its agents can use the `gateway` plugin and Orbit refuses them `cloudcli` ([gateway.md](./gateway.md)).
- **Skills:** `orchestrator`, `library-conventions` (how to cite, write pages and package outputs), and one skill per pack (`meetings`, later `receipts`).

A definition lands only with the tools it calls. Every definition's
`required_tools` lists every plugin tool its steps call, and no definition
requires an outward-facing action.

## Config and permissions

- **Plugin config** (`[plugins.office]`, schema `schemas/config.json`)
  holds only `library_dir`, the Library directory relative to the
  workspace (default `library`; it must stay inside the write grant).
- **Library settings** live in `library/.office/config.yaml`: `retention`
  per kind, default `sensitivity` per kind, enabled packs, signal
  thresholds and `open_weights_endpoint`. `office.init` writes the
  defaults, the app changes them through `office.config.set`, and the
  plugin validates the file on every read. Secrets are never in it.
- **Permissions** (`spec.permissions`, granted at `orbit plugin enable`):
  - fs read and write on `{{workspace}}/library` and `{{plugin_state}}`.
  - network `any`, the only setting that reaches remote hosts. The plugin calls only the Meta and Baseten endpoints.
  - `orbit_tools`: the callbacks listed above.
  - Secrets: `meta_api_key` and `baseten_token` (`spec.secrets`).
- **Required programs:** `spec.requires.programs: [git, ffmpeg, uv]`, plus
  `cargo` under `spec.build` for source-built installs. Programs are
  resolved against the operator's `PATH` when the plugin is enabled, and
  that path is the one granted. `ffmpeg` splits long audio for
  transcription; `uv` runs approved scripts ([scripts.md](./scripts.md)) and is the
  install's own copy, on the `PATH` the install enables the plugin with.
- Transcription runs as a deterministic `plugin.tool_call` job step from
  the meeting routine. That step is bounded by the operator's grant, so it
  can spawn `ffmpeg`. An agent calling `office.transcribe` would also need
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
  meeting: 90
  chat: null
  email: null
  doc: null
  receipt: null
  bill: null
  conversation: null
  note: null
sensitivity:
  meeting: normal
  chat: normal
  email: normal
  doc: normal
  receipt: normal
  bill: normal
  conversation: normal
  note: normal
packs: [meetings]
thresholds:
  repetition_count: 3
  window_days: 30
  stale_days: 90
open_weights_endpoint: null
```

Retention is a positive integer up to 36,500 days or `null` (keep indefinitely).
Sensitivity is `normal` or `private`. Packs are distinct names from `meetings`
and `receipts`; an empty list disables all packs. Repetition count is 1–10,000;
window/stale days are 1–36,500. The optional `open_weights_endpoint` is a
Baseten HTTPS URL (`https://inference.baseten.co/...` or
`https://<deployment>.api.baseten.co/...`) or a loopback HTTP(S) URL
(`localhost`, `127.0.0.1`, or `::1`), with no embedded credentials. That is
the open-weights subset of the model host allowlist: hosted calls may also
use `api.meta.ai`, and that host is not an open-weights endpoint. Any other
host is refused at load with `invalid_library_setting` on
`open_weights_endpoint`; the raw URL is not echoed. No timestamp is
synthesized by defaults.
These are Library policy settings, independent of plugin installation config.
