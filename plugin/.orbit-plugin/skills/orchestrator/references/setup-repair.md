# Setup: check and repair the office

Use this to check that Orbit Office works and to fix a broken setup.

Contents:

- [Your part and the user's part](#your-part-and-the-users-part)
- [Health check: orbit-office status](#health-check-orbit-office-status)
- [Troubleshooting](#troubleshooting)

## Your part and the user's part

- You run only `orbit-office status [path]` (and `--json`, and `office.status`
  over MCP). They change nothing.
- The user runs every command that fixes, installs, changes or removes
  anything. Give them the command and let them run it in their own terminal.
- Never handle a secret value: don't ask for a key in chat, don't put one in a
  command line, and don't read credential files, the service unit, `~/.orbit`
  or shell profiles. If the user pastes a key into chat, tell them to rotate
  it.
- Don't edit the login service, the agent CLI's settings, the MCP entry or the
  deny rules.

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
| `Node.js 24+ is required` | Node missing or old | Install Node 24+, re-run `install.sh` |
| Node resolves under `fnm_multishells` | Per-shell shim would vanish | Install a stable Node 24+ and put its bin first on `PATH`, re-run |
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
