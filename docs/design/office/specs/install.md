---
type: design
summary: "Spec: install script — one idempotent command that installs the Orbit Office distribution, its Orbit engine, component plugins, workspace, Library and app"
last_validated: 2026-10-27
---

# Spec: Install script

Source: spec §13.5.

Run `./install.sh` from a clone to install everything without a manual Orbit or scaffold step. A piped install also requires a prepared plugin bundle and a prepared app bundle, as described below. The script is idempotent: every step checks first and
does only what is missing, so re-running it repairs a partial install and changes nothing on
a complete one. It prints each step as it goes and a summary at the end.

**Defaults.** The workspace is `~/orbit`, with the Library at `~/orbit/library`.
`--home DIR` picks another folder. Orbit's own state stays in `~/.orbit`; the two are
different folders. The app and the plugin export are unpacked to the per-user data folder
(`~/Library/Application Support/orbit-office` on macOS, `~/.local/share/orbit-office` on
Linux), never inside the workspace. The tools the script installs for agents (`uv` and a
pinned Python) go in `~/.local/share/orbit-office/tools` on both systems, a path with no
spaces, so script shebangs that name the interpreter keep working.

## Office workspace preset (pending Orbit support)

Orbit Office is a distribution of Orbit: an app built on Orbit as its engine.
Installation assembles the app and the `office`, `gateway` and optional `cloudcli`
component plugins. The office workspace preset is follow-up work that depends on
Orbit's workspace-presets contract; this change only filters the app's Routines view.

