---
type: design
summary: "Spec: scratch space and scripts — where one-off files go, and how a routine runs an approved script with office.script-run"
last_validated: 2026-10-26
---

# Spec: Scratch space and scripts

Agents write throwaway files all the time: a one-off script, an
intermediate CSV, a rendered chart. Some scripts earn a place in a routine.
This spec says where scratch goes, and how a routine runs an approved script
([decision](../4_decisions.md#routines-run-approved-scripts-through-officescriptrun-and-their-outputs-wait-for-review)).

## Scratch space

| Who | Scratch | Lifetime |
|---|---|---|
| An implementing agent's run | `$ORBIT_SCRATCH_DIR` (`.orbit/tmp/` in the run's worktree) | Deleted with the worktree |
| The orchestrator session | `~/orbit/.orbit/tmp/session-<yyyymmdd>/` | Pruned after 7 days (below) |
| A plugin tool | its run directory under `plugin_state/runs/` (below) | Pruned after 7 days |

- **Nothing in scratch is a result.** Output worth keeping becomes a task
  artifact (`orbit.task.artifact.put`, whose `source_path` may be under
  `.orbit/tmp/` but never `/tmp`), and reaches the Library only by
  promotion.
- **Never the Library.** Scratch is never written under `library/`, whose
  every file is committed and indexed.
- **Never tracked files.** `.orbit/` is ignored by the workspace repository,
  so scratch never trips `primary_checkout_drift` for running agent tasks.
- **Pruning.** The orchestrator skill's first step after the session brief
  deletes only its own `session-*` folders older than 7 days, and nothing
  else under `.orbit/tmp/`. Plugin run directories are pruned by
  `office.script-run` (below).

## Running a script from a routine

A `script` artifact ([library.md](./library.md#artifacts)) runs only as a
step of a routine or auto-task whose accepted proposal named it. Orbit
routines run deterministic `plugin.tool_call` steps and have no generic
command step, so that step calls **`office.script-run`** (mutating,
`mcp_scope: none`).

### Step input

Fixed in the routine definition, because routine jobs take no parameters
from the caller:

```yaml
- kind: plugin.tool_call
  tool: office.script-run
  input:
    artifact: art-20261102-receipt-catalog-script
    sha256: 3f1c…              # the approved version, pinned
    inputs:                    # copied into in/, read-only
      - art-20261101-receipts-2025       # an artifact's files
      - knowledge/domain/dom-systems.md  # a Library file or folder
    args: [--month, previous]  # fixed arguments, passed as argv, never through a shell
    output: { title: Receipt catalog, kind: data, series: receipt-catalog }
```

### What a run does

1. **Checks before anything runs.** The artifact exists, is `kind: script`,
   and its file's SHA-256 equals `sha256`. An accepted proposal names that
   artifact and version. Every input resolves inside the Library. Any
   failure refuses the call (`script_not_approved`, `script_digest_mismatch`,
   `input_outside_library`) and nothing is created.
2. **Run directory.** Creates `plugin_state/runs/<run-id>/` with `in/`
   (inputs copied in and made read-only), `out/`, `tmp/` and `run.log`.
   `<run-id>` comes from `ORBIT_RUN_ID` when Orbit supplies it, with a
   random suffix otherwise.
3. **Runs the script** with the install's `uv` and pinned Python
   ([install.md](./install.md)):
   `uv run --offline --python <pinned> --script script.py <args>`, with the
   run directory as the current directory. The child's environment is an
   allowlist (STD-05@1): `PATH`, `LANG`, `TZ`, `HOME` and `TMPDIR` set to
   the run's own folders, `UV_CACHE_DIR` set to the plugin's dependency
   cache, `UV_PYTHON_DOWNLOADS=never`, and nothing else. Secrets never reach
   it. The script finds its folders as `./in`, `./out` and `./tmp`.
4. **Bounds** (Library settings under `scripts`, validated on read):
   a wall-clock timeout (default 5 minutes, at most 30), after which the
   whole process group is killed; stdout and stderr captured to `run.log`,
   capped at 1 MiB; at most 8 output files of at most 1 MiB each, the task
   artifact limit. Exceeding any bound fails the run and names the bound.
5. **Outputs wait for review.** When `out/` holds files, the tool files one
   Orbit task tagged `script-output`, with the files as task artifacts plus
   a manifest (title and kind from `output`, the script artifact and
   version, the inputs, the routine run). It follows the proposal record
   and close path ([proposals.md](./proposals.md#record-and-lifecycle)):
   the app shows it under Needs you, **Approve** promotes the files with
   `office.artifact-promote` into the `output` series, and **Dismiss**
   rejects it. An empty `out/` files nothing.
6. **Result.** The step result reports the exit code, the duration, the
   output task (if any) and the last 50 lines of `run.log`. A non-zero exit
   fails the step, so the Routines screen shows the run as failed with that
   tail. The run directory is kept either way, for 7 days.
7. **Pruning.** Each call first deletes run directories older than 7 days,
   only those whose names match the run-id pattern directly under
   `plugin_state/runs/` (STD-03@2 ownership-proven deletion).

### Dependencies are fetched once, at approval

A script declares its dependencies inline (PEP 723). When
`office.proposal-accept` applies a routine or auto-task that runs a script,
it first resolves and downloads those dependencies into the plugin's cache
(`plugin_state/uv-cache`) with the pinned Python. A dependency that cannot
be resolved refuses the acceptance, and nothing is enabled. Every later run
is `--offline`: it uses exactly what was fetched at approval and fails
rather than download anything new. A new script version means a new
proposal, and so a new fetch.

### Honest limits

The script runs as a child of the plugin process, so it holds the plugin's
grants: it can read and write the Library and plugin state, and it can open
network connections, because the plugin's network grant is `any`. The
mitigations are that the user approved these exact bytes, that dependencies
are fixed at approval with no downloads afterwards, that the environment
carries no secrets, and that the bounds above stop runaway runs. The demo's
scripts do not use the network. A tighter sandbox for scripts depends on
Orbit offering one to plugin children, and this spec does not claim it.
