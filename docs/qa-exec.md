# QA fixture: exercising office tools Orbit refuses to agents

Full-QA areas: when a required scenario calls a mutating office tool
that Orbit refuses to agents (Operator/Runner-only tools such as
`office.init`, `office.reindex`, `office.config-set`,
`office.proposal-accept`, `office.selection-put`, `office.share-set`,
`office.library-sync`, `office.calendar-sync`, `office.script-run` or
`office.consolidate`), do not record it BLOCKED on operator capability
and do not look for an agent authority route. Use the disposable-data
fixture in this repository instead:

```sh
make qa-exec TOOL=<tool> INPUT=<file>
```

`INPUT` is a file holding that one tool's input as a single JSON
object (for example `{}` or `{"library_dir": "library"}`). The target
builds the `office` binary, creates a fresh temp root, and pipes one
exec envelope to `office orbit-tool`.

This fixture is a named, audited test-fixture exception recorded in
[design/office/4_decisions.md](./design/office/4_decisions.md), citing
STD-05@1 §R3–R4 (ORB-20946). Updating the external qa-full-sweep
inventory sources is out of scope for this repository; this runbook is
the in-repo instruction full-QA areas follow.

## What it does — and does not — prove

- It exercises the tool's **backend behaviour on disposable data**:
  the freshly built binary, the exec envelope, the handler and the
  store, all against a temp root the target creates itself.
- It does **not** exercise Orbit authorization. No MCP scope,
  allowlist or plugin manifest is involved, the call context carries
  no identity or operator claim, and `ORBIT_OPERATOR` is never set.
  A pass is evidence about backend behaviour only; a scenario whose
  point is that Orbit refuses an agent still needs the real surface.
- It has no access to a real workspace, Library or task store. The
  context carries only `workspace_root`, `plugin_state` and
  `library_dir`, all inside the temp root; the child runs with a
  cleared environment and an empty `HOME` inside that root, and Orbit
  callbacks are unavailable (`ORBIT_BIN=/bin/false`, no callback
  state), so a handler that tries to call back into Orbit fails
  instead of reaching a real task store.
- Tools that need Orbit callbacks, network or model routes fail in
  the fixture by design. Record that as a fixture limitation, not as
  a product result.

## Audit and confinement

Every run appends an audit line naming the tool, the temp root and
the UTC time to `qa-exec-audit.log` inside the temp root and prints
the same line on stdout, before the response envelope:

```text
qa-exec audit tool=office.init root=/tmp/office-qa-exec.AbC123 time=2026-11-01T21:00:00Z fixture=make-qa-exec
{"ok":true,"output":{...}}
```

A fixture run is never indistinguishable from an ordinary call.

The target refuses, before running the tool:

- a supplied root (`QA_EXEC_ROOT`, alias `ROOT`) that is outside the
  system temp directory or was not created by this fixture (no
  `qa-exec-audit.log` marker), and
- any input string that is an absolute path outside the temp root,
  or that escapes it with a `..` path component.

Relative input paths are resolved by the backend under the supplied
roots, as on the real surface. This confinement is an accident guard
for disposable data, not a security boundary against the account
running `make`.

## Worked example: `office.init` in an empty root

```sh
printf '{}' > /tmp/qa-init-input.json
make qa-exec TOOL=office.init INPUT=/tmp/qa-init-input.json
```

Expected shape (temp root and time vary):

```text
qa-exec audit tool=office.init root=/tmp/office-qa-exec.AbC123 time=2026-11-01T21:00:00Z fixture=make-qa-exec
{"ok":true,"output":{"already_initialized":false,"created":[...]}}
```

The Library, its git repository, config and index are all created
under `/tmp/office-qa-exec.AbC123/workspace/library`, plugin state
under `/tmp/office-qa-exec.AbC123/plugin_state`, and nothing is
written outside that root. Read the root from the audit line to
inspect the result.

## Chaining a second tool in the same disposable root

Each invocation creates a fresh root, so a tool that needs an
initialized Library takes two calls in one root. Reuse the root from
the first call's audit line:

```sh
make qa-exec TOOL=office.init INPUT=/tmp/qa-init-input.json
printf '{}' > /tmp/qa-input.json
make qa-exec TOOL=office.reindex INPUT=/tmp/qa-input.json QA_EXEC_ROOT=/tmp/office-qa-exec.AbC123
```

Reuse is accepted only for a fixture-created root under the system
temp directory; any other root is refused (see above).

## Cleanup

The target leaves its temp root in place so the run can be inspected.
When the scenario is done, delete exactly the root the audit line
named:

```sh
rm -rf /tmp/office-qa-exec.AbC123
```

Never point the fixture at — or clean up — a root it did not print.