Once Orbit's workspace-presets support is available, the distribution will ship
an office preset in the supported format. It must select `seeding.routines: none`
and `seeding.auto_tasks: none`, enable `office-daily-digest`, `office-kt-inbox` and
`office-signals` by name, and leave `office-bootstrap` under the confirmation
rules in [library-bootstrap.md](./library-bootstrap.md#batch-preview-and-confirmation).
`office init` and this installer must apply that preset through Orbit's workspace
initialization interface. A fresh temporary workspace must then demonstrate that
no Orbit shipped developer routines or auto-tasks were seeded and that the
selected office definitions are enabled.

**Blocked on Orbit's preset contract.** There is no preset file or preset
application in the current installer. Initialization still uses ordinary Orbit
workspace defaults. Do not replace the missing policy by deleting those defaults
one by one. This section describes pending work, not an available install option.

## Installation steps

**Steps:**

1. **Preflight.** macOS or Linux (Windows through WSL2). Node 20+ with `npm` is the one
   prerequisite the script does not install: without it the script stops and says how to
   get it. It checks `git` (macOS: runs `xcode-select --install` and asks to re-run when it
   is missing) and `ffmpeg` (installs it with Homebrew or the system package manager when
   one is available, otherwise stops with instructions).
2. **Python.** Agents write and run small Python scripts (data cleanup, `script` artifacts),
   and neither macOS nor Windows ships a Python that can be relied on: macOS has only the
   old Python that comes with the Xcode command line tools, and Windows has none (WSL2
   Ubuntu has its own, at whatever version that release ships). So the script installs its
   own, which leaves the system Python and the user's shell profile alone
   ([decision](../4_decisions.md#agents-get-a-pinned-python-installed-with-uv)):
   - **`uv`** at the version pinned in the script (`UV_VERSION`). The script downloads that
     release's archive for the platform and checks its SHA-256 against the checksum
     recorded beside the pin before unpacking, then places the binary at `tools/bin/uv`. A
     `uv` already on the user's `PATH` is ignored, so every install runs the same version.
   - **Python** at the minor version pinned in the script (`PYTHON_VERSION`, 3.12):
     `uv python install` with `UV_PYTHON_INSTALL_DIR=tools/python`. Then `tools/bin/python3`
     and `tools/bin/python` are linked to the interpreter that `uv python find` reports.
   - **Reaching agents.** The app's login service (step 9) puts `tools/bin` first on the
     `PATH` it starts with, and the dispatch pass passes that `PATH` to the runs it starts.
     The agent CLI registration (step 8) adds the same `PATH` to the Orbit MCP entry's
     environment. That generated `PATH` is `tools/bin` first, then the directories
     of the `orbit` and `node` executables this installer resolved (captured after
     step 3 has ensured Orbit is present, so an Orbit installed by step 3 itself is
     included), then the usual system directories. An existing Orbit or Node under
     a user prefix (nvm, a user npm prefix, `~/.local/bin`, Homebrew) therefore
     stays reachable from the MCP entry and the service without being moved or
     changed; only absolute directories are captured. Re-running the installer
     recaptures them if they moved, and a re-run where nothing moved changes
     nothing. The `library-conventions` skill tells agents to use `python3`, and
     `uv run` for a script that declares dependencies inline (PEP 723).
   - **Check.** `tools/bin/python3 --version` must print the pinned minor version, or the
     step fails and names what went wrong. A re-run with both already in place changes
     nothing.
3. **Orbit.** If `orbit` is missing, installs the tested release:
   `npm install -g @orbit-tools/cli@0.34.0`. An existing Orbit is never upgraded or
   downgraded: an older one stops the script with the command to upgrade, and a newer one
   continues with a warning.
4. **Git and the workspace.** Delegated to the installed `office init` command below. It creates `~/orbit` and runs `git init -b main` if it is not
   already a repository; writes `.gitignore` with `/library/`; makes the first commit if
   the repository has none (Orbit's runs need a commit to create worktrees from). If git
   has no `user.name`/`user.email`, it sets a repo-local identity from the login name. Then
   `orbit workspace init` if the folder is not yet registered. A non-empty folder that is
   not already an office workspace is refused, never adopted. This installer owns a folder
   only when the install state records `workspace_path` for that path, or when the only
   top-level entries are `.git`, `.gitignore`, and `.orbit`, and `.gitignore` has a line
   that is exactly `/library/` (`.git` and `.gitignore` are required; `.orbit` may be absent).
   A `.orbit` directory or a `library` entry on its own, or a path that merely
   appears in `orbit workspace list`, is not ownership.
5. **Plugin.** `orbit plugin add` from the prebuilt export attached to the orbit-office
   release (a local directory install, so no Rust toolchain is needed); `--from-source`
   uses the pinned `git+` source and needs `cargo`. A clone whose local plugin lacks an
   executable backend also uses `git+` at the clone HEAD with `--allow-build`; the commit
   must be available from the repository remote and `cargo` is required. An explicit local
   office source or bundle must already contain an executable `bin/office-bin`; a missing
   backend or failed build stops before office is recorded as added or enabled. Then
   workspace enablement through `office init` using `orbit plugin enable office` with the grants the manifest requests (`fs`, `network`, `orbit_tools`), after printing them and
   asking once. A grant left out leaves the plugin inactive, and `office.init` is then refused.
   The question needs a terminal on stdin. A piped install without one (piped stdin is
   the script itself, for example) prints the grants and enables the plugin without asking,
   and its output says so. `--yes` skips the question in either case. The `gateway` plugin is
   added and enabled the same way, in the same consent prompt, and `cloudcli` only with
   `--with-cloudcli` ([gateway.md](./gateway.md)). Both come from the same release export, and
   neither has a build step.
   `ORBIT_OFFICE_PLUGIN_BUNDLE` selects an unpacked bundle with `office/`, `gateway/`
   and, when requested, `cloudcli/` exports, each containing `.orbit-plugin/`.
   In a clone without a bundle, the installer assembles the gateway exports in
   `<data>/plugins/`, including their shared Python package, and records each plugin
   separately. It retains the original office install markers on upgrades.
6. **Secrets.** Prompts for the Meta API key, and optionally the private route's token (`open_weights_token`), with hidden
   input, and stores them with `orbit plugin secret set office <name>`. They are never
   written to a file or echoed. Skipped when already set. The prompt needs a terminal
   on stdin: a piped install without one (piped stdin is the script itself)
   never reads stdin for a secret and skips each unset secret. The skip line depends
   on the secret. A required secret (the Meta API key) prints the
   `orbit plugin secret set office <name>` command to set it later. An optional secret
   (`open_weights_token`) prints only `secrets: open_weights_token skipped (optional)`,
   with no command; set it later with `orbit plugin secret set office open_weights_token`.
   Values supplied through the environment are still stored in either case.
7. **Scaffold.** Delegated to the same `office init` invocation, which runs `orbit tool run office.init --json --full`: the Library's layout, git
   repository, config, `.gitignore` and index, then the first Library commit. The CLI exits 0
   and prints the tool's output object on stdout. That object has no top-level `ok`. The step
   is done for that object, including a retry that prints `{"already_initialized": true, "created": []}`
   and exits 0. An `{ok: true}` envelope is also accepted. A non-zero exit stops the install.
   The office code is read from the JSON object on stderr (`code`, or `error.code`), including
   when a WARN line precedes the JSON, and the failure names that code. An `{ok: false}`
   envelope on stdout, even with exit 0, also stops the install and names its code. Nothing
   is recorded on failure, so the next run asks `office.init` again.
8. **Agent CLI.** Adds the Orbit MCP entry (agent authority) to Claude Code's
   `${CLAUDE_CONFIG_DIR:-$HOME}/.claude.json` — the file Claude Code and the
   app's connection check actually read — keeping a backup of the old file
   (`.claude.json.bak`), and installs the office skills. A
   skill already present under the same name is left untouched and is not claimed: the
   installer records only the skills it itself copies, as one `<name> <sha256>` line per
   skill in `<data>/agent-skills-<agent>.txt`, where the digest covers every file's
   relative path and bytes in the copied tree. It
   also adds deny rules for direct shell use of `curl`, `wget`, `http`, `aws` and `gcloud`,
   so API and cloud calls go through the gateway ([gateway.md](./gateway.md#agent-cli-deny-rules)),
   and lists them in the summary. Rules the user already has are kept, and uninstall
   removes only the ones it added. With `--agent claude|codex` it registers the other CLI
   instead. A CLI with no per-command deny rules is reported as such.
   Claude gets `Bash(curl *)`, `Bash(wget *)`, `Bash(http *)`, `Bash(aws *)` and
   `Bash(gcloud *)` in `settings.json`'s `permissions.deny` (under
   `$CLAUDE_CONFIG_DIR` when set, otherwise `~/.claude/settings.json`), using the
   [documented Bash permission syntax](https://code.claude.com/docs/en/permissions).
   The installer splices JSON entries in both files without reformatting other
   settings, keeps each file's original bytes in a sibling `.bak`, and records
   ownership separately: only newly added deny rules and containers in
   `<data>/claude-settings.json`, and whether it created or updated the MCP
   entry (and any container it created) in `<data>/claude-mcp.json`. Uninstall
   removes the MCP entry only when that receipt proves the installer owns it,
   and removes the recorded deny rules while keeping pre-existing rules, a
   pre-existing identical MCP entry and later user additions. Installers
   before this fix wrote `mcpServers.orbit` into `~/.claude/settings.json`,
   where no reader looks for it: install migrates that misplaced entry out of
   the settings file (keeping any other `mcpServers` entries and settings)
   while writing the effective `.claude.json` entry, and uninstall cleans the
   misplaced entry only when an installer receipt proves a prior install.
   Invalid JSON, duplicate keys, incompatible field types and unsafe
   settings/receipt files stop the edit.
   Codex has no equivalent deny settings for sandboxed shell calls: its
   [execpolicy rules](https://developers.openai.com/codex/rules) govern commands outside
   the sandbox. The summary reports this limitation instead of claiming those calls
   are blocked.
9. **App.** Provisions and verifies the app, then installs it as a login service (a launchd
   agent on macOS, a systemd user unit on Linux), then opens `http://localhost:<port>`,
   where the browser offers to install it as a PWA. In a clone the app is built from the
   adjacent `app/` tree (`npm run build` whenever `packages/server/dist/main.js` is missing
   or unrunnable, whatever an earlier marker claims). Without an adjacent tree,
   `ORBIT_OFFICE_APP_BUNDLE` selects an unpacked app bundle containing
   `packages/server/dist/main.js` and its runtime files, including the shipped
   `office` and `gateway` manifests and definition trees at
   `plugins/{office,gateway}/.orbit-plugin` (see [web-app.md](./web-app.md#distribution-routines)); the bundle is copied to
   `app/` under the per-user data folder. With neither a tree nor a bundle, the step
   stops before any service file is written or enabled and names the missing
   entrypoint and both remedies (run from a clone, or set `ORBIT_OFFICE_APP_BUNDLE`).
   An invalid bundle (not a directory, or no runnable entrypoint) stops the same way.
   The entrypoint must exist, be non-empty and parse under `node --check` before the
   service is written. A complete app already under the data folder needs no bundle
   on re-run and changes nothing; a partial one is repaired by supplying the bundle
   again, and without one the failure names the concrete missing entrypoint.

## `office init [path]`

`install.sh` installs a standalone human launcher at `~/.local/bin/office` and
calls that installed command for steps 4, workspace enablement in 5, and 7.
Machine setup still installs Python, Orbit and plugin exports, stores secrets,
registers the agent CLI and provisions the app. `office init` installs none of
those. Add `~/.local/bin` to your PATH if your shell does not already include it;
the installer prints the absolute launcher path and does not edit shell profiles.
The launcher is embedded in the installer, so a piped install needs no extra
script download. It needs Node, git, Orbit, and the already added plugins.

Run `office init` for `~/orbit`, `office init ~/another-office` for a second
workspace, or either again to repair setup. `ORBIT_OFFICE_HOME` supplies the
path default when set; an explicit path wins. Relative paths resolve from the
calling directory. `ORBIT_OFFICE_DATA_DIR` selects the same install-state folder
as the installer. `office init --yes [path]` skips the consent question.
`office --help` and `office init --help` describe the same command.

The installer adds the machine plugins first, then calls `office init` once
for steps 4, 5 and 7, before prompting for secrets in step 6. The numbers above
name the responsibilities rather than their execution order.

The command does these steps in order:

1. Refuse a non-empty folder unless the install state records its exact
   `workspace_path`, or it is the partial office tree defined in step 4 above.
   A registered path, a bare `.orbit` directory or Library alone is not proof.
   Refusal names the folder and reason and changes nothing. HOME, `/`, a file
   and a symlink destination are refused too. Resolve existing parents before
   checking ownership and use that resolved path throughout.
2. Create the folder and `git init -b main`, ensure the `/library/` ignore
   line, fill missing repo-local git identity fields from the login name, and
   make the first commit if absent. Preserve existing commits and identity.
3. Read `orbit workspace list --json` and register the exact folder with
   `orbit workspace init` when absent; an old marker or `.orbit` directory
   alone never substitutes for registration. Record ownership in install state.
4. Enable installed `office` and `gateway`, plus `cloudcli` when installed,
   with the manifest grants from step 5. Print grants and ask once when setup
   needs enablement and stdin is a terminal; decline fails. `--yes` prints
   explicit consent. Non-interactive stdin prints that it proceeds without a
   question and is never consumed. Read live enablement and permissions rather
   than global install markers. Record host grants when needed, then enable
   the selected workspace with `--scope workspace` to seed its definitions;
   a workspace toggle never receives host-only grant flags.
5. Always call `orbit tool run office.init --json --full --input '{}'` in
   that workspace, interpreting stdout, stderr, exit status and error codes
   exactly as step 7 above. `already_initialized` is success. Re-running a
   complete setup changes no workspace or install-state bytes. Always asking
   the initializer also repairs a missing Library despite an old scaffold marker.

Exit codes: **0** success (including already initialized), **1** setup,
consent, prerequisite or tool failure, **2** invalid command/option/path arguments.
A tool failure prints the office error code read from its JSON output, or
`unreadable` when no code can be recovered. Earlier successful setup is retained
for repair on the next run; a scaffold failure records no success marker.

## `office open [path]`

Open the app for a workspace in the default browser.

Arguments: an optional workspace `path`. Without a path, the workspace is
`ORBIT_OFFICE_HOME` when set, otherwise the workspace the installed app
service is bound to (`ORBIT_OFFICE_WORKSPACE` in its launchd plist or
systemd unit), otherwise the last `workspace_path` recorded in install
state, otherwise `~/orbit`. Relative paths resolve from the calling
directory, as for `office init`.

The app service is the login service from step 9: the launchd agent
`com.orbit-office.app` (plist under `~/Library/LaunchAgents/`) on macOS,
or the systemd user unit `orbit-office.service` on Linux. Its port is
read from that service definition (`PORT`), falling back to
`ORBIT_OFFICE_PORT` and then `4317`; the app URL is
`http://localhost:<port>/`.

Behaviour, in order:

1. If no service definition is installed, exit **1** with
   `app service is not installed; run install.sh to install it` (naming
   `install.sh`) and open nothing.
2. If the installed service is bound to a different workspace than the
   requested one, exit **1** naming both workspaces and the fix
   (`install.sh --home <path>`); the service serves only its bound
   workspace, so opening another workspace's URL would show the wrong
   Library.
3. If the service is installed but stopped, start it
   (`launchctl load` on macOS, `systemctl --user start` on Linux) and
   verify it is running; a service that does not start exits **1**.
4. Open the app URL with the platform browser opener (`open` on macOS,
   `xdg-open` on Linux; `ORBIT_OFFICE_BROWSER` overrides the command).
   A missing or failing opener exits **1**. On success the command
   prints `app: http://localhost:<port>/` and exits **0**.

Exit codes: **0** the URL was opened, **1** no service installed, a
workspace mismatch, a start failure or a browser failure, **2** invalid
arguments (more than one path, an unknown option, an empty path).
`office open --help` prints the `open` usage. Starting the service and
opening the browser are the only effects; no workspace files change.

## `office status [path] [--json]`

Report, in plain words, whether everything the app needs is wired for
one workspace. It changes nothing: it never starts the service, never
opens a browser and never writes to the workspace.

Arguments: an optional workspace `path` (same defaulting as
`office open`) and `--json`. Unknown options, a second path or an empty
path exit **2**.

The report covers, in this order:

- **Workspace** — whether the folder is a registered Orbit workspace,
  from `orbit workspace list --json` (an exact active registration for
  the resolved path).
- **Plugins** — whether `office` and `gateway` are enabled there, from
  `orbit plugin list --json --workspace <path>` (host enabled and the
  workspace toggle on).
- **Library** — health from `orbit tool run office.status --json`
  (never from reading `.orbit/` or the Library directly): whether it is
  initialized (scaffolded) and how many documents it holds.
- **Meta API key** — presence only (`set`/`unset`), from the
  `office.status` secrets report, falling back to
  `orbit plugin secret list office` when the tool call cannot run. The
  key's value never appears in any output.
- **Private route** — the `open_weights` endpoint from `office.status`
  (endpoint, model, kind and label), or `not configured`. The private
  route is optional: not configuring one is reported but is not a
  failure, and its token likewise appears only as `set`/`unset`.
- **App service** — whether the login service (as in `office open`) is
  installed, whether it is running, and the app URL
  `http://localhost:<port>/` it serves.
- **Agent CLI** — whether an Orbit MCP registration is present in the
  agent CLI's user config (Claude Code's `.claude.json`, the Muse
  settings file, or Codex's `config.toml`, matching the app's
  `agentCliStatus` reader). File contents are never printed.

Plain output is one line per fact, then either `Status: OK` or a
`Problems:` list where each entry names the problem and its fix
command, for example:

```text
Workspace: /home/user/orbit — registered
Plugins: office enabled; gateway enabled
Library: initialized (12 documents)
Meta API key: set
Private route: not configured (optional)
App service: running at http://localhost:4317/
Agent CLI: registered
Status: OK
```

With `--json`, stdout is exactly one JSON object with the same facts
and no other text:

```json
{
  "workspace": "/home/user/orbit",
  "workspace_registered": true,
  "plugins": { "office": true, "gateway": true },
  "library": { "initialized": true, "documents": 12 },
  "secrets": { "meta_api_key": "set" },
  "meta_api_key": "set",
  "private_route": null,
  "app_service": { "installed": true, "running": true, "url": "http://localhost:4317/", "port": 4317, "workspace": "/home/user/orbit" },
  "agent_cli": { "registered": true },
  "ok": true,
  "problems": []
}
```

`library` also carries the tool's `documents_by_kind`,
`pages_off_template` and `template_invalid` when the tool reports them.
`private_route` is the tool's `open_weights` object or `null`.
`problems` is a list of `{"check", "problem", "fix"}` objects (empty
when healthy). No secret value appears in either form.

Exit codes: **0** when every needed check passes, **1** when anything
needed for the app to work is missing, **2** for invalid arguments.
Each missing prerequisite is a problem with its fix command:

| Problem | Fix named in the report |
| --- | --- |
| Workspace not registered | `office init <path>` |
| `office` or `gateway` plugin not enabled | `office init <path>` |
| Library not initialized | `office init <path>` |
| Meta API key unset | `orbit plugin secret set office meta_api_key` |
| App service not installed | `install.sh` |
| App service stopped | `office open <path>` |
| Agent CLI registration missing | `install.sh` |

`office status --help` prints the `status` usage.

The installer refuses to replace an unrelated `~/.local/bin/office`. It records
`launcher_path` only when it creates the launcher, updates an owned launcher on
reinstall, and keeps an identical pre-existing launcher unclaimed. Uninstall
removes that path only with the creation receipt and a matching recorded SHA-256; an unrecorded or
user-modified launcher is kept. Reinstall refuses to overwrite a modified launcher.
It keeps workspace ownership records for all workspaces initialized by this
command. `--purge --home DIR` still applies only to the selected owned workspace.

The summary lists what was created, where, and the uninstall command
(`install.sh --uninstall`). Uninstall removes the service, the MCP entry and its deny rules, the plugins, its recorded `office` launcher and the
`tools` folder (`uv` and its Python, which nothing else uses). For skills it removes only a
skill recorded in `agent-skills-<agent>.txt` whose tree still matches its recorded digest;
a pre-existing same-named skill, a skill the user edited after copying, and any unrecorded
skill are kept in place and named in the output with their path, and a repeated uninstall
is a safe no-op for them. It keeps `~/orbit` and the
Library unless `--purge` is given. `--purge` deletes that workspace only after the same
ownership check as adoption, and otherwise refuses with a message that names the path and
removes nothing, including the per-user data folder. A workspace or data folder (set with
`--home` or `ORBIT_OFFICE_DATA_DIR`) that is the home folder or `/` is refused the same way,
before any teardown and again just before the deletes. The check compares the folder the
path names, not its spelling: a trailing or repeated slash, `.` and `..` segments and a
symlink to either are refused too.

Plugin-authored commits in the Library use a fixed author (`orbit-office
<office@orbit.local>`) passed per commit, so they need no git identity and are easy to tell
apart from the user's own edits.

Publishing the orbit-office release that carries the prebuilt export is the maintainer's
call. Until one exists, run from a clone (`--from-source` builds office from that
clone's commit) or supply a prepared `ORBIT_OFFICE_PLUGIN_BUNDLE`. A piped install
needs that bundle; it cannot assemble gateway exports without the source tree.
A standalone install (no adjacent `app/` tree, as in the piped form) likewise needs
a prepared `ORBIT_OFFICE_APP_BUNDLE` for the app, as step 9 describes; without one
it stops at the app step rather than installing a service with nothing to run.

Run `sh scripts/install/test-install.sh` for the installer acceptance harness; CI
runs it on Linux and macOS with Node 24 (the fixture imports TypeScript directly).
`UPDATE_GOLDENS=1 sh scripts/install/test-install.sh` explicitly refreshes the
installed launcher help golden. The launcher itself supports Node 20+. The harness uses
temporary homes, stub Orbit and service managers, local archive fixtures and a PTY
for the hidden-input and single-consent checks; it never contacts a provider.
