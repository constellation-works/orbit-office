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
| `office.search` | read_only | workspace | FTS over sources, knowledge and artifacts, with filters by kind, tag, series and date |
| `office.knowledge.show` | read_only | workspace | Read a page and its back-links |
| `office.knowledge.write` | mutating | **none** | Create or update a page; validate frontmatter and cites; commit |
| `office.artifact.show` | read_only | workspace | Read an artifact, its files, its series history and what cites it |
| `office.artifact.promote` | mutating | **none** | Copy an approved task's output into the Library as an artifact; write provenance; commit |
| `office.attach` | read_only | workspace | Pick the knowledge, source and example artifact files for a task |
| `office.signals.scan` | read_only | workspace | Run the proposal signals and return candidate proposals with evidence |
| `office.proposal.create` | mutating | workspace | File a proposal task, its artifact and its evidence page |
| `office.proposal.accept` | mutating | **none** | Apply a proposal's payload; close the task |
| `office.metrics` | read_only | workspace | Compute the analytics metrics for a window |
| `office.selection.get` | mutating | workspace | Return the user's fresh, unused selections from the app and mark them consumed. Mutating because it consumes |
| `office.selection.put` | mutating | **none** | Record a selection from the app; `clear: true` empties the tray |
| `office.review.inputs` | read_only | workspace | Gather a period's completed work, conversions, metrics, decisions and knowledge changes for a review |
| `office.reindex` | mutating | **none** | Rebuild the SQLite index from the Library |
| `office.status` | read_only | workspace | Library and index health (also a panel) |

Rules:

- `mcp_scope: none` tools are callable only by the app, through
  `orbit tool run`. The orchestrator cannot approve its own proposals,
  write knowledge, promote artifacts, record selections or reindex
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

## Definitions shipped

Seeded disabled, enabled during onboarding:

- **Routines:**
  - `office-meeting-inbox`: every 15 minutes, process new meeting uploads, then capture, transcribe, extract, file to-dos and propose knowledge updates.
  - `office-signals`: daily.
- **Auto-tasks:**
  - `office-knowledge-maintenance`: weekly. Re-verifies stale pages against newer sources and frictions, and files `knowledge_update` proposals.
  - `office-review`: on period end.
- **Activities:** `office-agent-todo`, an agent loop that executes an `owner:agent` to-do with its attached runbook and example artifacts, and writes its output as task artifacts with the manifest.
- **Skills:** `orchestrator`, `library-conventions` (how to cite, write pages and package outputs), and one skill per pack (`meetings`, later `receipts`).

A definition lands only with the tools it calls. Every definition's
`required_tools` lists every plugin tool its steps call, and no definition
requires an outward-facing action.

## Config and permissions

- `[plugins.office]` holds: `library_dir`; `retention` per kind; default
  `sensitivity` per kind; enabled packs; signal thresholds;
  `open_weights_endpoint`.
- **Permissions** (`spec.permissions`, granted at `orbit plugin enable`):
  - fs read and write on `{{workspace}}/library` and `{{plugin_state}}`.
  - network `any`, the only setting that reaches remote hosts. The plugin calls only the Meta and Baseten endpoints.
  - `orbit_tools`: the callbacks listed above.
  - Secrets: `meta_api_key` and `baseten_token` (`spec.secrets`).
- **Required programs:** `spec.requires.programs: [git, ffmpeg]`, plus
  `cargo` under `spec.build` for source-built installs. Programs are
  resolved against the operator's `PATH` when the plugin is enabled, and
  that path is the one granted. `ffmpeg` splits long audio for
  transcription.
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
