# Architecture

The layout every change follows. It is built for **many agents working at once**: each
feature lives in its own folders, every shared list is written once up front, and the
boundaries are checked by `make structure`. A lower layer never imports a higher one
(STD-02 R1, R6). Adding a crate or an edge changes this file and
`scripts/check-dependency-direction.sh` in the same commit. Why this shape:
[docs/design/office/4_decisions.md](docs/design/office/4_decisions.md#a-crate-per-enforceable-edge-a-folder-per-tool-and-every-shared-list-written-once).

Four pieces live in this repo: the Rust workspace (the plugin's exec backend), `plugin/`
(the installable Orbit plugin tree), `app/` (the TypeScript web app, which reaches the
backend only through Orbit's tools) and `gateway/` (two small Python plugins, see below).

## Rust layers

| Tier | Crate | Owns | May depend on (internal) |
|------|-------|------|--------------------------|
| 0. Foundation | `office-common` | The common error, fs mechanisms (path safety, atomic writes, SHA-256), SQLite connection defaults (`sqlite` feature), bounded subprocess supervision shared by adapters and fixtures, small shared types. | — |
| 1. Domain | `office-core` | **Pure.** The exec envelope (`protocol`), tool contract types (`tool`), declared secret names, the domain model and its rules (`domain/`, including Library records and validated settings), the bundled knowledge templates (`templates/*.md`, compiled in with `include_str!`), and the port traits the application calls (`ports/`). No I/O. | `office-common` |
| 2. Adapters | `office-store` | Implements the Library port: Markdown files (the source of truth), the rebuildable SQLite index, the Library's own git repository and its writer lock. Also owns plugin state, so it implements the script-runner port: run directories under `plugin_state/runs/` and the supervised `uv` child (specs/scripts.md). Structured after `orbit-store`. | `office-common`, `office-core` |
| 2. Adapters | `office-models` | Implements the model ports: Meta Model API (Muse Spark, Muse Voice Transcribe), Baseten (Muse Spark Open), privacy routing and local JSON Schema validation of extraction replies. The only crate that may speak HTTP. | `office-common`, `office-core` |
| 2. Adapters | `office-orbit` | Implements the Orbit port: the manifest's `orbit_tools` callbacks (`task.add`, `task.artifact.put`, …). | `office-common`, `office-core` |
| 3. Application | `office-tools` | One module per tool, the tool registry and dispatch. Talks to adapters **only through `office-core` ports**, so every tool runs against fakes. | `office-common`, `office-core` |
| 4. Composition + surface | `office` (binary) | `app`: builds the adapters, hands them to `office-tools`, serves one exec call. `cli`, `output`: argv, the one stdout envelope, exit codes. | every crate above |
| Test support | `office-testkit` | Fake ports, a Library fixture builder, envelope helpers. **Dev-dependency only**: no crate may list it under `[dependencies]`. | `office-common`, `office-core`, `office-store` |

`office-store` now uses its allowed `office-core` edge for the Library port and shared
records; `office-core` uses `office-common` for the shared sensitivity enum. Store record
exports remain compatible re-exports.

`office-orbit` uses its `office-core` edge for the eight Orbit port methods and its
`office-common` edge for bounded CLI callback supervision. The composing `office` binary
constructs this adapter from the host call context and inherited callback environment.
The testkit's recording Orbit fake shares observations across clones.

Port traits are declared in `office-core` because an adapter crate must depend on the crate
that defines the trait it implements (Rust's orphan rule). A trait declaration does no I/O,
so the domain stays pure. The four port interfaces and fakes are established together for dispatch; adapter
operations arrive with the first task that needs them. Each port has its own file.
The testkit depends on core for those fakes; tools use the testkit only as a dev-dependency.

The plugin backend is one process per call, so it is synchronous: no async runtime. HTTP
goes through a blocking client inside `office-models`.

`office-models` uses the external `jsonschema` crate with default features disabled.
Schema compilation and reply validation run in memory; HTTP and file retrieval are
disabled so schema references cannot add I/O outside the model routing boundary.
This dependency adds no internal crate edge.

## Crate policies

Checked by `scripts/check-dependency-direction.sh` from the manifests.

| Crate | Banned external crates |
|-------|------------------------|
| every library crate | surface crates: `clap`, `tracing-subscriber` and terminal crates |
| every crate except `office-models` and `office` | HTTP clients: `ureq`, `reqwest`, `hyper`, `isahc`, `attohttpc`, `curl` |

## Module rules

Checked by grep over non-test sources:

- `office-core` (every module): no `std::fs`, `std::net`, `std::process`, `tempfile` or `rustix`, and no environment or path discovery (`std::env::`, `home_dir`, `current_dir`).
- `office-tools`: the same bans. A tool reaches files, models and Orbit only through ports.
- `office-models`: no `std::process` except `src/media.rs`, the one `ffmpeg` audio-splitting subprocess, run under the bounded supervisor in `office-common` (STD-03 R11–R15).
- `office-store/src/contracts.rs`: no `std::fs`, `std::net` or `std::process` (record shapes only).
- `office/src/app.rs` imports no `cli` or `output`; `office/src/output/` imports no `app` or `cli`.
- Only `office/src/output/` touches stdout, stderr, TTY state or the color/format variables (`scripts/check-terminal-guard.sh`).

## Tools: one file each, one registry

```
crates/office-tools/src/
  registry.rs            every tool, one row each (integrator-owned)
  dispatch.rs            envelope → tool → envelope
  tools/
    status.rs
    sources/             capture  transcribe  video_plan  frames  extract  show   (specs/library.md, specs/kt-videos.md)
    knowledge/           show  write                                  (specs/library.md)
    artifacts/           show  promote                                (specs/library.md)
    library/             init  reindex  config_set  search  attach    (specs/library.md, plugin-surface.md)
    proposals/           signals_scan  create  accept                 (specs/proposals.md)
    selection/           get  put                                     (specs/selection-sharing.md)
    sessions/            brief  digest                                (specs/sessions.md)
    scripts/             run                                          (specs/scripts.md)
    bootstrap/           batch_plan  consolidate  status              (specs/library-bootstrap.md)
    insights/            metrics  review_inputs                       (specs/analytics-and-reviews.md)
```

Each tool file exports one `TOOL` constant: its `ToolSpec` (name, execution kind, MCP
scope) and, once built, its accepted input keys and handler.

- **Planned tools** are `Tool::planned(spec)`: no handler, so dispatch refuses them as
  `unknown_tool`, and the manifest does not list them. Nothing advertises a tool that does
  not run (STD-01 R36).
- **Building a tool** means, in that tool's own files only: switch its `TOOL` to
  `Tool::built(spec, accepts, handler)`, write the handler, add its schemas
  (`plugin/.orbit-plugin/schemas/<name>.{request,response}.json`), uncomment its reserved
  block in `plugin.yaml`, and add its conformance golden
  (`plugin/.orbit-plugin/tests/conformance/<name>.yaml`).
- `crates/office/tests/plugin_tree.rs` holds this together: every built tool is declared
  with a closed request schema and a golden, and every planned tool is absent from the
  manifest but keeps its reserved, commented slot.

## Plugin tree

```
plugin/.orbit-plugin/
  plugin.yaml          built tools declared; each planned tool has a commented block in its slot
  schemas/             <tool>.request.json, <tool>.response.json, config.json
  definitions/         routines/  auto_tasks/  activities/   one file per definition
  skills/<skill>/      SKILL.md
  tests/conformance/   one golden file per tool (the manifest globs *.yaml)
```

## Web app

npm workspaces under `app/`, one TypeScript project per package, joined by project
references (`tsc -b`). No framework and no bundler.

```
app/packages/
  contracts/   types shared by server and web: the app's HTTP API, feature and screen names
  server/src/
    main.ts          composition only
    http/            router.ts (every feature, integrator-owned), guard.ts, static.ts, types.ts
    orbit/           the `orbit tool run --json` client
    dispatch/        the 30-second dispatch pass
    features/<f>/    one folder per feature: today, sources, todos, approvals, routines,
                     library, insights, review, selection, settings
  web/
    static/          index.html, demo.html, manifest.webmanifest, icons/, style.css
    src/
      main.ts        mounts the router
      router.ts      every screen (integrator-owned)
      screen.ts      the Screen contract
      api/           typed fetch helpers
      ui/            shared pieces (cards, pills, provenance links, primary actions)
      screens/<s>/   one folder per screen: today, source, todos, routines, library,
                     insights, review
```

A feature or screen is a module exporting one constant with `status: "planned"` until it is
built. A planned server feature mounts no routes. A planned screen is left out of the
navigation.

Boundaries, checked by `scripts/check-app-boundaries.py`:

- `server/src/features/<f>/` imports only its own folder, `server/src/orbit/`,
  `server/src/http/types.ts`, `node:*` and `@orbit-office/contracts`.
- `web/src/screens/<s>/` imports only its own folder, `web/src/api/`, `web/src/ui/`,
  `web/src/screen.ts` and `@orbit-office/contracts`.
- `web` never imports `node:*` or the server; the server never imports `web`.
- `contracts` imports nothing outside itself. Web imports it with `import type` only, so
  the browser never needs to resolve it.

## Gateway plugins

`gateway` and `cloudcli` (docs/design/office/specs/gateway.md) are Python 3.12 plugins with
no third-party packages. They share one package, and neither depends on the Rust crates or
the app:

```
gateway/
  src/orbit_gateway/     shared: envelope, redact, audit, state (owner-only files), callbacks
    connections.py  http.py  drafts.py      used by the gateway plugin only
    cloud.py                                used by the cloudcli plugin only
  plugins/gateway/.orbit-plugin/    plugin.yaml, bin/backend.py, schemas/, skills/, tests/conformance/
  plugins/cloudcli/.orbit-plugin/   the same shape
  tests/                 unittest, standard library only
  Makefile               test, check, export (copies src/orbit_gateway into each export's lib/)
```

- Only `http.py` and `drafts.py` open network connections. Only `cloud.py` spawns a
  process, apart from `callbacks.py`, which runs only `orbit tool run` for the manifest's
  `orbit_tools` (supervised, bounded, never retried). The gateway backend never imports `cloud.py`, and the cloudcli backend never
  imports `http.py` or `drafts.py`. `make -C gateway check` enforces this with an import scan.
- Every writer that persists text goes through `redact.py`. Its inventory is a test.

## Integrator-owned files

Feature tasks do not edit these. A change to one is its own small task:

- `Cargo.toml` (workspace members and dependencies), `ARCHITECTURE.md`, `scripts/`
- `crates/office-tools/src/registry.rs` and each `tools/<family>/mod.rs`
- `plugin/.orbit-plugin/plugin.yaml` outside a tool's own reserved block
- `app/package.json`, `app/tsconfig*.json`, `app/packages/*/package.json`,
  `server/src/http/router.ts`, `web/src/router.ts`

## Task footprints

A feature task's `context_files` are the folders of that feature in each layer, for example
for proposals: `dir:crates/office-tools/src/tools/proposals`,
`dir:app/packages/server/src/features/approvals`, the tools' schema and golden files, and
`docs/design/office/specs/proposals.md`. Disjoint footprints let Orbit run the tasks in
parallel.

## Runtime concurrency

Routines, the app and the agent CLI can call tools at the same time, each in its own
process.

- Today: every SQLite connection runs in WAL mode with a 5-second busy timeout
  (`office-common` `storage::sqlite`).
- `office-store` takes one OS advisory writer lock at
  `plugin_state/library.writer.lock` around a write, its index update and its Library
  commit (and around reindex/recovery). Acquisition uses the bounded `File::try_lock`
  form of the standard-library file lock, with a five-second deadline and a diagnostic
  holder record. Lock order is writer file lock, then the short-lived index mutex.
  Reads never wait for the writer file lock or acquire it exclusively. If a write journal
  exists, a nonblocking shared probe of the existing lock distinguishes a live writer
  from an abandoned journal without creating lock/holder state. Reads during a live
  write may observe old or new files, index records and committed history, with no
  snapshot across them. A journal remaining under an idle probe (or with no lock file)
  refuses reads until recovery; see [store.md](docs/design/office/specs/store.md).
  The app server's Orbit client caps concurrent `orbit` subprocesses at four by default
  (configurable), queues calls FIFO, times out every call, and shares one in-flight poll
  between browser tabs. Its dispatch pass is single-flight.
- Orbit callbacks in the exec backend are synchronous and bounded to 30 seconds and
  512 KiB per output stream by `office-common::process`. The inherited callback descriptor
  stays open across the CLI child; Orbit owns authentication and live grant checks.
