---
type: design
summary: "Spec: install script — one idempotent command that installs Orbit and a pinned Python, sets up git and the workspace, enables the plugin, scaffolds the Library and installs the app"
last_validated: 2026-10-26
---

# Spec: Install script

Source: spec §13.5.

One command installs everything, with no manual git, Orbit or scaffold step:

```bash
curl -fsSL https://raw.githubusercontent.com/constellation-works/orbit-office/main/install.sh | sh
```

(or `./install.sh` from a clone). The script is idempotent: every step checks first and
does only what is missing, so re-running it repairs a partial install and changes nothing on
a complete one. It prints each step as it goes and a summary at the end.

**Defaults.** The workspace is `~/orbit`, with the Library at `~/orbit/library`.
`--home DIR` picks another folder. Orbit's own state stays in `~/.orbit`; the two are
different folders. The app and the plugin export are unpacked to the per-user data folder
(`~/Library/Application Support/orbit-office` on macOS, `~/.local/share/orbit-office` on
Linux), never inside the workspace. The tools the script installs for agents (`uv` and a
pinned Python) go in `~/.local/share/orbit-office/tools` on both systems, a path with no
spaces, so script shebangs that name the interpreter keep working.

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
     environment. The `library-conventions` skill tells agents to use `python3`, and
     `uv run` for a script that declares dependencies inline (PEP 723).
   - **Check.** `tools/bin/python3 --version` must print the pinned minor version, or the
     step fails and names what went wrong. A re-run with both already in place changes
     nothing.
3. **Orbit.** If `orbit` is missing, installs the tested release:
   `npm install -g @orbit-tools/cli@0.34.0`. An existing Orbit is never upgraded or
   downgraded: an older one stops the script with the command to upgrade, and a newer one
   continues with a warning.
4. **Git and the workspace.** Creates `~/orbit` and runs `git init -b main` if it is not
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
   uses the pinned `git+` source and needs `cargo`. Then `orbit plugin enable office` with
   the grants the manifest requests (`fs`, `network`), after printing them and asking once.
   The question needs a terminal on stdin. A run without one (the `curl ... | sh` line at the
   top of this page, for example) prints the grants and enables the plugin without asking,
   and its output says so. `--yes` skips the question in either case. The `gateway` plugin is
   added and enabled the same way, in the same consent prompt, and `cloudcli` only with
   `--with-cloudcli` ([gateway.md](./gateway.md)). Both come from the same release export, and
   neither has a build step.
6. **Secrets.** Prompts for the Meta API key, and optionally the private route's token (`open_weights_token`), with hidden
   input, and stores them with `orbit plugin secret set office <name>`. They are never
   written to a file or echoed. Skipped when already set.
7. **Scaffold.** `orbit tool run office.init --json --full`: the Library's layout, git
   repository, config, `.gitignore` and index, then the first Library commit. The CLI exits 0
   and prints the tool's output object on stdout. That object has no top-level `ok`. The step
   is done for that object, including a retry that prints `{"already_initialized": true, "created": []}`
   and exits 0. An `{ok: true}` envelope is also accepted. A non-zero exit stops the install.
   The office code is read from the JSON object on stderr (`code`, or `error.code`), including
   when a WARN line precedes the JSON, and the failure names that code. An `{ok: false}`
   envelope on stdout, even with exit 0, also stops the install and names its code. Nothing
   is recorded on failure, so the next run asks `office.init` again.
8. **Agent CLI.** Adds the Orbit MCP entry (agent authority) to Muse Code's
   `settings.json`, keeping a backup of the old file, and installs the office skills. It
   also adds deny rules for direct shell use of `curl`, `wget`, `http`, `aws` and `gcloud`,
   so API and cloud calls go through the gateway ([gateway.md](./gateway.md#agent-cli-deny-rules)),
   and lists them in the summary. Rules the user already has are kept, and uninstall
   removes only the ones it added. With `--agent claude|codex` it registers the other CLI
   instead. A CLI with no per-command deny rules is reported as such.
9. **App.** Builds the app and installs it as a login service (a launchd agent on macOS, a
   systemd user unit on Linux), then opens `http://localhost:<port>`, where the browser
   offers to install it as a PWA.

The summary lists what was created, where, and the uninstall command
(`install.sh --uninstall`). Uninstall removes the service, the MCP entry and its deny rules, the plugins and the
`tools` folder (`uv` and its Python, which nothing else uses). It keeps `~/orbit` and the
Library unless `--purge` is given. `--purge` deletes that workspace only after the same
ownership check as adoption, and otherwise refuses with a message that names the path and
removes nothing, including the per-user data folder.

Plugin-authored commits in the Library use a fixed author (`orbit-office
<office@orbit.local>`) passed per commit, so they need no git identity and are easy to tell
apart from the user's own edits.

Publishing the orbit-office release that carries the prebuilt export is the maintainer's
call; until one exists, the script uses `--from-source`.
