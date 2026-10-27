#!/bin/sh
# install.sh - one idempotent command that installs orbit-office.
#
# Spec: docs/design/office/specs/install.md
#
# Every step checks first and does only what is missing, so re-running
# repairs a partial install and changes nothing on a complete one.
# An existing Orbit is never upgraded or downgraded. Secrets are read
# with hidden input and piped straight to `orbit plugin secret set`;
# they are never echoed and never written to a file.
set -eu

TESTED_ORBIT_VERSION="0.34.0"
ORBIT_NPM_PACKAGE="@orbit-tools/cli@0.34.0"
PLUGIN_NAME="office"
REPO_GIT_URL="https://github.com/constellation-works/orbit-office.git"
DEFAULT_PORT="4317"

say() { printf '%s\n' "$*"; }
step() { printf '==> %s\n' "$*"; }
warn() { printf 'warning: %s\n' "$*" >&2; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

usage() {
    cat <<'EOF'
Usage: install.sh [options]

Install orbit-office: Orbit (when missing), the ~/orbit workspace,
the office plugin, secrets, the Library scaffold, agent CLI
registration and the app as a login service. Idempotent.

Options:
  --home DIR       Workspace folder (default: ~/orbit)
  --yes            Skip the plugin-enable consent question
  --from-source    Install the plugin from the pinned git+ source
                   (needs cargo) instead of a local/prebuilt export
  --agent NAME     Agent CLI to register: claude (default) or codex
  --port PORT      App port (default: 4317, or $ORBIT_OFFICE_PORT)
  --uninstall      Remove the service, the MCP entry and the plugin;
                   keeps the workspace and the Library
  --purge          Like --uninstall, and also removes the workspace
  -h, --help       Show this help

Environment:
  ORBIT_OFFICE_HOME          Same as --home
  ORBIT_OFFICE_DATA_DIR      Per-user data folder override
  ORBIT_OFFICE_PLUGIN_SOURCE Plugin source for `orbit plugin add`
  ORBIT_OFFICE_META_API_KEY  Meta API key (used instead of prompting)
  ORBIT_OFFICE_BASETEN_TOKEN Baseten token (used instead of prompting)
EOF
}

# ---------------------------------------------------------------------------
# Arguments
# ---------------------------------------------------------------------------

WORKSPACE="${ORBIT_OFFICE_HOME:-}"
ASSUME_YES=0
FROM_SOURCE=0
AGENT="claude"
PORT="${ORBIT_OFFICE_PORT:-$DEFAULT_PORT}"
MODE="install"

while [ $# -gt 0 ]; do
    case "$1" in
        --home)
            [ $# -ge 2 ] || die "--home needs a directory"
            WORKSPACE="$2"; shift 2 ;;
        --home=*) WORKSPACE=${1#--home=}; shift ;;
        --yes) ASSUME_YES=1; shift ;;
        --from-source) FROM_SOURCE=1; shift ;;
        --agent)
            [ $# -ge 2 ] || die "--agent needs claude or codex"
            AGENT="$2"; shift 2 ;;
        --agent=*) AGENT=${1#--agent=}; shift ;;
        --port)
            [ $# -ge 2 ] || die "--port needs a number"
            PORT="$2"; shift 2 ;;
        --port=*) PORT=${1#--port=}; shift ;;
        --uninstall) MODE="uninstall"; shift ;;
        --purge) MODE="purge"; shift ;;
        -h|--help) usage; exit 0 ;;
        *) printf 'error: unknown option: %s\n' "$1" >&2; usage >&2; exit 2 ;;
    esac
done

case "$AGENT" in
    claude|codex) ;;
    *) die "--agent must be claude or codex (got: $AGENT)" ;;
esac
case "$PORT" in
    ''|*[!0-9]*) die "--port must be a number (got: $PORT)" ;;
    *) ;;
esac
[ -n "${HOME:-}" ] || die "HOME is not set"
[ -n "$WORKSPACE" ] || WORKSPACE="$HOME/orbit"
# A trailing slash would break path comparisons below.
case "$WORKSPACE" in
    */) WORKSPACE=${WORKSPACE%/} ;;
    *) ;;
esac
[ -n "$WORKSPACE" ] || die "--home must not be empty"

# ---------------------------------------------------------------------------
# Platform and paths
# ---------------------------------------------------------------------------

OS_RAW="${ORBIT_OFFICE_OS:-$(uname -s 2>/dev/null || printf 'unknown')}"
case "$OS_RAW" in
    Darwin*|darwin*|macos|macOS) OS="macos" ;;
    Linux*|linux*) OS="linux" ;;
    *) OS="other" ;;
