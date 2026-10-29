# Setup: install, check and repair the office

Use this to help the user install Orbit Office, check that it works, and fix a broken setup.

## Your part and the user's part

The user runs every command that installs, changes or removes anything. You explain, check
and point to the fix.

- **You may run:** `orbit-office status [path]` and `orbit-office status [path] --json`. They
  change nothing. You may also call `office.status` over MCP.
- **You never run:** `install.sh` in any mode, `orbit-office init`, `orbit-office open`,
  `install.sh --uninstall` or `--purge`, `orbit plugin secret set`, `orbit plugin enable`, or
  hidden tools through `orbit tool run`, such as `office.init`. Give the user the command and
  let them run it in their own terminal.
- **Never handle a secret value.** Don't ask for a key in chat, don't put one in a command
  line, and don't read credential files, the service unit, `~/.orbit` or shell profiles.
  If the user pastes a key into chat, tell them to rotate it.
- **Don't edit** the login service, the agent CLI's settings, the MCP entry or the deny rules.

## What the installer does

`./install.sh` from a clone installs everything. It is idempotent: each step checks first
and does only what is missing. Running it again repairs a partial install and changes
nothing on a complete one. It prints each step and a summary that names the uninstall command.

1. **Preflight.** macOS or Linux (Windows through WSL2). Node 20+ with `npm` must already be
   installed; the script stops and says how to get it. It checks `git` (on macOS it starts
   `xcode-select --install` and asks for a re-run) and `ffmpeg` (installs it with Homebrew or
   the system package manager when it can).
2. **Python.** A pinned `uv` (checksum-verified) and Python 3.12 in
   `~/.local/share/orbit-office/tools` on both systems. The system Python and shell profiles
   are left alone. `tools/bin` goes first on the `PATH` of the app service and the MCP entry.
3. **Orbit.** If `orbit` is missing, installs `@orbit-tools/cli@0.37.0` with npm. An existing
   Orbit is never upgraded or downgraded: an older one stops the script with the upgrade
   command; a newer one continues with a warning.
4. **Plugins.** Adds `office` and `gateway`, and `cloudcli` only with `--with-cloudcli`.
5. **Workspace, enablement and Library.** Calls `orbit-office init` (below).
6. **Secrets.** Asks for the Meta API key, then optionally the private route's token, with
   hidden input. Skipped when already set. Without a terminal on stdin it skips them and
   prints the command to set the Meta key later.
7. **Agent CLI.** Registers the Orbit MCP entry, copies the office skills, and adds the
   shell deny rules (below).
8. **App.** Installs the app as a login service and opens `http://localhost:<port>/`, where
   the browser offers to install it as a PWA.

### Installer options

Copied from `install.sh --help`:

| Option | Meaning |
|---|---|
| `--home DIR` | Workspace folder (default `~/orbit`) |
| `--yes` | Skip the plugin-enable consent question |
| `--with-cloudcli` | Also install the optional `cloudcli` plugin |
| `--from-source` | Build office from this clone's HEAD (needs `cargo`) instead of a prebuilt export |
| `--agent NAME` | Agent CLI to register: `claude` (default), `codex` or `muse` |
| `--port PORT` | App port (default 4317, or `$ORBIT_OFFICE_PORT`) |
| `--uninstall` | Remove the service, MCP entry, added deny rules, plugins and tools; keeps the workspace and Library |
| `--purge` | Like `--uninstall`, and also removes the workspace when this installer owns it; otherwise refuses |

Useful environment variables: `ORBIT_OFFICE_HOME` (same as `--home`), `ORBIT_OFFICE_DATA_DIR`
(per-user data folder), `ORBIT_OFFICE_PLUGIN_BUNDLE` (unpacked office, gateway and optional
cloudcli exports), `ORBIT_OFFICE_APP_BUNDLE` (unpacked app bundle, needed when there is no
adjacent `app/` tree), `CLAUDE_CONFIG_DIR`, `XDG_CONFIG_HOME`. `ORBIT_OFFICE_META_API_KEY` and
`ORBIT_OFFICE_OPEN_WEIGHTS_TOKEN` supply secrets instead of the prompt; never suggest putting
a real key in a command line you can see.

Until a release with a prebuilt export exists, install from a clone (`--from-source` builds
office) or with a prepared `ORBIT_OFFICE_PLUGIN_BUNDLE`. A piped install needs both bundles.

### Where things live

| What | Where |
|---|---|
| Workspace | `~/orbit` (or `--home`), a git repo with `/library/` in `.gitignore` |
| Library | `<workspace>/library`, its own git repository |
| Orbit state | `~/.orbit` (never read it) |
| App and plugin exports | `~/Library/Application Support/orbit-office` (macOS), `~/.local/share/orbit-office` (Linux) |
| Pinned `uv` and Python | `~/.local/share/orbit-office/tools` |
| Launcher | `~/.local/bin/orbit-office` (the user may need `~/.local/bin` on `PATH`) |

