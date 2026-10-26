# Architecture

The layer model that `scripts/check-dependency-direction.sh` enforces
(STD-02 R1, R6). A lower layer never imports a higher one. Adding a crate or
an edge changes this file and that script in the same commit.

The Rust workspace is the office plugin's backend. Two other pieces live in
this repo and are not Rust layers: `plugin/` (the installable Orbit plugin
tree) and `app/` (the JavaScript web app, which reaches this backend only
through Orbit's tools).

## Layers

| Tier | Unit | Owns | May depend on |
|------|------|------|---------------|
| 1. Persistence | `office-store` (`contracts`, `fs`, `driver::{file,sqlite}`, `repository`) | The Library on disk (plain Markdown, the source of truth) and the rebuildable SQLite FTS index; atomic owner-only writes, path safety, ordered schema migrations. Structured after `orbit-store`. | nothing internal |
| 1. Protocol domain | `office-core::protocol`, `::tools`, `::error` | The Orbit exec envelope dispatch, the tool table and handlers, secret-presence reporting; the crate's typed `Error`. No I/O. | nothing internal |
| 2. Composition | `office::app` | Reads one request envelope from stdin, hands it to the core, returns the one response envelope. Will open `office-store` from the call context and pass it to handlers whose tools need the Library. | `office-core`, `office-store` |
| 3. Surface | `office::cli`, `::output`, `::error` | The one subcommand (`orbit-tool`); writing the envelope to stdout; parse errors as usage errors (exit 2). The only stdout/stderr access. | tiers 1–2 |

## Crates

| Crate | Kind | Internal dependencies | Banned |
|-------|------|-----------------------|--------|
| `office-core` | library | — | `clap`, `tracing-subscriber` and any terminal crate |
| `office-store` | library | — | `clap`, `tracing-subscriber` and any terminal crate |
| `office` | binary `office` | `office-core`, `office-store` (allowed; wired when the first Library tool lands) | — |

Why two crates rather than one: [docs/design/office/4_decisions.md](docs/design/office/4_decisions.md#a-domain-library-crate-and-a-cli-crate).

## Module rules inside crates

Checked by grep over non-test sources:

- Every `office-core` module imports no `std::fs`, `std::net`, `std::process`, `tempfile` or `rustix`, and reads no environment or paths (`std::env::`, `home_dir`, `current_dir`).
- `office/src/app.rs` imports no `cli` or `output`.
- `office/src/output/` imports no `app` or `cli`.
- Only `office/src/output/` touches stdout, stderr, TTY state or the color/format variables (`scripts/check-terminal-guard.sh`).