esac

if [ -n "${ORBIT_OFFICE_DATA_DIR:-}" ]; then
    DATA_DIR="$ORBIT_OFFICE_DATA_DIR"
elif [ "$OS" = "macos" ]; then
    DATA_DIR="$HOME/Library/Application Support/orbit-office"
else
    DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/orbit-office"
fi
STATE_FILE="$DATA_DIR/install-state"
SERVICE_NAME="orbit-office"
SYSTEMD_UNIT="$HOME/.config/systemd/user/orbit-office.service"
LAUNCHD_PLIST="$HOME/Library/LaunchAgents/com.orbit-office.app.plist"

SCRIPT_DIR=""
case "$0" in
    */*)
        _dir=$(dirname -- "$0")
        if [ -d "$_dir" ]; then
            SCRIPT_DIR=$(CDPATH='' cd -- "$_dir" 2>/dev/null && pwd) || SCRIPT_DIR=""
        fi
        ;;
    *) ;;
esac

# The app/plugin sources exist only in a clone, not in a curl pipe.
APP_SRC=""
PLUGIN_SRC=""
if [ -n "$SCRIPT_DIR" ] && [ -f "$SCRIPT_DIR/app/package.json" ]; then
    APP_SRC="$SCRIPT_DIR/app"
fi
if [ -n "$SCRIPT_DIR" ] && [ -f "$SCRIPT_DIR/plugin/.orbit-plugin/plugin.yaml" ]; then
    PLUGIN_SRC="$SCRIPT_DIR/plugin"
fi

AGENT_SETTINGS=""
SKILLS_DIR=""
case "$AGENT" in
    claude) AGENT_SETTINGS="$HOME/.claude/settings.json"; SKILLS_DIR="$HOME/.claude/skills" ;;
    codex) AGENT_SETTINGS="$HOME/.codex/config.toml"; SKILLS_DIR="$HOME/.codex/skills" ;;
esac

# ---------------------------------------------------------------------------
# Small helpers
# ---------------------------------------------------------------------------

# Install-state markers live in the data folder, never in the workspace.
state_has() {
    [ -f "$STATE_FILE" ] && grep -qxF "$1" "$STATE_FILE" 2>/dev/null
}

state_mark() {
    state_has "$1" && return 0
    mkdir -p "$DATA_DIR"
    printf '%s\n' "$1" >> "$STATE_FILE"
}

state_clear() {
    [ -f "$STATE_FILE" ] || return 0
    _tmp="$STATE_FILE.tmp.$$"
    grep -vxF "$1" "$STATE_FILE" > "$_tmp" || true
    if [ -s "$_tmp" ]; then
        mv "$_tmp" "$STATE_FILE"
    else
        rm -f "$_tmp" "$STATE_FILE"
    fi
}

# write_if_changed <path> : reads stdin; returns 0 when it wrote, 1 when
# the file already held exactly this content.
write_if_changed() {
    _path="$1"
    _dir=$(dirname -- "$_path")
    mkdir -p "$_dir"
    _tmp="$_dir/.install-tmp.$$"
    cat > "$_tmp"
    if [ -f "$_path" ] && cmp -s "$_tmp" "$_path"; then
        rm -f "$_tmp"
        return 1
    fi
    mv "$_tmp" "$_path"
    return 0
}

# First x.y.z found in a string (e.g. `orbit --version` output).
extract_version() {
    printf '%s' "$1" | sed -n 's/^[^0-9]*\([0-9][0-9]*\.[0-9][0-9]*\.[0-9][0-9]*\).*$/\1/p' | head -n 1
}

# version_cmp A B : prints -1, 0 or 1.
version_cmp() {
    _a1=${1%%.*}; _rest=${1#*.}; _a2=${_rest%%.*}; _a3=${_rest#*.}
    _b1=${2%%.*}; _rest=${2#*.}; _b2=${_rest%%.*}; _b3=${_rest#*.}
    for _pair in "$_a1:$_b1" "$_a2:$_b2" "$_a3:$_b3"; do
        _x=${_pair%%:*}; _y=${_pair#*:}
        case "$_x" in ''|*[!0-9]*) _x=0 ;; *) ;; esac
        case "$_y" in ''|*[!0-9]*) _y=0 ;; *) ;; esac
        if [ "$_x" -lt "$_y" ]; then printf '%s\n' "-1"; return 0; fi
        if [ "$_x" -gt "$_y" ]; then printf '%s\n' "1"; return 0; fi
    done
    printf '%s\n' "0"
}

dir_is_empty() {
    [ -d "$1" ] || return 0
    [ -z "$(ls -A "$1" 2>/dev/null)" ]
}

# Is $WORKSPACE already an office workspace (or a partial install of one)?
is_office_workspace() {
    [ -d "$WORKSPACE/.orbit" ] && return 0
    [ -e "$WORKSPACE/library" ] && return 0
    if state_has "workspace_path=$WORKSPACE"; then
        return 0
    fi
    # A partial install of ours: only git plus a .gitignore we wrote.
    if [ -d "$WORKSPACE/.git" ] && [ -f "$WORKSPACE/.gitignore" ] \
        && grep -qx '/library/' "$WORKSPACE/.gitignore" 2>/dev/null; then
        _extra=0
        for _entry in "$WORKSPACE"/* "$WORKSPACE"/.[!.]*; do
            [ -e "$_entry" ] || continue
            case "$_entry" in
                "$WORKSPACE/.git"|"$WORKSPACE/.gitignore") ;;
                *) _extra=1 ;;
            esac
        done
        [ "$_extra" -eq 0 ] && return 0
    fi
    # Ask Orbit, when it is here and already knows the folder.
    if command -v orbit >/dev/null 2>&1; then
        if orbit workspace list 2>/dev/null | grep -Fq "$WORKSPACE"; then
            return 0
        fi
    fi
    return 1
}

check_workspace_adoptable() {
    if [ -e "$WORKSPACE" ] && [ ! -d "$WORKSPACE" ]; then
        die "workspace path $WORKSPACE exists and is not a folder"
    fi
    case "$WORKSPACE" in
        "$HOME"|"/"|"") die "refusing to use $WORKSPACE as the workspace" ;;
        *) ;;
    esac
    if [ -d "$WORKSPACE" ] && ! dir_is_empty "$WORKSPACE" \
        && ! is_office_workspace; then
        die "refusing to adopt non-empty folder $WORKSPACE: it is not an office workspace (move it aside or pass --home DIR)"
    fi
}

# Ask once for consent. --yes and non-interactive runs proceed after
# printing the grants, so a piped install never hangs on a question.
consent_enable_plugin() {
    say "The $PLUGIN_NAME plugin requests these grants:"
    say "  fs      {{workspace}}, {{plugin_state}}"
    say "  network any (Meta Model API and the open-weights endpoint only)"
    if [ "$ASSUME_YES" -eq 1 ]; then
        say "Consent: --yes given, enabling $PLUGIN_NAME."
        return 0
    fi
    if [ -t 0 ]; then
        printf 'Enable the %s plugin with these grants? [y/N] ' "$PLUGIN_NAME"
        _answer=""
        read -r _answer || _answer=""
        case "$_answer" in
            y|Y|yes|YES) return 0 ;;
            *) die "plugin enable declined; re-run with --yes to consent" ;;
        esac
    fi
    say "Consent: non-interactive run, enabling $PLUGIN_NAME (pass --yes to confirm explicitly)."
    return 0
}

secret_is_set() {
    _name="$1"
    state_has "secret.$_name" && return 0
    command -v orbit >/dev/null 2>&1 || return 1
    _list=$(orbit plugin secret list "$PLUGIN_NAME" 2>/dev/null) || return 1
    # A line naming the secret and saying `set` (but not `unset`).
    printf '%s\n' "$_list" | grep "$_name" | grep -w 'set' \
        | grep -v 'unset' >/dev/null 2>&1
}

# Store one secret without echoing it or writing it to a file.
set_secret() {
    _name="$1"; _env_value="$2"; _optional="$3"
    if secret_is_set "$_name"; then
        say "secrets: already done ($_name is set)"
        return 0
    fi
    _value=""
    if [ -n "$_env_value" ]; then
        _value="$_env_value"
    elif [ -t 0 ]; then
        printf 'Enter %s (hidden, empty to skip): ' "$_name"
        if [ -t 0 ]; then
            stty -echo 2>/dev/null || true
        fi
        read -r _value || _value=""
        if [ -t 0 ]; then
            stty echo 2>/dev/null || true
            printf '\n'
        fi
    else
        # Piped stdin: one line per secret; EOF means skip.
        read -r _value || _value=""
    fi
    if [ -z "$_value" ]; then
        if [ "$_optional" -eq 1 ]; then
            say "secrets: $_name skipped (optional)"
        else
            say "secrets: $_name skipped (no input; set it later with: orbit plugin secret set $PLUGIN_NAME $_name)"
        fi
        return 0
    fi
    printf '%s' "$_value" | orbit plugin secret set "$PLUGIN_NAME" "$_name"
    _value=""
    state_mark "secret.$_name"
    say "secrets: $_name stored with orbit plugin secret set"
}

# ---------------------------------------------------------------------------
# Install steps
# ---------------------------------------------------------------------------

step_preflight() {
    step "preflight"
    case "$OS" in
        macos|linux) say "preflight: already done (platform $OS)" ;;
        *) die "unsupported platform '$OS_RAW': macOS or Linux required (Windows: use WSL2)" ;;
    esac
    command -v node >/dev/null 2>&1 \
        || die "Node.js 20+ is required and is not installed: get it from https://nodejs.org/ (or your package manager), then re-run"
    _node_version=$(node --version 2>/dev/null || printf '')
    _node_major=$(extract_version "$_node_version")
    _node_major=${_node_major%%.*}
    case "$_node_major" in
        ''|*[!0-9]*) die "could not read the Node.js version from '$_node_version'; install Node.js 20+ from https://nodejs.org/" ;;
        *) ;;
    esac
    [ "$_node_major" -ge 20 ] \
        || die "Node.js 20+ is required (found $_node_version): get it from https://nodejs.org/, then re-run"
    command -v npm >/dev/null 2>&1 \
        || die "npm is required and is not installed: it ships with Node.js 20+ from https://nodejs.org/, then re-run"
    say "preflight: node $_node_version and npm present"
    if command -v git >/dev/null 2>&1; then
        say "preflight: git present"
    elif [ "$OS" = "macos" ]; then
        warn "git is missing; starting the Xcode command line tools install"
        xcode-select --install 2>/dev/null || true
        die "git is being installed by xcode-select; re-run this script when it finishes"
    else
        die "git is required and is not installed: install it with your package manager (e.g. apt-get install git), then re-run"
    fi
    if command -v ffmpeg >/dev/null 2>&1; then
        say "preflight: ffmpeg present"
    else
        warn "ffmpeg is missing; trying to install it"
        if command -v brew >/dev/null 2>&1; then
            brew install ffmpeg 2>/dev/null || true
        elif command -v apt-get >/dev/null 2>&1; then
            apt-get install -y ffmpeg 2>/dev/null || true
        elif command -v dnf >/dev/null 2>&1; then
            dnf install -y ffmpeg 2>/dev/null || true
        elif command -v apk >/dev/null 2>&1; then
            apk add ffmpeg 2>/dev/null || true
        fi
        if command -v ffmpeg >/dev/null 2>&1; then
            say "preflight: ffmpeg installed"
        elif [ "${ORBIT_OFFICE_REQUIRE_FFMPEG:-0}" = "1" ]; then
            die "ffmpeg is required: install it with Homebrew (brew install ffmpeg) or your package manager, then re-run"
        else
            warn "ffmpeg is still missing: install it with Homebrew or your package manager; media tools will fail until then"
            say "preflight: ffmpeg missing (warning only, continuing)"
        fi
    fi
}

step_orbit() {
    step "orbit"
    if command -v orbit >/dev/null 2>&1; then
        _raw=$(orbit --version 2>/dev/null || printf '')
        _version=$(extract_version "$_raw")
        if [ -z "$_version" ]; then
            warn "could not read the Orbit version from '$_raw'; continuing without changing it"
            say "orbit: already installed (version unreadable, left unchanged)"
            return 0
        fi
        _cmp=$(version_cmp "$_version" "$TESTED_ORBIT_VERSION")
        if [ "$_cmp" -lt 0 ]; then
            printf 'error: Orbit %s is older than the tested %s; upgrade it yourself with:\n  npm install -g %s\nthen re-run this script. This script never changes an existing Orbit.\n' \
                "$_version" "$TESTED_ORBIT_VERSION" "$ORBIT_NPM_PACKAGE" >&2
            exit 1
        fi
        if [ "$_cmp" -gt 0 ]; then
            warn "Orbit $_version is newer than the tested $TESTED_ORBIT_VERSION; continuing, leaving it unchanged"
        fi
        say "orbit: already installed (orbit $_version, left unchanged)"
        return 0
    fi
    say "orbit: installing the tested release with npm"
    npm install -g "$ORBIT_NPM_PACKAGE"
    command -v orbit >/dev/null 2>&1 \
        || die "npm installed $ORBIT_NPM_PACKAGE but 'orbit' is still not on PATH"
    say "orbit: installed ($ORBIT_NPM_PACKAGE)"
}

step_workspace() {
    step "workspace"
    check_workspace_adoptable
    if [ -d "$WORKSPACE/.git" ]; then
        say "workspace: git repository already done ($WORKSPACE)"
    else
        mkdir -p "$WORKSPACE"
        if git -C "$WORKSPACE" init -b main >/dev/null 2>&1; then
            say "workspace: ran git init -b main"
        else
            git -C "$WORKSPACE" init >/dev/null
            git -C "$WORKSPACE" symbolic-ref HEAD refs/heads/main
            say "workspace: ran git init"
        fi
    fi
    _ignore="$WORKSPACE/.gitignore"
    if [ -f "$_ignore" ] && grep -qx '/library/' "$_ignore"; then
        say "workspace: .gitignore already done"
    elif [ -f "$_ignore" ]; then
        printf '/library/\n' >> "$_ignore"
        say "workspace: added /library/ to .gitignore"
    else
        printf '/library/\n' > "$_ignore"
        say "workspace: wrote .gitignore"
    fi
    # A repo-local identity, only for the fields git does not have.
    _login=$(id -un 2>/dev/null || printf 'orbit')
    [ -n "$_login" ] || _login="orbit"
    if [ -z "$(git -C "$WORKSPACE" config user.name 2>/dev/null || true)" ]; then
        git -C "$WORKSPACE" config user.name "$_login"
    fi
    if [ -z "$(git -C "$WORKSPACE" config user.email 2>/dev/null || true)" ]; then
        git -C "$WORKSPACE" config user.email "$_login@localhost"
    fi
    if git -C "$WORKSPACE" rev-parse --verify HEAD >/dev/null 2>&1; then
        say "workspace: first commit already done"
    else
        git -C "$WORKSPACE" add .gitignore
        git -C "$WORKSPACE" commit -m "Initial commit" >/dev/null
        say "workspace: made the first commit"
    fi
    if [ -d "$WORKSPACE/.orbit" ] || state_has "workspace_path=$WORKSPACE"; then
        say "workspace: already registered ($WORKSPACE)"
    else
        (cd "$WORKSPACE" && orbit workspace init >/dev/null)
        say "workspace: registered with orbit workspace init"
    fi
    state_mark "workspace_path=$WORKSPACE"
}

plugin_is_listed() {
    command -v orbit >/dev/null 2>&1 || return 1
    _list=$(orbit plugin list 2>/dev/null) || return 1
    printf '%s\n' "$_list" | awk -v name="$PLUGIN_NAME" '$1 == name { found=1 } END { exit !found }'
}

step_plugin() {
    step "plugin"
    if plugin_is_listed || state_has "plugin_added"; then
        say "plugin: already added ($PLUGIN_NAME)"
    else
        _source=""
        _allow_build=0
        if [ -n "${ORBIT_OFFICE_PLUGIN_SOURCE:-}" ]; then
            _source="$ORBIT_OFFICE_PLUGIN_SOURCE"
        elif [ "$FROM_SOURCE" -eq 1 ] || [ -z "$PLUGIN_SRC" ]; then
            # No prebuilt export is published yet: use the pinned source.
            _ref="main"
            if [ -n "$SCRIPT_DIR" ] && [ -d "$SCRIPT_DIR/.git" ]; then
                _ref=$(git -C "$SCRIPT_DIR" rev-parse HEAD 2>/dev/null || printf 'main')
                [ -n "$_ref" ] || _ref="main"
            fi
            _source="git+$REPO_GIT_URL#$_ref"
            _allow_build=1
        else
            _source="$PLUGIN_SRC"
        fi
        if [ "$_allow_build" -eq 1 ]; then
            command -v cargo >/dev/null 2>&1 \
                || warn "--from-source builds with cargo, which is not on PATH (https://rustup.rs/); letting orbit plugin add report it"
            orbit plugin add "$_source" --allow-build
        else
            orbit plugin add "$_source"
        fi
        state_mark "plugin_added"
        say "plugin: added from $_source"
    fi
    if state_has "plugin_enabled"; then
        say "plugin: already enabled ($PLUGIN_NAME)"
        return 0
    fi
    consent_enable_plugin
    orbit plugin enable "$PLUGIN_NAME" --grant 'fs={{workspace}},{{plugin_state}}' --grant network
    state_mark "plugin_enabled"
    say "plugin: enabled ($PLUGIN_NAME)"
}

step_secrets() {
    step "secrets"
    set_secret "meta_api_key" "${ORBIT_OFFICE_META_API_KEY:-${META_API_KEY:-}}" 0
    set_secret "baseten_token" "${ORBIT_OFFICE_BASETEN_TOKEN:-${BASETEN_TOKEN:-}}" 1
}

step_scaffold() {
    step "scaffold"
    if [ -f "$WORKSPACE/library/.office/config.yaml" ] \
        || [ -d "$WORKSPACE/library/.git" ] \
        || state_has "scaffolded"; then
        # Still ask office.init when the Library exists but our marker
        # does not: it answers already_initialized and creates nothing.
        if state_has "scaffolded"; then
            say "scaffold: already done (office.init, $WORKSPACE/library)"
            return 0
        fi
    fi
    (cd "$WORKSPACE" && orbit tool run office.init --input '{}' >/dev/null)
    state_mark "scaffolded"
    say "scaffold: ran office.init ($WORKSPACE/library)"
}

# Register the Orbit MCP entry with the agent CLI. Node is a proven
# prerequisite, so JSON edits go through it; the old file is backed up
# once to <settings>.bak. Prints "already" when the entry is present.
register_mcp_claude() {
    _file="$AGENT_SETTINGS"
    mkdir -p "$(dirname -- "$_file")"
    if [ -f "$_file" ] && grep -q '"orbit"' "$_file" 2>/dev/null \
        && state_has "agent_registered"; then
        say "agent: already done (Orbit MCP entry in $_file)"
        return 0
    fi
    if [ -f "$_file" ] && [ ! -f "$_file.bak" ]; then
        cp "$_file" "$_file.bak"
        say "agent: backed up $_file to $_file.bak"
    fi
    SETTINGS_FILE="$_file" node <<'NODE'
const fs = require("fs");
const file = process.env.SETTINGS_FILE;
let data = {};
if (fs.existsSync(file)) {
  const text = fs.readFileSync(file, "utf8").trim();
  if (text) data = JSON.parse(text);
}
if (!data.mcpServers || typeof data.mcpServers !== "object") data.mcpServers = {};
data.mcpServers.orbit = { command: "orbit", args: ["mcp", "serve"] };
fs.writeFileSync(file, JSON.stringify(data, null, 2) + "\n");
NODE
    state_mark "agent_registered"
    say "agent: added the Orbit MCP entry to $_file"
}

register_mcp_codex() {
    _file="$AGENT_SETTINGS"
    mkdir -p "$(dirname -- "$_file")"
    if [ -f "$_file" ] && grep -q 'mcp_servers.orbit' "$_file" 2>/dev/null; then
        state_mark "agent_registered"
        say "agent: already done (Orbit MCP entry in $_file)"
        return 0
    fi
    if [ -f "$_file" ] && [ ! -f "$_file.bak" ]; then
        cp "$_file" "$_file.bak"
        say "agent: backed up $_file to $_file.bak"
    fi
    {
        printf '\n[mcp_servers.orbit]\n'
        printf 'command = "orbit"\n'
        printf 'args = ["mcp", "serve"]\n'
    } >> "$_file"
    state_mark "agent_registered"
    say "agent: added the Orbit MCP entry to $_file"
}

remove_mcp_claude() {
    _file="$AGENT_SETTINGS"
    [ -f "$_file" ] || { say "agent: already removed (no $_file)"; return 0; }
    if ! grep -q '"orbit"' "$_file" 2>/dev/null; then
        say "agent: already removed (no Orbit MCP entry)"
        return 0
    fi
    SETTINGS_FILE="$_file" node <<'NODE' || true
const fs = require("fs");
const file = process.env.SETTINGS_FILE;
const data = JSON.parse(fs.readFileSync(file, "utf8"));
if (data.mcpServers) delete data.mcpServers.orbit;
fs.writeFileSync(file, JSON.stringify(data, null, 2) + "\n");
NODE
    say "agent: removed the Orbit MCP entry from $_file"
}

remove_mcp_codex() {
    _file="$AGENT_SETTINGS"
    [ -f "$_file" ] || { say "agent: already removed (no $_file)"; return 0; }
    if ! grep -q 'mcp_servers.orbit' "$_file" 2>/dev/null; then
        say "agent: already removed (no Orbit MCP entry)"
        return 0
    fi
    _tmp="$_file.tmp.$$"
    awk '/^\[mcp_servers\.orbit\]/{skip=1; next} /^\[/{skip=0} !skip' "$_file" > "$_tmp" \
        && mv "$_tmp" "$_file"
    say "agent: removed the Orbit MCP entry from $_file"
}

step_agent() {
    step "agent"
    case "$AGENT" in
        claude) register_mcp_claude ;;
        codex) register_mcp_codex ;;
    esac
    # Office skills from the plugin tree, when this run has one.
    if [ -n "$PLUGIN_SRC" ] && [ -d "$PLUGIN_SRC/.orbit-plugin/skills" ]; then
        mkdir -p "$SKILLS_DIR"
        for _skill in "$PLUGIN_SRC/.orbit-plugin/skills"/*; do
            [ -d "$_skill" ] || continue
            _name=$(basename -- "$_skill")
            if [ -e "$SKILLS_DIR/$_name" ]; then
                say "agent: skill $_name already done"
            else
                cp -R "$_skill" "$SKILLS_DIR/$_name"
                say "agent: installed skill $_name"
            fi
        done
    else
        say "agent: skills already done (no local plugin tree; orbit plugin enable links them)"
    fi
    state_mark "agent_registered"
}

step_app() {
    step "app"
    _node_bin=$(command -v node 2>/dev/null || printf 'node')
    if [ -n "$APP_SRC" ]; then
        if [ -f "$APP_SRC/packages/server/dist/main.js" ] || state_has "app_built"; then
            say "app: build already done"
        else
            npm --prefix "$APP_SRC" run build >/dev/null
            state_mark "app_built"
            say "app: built ($APP_SRC)"
        fi
        _app_home="$APP_SRC"
    else
        _app_home="$DATA_DIR/app"
        say "app: build already done (no local app tree; service points at $_app_home)"
    fi
    _server_js="$_app_home/packages/server/dist/main.js"
    if [ "$OS" = "macos" ]; then
        if write_if_changed "$LAUNCHD_PLIST" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>com.orbit-office.app</string>
  <key>ProgramArguments</key>
  <array><string>$_node_bin</string><string>$_server_js</string></array>
  <key>EnvironmentVariables</key>
  <dict><key>PORT</key><string>$PORT</string><key>HOST</key><string>127.0.0.1</string></dict>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
</dict>
</plist>
EOF
        then
            say "app: wrote launchd agent $LAUNCHD_PLIST"
            _svc_changed=1
        else
            _svc_changed=0
            say "app: service already done ($LAUNCHD_PLIST)"
        fi
        if [ "$_svc_changed" -eq 1 ] || ! state_has "service_installed"; then
            if command -v launchctl >/dev/null 2>&1; then
                launchctl unload "$LAUNCHD_PLIST" >/dev/null 2>&1 || true
                launchctl load "$LAUNCHD_PLIST" >/dev/null 2>&1 || true
            fi
            state_mark "service_installed"
        fi
    else
        if write_if_changed "$SYSTEMD_UNIT" <<EOF
[Unit]
Description=orbit-office app
After=network.target

[Service]
Type=simple
WorkingDirectory=$_app_home
ExecStart=$_node_bin $_server_js
Environment=PORT=$PORT
Environment=HOST=127.0.0.1
Restart=on-failure

[Install]
WantedBy=default.target
EOF
        then
            say "app: wrote systemd user unit $SYSTEMD_UNIT"
            _svc_changed=1
        else
            _svc_changed=0
            say "app: service already done ($SYSTEMD_UNIT)"
        fi
        if [ "$_svc_changed" -eq 1 ] || ! state_has "service_installed"; then
            if command -v systemctl >/dev/null 2>&1; then
                systemctl --user daemon-reload >/dev/null 2>&1 || true
                systemctl --user enable "$SERVICE_NAME.service" >/dev/null 2>&1 || true
                systemctl --user restart "$SERVICE_NAME.service" >/dev/null 2>&1 \
                    || systemctl --user start "$SERVICE_NAME.service" >/dev/null 2>&1 || true
            fi
            state_mark "service_installed"
            say "app: service installed (login service $SERVICE_NAME)"
        else
            say "app: service already enabled ($SERVICE_NAME)"
        fi
    fi
    say "app: http://localhost:$PORT/ (open it to install the PWA)"
    if [ -t 1 ] && [ "${ORBIT_OFFICE_NO_OPEN:-0}" != "1" ]; then
        if [ "$OS" = "macos" ] && command -v open >/dev/null 2>&1; then
            open "http://localhost:$PORT/" >/dev/null 2>&1 || true
        elif command -v xdg-open >/dev/null 2>&1; then
            xdg-open "http://localhost:$PORT/" >/dev/null 2>&1 || true
        fi
    fi
}

# ---------------------------------------------------------------------------
# Uninstall / purge
# ---------------------------------------------------------------------------

do_uninstall() {
    _purge="$1"
    step "uninstall"
    # Service first.
    if [ "$OS" = "macos" ]; then
        if [ -f "$LAUNCHD_PLIST" ]; then
            if command -v launchctl >/dev/null 2>&1; then
                launchctl unload "$LAUNCHD_PLIST" >/dev/null 2>&1 || true
            fi
            rm -f "$LAUNCHD_PLIST"
            say "uninstall: removed the login service"
        else
            say "uninstall: service already removed"
        fi
    else
        if [ -f "$SYSTEMD_UNIT" ]; then
            if command -v systemctl >/dev/null 2>&1; then
                systemctl --user disable --now "$SERVICE_NAME.service" >/dev/null 2>&1 || true
                systemctl --user stop "$SERVICE_NAME.service" >/dev/null 2>&1 || true
            fi
            rm -f "$SYSTEMD_UNIT"
            if command -v systemctl >/dev/null 2>&1; then
                systemctl --user daemon-reload >/dev/null 2>&1 || true
            fi
            say "uninstall: removed the login service"
        else
            if command -v systemctl >/dev/null 2>&1; then
                systemctl --user disable --now "$SERVICE_NAME.service" >/dev/null 2>&1 || true
            fi
            say "uninstall: service already removed"
        fi
    fi
    case "$AGENT" in
        claude) remove_mcp_claude ;;
        codex) remove_mcp_codex ;;
    esac
    # Skills this installer copied in.
    if [ -n "$PLUGIN_SRC" ] && [ -d "$PLUGIN_SRC/.orbit-plugin/skills" ] \
        && [ -d "$SKILLS_DIR" ]; then
        for _skill in "$PLUGIN_SRC/.orbit-plugin/skills"/*; do
            [ -d "$_skill" ] || continue
            _name=$(basename -- "$_skill")
            if [ -n "$_name" ] && [ -e "$SKILLS_DIR/$_name" ]; then
                rm -rf "${SKILLS_DIR:?}/$_name"
                say "uninstall: removed skill $_name"
            fi
        done
    fi
    # Plugin.
    _plugin_done=0
    if command -v orbit >/dev/null 2>&1; then
        if plugin_is_listed || state_has "plugin_added"; then
            orbit plugin remove "$PLUGIN_NAME" --yes >/dev/null 2>&1 \
                || orbit plugin disable "$PLUGIN_NAME" >/dev/null 2>&1 || true
            _plugin_done=1
        fi
    fi
    if [ "$_plugin_done" -eq 1 ]; then
        say "uninstall: removed the $PLUGIN_NAME plugin"
    else
        say "uninstall: plugin already removed"
    fi
    state_clear "plugin_added"
    state_clear "plugin_enabled"
    state_clear "service_installed"
    state_clear "app_built"
    state_clear "agent_registered"
    if [ "$_purge" -eq 1 ]; then
        case "$WORKSPACE" in
            "$HOME"|"/"|"") die "refusing to purge $WORKSPACE" ;;
            *) ;;
        esac
        if [ -e "$WORKSPACE" ]; then
            rm -rf "$WORKSPACE"
            say "purge: removed the workspace $WORKSPACE"
        else
            say "purge: workspace already removed"
        fi
        rm -rf "$DATA_DIR"
        say "purge: removed $DATA_DIR; the workspace and Library are gone"
    else
        say "uninstall: kept the workspace and Library at $WORKSPACE"
        say "uninstall: run install.sh --purge to remove them too"
    fi
}

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

if [ "$MODE" = "uninstall" ]; then
    do_uninstall 0
    exit 0
fi
if [ "$MODE" = "purge" ]; then
    do_uninstall 1
    exit 0
fi

check_workspace_adoptable
step_preflight
step_orbit
step_workspace
step_plugin
step_secrets
step_scaffold
step_agent
step_app

step "summary"
say "summary: workspace  $WORKSPACE"
say "summary: library    $WORKSPACE/library"
say "summary: data       $DATA_DIR"
say "summary: app        http://localhost:$PORT/"
say "summary: uninstall  install.sh --uninstall (keeps $WORKSPACE)"
say "summary: purge      install.sh --purge (also removes $WORKSPACE)"
