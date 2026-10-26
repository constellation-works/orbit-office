---
type: design
summary: "Spec: install script — one idempotent command that installs Orbit, sets up git and the workspace, enables the plugin, scaffolds the Library and installs the app"
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
Linux), never inside the workspace.

**Steps:**

1. **Preflight.** macOS or Linux (Windows through WSL2). Node 20+ with `npm` is the one
   prerequisite the script does not install: without it the script stops and says how to
   get it. It checks `git` (macOS: runs `xcode-select --install` and asks to re-run when it
   is missing) and `ffmpeg` (installs it with Homebrew or the system package manager when
   one is available, otherwise stops with instructions).
2. **Orbit.** If `orbit` is missing, installs the tested release:
   `npm install -g @orbit-tools/cli@0.34.0`. An existing Orbit is never upgraded or
   downgraded: an older one stops the script with the command to upgrade, and a newer one
   continues with a warning.
3. **Git and the workspace.** Creates `~/orbit` and runs `git init -b main` if it is not
   already a repository; writes `.gitignore` with `/library/`; makes the first commit if
   the repository has none (Orbit's runs need a commit to create worktrees from). If git
   has no `user.name`/`user.email`, it sets a repo-local identity from the login name. Then
   `orbit workspace init` if the folder is not yet registered. A non-empty folder that is
   not already an office workspace is refused, never adopted.
4. **Plugin.** `orbit plugin add` from the prebuilt export attached to the orbit-office
   release (a local directory install, so no Rust toolchain is needed); `--from-source`
   uses the pinned `git+` source and needs `cargo`. Then `orbit plugin enable office` with
   the grants the manifest requests (`fs`, `network`), after printing them and asking once
   (`--yes` skips the question).
5. **Secrets.** Prompts for the Meta API key, and optionally the Baseten token, with hidden
   input, and stores them with `orbit plugin secret set office <name>`. They are never
   written to a file or echoed. Skipped when already set.
6. **Scaffold.** `orbit tool run office.init`: the Library's layout, git repository, config,
   `.gitignore` and index, then the first Library commit.
7. **Agent CLI.** Adds the Orbit MCP entry (agent authority) to Muse Code's
   `settings.json`, keeping a backup of the old file, and installs the office skills. With
   `--agent claude|codex` it registers the other CLI instead.
8. **App.** Builds the app and installs it as a login service (a launchd agent on macOS, a
   systemd user unit on Linux), then opens `http://localhost:<port>`, where the browser
   offers to install it as a PWA.

The summary lists what was created, where, and the uninstall command
(`install.sh --uninstall`). Uninstall removes the service, the MCP entry and the plugin. It
keeps `~/orbit` and the Library unless `--purge` is given.

Plugin-authored commits in the Library use a fixed author (`orbit-office
<office@orbit.local>`) passed per commit, so they need no git identity and are easy to tell
apart from the user's own edits.

Publishing the orbit-office release that carries the prebuilt export is the maintainer's
call; until one exists, the script uses `--from-source`.
