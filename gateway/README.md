# gateway plugins

Audited, redacted API requests through connections the user grants, and
read-only cloud CLI calls with the plugin's own identities
([spec](../docs/design/office/specs/gateway.md), ARCHITECTURE.md "Gateway
plugins"). Python 3.12, standard library only: nothing to fetch, pin or audit
at run time.

## Layout

- `src/orbit_gateway/` — the shared package: `envelope` (the Orbit `exec`
  envelope), `redact` (the one redactor and its writer inventory), `state`
  (owner-only files under `{{plugin_state}}`), `audit`, `callbacks` (Orbit's
  `orbit tool run` callbacks, supervised and never retried), for the
  gateway plugin only `connections`, `http` and `drafts`, and for the
  cloudcli plugin only `cloud`.
- `plugins/gateway/.orbit-plugin/` — the `gateway` plugin: `plugin.yaml`,
  `bin/backend.py` (version check, then dispatch), `schemas/`, the
  `api-requests` skill and `tests/conformance/` goldens. Exports carry the
  package in `lib/`.
- `plugins/cloudcli/.orbit-plugin/` — the `cloudcli` plugin, the same shape,
  with the `cloud-cli` skill.
- `tests/` — unittest, standard library only, through the real backend
  against loopback servers and stub `aws`/`gcloud` executables.
- `scripts/` — `export.py` (used by `make export` and the tests) and
  `check_imports.py` (the import-direction scan `make check` runs).

Tools: `gateway.http-get`, `gateway.http-draft`, `gateway.http-send` (app
only), `gateway.connection-list`, `gateway.connection-put` (app only) and
`gateway.audit-list`.

`http-get` follows up to three redirects only when each target stays on the
connection's origin and within its base path. An outside-base redirect is
returned without a request to its target, with `redirect.reason` set to
`outside_base`.

A write is drafted, approved and sent once. `http-draft` stores the request
in `{{plugin_state}}/drafts/<id>.json` with its SHA-256 and files one
`proposed` task tagged `gateway-draft` (through the `orbit.task.add` and
`orbit.task.artifact.put` callbacks; the artifact is staged in
`{{workspace}}/.orbit-gateway/` and removed after the put). The app's Approve
calls `http-send` with the draft id and that digest; Dismiss rejects the task
and the draft is never sent. A draft moves `filing` → `pending` → `sending` →
`sent` or `outcome_unknown` (or `unfiled`, `dismissed`), and only `pending` is
ever sent.

## cloudcli

One tool, `cloudcli.run` (read_only), for the orchestrator only: the plugin
declares `aws` and `gcloud`, so Orbit refuses it to activities whose
`proc.spawn` allowlist lacks them. It runs `aws <service> <operation>` with a
`describe-`, `list-` or `get-` operation and `gcloud … list|describe`, after a
denylist of commands that read secrets, data or tokens; global flags and file
or URL parameter values are refused, and output is forced to JSON.

Read-only is enforced by the credentials: the user creates an AWS IAM user
with `ReadOnlyAccess` and a Google Cloud service account with `roles/viewer`
(the `cloud-cli` skill says how) and stores `aws_access_key_id`,
`aws_secret_access_key` and `gcp_service_account_json` as the plugin's
secrets. Each call writes them to `0600` files in a fresh `0700` directory
under `{{plugin_state}}/calls/`, points `AWS_SHARED_CREDENTIALS_FILE` and
`AWS_CONFIG_FILE` (or `CLOUDSDK_CONFIG`) and `HOME` at it, runs the CLI with
argv only and an allowlisted environment, stops it after 60 s, keeps 1 MiB
of output, redacts it, appends to `{{plugin_state}}/audit/` and deletes the
directory (a directory a killed backend left is swept by the next call).

## Develop

```bash
make -C gateway test                 # the unittest suite
make -C gateway check                # Python >= 3.12, import scan, schema JSON
out=$(make -s -C gateway export)     # a fresh export of the working tree
orbit plugin validate "$out"
orbit plugin test "$out" --grant fs,network,orbit_tools
out=$(make -s -C gateway export PLUGIN=cloudcli)
orbit plugin validate "$out"
orbit plugin test "$out" --grant fs,network
```

CI runs `make -C gateway check test` on Python 3.12 (the `gateway` job).

## Install (operator)

```bash
orbit plugin add "$out"
orbit plugin enable gateway --grant 'fs={{plugin_state}},{{workspace}}/.orbit-gateway' --grant network --grant orbit_tools
```

Connections are added from the app's Settings (`gateway.connection-put`),
which then stores the secret with
`orbit plugin secret set gateway <slot>` (value on stdin). Agents never
create connections or see a secret.

`cloudcli` is optional (the install's `--with-cloudcli`), and needs `aws`
and `gcloud` on the enabling `PATH` so Orbit records them:

```bash
out=$(make -s -C gateway export PLUGIN=cloudcli)
orbit plugin add "$out"
orbit plugin enable cloudcli --grant 'fs={{plugin_state}}' --grant network
orbit plugin secret set cloudcli aws_access_key_id < <file>   # each value on stdin
orbit plugin secret set cloudcli aws_secret_access_key < <file>
orbit plugin secret set cloudcli gcp_service_account_json < key.json
```
