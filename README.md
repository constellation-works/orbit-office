# orbit-office

Local-first office work on [Orbit](https://github.com/constellation-works/orbit), for
any office worker, software engineers included. An Orbit plugin (`office`, Rust) and a
standalone web app (TypeScript) turn onboarding material, hour-long knowledge-transfer
videos, pasted chats and other sources into a cited Library (KT videos come back as a
chaptered index with the tangents folded away), to-dos for you and for agents,
evidence-backed proposals and generated reviews. Private material can go to Meta's
open-weights Muse Spark Open on an endpoint you choose instead of the hosted API.
Built with Muse Spark Open.

> **Pre-existing vs built for the hackathon:** Orbit is a pre-existing open-source
> agent runtime (MIT), used here as a labelled dependency pinned to a tested commit
> and never vendored, forked or added as a submodule. This repository was started
> from the constellation Rust CLI template
> ([constellation-works/rust-cli-template](https://github.com/constellation-works/rust-cli-template));
> its structure and gates are the pattern the Rust code follows. Everything
> else in this repository — the `office` plugin surface, the orchestrator, the
> Library and the web app — is new work.

**Status: scaffold.** The only working plugin tool is `office.status`. The KT video
loop, capture, knowledge, proposals, metrics and review tools land in the slices
listed in [docs/architecture.md](docs/architecture.md).

## Layout

| Path | What |
|---|---|
| `crates/office-common/` | Shared foundation, after Orbit's `orbit-common`: common error type, fs mechanisms, SQLite connection defaults, small shared types. |
| `crates/office-core/` | Pure domain (no I/O, no clap, no terminal): the exec envelope, tool contract types, domain model and port traits. |
| `crates/office-store/` | Library persistence, structured after Orbit's `orbit-store`: file driver for sources/knowledge/artifacts, rebuildable SQLite FTS index, atomic writes, schema migrations. |
| `crates/office-models/`, `crates/office-orbit/` | Adapters behind `office-core` ports: the Meta and Baseten model APIs (the only crate that speaks HTTP), and the Orbit callbacks. |
| `crates/office-tools/` | The application: the tool registry, dispatch, and one module per tool, grouped by spec area. |
| `crates/office-testkit/` | Test support (dev-dependency only): fake ports, Library fixtures, envelope helpers. |
| `crates/office/` | The binary: `office orbit-tool` serves one Orbit exec call (JSON envelope on stdin, one on stdout). The plugin launcher execs this binary. |
| `plugin/` | The installable Orbit plugin tree in `.orbit-plugin/` (`plugin.yaml`, `bin/office` launcher, schemas, skills, definitions, conformance goldens). |
| `app/` | The local web app (TypeScript, npm workspaces `contracts`, `server`, `web`): a local server that calls Orbit as the user through `orbit tool run --json`, owns the dispatch pass, and serves the single-page UI. Conversation happens in the user's agent CLI, not here. |
| `examples/library/` | An example Library layout. A real Library lives in the user's Orbit workspace, not in this repo. |
| `demo/synthetic/` | Synthetic demo data only. No private or real user data is ever committed here. |
| `docs/` | Design docs and the vendored constellation standards. The layout and its boundaries are in [ARCHITECTURE.md](ARCHITECTURE.md). |

## The backend binary

```sh
office orbit-tool < request.json
office orbit-tool < request.json > response.json
office --help
```

Exit status is 0 on success (a tool failure is reported in the response envelope,
not the exit code), 1 when a command fails and 2 on a usage error.

## Dependency: Orbit

Tested against Orbit `0.34.0`. Orbit is not vendored. The install script
(`install.sh`, specified in `docs/design/office/specs/install.md`) installs
it from npm when it is missing; by hand, install it separately, then add this
plugin from a fresh export of a commit (see `plugin/README.md`).

## Development

```sh
make ci           # fmt, structure, standards, clippy, cargo-deny, tests, goldens
make goldens      # compare help/output goldens with the built binary
```

The web app CI job runs `npm --prefix app ci`, `npm --prefix app run check`,
`npm --prefix app test` and `python3 -I scripts/check-app-boundaries.py` on
Linux and macOS.

## License

MIT; see [LICENSE](LICENSE).