## `orbit-office` and `office` are different programs

- **`orbit-office`** is the human launcher: `init`, `open`, `status`. Use this name with the user.
- **`office`** is the plugin's exec backend that Orbit calls. Don't tell the user to run it.

## `orbit-office init [path] [--yes]`

Creates or repairs one office workspace and its Library. The installer calls it once; the
user runs it again to repair, or with a path for a second workspace
(`orbit-office init ~/another-office`). Plugins must already be installed. It:

1. Refuses a non-empty folder unless install state records it, or it holds only `.git`,
   `.gitignore` (with a `/library/` line) and `.orbit`. HOME, `/`, files and symlinks are
   refused too. Refusal changes nothing.
2. Runs `git init -b main`, adds the `/library/` ignore line, fills a missing repo-local git
   identity from the login name, and makes the first commit.
3. Registers the folder with Orbit as workspace `office-<folder name>` using the bootstrap
   preset.
4. Enables `office` and `gateway` (and `cloudcli` when installed) with the manifest grants:
   fs, network and Orbit tool callbacks. It prints the grants and asks once on a terminal;
   declining fails. A grant left out leaves the plugin inactive.
5. For a new workspace, applies the office preset: enables `office-daily-digest`,
   `office-kt-inbox` and `office-signals`. `office-bootstrap` stays off until a batch is
   confirmed. A repeat run keeps the user's later routine choices.
6. Runs the Library initializer (`office.init`): layout, git repository, config, page
   templates, index, first commit. `already_initialized` is success.

Exit codes: 0 success, 1 setup, consent or tool failure, 2 bad arguments. A tool failure
prints the office error code (or `unreadable`).

## Secrets

The agent never sees or sets a secret. Tell the user what to run in their own terminal:

| Secret | Needed for | User sets it with |
|---|---|---|
| `meta_api_key` (office) | Hosted Muse Spark, Muse Wonder and Muse Voice Transcribe calls | `orbit plugin secret set office meta_api_key` |
| `open_weights_token` (office) | The private route's endpoint, only if it needs a token | `orbit plugin secret set office open_weights_token`, or Settings › Private route in the app |
| `conn_01` … `conn_16` (gateway) | API connections | Settings › Connections in the app; never by hand |
| `aws_access_key_id`, `aws_secret_access_key`, `gcp_service_account_json` (cloudcli) | Read-only cloud queries | See the `cloud-cli` skill's setup section |

`orbit plugin secret set` reads the value from stdin. Values never go in config, the Library
or argv. Tools report presence only (`set`/`unset`); check with `office.status` or
`gateway.connection-list`.

