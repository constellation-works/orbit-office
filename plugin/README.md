# office plugin

The installable Orbit plugin (namespace `office`), implemented in Rust.
The plugin tree lives in `.orbit-plugin/`; `orbit plugin validate` /
`orbit plugin test` run against a fresh export of this directory (see
`make export`).

**Status: scaffold.** Only the `office.status` tool exists. Planned tools
(each lands with its schemas, handler, goldens and tests):

`capture`, `transcribe`, `extract`, `source.show`, `search`,
`knowledge.show`, `knowledge.write`, `artifact.show`, `artifact.promote`,
`attach`, `signals.scan`, `proposal.create`, `proposal.accept`, `metrics`,
`review.inputs`, `reindex`.

## Layout

- `.orbit-plugin/plugin.yaml` — manifest (tools, permissions, secrets,
  skills, panels, and `spec.build` for source-built installs).
- `.orbit-plugin/bin/office` — exec launcher; it execs the Rust backend
  beside it as `bin/office-bin` and builds nothing at call time.
- `.orbit-plugin/schemas/` — request/response JSON Schemas per tool, plus `config.json`.
- `.orbit-plugin/skills/` — `orchestrator`, `library-conventions`, `meetings`.
- `.orbit-plugin/definitions/` — routines, auto-tasks and activities seeded
  disabled. Empty in the scaffold; definitions land with the tools they call.
- `.orbit-plugin/tests/conformance/` — goldens for `orbit plugin test`
  (no network, no credentials).
- `../crates/office-core/`, `../crates/office/` — the Rust backend at the
  repo root: `office-core` dispatches the Orbit exec envelope and holds the
  handlers; the `office` binary is the thin surface (argv, stdio). Identity
  and secrets come from the call context, never input. The manifest,
  schemas and goldens are held in agreement by
  `../crates/office/tests/plugin_tree.rs`.

## Build

Orbit builds the backend at install time for `git+` pinned sources
(`spec.build`: `cargo build --release` → `bin/office-bin`). For a local
directory install, `make export` prebuilds the same binary into the export.

## Develop

```bash
make check                       # the root workspace gate (`make ci` at the repo root)
out=$(make -s export)            # fresh export of HEAD's plugin tree + backend
orbit plugin validate "$out"
orbit plugin test "$out" --grant fs,network
```

## Install (operator)

```bash
orbit plugin add /tmp/office-plugin-<sha>
orbit plugin enable office --grant 'fs={{workspace}},{{plugin_state}}' --grant network
orbit plugin secret set office meta_api_key
orbit plugin secret set office baseten_token
```

Enabling is the operator's consent to the plugin's filesystem and network
access; agents do not run `orbit plugin add`/`enable`.
