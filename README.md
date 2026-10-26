# orbit-office

Local-first office work on [Orbit](https://github.com/constellation-works/orbit): an
Orbit plugin (`office`) plus a standalone web app that together turn meetings,
pasted chats and other sources into to-dos for you and for agents, a compounding
Library of sources / knowledge / artifacts, evidence-backed proposals, and
generated reviews.

> **Pre-existing vs built for the hackathon:** Orbit is a pre-existing
> open-source agent runtime (MIT), used here as a labelled dependency pinned to
> a tested commit and never vendored, forked or added as a submodule.
> Everything in this repository — the `office` plugin, the orchestrator, the
> Library and the web app — is new work.

**Status: scaffold.** The only working plugin tool is `office.status`. The
meeting loop, capture, knowledge, proposals, metrics and review tools land in
the slices listed in [docs/architecture.md](docs/architecture.md).

## Layout

| Path | What |
|---|---|
| `plugin/` | The installable Orbit plugin (namespace `office`): the tree in `.orbit-plugin/` (`plugin.yaml`, `bin/office` launcher, schemas, `src/office/`, skills, definitions, conformance goldens) plus dev tooling (`pyproject.toml`, `tests/`). |
| `app/` | The local web app (TypeScript): a local server that talks to Orbit through one long-lived `orbit mcp` stdio client, the orchestrator agent loop (Muse Spark tool calling), and the single-page UI (Today, Chat, To-dos, Routines, Library, Insights, Review). |
| `examples/library/` | An example Library layout (`sources/`, `knowledge/`, `artifacts/`, `proposals/`, `.office/config.yaml`). A real Library lives in the user's Orbit workspace, not in this repo. |
| `demo/synthetic/` | Synthetic demo data only. No private or real user data is ever committed here. |
| `docs/` | Architecture and build-plan notes for this repo. |

## Dependency: Orbit

Tested against Orbit `0.34.0` (`d003608b5`). Orbit is not vendored: install it
separately, then add this plugin from a fresh export of a commit (see
`plugin/README.md`).

## Develop

```bash
make check        # plugin lint + tests; app typecheck when Node deps are installed
```

## License

MIT; see [LICENSE](LICENSE).
