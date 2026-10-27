# gateway plugins

Audited, redacted API requests through connections the user grants
([spec](../docs/design/office/specs/gateway.md), ARCHITECTURE.md "Gateway
plugins"). Python 3.12, standard library only: nothing to fetch, pin or audit
at run time.

## Layout

- `src/orbit_gateway/` — the shared package: `envelope` (the Orbit `exec`
  envelope), `redact` (the one redactor and its writer inventory), `state`
  (owner-only files under `{{plugin_state}}`), `audit`, and for the gateway
  plugin only `connections` and `http`.
- `plugins/gateway/.orbit-plugin/` — the `gateway` plugin: `plugin.yaml`,
  `bin/backend.py` (version check, then dispatch), `schemas/`, the `gateway`
  skill and `tests/conformance/` goldens. Exports carry the package in `lib/`.
- `tests/` — unittest, standard library only, through the real backend
  against loopback servers.
- `scripts/` — `export.py` (used by `make export` and the tests) and
  `check_imports.py` (the import-direction scan `make check` runs).

Tools: `gateway.http-get`, `gateway.connection-list`, `gateway.connection-put`
(app only) and `gateway.audit-list`. The draft and send tools are not built
yet.

## Develop

```bash
make -C gateway test                 # the unittest suite
make -C gateway check                # Python >= 3.12, import scan, schema JSON
out=$(make -s -C gateway export)     # a fresh export of the working tree
orbit plugin validate "$out"
orbit plugin test "$out" --grant fs,network
```

CI runs `make -C gateway check test` on Python 3.12 (the `gateway` job).

## Install (operator)

```bash
orbit plugin add "$out"
orbit plugin enable gateway --grant 'fs={{plugin_state}}' --grant network
```

Connections are added from the app's Settings (`gateway.connection-put`),
which then stores the secret with
`orbit plugin secret set gateway <slot>` (value on stdin). Agents never
create connections or see a secret.
