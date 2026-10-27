---
name: api-requests
description: Make API requests through the user's gateway connections instead of curl, wget or an HTTP client in the shell. Use whenever a task needs data from a web API the user has connected.
---

# API requests through the gateway

The `gateway` plugin sends API requests for you through **connections** the
user set up in the app's Settings. The connection holds the secret; you never
see it, every call is audited, and every response is redacted before it
reaches you.

## Use the gateway, not curl

Do not call an API with `curl`, `wget`, `http`, a Python HTTP client or any
other tool in your shell, and never ask the user to paste an API key. Use:

- `gateway.connection-list` — which connections exist, their base URLs, the
  methods they allow and whether their secret is set. From a shell:
  `orbit tool run gateway.connection-list --input '{}'`.
- `gateway.http-get` — a `GET` (or `HEAD`, if the connection lists it):

  ```bash
  orbit tool run gateway.http-get --input '{"connection":"<name>","path":"<path under base_url>","query":{"limit":10}}'
  ```

  `path` is relative to the connection's `base_url`; a full URL, another host
  or a `..` that climbs out is refused. You may set only the `Accept`,
  `Accept-Language`, `If-None-Match`, `If-Modified-Since` and `Range`
  headers; the gateway adds the authentication itself.
- `gateway.http-draft` — a write (`POST`, `PUT`, `PATCH` or `DELETE`, if the
  connection lists it in `drafts`) for the user to approve. Nothing is sent
  when you call it:

  ```bash
  orbit tool run gateway.http-draft --input '{"connection":"<name>","method":"PATCH","path":"<path>","body":{"stage":"won"}}'
  ```

  It takes the same `connection`, `path`, `query` and `headers` as
  `http-get` (plus `Content-Type`, `If-Match` and `If-Unmodified-Since`) and
  a `body`: a JSON value is sent as JSON, a string as text. It files one task
  under the user's Needs you; the user approves it (the app sends it, once)
  or dismisses it (it is never sent). You cannot send a draft yourself. Tell
  the user what you drafted and the task id the result names.
- `gateway.audit-list` — the recent calls, newest first.

## What comes back

The status, the content type, a fixed set of headers and the body: JSON
parsed, text as text, anything else as its size and SHA-256 only. A body
longer than the connection's limit is cut, with `truncated: true`.
`truncated: true` means those bytes are not the whole reply. A sent draft
whose server closed before the body it declared uses the same flag, and an
empty cut-off is `kind: none` with `truncated: true`, not a complete empty
reply. Anything that looks like a credential reads `[secret]`; that is the
gateway working, not a fault in the API.

A redirect to another host is returned, not followed (`redirect.reason:
other_origin`). Do not fetch that location yourself; tell the user.

## When the gateway refuses

- `unknown_connection` — no such connection, or it is disabled. You cannot
  create one: ask the user to add it in Settings.
- `secret_unset` — the connection exists but its secret is not set yet; ask
  the user to set it in Settings.
- `method_not_allowed` — only `GET` and `HEAD` run directly. A write (`POST`,
  `PATCH`, …) must be drafted with `gateway.http-draft`, never sent another
  way. A draft is refused too when the connection does not list that method
  in `drafts`: ask the user.
- `secret_in_body`, `secret_in_request` — a draft never holds a secret or a
  credential. Leave authentication out; the gateway adds it when the user's
  approval sends the request.
- `path_outside_connection`, `header_not_allowed` — fix the request; do not
  work around the rule.
