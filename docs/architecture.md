# Architecture (scaffold)

One loop drives the product:

```
observe -> propose -> approve -> automate -> measure
  ^                                            |
  +------------- knowledge compounds ----------+
```

## Pieces

- **Orbit (pre-existing, dependency).** Tasks, audit trail, routines,
  auto-tasks, frictions and agent runs. Pinned to a tested commit; never
  vendored. The office workspace never runs an auto drain: agent to-dos are
  dispatched explicitly by id from the app server.
- **`office` plugin (`plugin/`).** Namespace `office`, `exec` backend: the
  Rust workspace at the repo root, laid out in [ARCHITECTURE.md](../ARCHITECTURE.md)
  (one module per tool in `crates/office-tools`). Owns the Library (sources,
  knowledge, artifacts), its rebuildable SQLite index, and the Library's own
  git repository, which each mutation commits.
- **Web app (`app/`).** A local server plus single-page UI (TypeScript, npm
  workspaces). The server is the only process the browser talks to; it calls
  Orbit as the user through `orbit tool run --json` and owns the dispatch
  pass.
- **Orchestrator agent (the user's agent CLI).** Muse Code by default, running
  the plugin's `orchestrator` skill over Orbit MCP at agent authority, with
  the `office.*` tools plus a curated subset of `orbit.*` tools. It plans,
  routes, proposes and reports; approval tools are hidden from it
  (`mcp_scope: none`), and it never takes an outward-facing action without
  approval.
- **Implementing agents.** Ordinary Orbit task runs using Orbit's existing
  crews. Their output is a task artifact until the user approves it, when it
  is promoted into the Library as an artifact.

## Models

| Use | Model |
|---|---|
| Orchestrator (the user's agent CLI) | Muse Code |
| Summaries, extraction, receipts (vision) | Muse Spark (Meta Model API) |
| Meeting transcription | Muse Voice Transcribe |
| Sources marked `private` | Muse Spark Open (open weights) |

API keys are Orbit plugin secrets (`meta_api_key`, `baseten_token`), never
config or Library content.

## Build slices

1. Plugin manifest, `office.status`, Library layout, index + `reindex`.
2. `capture`, `transcribe`, `extract`, meeting routine; Today + To-dos screens.
3. `knowledge.*`, `attach`, agent to-do activity, `artifact.promote`.
4. `signals.scan`, proposals; Needs-you + Routines screens.
5. `metrics`, `review`; Insights + Review screens.
6. Open-weight routing for `private` sources; receipts pack if time allows.

## Non-goals

Multi-user/teams, mobile, sending real email or making purchases, hardware,
and any change to Orbit core.
