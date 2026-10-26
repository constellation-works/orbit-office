# office plugin

The installable Orbit plugin (namespace `office`). The plugin tree lives
in `.orbit-plugin/`; `orbit plugin validate` / `orbit plugin test` run
against a fresh export of this directory (see `make export`).

**Status: scaffold.** Only the `office.status` tool exists. Planned tools
(each lands with its schemas, handler, goldens and tests):

`capture`, `transcribe`, `extract`, `source.show`, `search`,
`knowledge.show`, `knowledge.write`, `artifact.show`, `artifact.promote`,
`attach`, `signals.scan`, `proposal.create`, `proposal.accept`, `metrics`,
`review.inputs`, `reindex`.

## Layout

- `.orbit-plugin/plugin.yaml` — manifest (tools, permissions, secrets, skills, panels).
- `.orbit-plugin/bin/office` — exec backend launcher. The runtime is the
  Python standard library for the scaffold; the launcher uses `uv` to find
  (or install into plugin state) Python >= 3.12 and runs `src/office/` from
  the plugin root.
- `.orbit-plugin/schemas/` — request/response JSON Schemas per tool, plus `config.json`.
- `.orbit-plugin/src/office/` — protocol dispatch (`protocol.py`) and tool
  handlers (`tools.py`). Identity and secrets come from the call context, never input.
- `.orbit-plugin/skills/` — `orchestrator`, `library-conventions`, `meetings`.
- `.orbit-plugin/definitions/` — routines, auto-tasks and activities seeded
  disabled. Empty in the scaffold; definitions land with the tools they call.
- `.orbit-plugin/tests/conformance/` — goldens for `orbit plugin test`
  (no network, no credentials).
- `tests/` — pytest suite holding the manifest, schemas, goldens and code
  in agreement. `pyproject.toml` / `uv.lock` carry dev tools only.

## Develop

```bash
make check                       # uv lock --check, ruff, basedpyright strict, pytest
out=$(make -s export)            # fresh export of HEAD's plugin tree
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
