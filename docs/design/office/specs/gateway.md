---
type: design
summary: "Spec: gateway plugins — audited, redacted API requests through named connections, and read-only cloud CLI calls, instead of raw curl and cloud CLIs"
last_validated: 2026-10-27
---

# Spec: Gateway plugins

People keep API requests in Postman, with API keys in them. An agent with a
shell would run the same requests with `curl` and the same key in the command
line, and nothing records what it called or what came back. The gateway puts
those calls behind Orbit tools: the user grants each connection, the secret
never reaches the agent, every call is audited, and every response is redacted
([decision](../4_decisions.md#api-and-cloud-cli-calls-go-through-gateway-plugins)).

Two plugins share one Python package:

| Plugin | Tools | Who may call | Priority |
|---|---|---|---|
| `gateway` | `gateway.http-get`, `gateway.http-draft`, `gateway.http-send`, `gateway.connection-list`, `gateway.connection-put`, `gateway.oauth-device-start`, `gateway.oauth-device-poll`, `gateway.audit-list` | the orchestrator and implementing agents (`http-send`, `connection-put` and the two `oauth-device-*` tools: the app only) | must-have (GET and drafts) |
| `cloudcli` | `cloudcli.run` | the orchestrator only | should-have |

Tool verbs are hyphenated (`http-get`, not `http.get`): Orbit accepts only
lowercase letters, digits and `-` in a plugin tool's name
([decision](../4_decisions.md#gateway-tool-verbs-are-hyphenated)).

They are separate plugins because Orbit bounds a plugin's declared programs
per caller. An implementing agent may call a plugin only if every program the
plugin declares is on its activity's `proc.spawn` allowlist. `cloudcli`
declares `aws` and `gcloud`, so putting them in `gateway` would mean putting
them on the agents' allowlist, which would let agents run those CLIs directly.
Kept apart, `office-agent-todo` allowlists `python3` only, so agents reach
`gateway` and Orbit refuses them `cloudcli`.

Neither plugin is part of Orbit's core, and neither is office-specific: any
workspace can enable them.

## Runtime

- **Python 3.12 standard library only** (`urllib.request`, `ssl`, `json`,
  `subprocess`): no third-party packages, so nothing to fetch, pin or audit
  at run time. The interpreter is the install's pinned Python
  ([install.md](./install.md)).
- `exec` backend, one process per call: `bin/backend.py`
  (`#!/usr/bin/env python3`), as `orbit plugin scaffold` generates it. Its
  first act is a version check. An older interpreter answers `python_too_old`,
  naming the install's fix. `python3` is in `spec.requires.programs`, so
  enabling records the install's interpreter.
- State lives in `{{plugin_state}}` with owner-only modes (STD-05@1 §R8):
  `connections.json`, `drafts/`, `audit/`, and `oauth/` (a sign-in in
  progress and each `oauth2_device` connection's slot versions).
- Orbit callbacks (`orbit.task.add`, `orbit.task.artifact.put`,
  `orbit.task.show`, `orbit.task.update`, the manifest's `orbit_tools`) run
  `orbit tool run` with the host's callback descriptor, a cleared
  environment, a 30-second deadline and capped output, and are never retried;
  a write whose reply is lost is `orbit_outcome_unknown`. Because
  `orbit.task.artifact.put` reads only files inside the workspace, a draft's
  artifact is staged in `{{workspace}}/.orbit-gateway/` and removed after the
  put.

## Connections

A connection is what the user grants. It is set up in the app's Settings,
never by an agent:

```json
{
  "name": "acme-crm",
  "base_url": "https://api.acme.example/v2/",
  "auth": { "scheme": "bearer", "secret": "conn_03" },
  "methods": ["GET", "HEAD"],
  "drafts": ["POST", "PATCH"],
  "idempotency_header": "Idempotency-Key",
  "timeout_seconds": 20,
  "max_response_bytes": 1048576
}
```

- **`base_url`** must be `https`, except plain `http` to a loopback address
  (the demo's mock API).
- **`auth.scheme`** is `bearer`, `basic`, `header` (with a header name in
  `auth.header`), `none`, `secret_path` or `oauth2_device` (both below; the
  calendar needs them, [calendar.md](./calendar.md#gateway-auth-schemes-this-needs),
  but neither knows about calendars). For `basic`, `auth.username` holds the
  user name and the slot holds the password.
- **`disabled: true`** keeps a connection but refuses calls through it as
  `unknown_connection`.
- A refused connection (`base_url`, scheme, method lists, bounds) is
  `invalid_connection`.
- **`methods`** run directly, and only `GET` and `HEAD` may be listed there.
  If omitted, the list defaults to `GET`; an explicit empty list allows no
  direct methods. **`drafts`** are the write methods an agent may draft for
  approval.
- **`idempotency_header`** (optional) names the header the API reads an
  idempotency key from. It cannot be a credential, transport or agent header,
  or the connection's own auth header.
- **Secrets in slots.** Orbit plugin secrets are names fixed in the
  manifest, so `gateway` declares sixteen slots, `conn_01` … `conn_16`.
  `gateway.connection-put` (mutating, `mcp_scope: none`, app only) writes the
  connection with a free slot and never takes a secret. The app then runs
  `orbit plugin secret set gateway conn_03` with the value on stdin, so it
  never appears in argv, a tool input, a log or the audit. Because each value
  sits in its own declared slot, Orbit's host masks it in every result as
  well. The slots are declared `rotatable`, because any of them may belong
  to an `oauth2_device` connection, whose refresh token reaches the slot
  through Orbit's `secret_updates` instead (below). A free slot is one no connection uses and that holds no value, so a
  stale secret is never handed to a new connection; with none left the put is
  refused (`no_free_slot`). A connection keeps its slot while its origin is
  unchanged; a new origin gets a new slot, and the put names the old one in
  `released_slot` so the app can remove its value. So does a change of what
  the value is: moving into or out of `secret_path` or `oauth2_device`, or a
  new token endpoint origin or `client_id`.
- `gateway.connection-list` (read_only, `workspace`) returns names, base
  URLs, methods and whether each slot is set, never a value. An
  `oauth2_device` connection also has `auth_state`: `signed_out`,
  `signed_in` or `expired`. An agent that needs a connection that does not
  exist asks the user; it cannot create one.

### `secret_path`

For a resource whose URL is the credential, such as a calendar's published
ICS feed:

- **`base_url` is the origin only** (`https://outlook.office365.com/`), and the
  slot holds the path and query (`/owa/calendar/…/calendar.ics?…`), appended
  at send time. A slot value that does not start with one `/`, or holds a
  fragment, a space, a control or non-ASCII character, is refused as
  `secret_invalid`.
- **A request `path` must be empty** (`path_not_allowed`): the connection
  names exactly one resource. An agent's `query` is appended after the
  slot's. A `secret_path` connection drafts no writes.
- **Nothing shows the path.** The returned `url`, a redirect's `location`
  and the audit's `path` read `<secret_path>`, a same-origin redirect's
  target included. Redaction replaces the slot value, its path, its query
  and each of their parts of 16 characters or more with `<secret_path>` in
  every response and error.

### `oauth2_device`

For an API that issues short-lived access tokens, such as Microsoft Graph:

- **The connection names** `token_url`, `device_url`, `client_id` and
  `scopes` in `auth`. Both URLs are `https` (or plain `http` to a loopback
  address, as for `base_url`); the scopes are OAuth scope tokens, at least
  one. The slot holds the refresh token.
- **Sign-in** is the OAuth device-code flow (RFC 8628), run by two tools,
  both mutating, `mcp_scope: none` and app only.
  `gateway.oauth-device-start` posts the client id and scopes to
  `device_url` and returns `user_code`, `verification_uri`, `expires_in`
  and `interval`; the device code stays in `{{plugin_state}}/oauth/<name>.device.json`
  and is never returned. `gateway.oauth-device-poll` asks `token_url` once
  per call and answers `pending` (with the interval to wait, raised on
  `slow_down`), `connected`, `denied` or `expired`; a poll with no sign-in
  in progress is `device_flow_missing`. A sign-in that brings no refresh
  token is `auth_failed` (Microsoft needs the `offline_access` scope).
- **The refresh token reaches the slot only through Orbit.** A plugin
  backend is refused `orbit plugin secret set`, and handing the token to
  the app would put it in a tool result. So on `connected` the poll answers
  with `secret_updates` for the connection's slot, which Orbit stores with
  compare-and-swap (only while the slot is unset, or only over the version
  that call was delivered) before it returns. The token is never in the
  output, argv, a log, the audit or plugin state.
- **Each call** (`http-get`, and `http-send` before the draft is marked
  `sending`) exchanges the refresh token at `token_url` for an access token
  held only in memory and sent as `Authorization: Bearer`. A rotated refresh
  token goes back to the slot through `secret_updates` in the same answer, so
  Orbit has stored it before the response is returned, whether the request
  then succeeds or not. Token endpoint replies are not followed through a
  redirect and are read up to 64 KiB.
- **A revoked or expired refresh token** (`invalid_grant`) is refused as
  `auth_expired`, and the slot version it came from is recorded in
  `{{plugin_state}}/oauth/<name>.json` (versions only, never values) so
  `connection-list` reads `expired` until the user signs in again. If a call
  that overlapped this one has just rotated that same version, the refusal is
  `auth_stale` (retryable) instead. Any other refusal is `auth_failed`, with
  the endpoint's OAuth error code in `detail`, never its description.
- Settings › Connections starts the flow, shows the user code and the
  verification address, polls until the connection is connected, and offers
  "Sign in again" on a connection that reads `expired`.

## `gateway.http-get`

Read_only, `mcp_scope: workspace`. Input: `connection`, `path` (relative to
`base_url`), `query` (object), `headers` (object), `method` (`GET` or `HEAD`,
default `GET`).

1. **Resolve.** An unknown or disabled connection is refused
   (`unknown_connection`), as is a method the connection does not list
   (`method_not_allowed`). The path is joined to `base_url` and normalized,
   and the result must stay under `base_url`: a scheme, a host or a `..`
   that climbs out is refused (`path_outside_connection`). A `secret_path`
   connection takes no path; its slot supplies it (above).
2. **Headers.** The agent may set only `Accept`, `Accept-Language`,
   `If-None-Match`, `If-Modified-Since` and `Range`. `Authorization`,
   `Cookie`, the connection's own auth header and anything else are refused
   (`header_not_allowed`).
3. **Send.** The gateway adds the auth from the slot (for `oauth2_device`,
   an access token exchanged for it). TLS verification is
   always on. Redirects are followed at most 3 times and only to the same
   origin. A redirect to another origin is returned as the response, not
   followed, so a secret is never sent to a host other than its own.
4. **Response.** Returns the status, the content type, a fixed set of
   headers and the body: JSON parsed, text as text, anything else as its
   size and SHA-256 only. The body is cut at `max_response_bytes`, with
   `truncated: true`, which means the bytes returned are not the whole
   reply. Everything returned is redacted first (below).
5. **Audit.** Appends one record (below).

## Writes: draft, approve, send

A write to someone else's system is an outward-facing action, so it never
runs on an agent's call ([orchestrator.md](./orchestrator.md#guardrails)):

1. **`gateway.http-draft`** (mutating, `workspace`) takes the same input plus
   `body`, for a method in the connection's `drafts`; `Content-Type`,
   `If-Match` and `If-Unmodified-Since` may also be set. A JSON `body` is sent
   as compact JSON, a string as text. It refuses a body that contains a slot
   value or a credential shape (`secret_in_body`; the same in the path, query
   or headers is `secret_in_request`), so a draft never holds a secret. It
   stores the draft in `drafts/<id>.json` with the SHA-256 of its exact
   request (method, URL, headers and body), and files one Orbit task tagged
   `gateway-draft` (and `proposal`, `proposal:gateway_draft`) showing the
   request in plain words, with the request as its `gateway-draft.json`
   artifact. That task follows the proposal record and close path
   ([proposals.md](./proposals.md#record-and-lifecycle)), so it appears under
   Needs you.
2. **Approve** in the app calls **`gateway.http-send`** (mutating,
   `mcp_scope: none`, app only) with the draft id and the digest the user
   saw. The send refuses if the stored request no longer matches that digest
   (`draft_changed`; STD-05@1 §R21), if the task was dismissed
   (`draft_dismissed`, checked with `orbit.task.show`), and if the draft is
   not `pending` (`draft_not_pending`): a draft is sent at most once.
3. **Send once.** The draft is marked `sending` before the request goes out.
   A failure to connect sends nothing and leaves the draft `pending`. A
   timeout or a lost reply after sending is recorded as `outcome_unknown`,
   never as a failure, and is never retried automatically (STD-03@2 §R31):
   the result and the audit say so, and the task gets a comment and stays
   open for the user to check. When the status did arrive and the body
   was cut off — fewer bytes than `Content-Length`, or the read failed
   after the status — the outcome is still `sent` (the write went out
   once) and `truncated` is true, even when no body bytes arrived
   (`body.kind` `none` with `truncated: true` is not a complete empty
   reply). The audit record for that send sets `truncated: true`.
   When the connection names an idempotency header, the draft id is sent
   in it. Redirects are returned, never followed. Then the response is
   redacted, the outcome audited and the task closed as done. `done` is
   accepted only from `review` and that step needs an execution summary,
   so the close walks there from the task's current status (`proposed`,
   `someday` and `blocked` go through `backlog`, which is what lets the
   next step be `in-progress` without a plan). A refused step is
   `task_closed: false` with `task_error`; the draft stays sent.
4. **Dismiss** rejects the task, and the draft is never sent.

## Redaction

One redactor, run at the durability boundary (STD-05@1 §R13). The same
function handles every returned result, every audit record, every draft file
and every error message, and a test checks that list of writers.

- **Values.** Every slot value, plus its URL-encoded and base64 forms and the
  base64 of `user:password` for `basic` auth, becomes `[secret]`. A
  `secret_path` slot, its path, its query and their parts of 16 characters
  or more become `<secret_path>`. What a call learns as it runs (an
  `oauth2_device` access token, a rotated refresh token, a device code) is
  masked from then on, in the answer and the audit alike.
- **The one unredacted write** is `secret_updates`, beside the answer on
  stdout: it carries a refresh token to Orbit, which stores it in the slot
  and masks it in what it returns.
- **Credential shapes** with high confidence only (STD-05@1 §R14): `Bearer`
  and `Basic` header values, AWS access key ids (`AKIA…`/`ASIA…`), PEM
  private-key blocks, and provider key prefixes (`ghp_`, `github_pat_`,
  `xox[abprs]-`, `sk-`). These catch a credential an API sends back, which
  the gateway does not know. Field names are never masked because of what
  they are called.

## Audit

`{{plugin_state}}/audit/<yyyy-mm>.jsonl`, append-only, one line per call:
time, caller (agent kind, model, task and session from the call context),
tool, connection, method, path, redacted query, status, bytes, duration,
SHA-256 of the redacted body, and outcome. A `secret_path` connection's path
is recorded as `<secret_path>`. The sign-in tools record the endpoint's
path and the outcome (`pending` for a started flow, `connected`, `denied`,
`expired` or the error code); a poll that is still pending is not recorded,
so a sign-in the user takes minutes over does not flood the view. A send whose reply body was
not fully received also has `truncated: true`; the field is absent when
the recorded body is complete. Bodies themselves are never
logged. `gateway.audit-list` (read_only, `workspace`) feeds an "API calls"
view in the app's Settings. Orbit's own tool-call audit records the same
calls one level up.

## `cloudcli.run` (should-have)

Read_only, `mcp_scope: workspace`, orchestrator only (above). Input:
`program` (`aws` or `gcloud`), `args` (an array, passed as argv and never
through a shell), `region`/`project`.

- **Read-only is enforced by the credentials.** The plugin uses its own
  identities, which the user creates with read-only roles (AWS
  `ReadOnlyAccess`, Google Cloud `roles/viewer`) and stores as declared
  secrets: `aws_access_key_id`, `aws_secret_access_key` and
  `gcp_service_account_json`. Each call writes them to owner-only files in
  a fresh private directory, points `AWS_SHARED_CREDENTIALS_FILE`,
  `AWS_CONFIG_FILE` and `CLOUDSDK_CONFIG` at it, and deletes it afterwards.
  The secret values never go in argv or the environment (STD-05@1 §R12),
  and the user's own `~/.aws` and gcloud logins are never read.
- **Command allowlist, as a second layer.** `aws <service> <operation>`
  whose operation starts with `describe-`, `list-` or `get-`, plus
  `aws sts get-caller-identity`. `gcloud … list` and `gcloud … describe`.
  Global flags are allowlisted too (`--region`, `--query`, `--max-items`,
  `--page-size` and `--starting-token` for aws; `--filter`, `--limit`,
  `--page-size` and `--sort-by` for gcloud); the rest, including an
  abbreviation of one, is refused, as are short flags, flags before the
  command words, and aws `file://`, `fileb://` or URL parameter values.
  Output is forced to JSON.
- **Denylist, checked first,** for commands that read secrets, data or
  tokens even though their verb looks read-only: `secretsmanager
  get-secret-value`, `ssm get-parameter`/`get-parameters` with
  `--with-decryption`, `kms decrypt`, `s3api get-object`, `lambda
  get-function`, `ecr get-login-password`, `ec2 get-password-data`,
  `rds generate-db-auth-token`, `eks get-token`, `gcloud secrets versions
  access`, `gcloud auth print-access-token`/`print-identity-token`,
  `gcloud sql generate-login-token`, `gcloud container clusters
  get-credentials`, and more of the same kind: temporary credentials and
  registry, database and identity tokens (`sts get-session-token`,
  `ecr get-authorization-token`, …), `dynamodb get-item`, `kinesis
  get-records`, `glacier get-job-output`, and the CLIs' own `aws configure`
  and `gcloud config` (the full list is `DENY` in `cloud.py`). A refusal is
  `command_not_allowed` and names the rule.
- The child gets an allowlisted environment (STD-05@1 §R10), a 60-second
  timeout, and output capped at 1 MiB. Output is redacted with the same
  redactor and the call is audited like an HTTP call.

## Agent CLI deny rules

The audit is only worth something if going around it is not the easy path.
The install's agent CLI step ([install.md](./install.md)) adds deny rules to
the agent CLI's permission settings for direct shell use of `curl`, `wget`,
`http`, `aws` and `gcloud`. The skill tells the agent to use the gateway
instead. For a CLI with no per-command deny rules, the install says so.

This is an accident guard, not a security boundary (STD-05@1 §R5). An agent
with a shell can still open a socket from Python, and implementing agents
keep the network access their own CLI needs. What the gateway guarantees is
narrower: a secret held in a connection never reaches the agent, every call
made through it is audited and redacted, and nothing it writes leaves
without approval.

## Demo

`demo/synthetic` runs a mock "Acme CRM" API on loopback with a fake token.
The demo shows three things: an agent `GET` whose response carries the token
back and shows `[secret]`, an agent-drafted `PATCH` waiting under Needs you,
and the API calls view.