The private route itself (endpoint, model, kind, label) is set in Settings › Private route,
which also runs the endpoint check. Without a verified endpoint, private material is refused,
never sent to the hosted API. See [tools-and-guardrails.md](tools-and-guardrails.md#privacy-routing).

## Agent CLI registration

The installer registers one agent CLI: Claude Code by default, or Codex with `--agent codex`,
or Muse Code with `--agent muse`. Muse Code is the default orchestrator provider; the
installer's own `--agent` default is `claude`.

- **Claude Code:** the Orbit MCP entry (`orbit mcp serve`) goes in
  `${CLAUDE_CONFIG_DIR:-$HOME}/.claude.json`. Deny rules `Bash(curl *)`, `Bash(wget *)`,
  `Bash(http *)`, `Bash(aws *)` and `Bash(gcloud *)` go in `settings.json`'s
  `permissions.deny`. Office skills are copied to `~/.claude/skills` (or
  `$CLAUDE_CONFIG_DIR/skills`). Each file it changes keeps a `.bak`.
- **Codex:** MCP entry in `~/.codex/config.toml`, skills in `~/.codex/skills`. Codex has no
  per-command deny rules for sandboxed shell calls; the summary says so.
- **Muse Code:** with `--agent muse`, the Orbit MCP entry goes in `mcpServers` in
  `~/.config/muse/settings.json` (or under `$XDG_CONFIG_HOME`), and office skills are
  copied to `~/.agents/skills`. The file it changes keeps a `.bak`, and uninstall removes
  only the entry the installer wrote. Muse Code has no per-command shell deny settings,
  so the summary says so. `orbit-office status` also recognises a hand-written entry in
  that file. Muse Code also loads Claude Code and Codex personal skills unless that is off.

The deny rules are an accident guard. Respect them even where they don't apply: use the
gateway, never a shell HTTP client or cloud CLI.

A skill already present under the same name is left untouched. Uninstall removes only skills
it copied and that are unchanged.

## The app service

One login service per user, bound to one workspace at a time:

- macOS: launchd agent `com.orbit-office.app` (plist in `~/Library/LaunchAgents/`), logging
  to the owner-only `app.log` in the data folder.
- Linux: systemd user unit `orbit-office.service`; logs in
  `journalctl --user -u orbit-office.service`.
- It listens on `127.0.0.1`, port from the unit (`PORT`), else `ORBIT_OFFICE_PORT`, else 4317.
  The URL is `http://localhost:<port>/`.
- The app owns the dispatch pass, which starts ready `owner:agent` to-dos. With the service
  stopped, agent work does not start and scheduled to-dos wait.

`orbit-office open [path]` starts a stopped service and opens the browser. It refuses if no
service is installed, or if the service is bound to a different workspace. A second workspace
made with `orbit-office init` does not move the service; `install.sh --home <path>` rebinds it.

## Health check: `orbit-office status`

Run it when the user reports a problem, after an install, or when a tool fails in a way that
suggests setup is broken. Exit 0 is healthy, 1 means something needed is missing, 2 is a usage
error. With `--json` it prints one object with `ok` and a `problems` list of
`{check, problem, fix}`.

| Line | Healthy reading | If not |
|---|---|---|
| `Workspace:` | `<path> — registered` | Not registered: user runs `orbit-office init <path>` |
| `Plugins:` | `office enabled; gateway enabled` | Not enabled: `orbit-office init <path>` |
| `Library:` | `initialized (<n> documents)` | Not initialized: `orbit-office init <path>` |
| `Meta API key:` | `set` | `unset`: `orbit plugin secret set office meta_api_key` |
| `Private route:` | `configured (...)` or `not configured (optional)` | Not a failure. To add one: Settings › Private route |
| `App service:` | `running at http://localhost:<port>/` | `not installed`: `install.sh`. `stopped`: `orbit-office open <path>` |
| `App service log:` | a path (macOS) or the journalctl command (Linux) | Read only to diagnose a start failure; ask the user to look |
| `Agent CLI:` | `registered` | Missing: `install.sh` (with `--agent` for Codex or Muse Code) |

Report the problems and their fixes in plain words. Don't run the fixes. The JSON form also
reports `app_service.workspace`, the workspace the service is bound to; compare it with the
user's workspace.

## Troubleshooting

| Symptom | Cause | Fix the user runs |
|---|---|---|
| `orbit-office open` says the service serves another workspace | One service, bound to one workspace | `install.sh --home <path>` |
| `app service is not installed` | Install never reached the app step | `install.sh` |
| App service stopped, or the browser shows nothing | Login service not running | `orbit-office open <path>`; if it won't start, check the service log |
| `app: no local app tree and no app bundle` | Standalone install without the app | Run from a clone, or set `ORBIT_OFFICE_APP_BUNDLE` |
| `plugin export unavailable` | No prebuilt export yet | Run from a clone, or set `ORBIT_OFFICE_PLUGIN_BUNDLE` |
| `building the office plugin needs cargo` | `--from-source` without Rust | Install Rust from rustup.rs, or use a bundle |
| `Node.js 20+ is required` | Node missing or old | Install Node 20+, re-run `install.sh` |
| Node resolves under `fnm_multishells` | Per-shell shim would vanish | Install a stable Node 20+ and put its bin first on `PATH`, re-run |
| `refusing to adopt non-empty folder` | Folder is not an office workspace | Move it aside, or pass `--home DIR` |
| `plugin enable declined` | Consent refused | `orbit-office init --yes <path>` once the user agrees |
| Meta API key `unset`; model calls fail | Secret skipped at install | `orbit plugin secret set office meta_api_key` |
| `private_endpoint_unconfigured` / `_unverified` / `private_media_unsupported` | Private source with no, or an unchecked, or a too-limited endpoint | Settings › Private route: configure and check the endpoint |
| `converter_unavailable` on a PDF | `pdftotext` (poppler) not installed | User installs poppler, then captures again |
| `python_too_old` from gateway | Gateway enabled with an old Python | Re-run the install, then `orbit plugin enable gateway` |
| `unknown_connection` / `secret_unset` / `auth_expired` | Connection missing, unset or signed out | Settings › Connections |
| `secret_unset` from `cloudcli.run` | Read-only cloud identity not set | The `cloud-cli` skill's setup steps |
| No `office.*` or `orbit.*` tools in your session | Orbit MCP entry missing for this agent CLI | `orbit-office status`; if `Agent CLI` is not registered, `install.sh` (with `--agent` as needed) |

If something fails that is not in this table, run `orbit-office status`, quote the exact
error, and suggest re-running `install.sh` (it repairs). Don't guess at file edits.

## Removing the office

`install.sh --uninstall` removes the service, the MCP entry, the deny rules it added, the
plugins, its launcher and the tools folder. It keeps the workspace and the Library.
`--purge` also deletes the workspace, only if the installer owns it. Both are the user's
decision; confirm they understand `--purge` deletes their Library, and never run either.
