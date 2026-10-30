#!/bin/sh
# GENERATED FILE - do not edit directly.
# Sources: scripts/install/lib/helpers.sh, scripts/install/lib/python.sh,
#          scripts/install/lib/agent.sh, scripts/install/lib/steps.sh
# Regenerate: sh scripts/install/build-install.sh
# Check:      sh scripts/install/build-install.sh --check

# ---------------------------------------------------------------------------
# Module: scripts/install/lib/helpers.sh
# ---------------------------------------------------------------------------

# helpers.sh — arguments, platform/paths, small helpers and install state.
# Fragment of the generated install.sh: assembled by scripts/install/build-install.sh,
# never sourced at runtime. Edit this file, then regenerate install.sh.

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

TESTED_ORBIT_VERSION="0.37.0"
ORBIT_NPM_PACKAGE="@orbit-tools/cli@0.37.0"
PLUGIN_NAME="office"
DEFAULT_PORT="4317"

# Pinned agent toolchain (spec step 2). The four archive digests were
# recorded from the uv 0.13.6 release asset sha256.sum and checked by
# hashing those archives (2026-10-26). Nothing in the environment can
# replace a digest. ORBIT_OFFICE_UV_URL may only change which archive
# is fetched; the bytes are still checked against the digest recorded
# for that file name (STD-05@1 §R23). The fixture name is the harness
# archive scripts/install/fixtures/uv-test-fixture.tar.gz.
UV_VERSION="0.13.6"
PYTHON_VERSION="3.12"
UV_SHA256_MACOS_ARM64="60fbd5c8a1ed681845eea28d9d622fcde03c51e91a43465b64a1e17db1ed3472"
UV_SHA256_MACOS_X86_64="63ba17fdda55ad1a230d4b37fc6017ffa10c23688c83818bd3bfdf914c652a02"
UV_SHA256_LINUX_X86_64="f29c3e94ffabd7e6f3ba521dba169a9b7ec12100bf3257087f0f24f6123f3645"
UV_SHA256_LINUX_AARCH64="229e194255c7958d86d56e8790d969a6f74736592133f0dfd0076f754c642c1d"
UV_SHA256_FIXTURE="1865ac48a9aea37d3cc88091cc20ed9d513c5c73985a0823384ffb37e3542e20"

say() { printf '%s\n' "$*"; }
step() { printf '==> %s\n' "$*"; }
warn() { printf 'warning: %s\n' "$*" >&2; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

usage() {
    cat <<'EOF'
Usage: install.sh [options]

Install orbit-office: a pinned uv and Python 3.12, Orbit (when
missing), the ~/orbit workspace, office and gateway, secrets, the
Library scaffold, agent CLI registration and the app as a login
service. Idempotent.

Options:
  --home DIR       Workspace folder (default: ~/orbit)
  --yes            Skip the plugin-enable consent question
  --with-cloudcli  Also install the optional cloudcli plugin
  --from-source    Build office from this clone's HEAD (needs cargo)
                   instead of using a prebuilt export
  --agent NAME     Agent CLI to register: claude (default), codex or muse
  --port PORT      App port (default: 4317, or $ORBIT_OFFICE_PORT)
  --uninstall      Remove the service, MCP entry, added deny rules,
                   plugins and tools (uv and Python); keeps the
                   workspace and the Library
  --purge          Like --uninstall, and also removes the workspace
                   when this installer owns it; otherwise refuses
  -h, --help       Show this help

Environment:
  ORBIT_OFFICE_HOME          Same as --home
  ORBIT_OFFICE_DATA_DIR      Per-user data folder override
  ORBIT_OFFICE_PLUGIN_SOURCE Office source for `orbit plugin add`
  ORBIT_OFFICE_PLUGIN_BUNDLE Directory of office, gateway and optional
                             cloudcli exports from one release
  ORBIT_OFFICE_APP_BUNDLE    Unpacked app bundle containing
                             packages/server/dist/main.js and its runtime
                             files; required when install.sh has no
                             adjacent app tree (standalone install)
  ORBIT_OFFICE_META_API_KEY  Meta API key (used instead of prompting)
  ORBIT_OFFICE_OPEN_WEIGHTS_TOKEN  Private-route endpoint token (used
                             instead of prompting; optional)
  ORBIT_OFFICE_BASETEN_TOKEN Deprecated alias of
                             ORBIT_OFFICE_OPEN_WEIGHTS_TOKEN, accepted
                             with a warning for one release
  BASETEN_TOKEN              Deprecated alias of OPEN_WEIGHTS_TOKEN,
                             accepted with a warning for one release
  CLAUDE_CONFIG_DIR          Claude Code config directory (default: HOME);
                             its .claude.json holds the MCP entry
  XDG_CONFIG_HOME            Muse Code config root (default: ~/.config);
                             its muse/settings.json holds the MCP entry
  ORBIT_OFFICE_OS            Platform override (macos or linux)
  ORBIT_OFFICE_ARCH          Architecture override (x86_64 or arm64)
  ORBIT_OFFICE_UV_URL        Fetch this uv archive instead of the
                             GitHub asset. The recorded SHA-256 for
                             that archive name still applies; this
                             does not override the checksum
EOF
}

# ---------------------------------------------------------------------------
# Arguments
# ---------------------------------------------------------------------------

WORKSPACE="${ORBIT_OFFICE_HOME:-}"
ASSUME_YES=0
FROM_SOURCE=0
WITH_CLOUDCLI=0
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
        --with-cloudcli) WITH_CLOUDCLI=1; shift ;;
        --agent)
            [ $# -ge 2 ] || die "--agent needs claude, codex or muse"
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
    claude|codex|muse) ;;
    *) die "--agent must be claude, codex or muse (got: $AGENT)" ;;
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
# A relative workspace must be made absolute before any comparison or
# recorded state (workspace_path, is_office_workspace) uses it, and before
# it is delivered to the app as ORBIT_OFFICE_WORKSPACE.
case "$WORKSPACE" in
    /*) ;;
    *) WORKSPACE="$PWD/$WORKSPACE" ;;
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

ARCH_RAW="${ORBIT_OFFICE_ARCH:-$(uname -m 2>/dev/null || printf 'unknown')}"
case "$ARCH_RAW" in
    arm64|aarch64) ARCH="arm64" ;;
    x86_64|amd64) ARCH="x86_64" ;;
    *) ARCH="$ARCH_RAW" ;;
esac

# uv and Python live here on both systems, not under DATA_DIR. On macOS
# DATA_DIR has spaces and follows ORBIT_OFFICE_DATA_DIR; shebangs need
# a stable path with no spaces.
TOOLS_DIR="$HOME/.local/share/orbit-office/tools"
TOOLS_BIN="$TOOLS_DIR/bin"
TOOLS_PYTHON="$TOOLS_DIR/python"
TOOLS_CACHE="$TOOLS_DIR/cache"
case "$OS" in
    macos) _path_rest="/usr/bin:/bin:/usr/sbin:/sbin" ;;
    *) _path_rest="/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin" ;;
esac
# A per-shell shim directory does not survive the shell that created it
# (fnm creates fnm_multishells/<pid>_<timestamp> per interactive shell
# and removes it later), so it must never be recorded in a login service
# unit or its PATH. Only the fnm_multishells layout is known; any other
# shim is caught by resolving to a physical path and refusing a result
# that still lives under such a directory.
path_is_per_shell() {
    case "$1" in
        */fnm_multishells|*/fnm_multishells/*) return 0 ;;
        *) return 1 ;;
    esac
}
physical_dir() {
    CDPATH='' cd -P -- "$1" 2>/dev/null && pwd -P
}
# stable_path_of_file FILE : the symlink-free absolute path of FILE;
# fails when FILE is missing or the resolved path is per-shell.
stable_path_of_file() {
    _spf=$1
    _spf_n=0
    while [ -L "$_spf" ] && [ "$_spf_n" -lt 40 ]; do
        _spf_target=$(readlink "$_spf") || return 1
        case "$_spf_target" in
            /*) _spf=$_spf_target ;;
            *) _spf="$(dirname -- "$_spf")/$_spf_target" ;;
        esac
        _spf_n=$((_spf_n + 1))
    done
    [ -f "$_spf" ] || return 1
    _spf_phys=$(physical_dir "$(dirname -- "$_spf")") || return 1
    _spf_stable="$_spf_phys/$(basename -- "$_spf")"
    if path_is_per_shell "$_spf_stable"; then return 1; fi
    printf '%s\n' "$_spf_stable"
}
# stable_node_path : a stable absolute node path, resolving the node on
# PATH through symlinks and its physical directory; when that node is a
# per-shell shim that does not resolve to a stable file, later PATH
# entries are tried. Fails when no stable node can be resolved.
stable_node_path() {
    _snp_raw=$(command -v node 2>/dev/null || printf '')
    if [ -n "$_snp_raw" ]; then
        if _snp=$(stable_path_of_file "$_snp_raw"); then
            printf '%s\n' "$_snp"; return 0
        fi
    fi
    _snp_rest=${PATH:-}
    while [ -n "$_snp_rest" ]; do
        case "$_snp_rest" in
            *:*)
                _snp_dir=${_snp_rest%%:*}
                _snp_rest=${_snp_rest#*:} ;;
            *)
                _snp_dir=$_snp_rest
                _snp_rest="" ;;
        esac
        [ -n "$_snp_dir" ] && [ -x "$_snp_dir/node" ] || continue
        if _snp=$(stable_path_of_file "$_snp_dir/node"); then
            printf '%s\n' "$_snp"; return 0
        fi
    done
    return 1
}

# Generated runtime PATH (MCP entries and the login service): tools/bin
# first, then the directories holding the resolved `orbit` and `node`
# executables, then the system directories. An Orbit or Node installed
# under a user prefix (nvm, a user npm prefix, ~/.local/bin, Homebrew)
# is visible to this installer through its PATH but not through the
# fixed system directories alone, so those directories are captured
# here and refreshed after step 3 has ensured Orbit is present (a
# just-installed Orbit was not resolvable when this script started).
# Only absolute directories are captured: a relative PATH entry would
# resolve against a different working directory in the generated
# environment. The existing Orbit is never moved or changed by this.
AGENT_PATH="$TOOLS_BIN"
agent_path_append() {
    if path_is_per_shell "$1"; then return 0; fi
    case ":$AGENT_PATH:" in
        *":$1:"*) ;;
        *) AGENT_PATH="$AGENT_PATH:$1" ;;
    esac
}
compute_agent_path() {
    AGENT_PATH="$TOOLS_BIN"
    for _rt_tool in orbit node; do
        if [ "$_rt_tool" = node ]; then
            _rt_resolved=$(stable_node_path 2>/dev/null || printf '')
        else
            _rt_resolved=$(command -v "$_rt_tool" 2>/dev/null || printf '')
        fi
        case "$_rt_resolved" in
            /*/*)
                _rt_dir=$(dirname -- "$_rt_resolved")
                if [ "$_rt_tool" != node ] && path_is_per_shell "$_rt_dir"; then
                    _rt_dir=$(physical_dir "$_rt_dir" || printf '')
                fi
                if [ -n "$_rt_dir" ]; then agent_path_append "$_rt_dir"; fi ;;
            *) ;;
        esac
    done
    _rt_rest="$_path_rest"
    while [ -n "$_rt_rest" ]; do
        case "$_rt_rest" in
            *:*)
                agent_path_append "${_rt_rest%%:*}"
                _rt_rest="${_rt_rest#*:}" ;;
            *)
                agent_path_append "$_rt_rest"
                _rt_rest="" ;;
        esac
    done
}
compute_agent_path
_nl='
'
validate_agent_path() {
    case "$AGENT_PATH" in
        *\"*|*"$_nl"*|*\\*) die "agent PATH cannot be encoded: $AGENT_PATH" ;;
    esac
}
validate_agent_path
SERVICE_NAME="orbit-office"
SYSTEMD_UNIT="$HOME/.config/systemd/user/orbit-office.service"
LAUNCHD_PLIST="$HOME/Library/LaunchAgents/com.orbit-office.app.plist"
SERVICE_LOG="$DATA_DIR/app.log"

SCRIPT_DIR=""
case "$0" in
    */*)
        _dir=$(dirname -- "$0")
        if [ -d "$_dir" ]; then
            SCRIPT_DIR=$(CDPATH='' cd -- "$_dir" 2>/dev/null && pwd) || SCRIPT_DIR=""
        fi
        ;;
    *)
        # `sh install.sh` from the clone root: $0 has no slash but names this
        # file. A curl pipe's $0 is the shell, never a copy of the installer.
        if [ -f "$0" ] && grep -q '^OFFICE_LAUNCHER$' "$0" 2>/dev/null; then
            SCRIPT_DIR=$(pwd) || SCRIPT_DIR=""
        fi
        ;;
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
# Claude Code reads its MCP servers from ${CLAUDE_CONFIG_DIR:-$HOME}/.claude.json
# (the app's agentCliStatus reader checks exactly that file), while its
# permissions.deny rules live in settings.json under its config directory.
# The two files are therefore edited separately; the legacy settings path
# is where installers before this fix misplaced the MCP entry.
CLAUDE_CONFIG_FILE=""
CLAUDE_LEGACY_SETTINGS="$HOME/.claude/settings.json"
case "$AGENT" in
    claude)
        if [ -n "${CLAUDE_CONFIG_DIR:-}" ]; then
            _claude_config_dir="$CLAUDE_CONFIG_DIR"
            AGENT_SETTINGS="$CLAUDE_CONFIG_DIR/settings.json"
            SKILLS_DIR="$CLAUDE_CONFIG_DIR/skills"
        else
            _claude_config_dir="$HOME"
            AGENT_SETTINGS="$HOME/.claude/settings.json"
            SKILLS_DIR="$HOME/.claude/skills"
        fi
        CLAUDE_CONFIG_FILE="$_claude_config_dir/.claude.json"
        ;;
    codex) AGENT_SETTINGS="$HOME/.codex/config.toml"; SKILLS_DIR="$HOME/.codex/skills" ;;
    muse)
        # Muse Code reads MCP servers from mcpServers in its settings.json
        # under $XDG_CONFIG_HOME (the status reader checks exactly that
        # file); its user skills live in $HOME/.agents/skills. It has no
        # per-command shell deny settings, so no deny rules are written.
        if [ -n "${XDG_CONFIG_HOME:-}" ]; then
            AGENT_SETTINGS="$XDG_CONFIG_HOME/muse/settings.json"
        else
            AGENT_SETTINGS="$HOME/.config/muse/settings.json"
        fi
        SKILLS_DIR="$HOME/.agents/skills" ;;
esac
SKILLS_RECEIPT="$DATA_DIR/agent-skills-$AGENT.txt"

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

# lexical_path PATH : PATH made absolute with empty, `.` and `..` segments
# folded, without touching the filesystem ($HOME/, $HOME/., // and /. all
# come out as $HOME or /).
lexical_path() {
    case "$1" in
        /*) _lp_rest="$1/" ;;
        *) _lp_rest="$(pwd)/$1/" ;;
    esac
    _lp_out=""
    while [ -n "$_lp_rest" ]; do
        _lp_seg=${_lp_rest%%/*}
        _lp_rest=${_lp_rest#*/}
        case "$_lp_seg" in
            ''|.) ;;
            ..) _lp_out=${_lp_out%/*} ;;
            *) _lp_out="$_lp_out/$_lp_seg" ;;
        esac
    done
    printf '%s\n' "${_lp_out:-/}"
}

# physical_path DIR : the symlink-free path of an existing directory;
# fails when DIR is not one.
physical_path() {
    _pp=$(CDPATH='' cd -P -- "$1" 2>/dev/null && pwd -P) || return 1
    lexical_path "$_pp"
}

# Is PATH empty, HOME or /? Compares what the path names, not how it is
# spelled: a trailing slash, dot segments, repeated slashes and symlinks
# to either are caught. Purge deletes recursively, so either view
# matching is enough to refuse.
is_protected_path() {
    [ -n "$1" ] || return 0
    _home_lex=$(lexical_path "$HOME")
    case "$(lexical_path "$1")" in
        "/"|"$_home_lex") return 0 ;;
        *) ;;
    esac
    if _path_phys=$(physical_path "$1"); then
        _home_phys=$(physical_path "$HOME") || _home_phys=$_home_lex
        case "$_path_phys" in
            "/"|"$_home_phys") return 0 ;;
            *) ;;
        esac
    fi
    return 1
}

# Refuse to purge a workspace or data folder that is a protected path.
refuse_protected_purge() {
    if is_protected_path "$WORKSPACE"; then
        die "refusing to purge $WORKSPACE: it is a protected path"
    fi
    if is_protected_path "$DATA_DIR"; then
        die "refusing to purge the data folder $DATA_DIR: it is a protected path"
    fi
}

# Does this installer own $WORKSPACE?
# STD-03@2 §R29: ownership is recorded provenance, never a location, a
# name, or the absence of another claim. Proof is the install-state line
# this script writes (workspace_path=<path>), or the partial tree step 4
# writes before that line: only .git, the .gitignore it writes, and
# .orbit. A library entry, a bare .orbit directory, or a path that merely
# appears in `orbit workspace list` is not proof. Adoption and purge both
# use this check.
is_office_workspace() {
    if state_has "workspace_path=$WORKSPACE"; then
        return 0
    fi
    if [ -d "$WORKSPACE/.git" ] && [ -f "$WORKSPACE/.gitignore" ] \
        && grep -qx '/library/' "$WORKSPACE/.gitignore" 2>/dev/null; then
        _extra=0
        for _entry in "$WORKSPACE"/* "$WORKSPACE"/.[!.]* "$WORKSPACE"/..?*; do
            [ -e "$_entry" ] || continue
            case "$_entry" in
                "$WORKSPACE/.git"|"$WORKSPACE/.gitignore"|"$WORKSPACE/.orbit") ;;
                *) _extra=1 ;;
            esac
        done
        [ "$_extra" -eq 0 ] && return 0
    fi
    return 1
}

check_workspace_adoptable() {
    if [ -e "$WORKSPACE" ] && [ ! -d "$WORKSPACE" ]; then
        die "workspace path $WORKSPACE exists and is not a folder"
    fi
    WORKSPACE=$(lexical_path "$WORKSPACE")
    case "$WORKSPACE" in
        "$HOME"|"/"|"") die "refusing to use $WORKSPACE as the workspace" ;;
        *) ;;
    esac
    if [ -d "$WORKSPACE" ] && ! dir_is_empty "$WORKSPACE" \
        && ! is_office_workspace; then
        die "refusing to adopt non-empty folder $WORKSPACE: it is not an office workspace (move it aside or pass --home DIR)"
    fi
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
        # No terminal: never read stdin. In the documented
        # `curl ... | sh` form stdin is the installer script itself,
        # so a read here could consume the next script line and store
        # it as the secret. Skip; the message below says how to set it.
        _value=""
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
# Module: scripts/install/lib/python.sh
# ---------------------------------------------------------------------------

# python.sh — pinned uv and Python (step_python).
# Fragment of the generated install.sh: assembled by scripts/install/build-install.sh,
# never sourced at runtime. Edit this file, then regenerate install.sh.

# ---------------------------------------------------------------------------
# Pinned uv and Python (spec step 2)
# ---------------------------------------------------------------------------

sha256_file() {
    _f="$1"
    _line=""
    if command -v sha256sum >/dev/null 2>&1; then
        _line=$(sha256sum "$_f") || return 1
    elif command -v shasum >/dev/null 2>&1; then
        _line=$(shasum -a 256 "$_f") || return 1
    else
        return 2
    fi
    printf '%s\n' "${_line%% *}"
}

uv_target() {
    case "$OS:$ARCH" in
        macos:arm64) printf '%s\n' "aarch64-apple-darwin" ;;
        macos:x86_64) printf '%s\n' "x86_64-apple-darwin" ;;
        linux:x86_64) printf '%s\n' "x86_64-unknown-linux-gnu" ;;
        linux:arm64) printf '%s\n' "aarch64-unknown-linux-gnu" ;;
        *) die "unsupported platform $OS/$ARCH for the pinned uv $UV_VERSION archive" ;;
    esac
}

# URL of the archive to fetch. ORBIT_OFFICE_UV_URL is a seam for the
# harness; it does not select the expected digest.
uv_archive_url() {
    if [ -n "${ORBIT_OFFICE_UV_URL:-}" ]; then
        printf '%s\n' "$ORBIT_OFFICE_UV_URL"
        return 0
    fi
    printf 'https://github.com/astral-sh/uv/releases/download/%s/uv-%s.tar.gz\n' \
        "$UV_VERSION" "$(uv_target)"
}

# Digest recorded for this archive name. Empty when the name is not pinned.
uv_expected_sha256() {
    _base="$1"
    _base=${_base%%\?*}
    _base=${_base%%#*}
    _base=${_base%/}
    _base=${_base##*/}
    case "$_base" in
        uv-aarch64-apple-darwin.tar.gz) printf '%s\n' "$UV_SHA256_MACOS_ARM64" ;;
        uv-x86_64-apple-darwin.tar.gz) printf '%s\n' "$UV_SHA256_MACOS_X86_64" ;;
        uv-x86_64-unknown-linux-gnu.tar.gz) printf '%s\n' "$UV_SHA256_LINUX_X86_64" ;;
        uv-aarch64-unknown-linux-gnu.tar.gz) printf '%s\n' "$UV_SHA256_LINUX_AARCH64" ;;
        uv-test-fixture.tar.gz) printf '%s\n' "$UV_SHA256_FIXTURE" ;;
        *) printf '\n' ;;
    esac
}

uv_download_cleanup() {
    rm -rf "$TOOLS_DIR/.download"
    rmdir "$TOOLS_DIR" 2>/dev/null || true
}

# Install tools/bin/uv from the pinned archive. Never uses a uv already
# on PATH. Checksum failure returns before tar runs.
install_pinned_uv() {
    command -v curl >/dev/null 2>&1 || die "curl is required to download uv $UV_VERSION"
    _url=$(uv_archive_url)
    _expected=$(uv_expected_sha256 "$_url")
    if [ -z "$_expected" ]; then
        die "uv: no recorded SHA-256 for $_url; refusing to unpack"
    fi
    mkdir -p "$TOOLS_DIR"
    _work="$TOOLS_DIR/.download"
    rm -rf "$_work"
    mkdir -p "$_work"
    _archive="$_work/uv.tar.gz"
    if ! curl -fsSL "$_url" -o "$_archive"; then
        uv_download_cleanup
        die "uv: failed to download $_url"
    fi
    _sha_rc=0
    _got=$(sha256_file "$_archive") || _sha_rc=$?
    if [ "$_sha_rc" -eq 2 ]; then
        uv_download_cleanup
        die "sha256sum or shasum is required to verify the uv archive"
    fi
    if [ "$_sha_rc" -ne 0 ] || [ -z "${_got:-}" ]; then
        uv_download_cleanup
        die "uv: could not compute the SHA-256 of the downloaded archive"
    fi
    if [ "$_got" != "$_expected" ]; then
        uv_download_cleanup
        die "uv archive SHA-256 mismatch for $_url: expected $_expected, got $_got; not unpacked"
    fi
    command -v tar >/dev/null 2>&1 || {
        uv_download_cleanup
        die "tar is required to unpack the uv archive"
    }
    mkdir -p "$_work/unpack"
    if ! tar -xzf "$_archive" -C "$_work/unpack"; then
        uv_download_cleanup
        die "uv: failed to unpack the archive"
    fi
    _bin=""
    for _cand in "$_work/unpack"/*/uv "$_work/unpack"/uv; do
        if [ -f "$_cand" ] && [ ! -L "$_cand" ]; then
            _bin="$_cand"
            break
        fi
    done
    if [ -z "$_bin" ]; then
        uv_download_cleanup
        die "uv: archive contains no uv binary"
    fi
    mkdir -p "$TOOLS_BIN"
    cp "$_bin" "$TOOLS_BIN/uv.tmp.$$"
    chmod 755 "$TOOLS_BIN/uv.tmp.$$"
    mv "$TOOLS_BIN/uv.tmp.$$" "$TOOLS_BIN/uv"
    rm -rf "$_work"
}

uv_is_pinned() {
    [ -x "$TOOLS_BIN/uv" ] || return 1
    _uv_out=$("$TOOLS_BIN/uv" --version 2>/dev/null || true)
    _uv_got=$(printf '%s\n' "$_uv_out" | awk 'NR == 1 { print $2 }')
    [ "$_uv_got" = "$UV_VERSION" ]
}

# True when python3 --version prints the pinned minor (3.12 or 3.12.x).
python_version_ok() {
    [ -x "$TOOLS_BIN/python3" ] || return 1
    _py_out=$("$TOOLS_BIN/python3" --version 2>&1) || return 1
    _py_got=$(printf '%s\n' "$_py_out" | awk 'NR == 1 { print $2 }')
    case "$_py_got" in
        "$PYTHON_VERSION"|"$PYTHON_VERSION".*) return 0 ;;
        *) return 1 ;;
    esac
}

python_already_installed() {
    uv_is_pinned || return 1
    [ -e "$TOOLS_BIN/python" ] || return 1
    [ -e "$TOOLS_BIN/python3" ] || return 1
    python_version_ok
}

install_pinned_python() {
    mkdir -p "$TOOLS_PYTHON" "$TOOLS_CACHE" "$TOOLS_BIN"
    # --no-bin (also UV_PYTHON_INSTALL_BIN=0) keeps uv from linking
    # interpreters into ~/.local/bin. The cache stays under tools/.
    # Confirmed on the pinned uv: `python install --help` lists both.
    if ! UV_PYTHON_INSTALL_DIR="$TOOLS_PYTHON" \
        UV_PYTHON_INSTALL_BIN=0 \
        UV_CACHE_DIR="$TOOLS_CACHE" \
        "$TOOLS_BIN/uv" python install --no-bin --cache-dir "$TOOLS_CACHE" "$PYTHON_VERSION"
    then
        die "uv python install $PYTHON_VERSION failed"
    fi
    _py=$(UV_PYTHON_INSTALL_DIR="$TOOLS_PYTHON" \
        UV_CACHE_DIR="$TOOLS_CACHE" \
        "$TOOLS_BIN/uv" python find --no-project "$PYTHON_VERSION") || \
        die "uv python find $PYTHON_VERSION failed"
    case "$_py" in
        "$TOOLS_PYTHON"/*) ;;
        *) die "uv python find $PYTHON_VERSION returned '${_py:-nothing}', outside $TOOLS_PYTHON" ;;
    esac
    [ -x "$_py" ] || die "uv python find $PYTHON_VERSION is not executable: $_py"
    ln -sfn "$_py" "$TOOLS_BIN/python3"
    ln -sfn "$_py" "$TOOLS_BIN/python"
    if ! python_version_ok; then
        _py_out=$("$TOOLS_BIN/python3" --version 2>&1 || true)
        die "python: $TOOLS_BIN/python3 --version must print the pinned minor version $PYTHON_VERSION; got: ${_py_out:-nothing}"
    fi
    say "python: installed uv $UV_VERSION and Python $PYTHON_VERSION ($TOOLS_DIR)"
}

step_python() {
    step "2 python"
    if python_already_installed; then
        say "python: already done (uv $UV_VERSION and Python $PYTHON_VERSION in $TOOLS_DIR)"
        return 0
    fi
    # A uv on PATH is ignored; only the binary we just verified is used.
    if ! uv_is_pinned; then
        install_pinned_uv
    fi
    install_pinned_python
}


# ---------------------------------------------------------------------------
# Module: scripts/install/lib/agent.sh
# ---------------------------------------------------------------------------

# agent.sh — agent CLI registration (step_agent).
# Fragment of the generated install.sh: assembled by scripts/install/build-install.sh,
# never sourced at runtime. Edit this file, then regenerate install.sh.

# ---------------------------------------------------------------------------
# Agent CLI registration
# ---------------------------------------------------------------------------

edit_claude_settings() {
    SETTINGS_FILE="$AGENT_SETTINGS" SETTINGS_RECEIPT="$DATA_DIR/claude-settings.json" \
    MCP_FILE="$CLAUDE_CONFIG_FILE" MCP_RECEIPT="$DATA_DIR/claude-mcp.json" \
    LEGACY_FILE="$CLAUDE_LEGACY_SETTINGS" \
    SETTINGS_MODE="$1" ORBIT_OFFICE_AGENT_PATH="$AGENT_PATH" node <<'NODE'
const fs = require("fs");
const path = require("path");
const crypto = require("crypto");
const settingsFile = process.env.SETTINGS_FILE;
const settingsReceiptFile = process.env.SETTINGS_RECEIPT;
const mcpFile = process.env.MCP_FILE;
const mcpReceiptFile = process.env.MCP_RECEIPT;
const legacyFile = process.env.LEGACY_FILE;
const installing = process.env.SETTINGS_MODE === "install";
const rules = ["curl", "wget", "http", "aws", "gcloud"].map((p) => `Bash(${p} *)`);
const object = (v) => v !== null && typeof v === "object" && !Array.isArray(v);
function regular(p) {
  try {
    const stat = fs.statSync(p);
    if (!stat.isFile() || stat.uid !== process.getuid() || (stat.mode & 0o002) !== 0) {
      throw new Error("expected an owned regular file without world write access");
    }
    return true;
  } catch (e) {
    if (e.code === "ENOENT") return false;
    throw e;
  }
}
function write(p, text) {
  regular(p);
  let dest = p;
  try {
    if (fs.lstatSync(p).isSymbolicLink()) dest = fs.realpathSync(p);
  } catch (e) {
    if (e.code !== "ENOENT") throw e;
  }
  fs.mkdirSync(path.dirname(dest), { recursive: true, mode: 0o700 });
  const tmp = dest + "." + crypto.randomBytes(12).toString("hex");
  try {
    const fd = fs.openSync(tmp, "wx", 0o600);
    try { fs.writeFileSync(fd, text); fs.fsyncSync(fd); }
    finally { fs.closeSync(fd); }
    fs.renameSync(tmp, dest);
  } finally {
    if (fs.existsSync(tmp)) fs.unlinkSync(tmp);
  }
}
// A validated JSON tree with source ranges. Duplicate keys are refused.
function makeDoc(original) {
  const doc = { text: original };
  doc.tree = function () {
    if (!object(JSON.parse(doc.text))) throw new Error("settings must be an object");
    let pos = 0;
    const text = doc.text;
    const ws = () => { while (/\s/.test(text[pos] || "x")) pos++; };
    function string() {
      const start = pos++;
      while (text[pos] !== '"') { if (text[pos] === "\\") pos++; pos++; }
      pos++;
      return JSON.parse(text.slice(start, pos));
    }
    function value() {
      ws();
      const start = pos;
      const kind = text[pos];
      const entries = [];
      if (kind === "{" || kind === "[") {
        const close = kind === "{" ? "}" : "]";
        pos++; ws();
        const keys = new Set();
        while (text[pos] !== close) {
          const entryStart = pos;
          let key;
          if (kind === "{") {
            key = string(); ws(); pos++;
            if (keys.has(key)) throw new Error("duplicate key");
            keys.add(key);
          }
          const node = value();
          entries.push({ key, start: entryStart, end: node.end, node });
          ws();
          if (text[pos] === close) break;
          pos++; ws();
        }
        pos++;
      } else if (kind === '"') {
        string();
      } else {
        while (pos < text.length && !/[\s,}\]]/.test(text[pos])) pos++;
      }
      return { start, end: pos, kind, entries };
    }
    return value();
  };
  doc.locate = function (keys) {
    let node = doc.tree();
    for (const key of keys) {
      if (node.kind !== "{") throw new Error("expected object");
      const entry = node.entries.find((e) => e.key === key);
      if (!entry) return undefined;
      node = entry.node;
    }
    return node;
  };
  doc.splice = function (start, end, added) { doc.text = doc.text.slice(0, start) + added + doc.text.slice(end); };
  doc.append = function (node, encoded) {
    const last = node.entries.at(-1);
    const offset = last ? last.end : node.start + 1;
    doc.splice(offset, offset, (last ? "," : "") + encoded);
  };
  doc.set = function (keys, value) {
    const parent = doc.locate(keys.slice(0, -1));
    const key = keys.at(-1);
    const entry = parent.entries.find((e) => e.key === key);
    const encoded = JSON.stringify(value, null, 2);
    if (entry) doc.splice(entry.node.start, entry.node.end, encoded);
    else doc.append(parent, JSON.stringify(key) + ": " + encoded);
  };
  doc.removeEntry = function (parent, index) {
    const entry = parent.entries[index];
    if (index > 0) doc.splice(parent.entries[index - 1].end, entry.end, "");
    else if (parent.entries.length > 1) doc.splice(entry.start, parent.entries[1].start, "");
    else doc.splice(entry.start, entry.end, "");
  };
  doc.remove = function (keys) {
    const parent = doc.locate(keys.slice(0, -1));
    if (!parent) return;
    const index = parent.entries.findIndex((e) => e.key === keys.at(-1));
    if (index >= 0) doc.removeEntry(parent, index);
  };
  doc.ensureObject = function (keys, containers) {
    const node = doc.locate(keys);
    if (node) {
      if (node.kind !== "{") throw new Error("expected object");
      return;
    }
    doc.set(keys, {});
    containers.push(keys.join("."));
  };
  // Remove a misplaced mcpServers.orbit from a settings document; drop
  // the container only when it becomes empty.
  doc.removeMisplacedOrbit = function () {
    if (!doc.locate(["mcpServers", "orbit"])) return;
    doc.remove(["mcpServers", "orbit"]);
    const servers = doc.locate(["mcpServers"]);
    if (servers && servers.entries.length === 0) doc.remove(["mcpServers"]);
  };
  return doc;
}
try {
  let settingsReceipt = { added: [], containers: [] };
  const settingsReceiptExisted = regular(settingsReceiptFile);
  if (settingsReceiptExisted) settingsReceipt = JSON.parse(fs.readFileSync(settingsReceiptFile, "utf8"));
  if (!object(settingsReceipt) || !Array.isArray(settingsReceipt.added) || !Array.isArray(settingsReceipt.containers) ||
      settingsReceipt.added.some((r) => !rules.includes(r)) ||
      settingsReceipt.containers.some((p) => !["permissions", "permissions.deny", "mcpServers"].includes(p))) {
    throw new Error("invalid receipt");
  }
  let mcpReceipt = { owned: false, containers: [] };
  const mcpReceiptExisted = regular(mcpReceiptFile);
  if (mcpReceiptExisted) mcpReceipt = JSON.parse(fs.readFileSync(mcpReceiptFile, "utf8"));
  if (!object(mcpReceipt) || typeof mcpReceipt.owned !== "boolean" || !Array.isArray(mcpReceipt.containers) ||
      mcpReceipt.containers.some((p) => p !== "mcpServers")) {
    throw new Error("invalid mcp receipt");
  }
  const load = (file) => {
    const existed = regular(file);
    const raw = existed ? fs.readFileSync(file, "utf8") : "{}\n";
    const original = raw.trim() === "" ? "{}\n" : raw;
    return { file, existed, original, doc: makeDoc(original) };
  };
  const mcp = load(mcpFile);
  const settings = load(settingsFile);
  const legacyDistinct = legacyFile && legacyFile !== settingsFile && legacyFile !== mcpFile;
  const legacy = legacyDistinct ? load(legacyFile) : null;
  if (!installing && !mcp.existed && !settings.existed && !legacy?.existed) {
    if (mcpReceiptExisted) fs.unlinkSync(mcpReceiptFile);
    if (settingsReceiptExisted) fs.unlinkSync(settingsReceiptFile);
    process.exit(0);
  }
  // Validate every document before any edit.
  mcp.doc.tree(); settings.doc.tree(); if (legacy) legacy.doc.tree();
  if (installing) {
    // MCP registration in the file the reader checks.
    const desired = { command: "orbit", args: ["mcp", "serve"], env: { PATH: process.env.ORBIT_OFFICE_AGENT_PATH } };
    const priorOrbit = mcp.doc.locate(["mcpServers", "orbit"]);
    const priorMatches = priorOrbit &&
      JSON.stringify(JSON.parse(mcp.doc.text.slice(priorOrbit.start, priorOrbit.end))) === JSON.stringify(desired);
    if (!priorMatches) mcpReceipt.owned = true;
    mcp.doc.ensureObject(["mcpServers"], mcpReceipt.containers);
    const orbit = mcp.doc.locate(["mcpServers", "orbit"]);
    if (!orbit || JSON.stringify(JSON.parse(mcp.doc.text.slice(orbit.start, orbit.end))) !== JSON.stringify(desired)) {
      mcp.doc.set(["mcpServers", "orbit"], desired);
    }
    // Deny rules stay in settings.json.
    settings.doc.ensureObject(["permissions"], settingsReceipt.containers);
    if (!settings.doc.locate(["permissions", "deny"])) {
      settings.doc.set(["permissions", "deny"], []);
      settingsReceipt.containers.push("permissions.deny");
    }
    let deny = settings.doc.locate(["permissions", "deny"]);
    if (deny.kind !== "[") throw new Error("deny must be an array");
    const existing = JSON.parse(settings.doc.text.slice(deny.start, deny.end));
    if (existing.some((r) => typeof r !== "string")) throw new Error("deny rules must be strings");
    for (const rule of rules) {
      if (existing.includes(rule)) continue;
      deny = settings.doc.locate(["permissions", "deny"]);
      settings.doc.append(deny, JSON.stringify(rule));
      if (!settingsReceipt.added.includes(rule)) settingsReceipt.added.push(rule);
    }
    settingsReceipt.containers = settingsReceipt.containers.filter((c) => c !== "mcpServers");
    // Migrate the earlier misplaced registration out of settings files.
    settings.doc.removeMisplacedOrbit();
    if (legacy && legacy.existed) legacy.doc.removeMisplacedOrbit();
  } else {
    if (mcpReceipt.owned) {
      mcp.doc.remove(["mcpServers", "orbit"]);
      for (const key of [...mcpReceipt.containers].reverse()) {
        const keys = key.split(".");
        const node = mcp.doc.locate(keys);
        if (node && node.entries.length === 0) mcp.doc.remove(keys);
      }
    }
    for (const rule of settingsReceipt.added) {
      const deny = settings.doc.locate(["permissions", "deny"]);
      if (!deny) continue;
      if (deny.kind !== "[") throw new Error("deny must be an array");
      const index = deny.entries.findIndex((e) => JSON.parse(settings.doc.text.slice(e.start, e.end)) === rule);
      if (index >= 0) settings.doc.removeEntry(deny, index);
    }
    // Legacy misplaced entry: owned when an installer receipt proves a
    // prior installer edited that settings family.
    if (settingsReceiptExisted || mcpReceiptExisted) settings.doc.removeMisplacedOrbit();
    for (const key of [...settingsReceipt.containers].reverse()) {
      if (key === "mcpServers") continue;
      const keys = key.split(".");
      const node = settings.doc.locate(keys);
      if (node && node.entries.length === 0) settings.doc.remove(keys);
    }
    if (legacy && legacy.existed && (settingsReceiptExisted || mcpReceiptExisted)) {
      legacy.doc.removeMisplacedOrbit();
    }
  }
  mcp.doc.tree(); settings.doc.tree(); if (legacy) legacy.doc.tree();
  const persistReceipt = (file, receipt, existed) => {
    const serialized = JSON.stringify(receipt) + "\n";
    if (!existed || fs.readFileSync(file, "utf8") !== serialized) write(file, serialized);
  };
  if (installing) {
    persistReceipt(mcpReceiptFile, mcpReceipt, mcpReceiptExisted);
    persistReceipt(settingsReceiptFile, settingsReceipt, settingsReceiptExisted);
  }
  const persistDoc = (entry, createWhenMissing) => {
    if (entry.doc.text !== entry.original) {
      if (entry.existed && !regular(entry.file + ".bak")) {
        fs.writeFileSync(entry.file + ".bak", entry.original, { flag: "wx", mode: 0o600 });
      }
      write(entry.file, entry.doc.text);
    } else if (!entry.existed && installing && createWhenMissing) {
      write(entry.file, entry.doc.text);
    }
  };
  persistDoc(mcp, true); persistDoc(settings, true); if (legacy) persistDoc(legacy, false);
  if (!installing) {
    if (regular(mcpReceiptFile)) fs.unlinkSync(mcpReceiptFile);
    if (regular(settingsReceiptFile)) fs.unlinkSync(settingsReceiptFile);
  }
} catch (_) {
  process.stderr.write("agent settings update refused; existing JSON and ownership receipt must be valid regular files\n");
  process.exit(1);
}
NODE
}

register_mcp_claude() {
    edit_claude_settings install || die "agent: could not update $CLAUDE_CONFIG_FILE and $AGENT_SETTINGS (check JSON, types and file permissions)"
    state_mark "agent_registered"
    say "agent: already configured (Orbit MCP entry in $CLAUDE_CONFIG_FILE and gateway deny rules in $AGENT_SETTINGS)"
}

# True when the Codex orbit table already carries exactly the current
# generated PATH. The env map is inline on that table so uninstall,
# which stops at the next [header], removes it with the section.
codex_orbit_path_current() {
    [ -f "$AGENT_SETTINGS" ] || return 1
    awk -v want="env = { PATH = \"$AGENT_PATH\" }" '
        /^\[mcp_servers\.orbit\]$/ { in_s = 1; next }
        /^\[/ { in_s = 0 }
        in_s && $0 == want { found = 1 }
        END { exit !found }
    ' "$AGENT_SETTINGS"
}

strip_codex_orbit_section() {
    _file="$1"
    _tmp="$_file.tmp.$$"
    awk '/^\[mcp_servers\.orbit\]$/{skip=1; next} /^\[/{skip=0} !skip' "$_file" > "$_tmp"
    mv "$_tmp" "$_file"
}

register_mcp_codex() {
    _file="$AGENT_SETTINGS"
    mkdir -p "$(dirname -- "$_file")"
    if codex_orbit_path_current; then
        state_mark "agent_registered"
        say "agent: already done (Orbit MCP entry in $_file)"
        return 0
    fi
    if [ -f "$_file" ] && [ ! -f "$_file.bak" ]; then
        cp "$_file" "$_file.bak"
        say "agent: backed up $_file to $_file.bak"
    fi
    if [ -f "$_file" ] && grep -q '^\[mcp_servers\.orbit\]$' "$_file" 2>/dev/null; then
        strip_codex_orbit_section "$_file"
    fi
    if [ -s "$_file" ]; then
        printf '\n' >> "$_file"
    fi
    {
        printf '[mcp_servers.orbit]\n'
        printf 'command = "orbit"\n'
        printf 'args = ["mcp", "serve"]\n'
        printf 'env = { PATH = "%s" }\n' "$AGENT_PATH"
    } >> "$_file"
    state_mark "agent_registered"
    say "agent: added the Orbit MCP entry to $_file"
}

remove_mcp_claude() {
    if edit_claude_settings uninstall; then
        say "agent: removed the owned Orbit MCP entry from $CLAUDE_CONFIG_FILE and recorded deny rules from $AGENT_SETTINGS"
    else
        warn "agent: could not remove owned entries from $CLAUDE_CONFIG_FILE and $AGENT_SETTINGS; leaving them in place and continuing uninstall"
    fi
}

remove_mcp_codex() {
    _file="$AGENT_SETTINGS"
    [ -f "$_file" ] || { say "agent: already removed (no $_file)"; return 0; }
    if ! grep -q 'mcp_servers.orbit' "$_file" 2>/dev/null; then
        say "agent: already removed (no Orbit MCP entry)"
        return 0
    fi
    strip_codex_orbit_section "$_file"
    say "agent: removed the Orbit MCP entry from $_file"
}

# Muse Code settings: one JSON file holding mcpServers beside the user's
# other settings. Same splice discipline as the Claude editor: validate
# the whole document, edit only the mcpServers.orbit bytes, keep a .bak,
# and record ownership in muse-mcp.json so uninstall removes only an
# entry this installer wrote (a pre-existing identical entry is kept).
edit_muse_settings() {
    SETTINGS_FILE="$AGENT_SETTINGS" SETTINGS_RECEIPT="$DATA_DIR/muse-mcp.json" \
    SETTINGS_MODE="$1" ORBIT_OFFICE_AGENT_PATH="$AGENT_PATH" node <<'NODE'
const fs = require("fs");
const path = require("path");
const crypto = require("crypto");
const settingsFile = process.env.SETTINGS_FILE;
const receiptFile = process.env.SETTINGS_RECEIPT;
const installing = process.env.SETTINGS_MODE === "install";
const object = (v) => v !== null && typeof v === "object" && !Array.isArray(v);
function regular(p) {
  try {
    const stat = fs.statSync(p);
    if (!stat.isFile() || stat.uid !== process.getuid() || (stat.mode & 0o002) !== 0) {
      throw new Error("expected an owned regular file without world write access");
    }
    return true;
  } catch (e) {
    if (e.code === "ENOENT") return false;
    throw e;
  }
}
function write(p, text) {
  regular(p);
  let dest = p;
  try {
    if (fs.lstatSync(p).isSymbolicLink()) dest = fs.realpathSync(p);
  } catch (e) {
    if (e.code !== "ENOENT") throw e;
  }
  fs.mkdirSync(path.dirname(dest), { recursive: true, mode: 0o700 });
  const tmp = dest + "." + crypto.randomBytes(12).toString("hex");
  try {
    const fd = fs.openSync(tmp, "wx", 0o600);
    try { fs.writeFileSync(fd, text); fs.fsyncSync(fd); }
    finally { fs.closeSync(fd); }
    fs.renameSync(tmp, dest);
  } finally {
    if (fs.existsSync(tmp)) fs.unlinkSync(tmp);
  }
}
function makeDoc(original) {
  const doc = { text: original };
  doc.tree = function () {
    if (!object(JSON.parse(doc.text))) throw new Error("settings must be an object");
    let pos = 0;
    const text = doc.text;
    const ws = () => { while (/\s/.test(text[pos] || "x")) pos++; };
    function string() {
      const start = pos++;
      while (text[pos] !== '"') { if (text[pos] === "\\") pos++; pos++; }
      pos++;
      return JSON.parse(text.slice(start, pos));
    }
    function value() {
      ws();
      const start = pos;
      const kind = text[pos];
      const entries = [];
      if (kind === "{" || kind === "[") {
        const close = kind === "{" ? "}" : "]";
        pos++; ws();
        const keys = new Set();
        while (text[pos] !== close) {
          const entryStart = pos;
          let key;
          if (kind === "{") {
            key = string(); ws(); pos++;
            if (keys.has(key)) throw new Error("duplicate key");
            keys.add(key);
          }
          const node = value();
          entries.push({ key, start: entryStart, end: node.end, node });
          ws();
          if (text[pos] === close) break;
          pos++; ws();
        }
        pos++;
      } else if (kind === '"') {
        string();
      } else {
        while (pos < text.length && !/[\s,}\]]/.test(text[pos])) pos++;
      }
      return { start, end: pos, kind, entries };
    }
    return value();
  };
  doc.locate = function (keys) {
    let node = doc.tree();
    for (const key of keys) {
      if (node.kind !== "{") throw new Error("expected object");
      const entry = node.entries.find((e) => e.key === key);
      if (!entry) return undefined;
      node = entry.node;
    }
    return node;
  };
  doc.splice = function (start, end, added) { doc.text = doc.text.slice(0, start) + added + doc.text.slice(end); };
  doc.append = function (node, encoded) {
    const last = node.entries.at(-1);
    const offset = last ? last.end : node.start + 1;
    doc.splice(offset, offset, (last ? "," : "") + encoded);
  };
  doc.set = function (keys, value) {
    const parent = doc.locate(keys.slice(0, -1));
    const key = keys.at(-1);
    const entry = parent.entries.find((e) => e.key === key);
    const encoded = JSON.stringify(value, null, 2);
    if (entry) doc.splice(entry.node.start, entry.node.end, encoded);
    else doc.append(parent, JSON.stringify(key) + ": " + encoded);
  };
  doc.removeEntry = function (parent, index) {
    const entry = parent.entries[index];
    if (index > 0) doc.splice(parent.entries[index - 1].end, entry.end, "");
    else if (parent.entries.length > 1) doc.splice(entry.start, parent.entries[1].start, "");
    else doc.splice(entry.start, entry.end, "");
  };
  doc.remove = function (keys) {
    const parent = doc.locate(keys.slice(0, -1));
    if (!parent) return;
    const index = parent.entries.findIndex((e) => e.key === keys.at(-1));
    if (index >= 0) doc.removeEntry(parent, index);
  };
  doc.ensureObject = function (keys, containers) {
    const node = doc.locate(keys);
    if (node) {
      if (node.kind !== "{") throw new Error("expected object");
      return;
    }
    doc.set(keys, {});
    containers.push(keys.join("."));
  };
  return doc;
}
try {
  let receipt = { owned: false, containers: [] };
  const receiptExisted = regular(receiptFile);
  if (receiptExisted) receipt = JSON.parse(fs.readFileSync(receiptFile, "utf8"));
  if (!object(receipt) || typeof receipt.owned !== "boolean" || !Array.isArray(receipt.containers) ||
      receipt.containers.some((p) => p !== "mcpServers")) {
    throw new Error("invalid muse receipt");
  }
  const existed = regular(settingsFile);
  const raw = existed ? fs.readFileSync(settingsFile, "utf8") : "{}\n";
  const original = raw.trim() === "" ? "{}\n" : raw;
  const doc = makeDoc(original);
  if (!installing && !existed) {
    if (receiptExisted) fs.unlinkSync(receiptFile);
    process.exit(0);
  }
  doc.tree();
  if (installing) {
    const desired = { command: "orbit", args: ["mcp", "serve"], env: { PATH: process.env.ORBIT_OFFICE_AGENT_PATH } };
    const prior = doc.locate(["mcpServers", "orbit"]);
    const priorMatches = prior &&
      JSON.stringify(JSON.parse(doc.text.slice(prior.start, prior.end))) === JSON.stringify(desired);
    if (!priorMatches) receipt.owned = true;
    doc.ensureObject(["mcpServers"], receipt.containers);
    const orbit = doc.locate(["mcpServers", "orbit"]);
    if (!orbit || JSON.stringify(JSON.parse(doc.text.slice(orbit.start, orbit.end))) !== JSON.stringify(desired)) {
      doc.set(["mcpServers", "orbit"], desired);
    }
  } else if (receipt.owned) {
    doc.remove(["mcpServers", "orbit"]);
    for (const key of [...receipt.containers].reverse()) {
      const keys = key.split(".");
      const node = doc.locate(keys);
      if (node && node.entries.length === 0) doc.remove(keys);
    }
  }
  doc.tree();
  if (installing) {
    const serialized = JSON.stringify(receipt) + "\n";
    if (!receiptExisted || fs.readFileSync(receiptFile, "utf8") !== serialized) write(receiptFile, serialized);
  }
  if (doc.text !== original) {
    if (existed && !regular(settingsFile + ".bak")) {
      fs.writeFileSync(settingsFile + ".bak", original, { flag: "wx", mode: 0o600 });
    }
    write(settingsFile, doc.text);
  } else if (!existed && installing) {
    write(settingsFile, doc.text);
  }
  if (!installing && regular(receiptFile)) fs.unlinkSync(receiptFile);
} catch (_) {
  process.stderr.write("agent settings update refused; existing JSON and ownership receipt must be valid regular files\n");
  process.exit(1);
}
NODE
}

register_mcp_muse() {
    edit_muse_settings install || die "agent: could not update $AGENT_SETTINGS (check JSON, types and file permissions)"
    state_mark "agent_registered"
    say "agent: already configured (Orbit MCP entry in $AGENT_SETTINGS)"
}

remove_mcp_muse() {
    if edit_muse_settings uninstall; then
        say "agent: removed the owned Orbit MCP entry from $AGENT_SETTINGS"
    else
        warn "agent: could not remove owned entries from $AGENT_SETTINGS; leaving them in place and continuing uninstall"
    fi
}

# Skill ownership (STD-03@2 §R29): a matching name is not ownership.
# The receipt at $SKILLS_RECEIPT holds one "<name> <sha256>" line per
# skill this installer itself copied, where the digest covers every
# file's relative path and bytes (and every symlink's target) in the
# copied tree. Install records a skill only when it copies it; a
# pre-existing same-named skill is left alone and never recorded.
# Uninstall removes only a recorded skill whose tree still matches its
# recorded digest; anything else is kept and named in the output.

skill_name_valid() {
    case "$1" in
        ''|.|..|*[!A-Za-z0-9._-]*) return 1 ;;
        *) return 0 ;;
    esac
}

skill_digest_valid() {
    [ "${#1}" -eq 64 ] || return 1
    case "$1" in
        *[!0-9a-f]*) return 1 ;;
        *) return 0 ;;
    esac
}

# Print the digest of the tree at $1, or fail.
skill_tree_digest() {
    _tree="$1"
    [ -d "$_tree" ] && [ ! -L "$_tree" ] || return 1
    _list=$(mktemp "${TMPDIR:-/tmp}/orbit-office-skills.XXXXXX") || return 1
    _hashes=$(mktemp "${TMPDIR:-/tmp}/orbit-office-skills.XXXXXX") || { rm -f "$_list"; return 1; }
    if ! (CDPATH='' cd -- "$_tree" && find . \( -type f -o -type l \) -print | LC_ALL=C sort) > "$_list"; then
        rm -f "$_list" "$_hashes"
        return 1
    fi
    _rc=0
    : > "$_hashes"
    while IFS= read -r _rel || [ -n "$_rel" ]; do
        _path="$_tree/$_rel"
        if [ -L "$_path" ]; then
            _target=$(readlink "$_path") || _rc=1
            printf 'link %s %s\n' "$_rel" "${_target:-}" >> "$_hashes"
        elif [ -f "$_path" ]; then
            _h=$(sha256_file "$_path") || _rc=1
            printf 'file %s %s\n' "${_h:-}" "$_rel" >> "$_hashes"
        else
            _rc=1
        fi
    done < "$_list"
    _digest=""
    if [ "$_rc" -eq 0 ]; then
        _digest=$(sha256_file "$_hashes") || _rc=1
    fi
    rm -f "$_list" "$_hashes"
    [ "$_rc" -eq 0 ] && [ -n "$_digest" ] || return 1
    printf '%s\n' "$_digest"
}

# Record (or replace) the ownership line for a skill this run copied.
skill_receipt_record() {
    _name="$1"; _digest="$2"
    mkdir -p "$DATA_DIR"
    if [ -L "$SKILLS_RECEIPT" ]; then
        warn "skills: ownership receipt $SKILLS_RECEIPT is a symlink; not recording $_name"
        return 1
    fi
    _tmp="$SKILLS_RECEIPT.tmp.$$"
    if [ -f "$SKILLS_RECEIPT" ]; then
        grep -v "^$_name " "$SKILLS_RECEIPT" > "$_tmp" || true
    else
        : > "$_tmp"
    fi
    printf '%s %s\n' "$_name" "$_digest" >> "$_tmp"
    mv "$_tmp" "$SKILLS_RECEIPT"
}

# Remove recorded skills that are still exactly as installed. Keep and
# name every other skill: unrecorded, malformed, missing-digest tooling,
# a symlink or non-directory, or content changed since the copy.
remove_recorded_skills() {
    if [ -L "$SKILLS_RECEIPT" ]; then
        warn "uninstall: skills ownership receipt $SKILLS_RECEIPT is a symlink; keeping all skills"
        return 0
    fi
    if [ ! -f "$SKILLS_RECEIPT" ]; then
        say "uninstall: no skills recorded as installed by this installer; keeping any skills present"
        return 0
    fi
    _snapshot=$(mktemp "${TMPDIR:-/tmp}/orbit-office-skills.XXXXXX") || {
        warn "uninstall: could not read the skills ownership receipt; keeping all skills"
        return 0
    }
    _kept=$(mktemp "${TMPDIR:-/tmp}/orbit-office-skills.XXXXXX") || {
        rm -f "$_snapshot"
        warn "uninstall: could not read the skills ownership receipt; keeping all skills"
        return 0
    }
    cp "$SKILLS_RECEIPT" "$_snapshot"
    : > "$_kept"
    while IFS=' ' read -r _name _digest _extra || [ -n "${_name:-}" ]; do
        if ! skill_name_valid "${_name:-}" || ! skill_digest_valid "${_digest:-}" \
            || [ -n "${_extra:-}" ]; then
            warn "uninstall: ignoring a malformed skills ownership entry; keeping its skill"
            printf '%s %s%s\n' "${_name:-}" "${_digest:-}" "${_extra:+ $_extra}" >> "$_kept"
            continue
        fi
        _dest="$SKILLS_DIR/$_name"
        if [ ! -e "$_dest" ] && [ ! -L "$_dest" ]; then
            say "uninstall: skill $_name already removed"
            continue
        fi
        if [ -L "$_dest" ] || [ ! -d "$_dest" ]; then
            say "uninstall: kept skill $_name at $_dest (not a directory this installer copied)"
            printf '%s %s\n' "$_name" "$_digest" >> "$_kept"
            continue
        fi
        _current=$(skill_tree_digest "$_dest") || _current=""
        if [ -z "$_current" ]; then
            warn "uninstall: could not verify skill $_name; keeping it at $_dest"
            printf '%s %s\n' "$_name" "$_digest" >> "$_kept"
            continue
        fi
        if [ "$_current" = "$_digest" ]; then
            rm -rf "${_dest:?}"
            say "uninstall: removed skill $_name"
        else
            say "uninstall: kept modified skill $_name at $_dest (it differs from the copy this installer installed; remove it manually if you no longer want it)"
            printf '%s %s\n' "$_name" "$_digest" >> "$_kept"
        fi
    done < "$_snapshot"
    # Name same-named skills that were never recorded as ours.
    if [ -n "$PLUGIN_SRC" ] && [ -d "$PLUGIN_SRC/.orbit-plugin/skills" ] \
        && [ -d "$SKILLS_DIR" ]; then
        for _skill in "$PLUGIN_SRC/.orbit-plugin/skills"/*; do
            [ -d "$_skill" ] || continue
            _name=$(basename -- "$_skill")
            skill_name_valid "$_name" || continue
            if [ -e "$SKILLS_DIR/$_name" ] \
                && ! grep -q "^$_name " "$_snapshot" 2>/dev/null; then
                say "uninstall: kept skill $_name at $SKILLS_DIR/$_name (not recorded as installed by this installer)"
            fi
        done
    fi
    rm -f "$_snapshot"
    if [ -s "$_kept" ]; then
        mv "$_kept" "$SKILLS_RECEIPT"
    else
        rm -f "$_kept" "$SKILLS_RECEIPT"
    fi
}

step_agent() {
    step "8 agent"
    compute_agent_path
    validate_agent_path
    case "$AGENT" in
        claude) register_mcp_claude ;;
        codex) register_mcp_codex ;;
        muse) register_mcp_muse ;;
    esac
    # Office skills from the plugin tree, when this run has one.
    if [ -n "$PLUGIN_SRC" ] && [ -d "$PLUGIN_SRC/.orbit-plugin/skills" ]; then
        mkdir -p "$SKILLS_DIR"
        for _skill in "$PLUGIN_SRC/.orbit-plugin/skills"/*; do
            [ -d "$_skill" ] || continue
            _name=$(basename -- "$_skill")
            if ! skill_name_valid "$_name"; then
                warn "agent: skipping skill with an unsafe name: $_name"
                continue
            fi
            if [ -e "$SKILLS_DIR/$_name" ] || [ -L "$SKILLS_DIR/$_name" ]; then
                # Pre-existing, previously installed or user-modified:
                # never overwrite and never claim ownership here.
                say "agent: skill $_name already done"
            else
                cp -R "$_skill" "$SKILLS_DIR/$_name"
                if _digest=$(skill_tree_digest "$SKILLS_DIR/$_name") \
                    && skill_receipt_record "$_name" "$_digest"; then
                    say "agent: installed skill $_name"
                else
                    warn "agent: installed skill $_name without an ownership record; uninstall will keep it"
                fi
            fi
        done
    else
        say "agent: skills already done (no local plugin tree; orbit plugin enable links them)"
    fi
    state_mark "agent_registered"
}


# ---------------------------------------------------------------------------
# Module: scripts/install/lib/steps.sh
# ---------------------------------------------------------------------------

# steps.sh — install steps, app, uninstall and main dispatch.
# Fragment of the generated install.sh: assembled by scripts/install/build-install.sh,
# never sourced at runtime. Edit this file, then regenerate install.sh.

# ---------------------------------------------------------------------------
# Install steps
# ---------------------------------------------------------------------------

step_preflight() {
    step "1 preflight"
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
    step "3 orbit"
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

# The same standalone launcher ships in clone and curl-to-sh installs.
launcher_digest() {
    node -e 'const fs = require("fs"), crypto = require("crypto"); process.stdout.write(crypto.createHash("sha256").update(fs.readFileSync(process.argv[1])).digest("hex"));' "$1"
}

# Before the rename the launcher was ~/.local/bin/office, which shadowed or was
# shadowed by the plugin's `office` exec backend binary on PATH.
LEGACY_LAUNCHER="$HOME/.local/bin/office"

# Remove a launcher only when install-state recorded it and its bytes still
# match the recorded digest; a changed or unrecorded file is kept.
remove_recorded_launcher() {
    _rl_path="$1"; _rl_label="$2"
    if state_has "launcher_path=$_rl_path"; then
        if [ -f "$_rl_path" ] && [ ! -L "$_rl_path" ] \
            && _rl_digest=$(launcher_digest "$_rl_path" 2>/dev/null) \
            && state_has "launcher_digest=$_rl_digest"; then
            rm -f "$_rl_path"
            state_clear "launcher_digest=$_rl_digest"
            say "$_rl_label: removed the office launcher ($_rl_path)"
        elif [ -e "$_rl_path" ] || [ -L "$_rl_path" ]; then
            warn "$_rl_label: kept changed launcher path $_rl_path"
        fi
        state_clear "launcher_path=$_rl_path"
    elif [ "$_rl_label" = uninstall ] && [ -e "$_rl_path" ]; then
        say "uninstall: kept any unrecorded office launcher ($_rl_path)"
    fi
}

step_launcher() {
    remove_recorded_launcher "$LEGACY_LAUNCHER" launcher
    LAUNCHER="$HOME/.local/bin/orbit-office"
    mkdir -p "$DATA_DIR"
    _candidate="$DATA_DIR/office-launcher.$$"
    _launcher_created=0
    trap 'rm -f "$_candidate" "$LAUNCHER.install-$$"' EXIT HUP INT TERM
    cat > "$_candidate" <<'OFFICE_LAUNCHER'
#!/usr/bin/env node
// Human workspace setup. The Rust office-bin remains Orbit's exec backend.
"use strict";
const fs = require("node:fs");
const path = require("node:path");
const os = require("node:os");
const { spawnSync } = require("node:child_process");
const help = `Usage: orbit-office <command> [path] [options]

Commands:
  init [path] [--yes]   Create or repair an office workspace and its Library
  open [path]           Open the app for a workspace in the default browser,
                        starting its installed app service first if stopped
  status [path] [--json]
                        Report workspace, plugin, Library, secret, private
                        route, app service and agent CLI health

Default path: ~/orbit (or ORBIT_OFFICE_HOME; for open/status, the workspace
the installed app service is bound to when known). Plugins must already be
installed for init.

Options:
  --yes       (init) Enable plugins without the consent question
  --json      (status) Print one JSON object instead of plain words
  -h, --help  Show this help (or one command's help, e.g. orbit-office status --help)

Exit codes: 0 success, 1 setup/health/tool failure, 2 usage error.

Examples:
  orbit-office init
  orbit-office init ~/another-office --yes
  orbit-office open
  orbit-office status
  orbit-office status ~/another-office --json
`;
const initHelp = `Usage: orbit-office init [path] [--yes]

Create or repair an office workspace and its Library.
Default path: ~/orbit (or ORBIT_OFFICE_HOME). Plugins must already be installed.

Options:
  --yes       Enable plugins without the consent question (default: ask on a TTY)
  -h, --help  Show this help

Non-interactive stdin prints the grants and proceeds without reading input.
Existing non-empty folders are refused unless owned by office setup.
Exit codes: 0 success, 1 setup/tool failure, 2 usage error.

Examples:
  orbit-office init
  orbit-office init ~/another-office --yes
`;
const openHelp = `Usage: orbit-office open [path]

Open the app for a workspace in the default browser. Starts the installed
app service first when it is stopped. Without a service, exits 1 naming
install.sh. Default path: the installed workspace (see orbit-office --help).

Options:
  -h, --help  Show this help

Exit codes: 0 opened, 1 no service/mismatch/start or browser failure, 2 usage.
`;
const statusHelp = `Usage: orbit-office status [path] [--json]

Report workspace registration, office/gateway enablement, Library health
(from office.status), Meta API key presence, the private route, the app
service and its URL, its log path, and agent CLI registration. Changes nothing.

Options:
  --json      Print one JSON object with the same facts
  -h, --help  Show this help

Exit codes: 0 healthy, 1 something needed is missing, 2 usage error.
`;
const say = (message) => process.stderr.write(message + "\n");
function fail(message, code = 1) {
  say("error: " + message);
  process.exit(code);
}
const args = process.argv.slice(2);
if (args.length === 1 && ["--help", "-h"].includes(args[0])) {
  process.stdout.write(help);
  process.exit(0);
}
const command = args.shift();
if (!["init", "open", "status"].includes(command)) fail("expected 'orbit-office init', 'orbit-office open' or 'orbit-office status'; use --help", 2);
let yes = false;
let jsonOutput = false;
let target;
for (const arg of args) {
  if (command === "init" && arg === "--yes") yes = true;
  else if (command === "status" && arg === "--json") jsonOutput = true;
  else if (["--help", "-h"].includes(arg)) {
    process.stdout.write(command === "init" ? initHelp : command === "open" ? openHelp : statusHelp);
    process.exit(0);
  } else if (arg.startsWith("-")) fail("unknown option: " + arg + "; use --help", 2);
  else if (target !== undefined) fail("expected one workspace path; use --help", 2);
  else target = arg;
}
if (!process.env.HOME) fail("HOME is not set");
if (target === "") fail("workspace path must not be empty", 2);
process.umask(0o077);
// office.init is a mutating plugin tool, which Orbit runs only for an
// operator. The person running orbit-office init (or install.sh) is that
// operator, but the launcher's piped child has no TTY and resolves as an
// agent, so pass Orbit's audited override. Inside a managed Orbit run the
// caller is an agent, so the override is never added there.
function operatorEnv() {
  const managed = process.env.ORBIT_MANAGED_RUN_CONTEXT || process.env.ORBIT_TASK_ACTOR_KIND === "agent";
  return managed ? {} : { ORBIT_OPERATOR: "1" };
}
function officeWorkspaceName(workspace) {
  return "office-" + path.basename(workspace);
}
function exists(file) {
  try { return fs.lstatSync(file); }
  catch (error) { if (error.code === "ENOENT") return null; throw error; }
}
function resolvePhysical(file) {
  const absolute = path.resolve(file);
  if (exists(absolute)) return fs.realpathSync(absolute);
  const parent = path.dirname(absolute);
  return path.join(resolvePhysical(parent), path.basename(absolute));
}
function run(command, argv, cwd, required = true, extraEnv = {}) {
  const result = spawnSync(command, argv, {
    cwd, encoding: "utf8", timeout: 300000, maxBuffer: 8 * 1024 * 1024,
    stdio: ["ignore", "pipe", "pipe"],
    env: { ...process.env, ...extraEnv, PATH: path.join(process.env.HOME, ".local/share/orbit-office/tools/bin") + path.delimiter + process.env.PATH },
  });
  if (required && (result.error || result.status !== 0)) {
    if (result.stderr) process.stderr.write(result.stderr);
    fail(command + " " + argv.join(" ") + " failed" + (result.error ? ": " + result.error.message : " (exit: " + result.status + ")"));
  }
  return result;
}
function json(command, argv, cwd) {
  const result = run(command, argv, cwd);
  try {
    const rows = JSON.parse(result.stdout);
    if (!Array.isArray(rows)) throw new Error("expected an array");
    return rows;
  } catch (_) { fail("could not read JSON from " + command + " " + argv.join(" ")); }
}
function parseLoose(text) {
  function parse(value) { try { return JSON.parse(value); } catch (_) { return undefined; } }
  const trimmed = String(text || "").trim();
  let value = parse(trimmed);
  if (value !== undefined) return value;
  const lines = trimmed.split(/\r?\n/);
  for (let i = lines.length - 1; i >= 0; i--) {
    if (!lines[i].trim().startsWith("{")) continue;
    value = parse(lines[i].trim());
    if (value !== undefined) return value;
    value = parse(lines.slice(i).join("\n").trim());
    if (value !== undefined) return value;
  }
  return parse(trimmed.slice(trimmed.indexOf("{"), trimmed.lastIndexOf("}") + 1));
}
function initStatus(result) {
  const object = (value) => value && typeof value === "object" && !Array.isArray(value) ? value : null;
  const code = (value) => {
    if (!value) return "";
    if (object(value.error) && typeof value.error.code === "string" && value.error.code) return value.error.code;
    return typeof value.code === "string" ? value.code : "";
  };
  const out = object(parseLoose(result.stdout));
  if (!result.error && result.status === 0) {
    if (!out) return "unreadable";
    if (out.ok === true || !Object.hasOwn(out, "ok")) return "ok";
    return out.ok === false ? code(out) || "unreadable" : "unreadable";
  }
  return code(object(parseLoose(result.stderr))) || code(out) || "unreadable";
}
function writeAtomic(file, content) {
  const temp = file + ".office-init-" + process.pid;
  const fd = fs.openSync(temp, "wx", 0o600);
  try {
    fs.writeFileSync(fd, content);
    fs.fsyncSync(fd);
  } finally { fs.closeSync(fd); }
  try {
    fs.renameSync(temp, file);
    const parent = fs.openSync(path.dirname(file), "r");
    try { fs.fsyncSync(parent); } finally { fs.closeSync(parent); }
  } finally { if (exists(temp)) fs.unlinkSync(temp); }
}
function isMacos() {
  return ["macos", "macOS", "Darwin", "darwin"].includes(process.env.ORBIT_OFFICE_OS || "") || (!process.env.ORBIT_OFFICE_OS && process.platform === "darwin");
}
function dataDir() {
  return path.resolve(process.env.ORBIT_OFFICE_DATA_DIR || (isMacos() ? path.join(process.env.HOME, "Library/Application Support/orbit-office") : path.join(process.env.XDG_DATA_HOME || path.join(process.env.HOME, ".local/share"), "orbit-office")));
}
function readInstallState() {
  try { return fs.readFileSync(path.join(dataDir(), "install-state"), "utf8").split("\n"); }
  catch (_) { return []; }
}
function runOptional(commandName, argv, cwd) {
  return spawnSync(commandName, argv, {
    cwd, encoding: "utf8", timeout: 300000, maxBuffer: 8 * 1024 * 1024,
    stdio: ["ignore", "pipe", "pipe"],
    env: { ...process.env, PATH: path.join(process.env.HOME, ".local/share/orbit-office/tools/bin") + path.delimiter + (process.env.PATH || "") },
  });
}
function xmlUnescape(value) {
  return value.replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&quot;/g, '"').replace(/&apos;/g, "'").replace(/&amp;/g, "&");
}
function serviceInfo() {
  const macos = isMacos();
  const file = macos ? path.join(process.env.HOME, "Library/LaunchAgents/com.orbit-office.app.plist") : path.join(process.env.HOME, ".config/systemd/user/orbit-office.service");
  const installed = Boolean(exists(file) && exists(file).isFile());
  let port = Number(process.env.ORBIT_OFFICE_PORT || 4317) || 4317;
  let bound = null;
  let logPath = null;
  if (installed) {
    let text = "";
    try { text = fs.readFileSync(file, "utf8"); } catch (_) {}
    if (macos) {
      const pm = /<key>PORT<\/key>\s*<string>([^<]+)<\/string>/.exec(text);
      if (pm && Number(pm[1])) port = Number(pm[1]);
      const wm = /<key>ORBIT_OFFICE_WORKSPACE<\/key>\s*<string>([^<]*)<\/string>/.exec(text);
      if (wm) bound = xmlUnescape(wm[1]);
      const lm = /<key>Standard(?:Out|Error)Path<\/key>\s*<string>([^<]*)<\/string>/.exec(text);
      logPath = lm ? xmlUnescape(lm[1]) : path.join(dataDir(), "app.log");
    } else {
      const pm = /Environment="?PORT=([0-9]+)"?/.exec(text);
      if (pm) port = Number(pm[1]);
      const wm = /Environment="?ORBIT_OFFICE_WORKSPACE=([^"\n]+)"?/.exec(text);
      if (wm) bound = wm[1].replace(/\\"/g, '"').replace(/%%/g, "%").replace(/\\\\/g, "\\");
    }
  }
  let running = false;
  if (installed) {
    const result = macos ? runOptional("launchctl", ["list", "com.orbit-office.app"], process.env.HOME) : runOptional("systemctl", ["--user", "is-active", "orbit-office.service"], process.env.HOME);
    running = !result.error && result.status === 0;
  }
  return { installed, running, file, macos, port, boundWorkspace: bound ? resolvePhysical(bound) : null, url: "http://localhost:" + port + "/", logPath, log_path: logPath, log_file: logPath };
}
function defaultWorkspace() {
  if (target) return path.resolve(target);
  if (process.env.ORBIT_OFFICE_HOME) return path.resolve(process.env.ORBIT_OFFICE_HOME);
  const svc = serviceInfo();
  if (svc.boundWorkspace) return svc.boundWorkspace;
  const recorded = readInstallState().filter((line) => line.startsWith("workspace_path=")).map((line) => line.slice("workspace_path=".length)).filter(Boolean);
  if (recorded.length) return path.resolve(recorded[recorded.length - 1]);
  return path.join(process.env.HOME, "orbit");
}
function resolveWorkspace() {
  if (target === "") fail("workspace path must not be empty", 2);
  return resolvePhysical(defaultWorkspace());
}
function startService(svc) {
  const result = svc.macos ? runOptional("launchctl", ["load", svc.file], process.env.HOME) : runOptional("systemctl", ["--user", "start", "orbit-office.service"], process.env.HOME);
  return !result.error && result.status === 0;
}
function agentCliRegistered() {
  const mentions = (value) => typeof value === "string" && value.toLowerCase().includes("orbit");
  const checkJson = (file) => {
    try {
      const parsed = JSON.parse(fs.readFileSync(file, "utf8"));
      const servers = parsed && typeof parsed === "object" ? parsed.mcpServers : null;
      if (!servers || typeof servers !== "object") return false;
      return Object.entries(servers).some(([name, entry]) => mentions(name) || mentions(entry && entry.command) || (Array.isArray(entry && entry.args) && entry.args.some(mentions)) || mentions(entry && entry.url));
    } catch (_) { return false; }
  };
  const claudeDir = process.env.CLAUDE_CONFIG_DIR || process.env.HOME;
  if (checkJson(path.join(claudeDir, ".claude.json"))) return true;
  if (checkJson(path.join(process.env.CLAUDE_CONFIG_DIR ? path.join(process.env.CLAUDE_CONFIG_DIR, "settings.json") : path.join(process.env.HOME, ".claude", "settings.json")))) return true;
  const museDir = process.env.XDG_CONFIG_HOME || path.join(process.env.HOME, ".config");
  if (checkJson(path.join(museDir, "muse", "settings.json"))) return true;
  try {
    const toml = fs.readFileSync(path.join(process.env.CODEX_HOME || path.join(process.env.HOME, ".codex"), "config.toml"), "utf8");
    if (/\[mcp_servers[.\]]/i.test(toml) && toml.toLowerCase().includes("orbit")) return true;
  } catch (_) {}
  return false;
}
function collectStatus(workspace) {
  const problems = [];
  const add = (check, problem, fix) => problems.push({ check, problem, fix });
  const cwd = exists(workspace) ? workspace : process.env.HOME;
  let registered = false;
  // As in init: no <workspace>/.orbit means not registered.
  const wsResult = exists(path.join(workspace, ".orbit")) ? runOptional("orbit", ["workspace", "list", "--json"], cwd) : { status: 1 };
  if (!wsResult.error && wsResult.status === 0) {
    try {
      const rows = JSON.parse(wsResult.stdout);
      registered = Array.isArray(rows) && rows.some((row) => row && typeof row.repo_root === "string" && resolvePhysical(row.repo_root) === workspace && row.status === "active");
    } catch (_) {}
  }
  if (!registered) add("workspace", "workspace is not registered: " + workspace, "orbit-office init " + workspace);
  const enabled = { office: false, gateway: false };
  const pluginResult = runOptional("orbit", ["plugin", "list", "--json", "--workspace", workspace], cwd);
  if (!pluginResult.error && pluginResult.status === 0) {
    try {
      const rows = JSON.parse(pluginResult.stdout);
      if (Array.isArray(rows)) for (const name of ["office", "gateway"]) {
        const row = rows.find((plugin) => plugin && plugin.name === name);
        enabled[name] = Boolean(row && row.host_enabled === true && row.workspace_toggle === true);
      }
    } catch (_) {}
  }
  for (const name of ["office", "gateway"]) if (!enabled[name]) add("plugin_" + name, "plugin '" + name + "' is not enabled in this workspace", "orbit-office init " + workspace);
  let tool = null;
  const toolResult = runOptional("orbit", ["tool", "run", "office.status", "--json", "--input", "{}", "--workspace", workspace], cwd);
  if (!toolResult.error && toolResult.status === 0) {
    const parsed = parseLoose(toolResult.stdout);
    if (parsed && typeof parsed === "object" && !Array.isArray(parsed) && parsed.library) tool = parsed;
  }
  const library = tool && tool.library ? {
    initialized: tool.library.initialized === true,
    documents: typeof tool.library.documents === "number" ? tool.library.documents : 0,
    ...(tool.library.documents_by_kind ? { documents_by_kind: tool.library.documents_by_kind } : {}),
    ...(tool.library.pages_off_template !== undefined ? { pages_off_template: tool.library.pages_off_template } : {}),
    ...(tool.library.template_invalid !== undefined ? { template_invalid: tool.library.template_invalid } : {}),
  } : { initialized: false, documents: 0 };
  if (!library.initialized) add("library", "Library is not initialized (not scaffolded)", "orbit-office init " + workspace);
  let meta = tool && tool.secrets && typeof tool.secrets.meta_api_key === "string" ? tool.secrets.meta_api_key : null;
  if (meta !== "set" && meta !== "unset") {
    meta = "unset";
    const secretResult = runOptional("orbit", ["plugin", "secret", "list", "office"], cwd);
    if (!secretResult.error && secretResult.status === 0) {
      for (const line of String(secretResult.stdout || "").split("\n")) {
        const parts = line.trim().split(/\s+/);
        if (parts[0] === "meta_api_key" && parts.includes("set") && !parts.includes("unset")) meta = "set";
      }
    }
  } else if (meta !== "set") {
    const secretResult = runOptional("orbit", ["plugin", "secret", "list", "office"], cwd);
    if (!secretResult.error && secretResult.status === 0) {
      for (const line of String(secretResult.stdout || "").split("\n")) {
        const parts = line.trim().split(/\s+/);
        if (parts[0] === "meta_api_key" && parts.includes("set") && !parts.includes("unset")) meta = "set";
      }
    }
  }
  if (meta !== "set") add("meta_api_key", "Meta API key is not set", "orbit plugin secret set office meta_api_key");
  const privateRoute = tool && tool.open_weights !== undefined ? tool.open_weights : null;
  const svc = serviceInfo();
  if (!svc.installed) add("app_service", "app service is not installed", "install.sh");
  else if (svc.boundWorkspace && svc.boundWorkspace !== workspace) add("app_service", "app service serves " + svc.boundWorkspace + " not " + workspace, "install.sh --home " + workspace);
  else if (!svc.running) add("app_service", "app service is not running", "orbit-office open " + workspace);
  const agentRegistered = agentCliRegistered();
  if (!agentRegistered) add("agent_cli", "agent CLI registration is missing (no Orbit MCP entry)", "install.sh");
  return {
    workspace, workspace_registered: registered,
    plugins: { office: enabled.office, gateway: enabled.gateway },
    library, secrets: { meta_api_key: meta }, meta_api_key: meta,
    private_route: privateRoute,
    app_service: { installed: svc.installed, running: svc.running, url: svc.url, port: svc.port, workspace: svc.boundWorkspace, log_path: svc.logPath, log_file: svc.logPath, service_log: svc.logPath },
    app: { installed: svc.installed, running: svc.running, url: svc.url },
    agent_cli: { registered: agentRegistered },
    ok: problems.length === 0, problems,
  };
}
function statusMain() {
  if (!process.env.HOME) fail("HOME is not set");
  const workspace = resolveWorkspace();
  const report = collectStatus(workspace);
  if (jsonOutput) {
    process.stdout.write(JSON.stringify(report) + "\n");
  } else {
    const lines = [];
    lines.push("Workspace: " + report.workspace + " — " + (report.workspace_registered ? "registered" : "not registered"));
    lines.push("Plugins: office " + (report.plugins.office ? "enabled" : "not enabled") + "; gateway " + (report.plugins.gateway ? "enabled" : "not enabled"));
    lines.push("Library: " + (report.library.initialized ? "initialized (" + report.library.documents + " documents)" : "not initialized"));
    lines.push("Meta API key: " + report.meta_api_key);
    if (report.private_route && typeof report.private_route === "object") {
      lines.push("Private route: configured (" + (report.private_route.endpoint || "") + (report.private_route.model ? ", model " + report.private_route.model : "") + ")");
    } else lines.push("Private route: not configured (optional)");
    if (!report.app_service.installed) lines.push("App service: not installed");
    else if (report.app_service.running) lines.push("App service: running at " + report.app_service.url);
    else lines.push("App service: stopped (installed; not running) — " + report.app_service.url);
    if (report.app_service.log_path) lines.push("App service log: " + report.app_service.log_path);
    else if (report.app_service.installed) lines.push("App service log: journalctl --user -u orbit-office.service");
    lines.push("Agent CLI: " + (report.agent_cli.registered ? "registered" : "not registered"));
    if (report.ok) lines.push("Status: OK");
    else {
      lines.push("Problems:");
      for (const problem of report.problems) lines.push("- " + problem.problem + " — fix: " + problem.fix);
    }
    process.stdout.write(lines.join("\n") + "\n");
  }
  process.exit(report.ok ? 0 : 1);
}
function openMain() {
  if (!process.env.HOME) fail("HOME is not set");
  if (target === "") fail("workspace path must not be empty", 2);
  const workspace = resolveWorkspace();
  let svc = serviceInfo();
  if (!svc.installed) fail("app service is not installed; run install.sh to install it");
  if (svc.boundWorkspace && svc.boundWorkspace !== workspace) fail("app service serves " + svc.boundWorkspace + " not " + workspace + "; run install.sh --home " + workspace + " to install the service for this workspace");
  if (!svc.running) {
    startService(svc);
    svc = serviceInfo();
    if (!svc.running) fail("app service did not start; fix the login service and re-run, or run install.sh");
  }
  const browser = process.env.ORBIT_OFFICE_BROWSER || (svc.macos ? "open" : "xdg-open");
  const result = spawnSync(browser, [svc.url], { stdio: "ignore", env: process.env });
  if (result.error || result.status !== 0) fail("could not open " + svc.url + " with " + browser + (result.error ? ": " + result.error.message : " (exit: " + result.status + ")"));
  say("app: " + svc.url);
}
function main() {
  const requested = path.resolve(target || process.env.ORBIT_OFFICE_HOME || path.join(process.env.HOME, "orbit"));
  const entry = exists(requested);
  if (entry && (entry.isSymbolicLink() || !entry.isDirectory())) fail("workspace path " + requested + " exists and is not a folder or is a symlink");
  const workspace = resolvePhysical(requested);
  if ([resolvePhysical(process.env.HOME), path.parse(workspace).root].includes(workspace)) fail("refusing to use " + workspace + " as the workspace");
  const macos = ["macos", "macOS", "Darwin", "darwin"].includes(process.env.ORBIT_OFFICE_OS || "") || (!process.env.ORBIT_OFFICE_OS && process.platform === "darwin");
  const data = path.resolve(process.env.ORBIT_OFFICE_DATA_DIR || (macos ? path.join(process.env.HOME, "Library/Application Support/orbit-office") : path.join(process.env.XDG_DATA_HOME || path.join(process.env.HOME, ".local/share"), "orbit-office")));
  const stateFile = path.join(data, "install-state");
  for (let current = stateFile; current !== path.dirname(current); current = path.dirname(current)) {
    const stat = exists(current);
    if (stat && stat.isSymbolicLink()) fail("refusing symlink install state path: " + current);
  }
  const stateStat = exists(stateFile);
  if (stateStat && (!stateStat.isFile() || stateStat.uid !== process.getuid() || (stateStat.mode & 0o022))) fail("unsafe install state: " + stateFile);
  let state = stateStat ? fs.readFileSync(stateFile, "utf8") : "";
  const has = (line) => state.split("\n").includes(line);
  function mark(line) {
    if (has(line)) return;
    fs.mkdirSync(data, { recursive: true, mode: 0o700 });
    const next = state + (state && !state.endsWith("\n") ? "\n" : "") + line + "\n";
    writeAtomic(stateFile, next);
    state = next;
  }
  const names = entry ? fs.readdirSync(workspace) : [];
  for (const name of [".git", ".gitignore", ".orbit"]) {
    const stat = exists(path.join(workspace, name));
    if (stat && stat.isSymbolicLink()) fail("refusing symlink workspace entry: " + path.join(workspace, name));
  }
  const ignore = path.join(workspace, ".gitignore");
  const ignoreStat = exists(ignore);
  const partial = exists(path.join(workspace, ".git"))?.isDirectory() && ignoreStat?.isFile() &&
    fs.readFileSync(ignore, "utf8").split("\n").includes("/library/") && names.every((name) => [".git", ".gitignore", ".orbit"].includes(name));
  if (names.length && !has("workspace_path=" + workspace) && !partial) fail("refusing to adopt non-empty folder " + workspace + ": it is not an office workspace (move it aside or choose another path)");
  run("git", ["--version"]);
  const installed = json("orbit", ["plugin", "list", "--json"]);
  for (const name of ["office", "gateway"]) {
    if (!installed.some((plugin) => plugin.name === name)) fail("plugin '" + name + "' is not installed; run install.sh first");
  }
  const officePlugin = installed.find((plugin) => plugin.name === "office");
  if (!officePlugin?.install_path) fail("office plugin has no installed path; reinstall it");
  const presetDir = path.join(officePlugin.install_path, "presets");
  const bootstrapPreset = path.join(presetDir, "office-bootstrap.toml");
  const officePreset = path.join(presetDir, "office.toml");
  for (const preset of [bootstrapPreset, officePreset]) if (!exists(preset)?.isFile()) fail("office preset missing: " + preset + "; reinstall the office plugin");
  const plugins = ["office", "gateway", ...(installed.some((plugin) => plugin.name === "cloudcli") ? ["cloudcli"] : [])];
  say("==> 4 workspace");
  fs.mkdirSync(workspace, { recursive: true, mode: 0o700 });
  if (!exists(path.join(workspace, ".git"))) {
    const result = run("git", ["init", "-b", "main"], workspace, false);
    if (result.status !== 0) {
      run("git", ["init"], workspace);
      run("git", ["symbolic-ref", "HEAD", "refs/heads/main"], workspace);
    }
    say("workspace: ran git init -b main (" + workspace + ")");
  } else say("workspace: git repository already done (" + workspace + ")");
  const oldIgnore = exists(ignore) ? fs.readFileSync(ignore, "utf8") : "";
  if (!oldIgnore.split("\n").includes("/library/")) writeAtomic(ignore, oldIgnore + (oldIgnore && !oldIgnore.endsWith("\n") ? "\n" : "") + "/library/\n");
  const login = os.userInfo().username || "orbit";
  for (const [key, fallback] of [["user.name", login], ["user.email", login + "@localhost"]]) {
    if (!run("git", ["config", key], workspace, false).stdout.trim()) run("git", ["config", key, fallback], workspace);
  }
  if (run("git", ["rev-parse", "--verify", "HEAD"], workspace, false).status !== 0) {
    run("git", ["add", ".gitignore"], workspace);
    run("git", ["commit", "-m", "Initial commit"], workspace);
    say("workspace: made the first commit");
  } else say("workspace: first commit already done");
  // `orbit workspace init` creates <workspace>/.orbit, so a folder without
  // one is not registered. Asking Orbit from such a folder can fail before
  // registration (a read-only join with no binding), so skip the query.
  const registrations = exists(path.join(workspace, ".orbit")) ? json("orbit", ["workspace", "list", "--json"], workspace) : [];
  if (!registrations.some((row) => typeof row.repo_root === "string" && resolvePhysical(row.repo_root) === workspace && row.status === "active")) {
    mark("preset_pending=" + workspace);
    // Orbit names a workspace after its folder, so ~/orbit would claim
    // `ws_orbit` and collide with a code workspace of that name. Office
    // workspaces get their own prefix.
    run("orbit", ["workspace", "init", "--preset", bootstrapPreset, "--name", officeWorkspaceName(workspace)], workspace);
    say("workspace: registered with office seeding policy");
  } else say("workspace: already registered (" + workspace + ")");
  mark("workspace_path=" + workspace);
  say("==> 5 workspace plugins");
  const rows = json("orbit", ["plugin", "list", "--json", "--workspace", workspace], workspace);
  const grants = {
    office: ["fs={{workspace}},{{plugin_state}}", "network", "orbit_tools"],
    gateway: ["fs={{workspace}}/.orbit-gateway,{{plugin_state}}", "network", "orbit_tools"],
    cloudcli: ["fs={{plugin_state}}", "network"],
  };
  const needsHost = (row, name) => row.host_enabled !== true || grants[name].some((grant) => !row.permissions?.some((permission) => permission.grant === grant.split("=")[0] && permission.granted === true));
  if (plugins.some((name) => { const row = rows.find((plugin) => plugin.name === name); return !row || needsHost(row, name) || row.workspace_toggle !== true; })) {
    say("The plugins " + plugins.join(" ") + " request these grants:");
    say("  office: fs {{workspace}}, {{plugin_state}}; network unscoped; orbit_tools task, friction, search and auto-task");
    say("  gateway: fs {{workspace}}/.orbit-gateway, {{plugin_state}}; network unscoped; orbit_tools task add/artifact/show/update");
    if (plugins.includes("cloudcli")) say("  cloudcli: fs {{plugin_state}}; network unscoped; programs python3, aws, gcloud");
    if (yes) say("Consent: --yes given, enabling " + plugins.join(" ") + ".");
    else if (require("node:tty").isatty(0)) {
      process.stderr.write("Enable plugins " + plugins.join(" ") + " with these grants? [y/N] ");
      let answer = "";
      const byte = Buffer.alloc(1);
      while (fs.readSync(0, byte, 0, 1, null) && byte[0] !== 10) answer += byte.toString();
      if (!["y", "yes"].includes(answer.trim().toLowerCase())) fail("plugin enable declined; re-run with --yes to consent");
    } else say("Consent: non-interactive run, enabling " + plugins.join(" ") + " (pass --yes to confirm explicitly).");
  }
  for (const name of plugins) {
    const row = rows.find((plugin) => plugin.name === name);
    if (!row) fail("plugin disappeared during setup: " + name);
    if (needsHost(row, name)) run("orbit", ["plugin", "enable", name, ...grants[name].flatMap((grant) => ["--grant", grant]), "--workspace", workspace], workspace);
    if (row.workspace_toggle !== true) run("orbit", ["plugin", "enable", name, "--scope", "workspace", "--workspace", workspace], workspace);
    mark(name === "office" ? "plugin_enabled" : "plugin." + name + ".enabled");
    say("plugin: already enabled or repaired (" + name + ", " + workspace + ")");
  }
  if (has("preset_pending=" + workspace) && !has("preset_applied=" + workspace)) {
    run("orbit", ["workspace", "init", "--force", "--preset", officePreset, "--name", officeWorkspaceName(workspace)], workspace);
    mark("preset_applied=" + workspace);
    say("workspace: office preset applied");
  }
  say("==> 7 scaffold");
  const result = run("orbit", ["tool", "run", "office.init", "--json", "--full", "--input", "{}", "--workspace", workspace], workspace, false, operatorEnv());
  if (result.stderr) process.stderr.write(result.stderr);
  const status = initStatus(result);
  if (result.error || result.status !== 0 || status !== "ok") fail("scaffold: office.init failed (code: " + status + ", exit: " + result.status + ")");
  mark("scaffolded");
  say("scaffold: already initialized or repaired (office.init, " + workspace + "/library)");
}
try { if (command === "open") openMain(); else if (command === "status") statusMain(); else main(); } catch (error) { fail(error.message); }
OFFICE_LAUNCHER
    if [ -e "$LAUNCHER" ] || [ -L "$LAUNCHER" ]; then
        [ -f "$LAUNCHER" ] && [ ! -L "$LAUNCHER" ] \
            || die "refusing to replace unrelated launcher $LAUNCHER"
        if state_has "launcher_path=$LAUNCHER"; then
            _old_digest=$(launcher_digest "$LAUNCHER")
            state_has "launcher_digest=$_old_digest" \
                || die "refusing to replace modified launcher $LAUNCHER (move it aside and re-run)"
        else
            cmp -s "$_candidate" "$LAUNCHER" && [ -x "$LAUNCHER" ] \
                || die "refusing to replace unrelated launcher $LAUNCHER (move it aside and re-run)"
        fi
    else
        mkdir -p "$(dirname -- "$LAUNCHER")"
        _launcher_created=1
    fi
    if [ ! -f "$LAUNCHER" ] || ! cmp -s "$_candidate" "$LAUNCHER"; then
        # Stage next to the destination so replacement is atomic.
        cp "$_candidate" "$LAUNCHER.install-$$"
        chmod 755 "$LAUNCHER.install-$$"
        mv "$LAUNCHER.install-$$" "$LAUNCHER"
    fi
    if [ "$_launcher_created" -eq 1 ] || state_has "launcher_path=$LAUNCHER"; then
        _new_digest=$(launcher_digest "$LAUNCHER")
        state_mark "launcher_digest=$_new_digest"
        state_mark "launcher_path=$LAUNCHER"
        if [ -n "${_old_digest:-}" ] && [ "$_old_digest" != "$_new_digest" ]; then
            state_clear "launcher_digest=$_old_digest"
        fi
    fi
    rm -f "$_candidate"
    trap - EXIT HUP INT TERM
    say "office: launcher already installed or updated ($LAUNCHER)"
}

step_workspace_init() {
    if [ "$ASSUME_YES" -eq 1 ]; then
        PATH="$TOOLS_BIN:$PATH" "$LAUNCHER" init "$WORKSPACE" --yes
    else
        PATH="$TOOLS_BIN:$PATH" "$LAUNCHER" init "$WORKSPACE"
    fi
}

plugin_is_listed() {
    command -v orbit >/dev/null 2>&1 || return 2
    _list=$(orbit plugin list 2>/dev/null) || return 2
    printf '%s\n' "$_list" | awk -v name="$1" '$1 == name { found=1 } END { exit !found }'
}

plugin_marker() {
    # Keep the original office markers for upgrades from older installs.
    if [ "$1" = "office" ]; then printf 'plugin_%s' "$2";
    else printf 'plugin.%s.%s' "$1" "$2"; fi
}

# gateway and cloudcli have no build step. In a clone, assemble the same
# export as gateway/scripts/export.py, including the shared Python package.
# A release bundle already has this layout and is used without rebuilding.
gateway_export() {
    if [ -z "$SCRIPT_DIR" ]; then
        printf '%s\n' "plugin export unavailable; run from a clone or set ORBIT_OFFICE_PLUGIN_BUNDLE" >&2
        return 1
    fi
    EXPORT_REPO="$SCRIPT_DIR" EXPORT_OUT="$DATA_DIR/plugins/$1" EXPORT_NAME="$1" node <<'NODE'
const fs = require("fs");
const path = require("path");
const repo = process.env.EXPORT_REPO;
const out = process.env.EXPORT_OUT;
const name = process.env.EXPORT_NAME;
if (!repo) {
  process.stderr.write("plugin export unavailable; run from a clone or set ORBIT_OFFICE_PLUGIN_BUNDLE\n");
  process.exit(1);
}
const source = path.join(repo, "gateway", "plugins", name, ".orbit-plugin");
const shared = path.join(repo, "gateway", "src", "orbit_gateway");
if (!fs.existsSync(path.join(source, "plugin.yaml")) || !fs.existsSync(shared)) {
  process.stderr.write("plugin export unavailable; run from a clone or set ORBIT_OFFICE_PLUGIN_BUNDLE\n");
  process.exit(1);
}
if (fs.existsSync(out)) {
  process.stderr.write("incomplete plugin export; move aside " + out + " and retry\n");
  process.exit(1);
}
const filter = (p) => !["__pycache__", ".DS_Store"].includes(path.basename(p)) && !p.endsWith(".pyc");
fs.mkdirSync(out, { recursive: true, mode: 0o700 });
try {
  fs.cpSync(source, path.join(out, ".orbit-plugin"), { recursive: true, filter });
  fs.cpSync(shared, path.join(out, ".orbit-plugin", "lib", "orbit_gateway"), { recursive: true, filter });
} catch (_) {
  fs.rmSync(out, { recursive: true, force: true });
  process.stderr.write("could not assemble plugin export\n");
  process.exit(1);
}
NODE
}

# Build office from the clone's HEAD into the same layout `make -C plugin
# export` produces. Orbit's git+ sources need `.orbit-plugin/` at the
# repository root, and this repository keeps it under plugin/, so office is
# built here and added as a directory. A throwaway local clone pins HEAD and
# keeps uncommitted and untracked files out of the export.
office_export() {
    _oe_out="$DATA_DIR/plugins/office"
    command -v cargo >/dev/null 2>&1 \
        || die "building the office plugin needs cargo (install Rust from https://rustup.rs), or set ORBIT_OFFICE_PLUGIN_BUNDLE"
    [ ! -e "$_oe_out" ] || die "incomplete plugin export; move aside $_oe_out and retry"
    _oe_rev=$(git -C "$SCRIPT_DIR" rev-parse --verify 'HEAD^{commit}' 2>/dev/null) \
        || die "--from-source needs a clone; otherwise set ORBIT_OFFICE_PLUGIN_BUNDLE"
    _oe_work=$(mktemp -d "${TMPDIR:-/tmp}/office-plugin-build.XXXXXX") || return 1
    say "plugin: building office from $_oe_rev (cargo build --release; this takes a few minutes)"
    if git clone --quiet --no-hardlinks --no-checkout "$SCRIPT_DIR" "$_oe_work/source" \
        && git -C "$_oe_work/source" checkout --quiet --detach "$_oe_rev" \
        && (cd "$_oe_work/source" && cargo build --release --locked -p office --target-dir "$_oe_work/target") \
        && mkdir -p "$DATA_DIR/plugins" \
        && cp -R "$_oe_work/source/plugin" "$_oe_out.build-$$" \
        && cp "$_oe_work/target/release/office" "$_oe_out.build-$$/.orbit-plugin/bin/office-bin" \
        && chmod 755 "$_oe_out.build-$$/.orbit-plugin/bin/office-bin" \
        && mv "$_oe_out.build-$$" "$_oe_out"; then
        rm -rf "$_oe_work"
        return 0
    fi
    rm -rf "$_oe_work" "$_oe_out.build-$$"
    return 1
}

step_plugin() {
    step "5 plugin"
    if [ -n "${ORBIT_OFFICE_PLUGIN_BUNDLE:-}" ]; then
        for _plugin in $INSTALL_PLUGINS; do
            [ -f "$ORBIT_OFFICE_PLUGIN_BUNDLE/$_plugin/.orbit-plugin/plugin.yaml" ] \
                || die "missing $_plugin export in ORBIT_OFFICE_PLUGIN_BUNDLE"
        done
    fi
    for _plugin in $INSTALL_PLUGINS; do
        _added=$(plugin_marker "$_plugin" added)
        if plugin_is_listed "$_plugin" || state_has "$_added"; then
            say "plugin: already added ($_plugin)"
        else
            if [ -n "${ORBIT_OFFICE_PLUGIN_BUNDLE:-}" ]; then
                _source="$ORBIT_OFFICE_PLUGIN_BUNDLE/$_plugin"
                [ -f "$_source/.orbit-plugin/plugin.yaml" ] || die "missing $_plugin export in ORBIT_OFFICE_PLUGIN_BUNDLE"
            elif [ "$_plugin" != "office" ]; then
                _source="$DATA_DIR/plugins/$_plugin"
                # Reuse a completed export when add failed on an earlier run.
                if ! state_has "plugin.$_plugin.exported"; then
                    gateway_export "$_plugin" || die "could not prepare $_plugin export"
                    state_mark "plugin.$_plugin.exported"
                fi
            elif [ -n "${ORBIT_OFFICE_PLUGIN_SOURCE:-}" ]; then
                _source="$ORBIT_OFFICE_PLUGIN_SOURCE"
            elif [ "$FROM_SOURCE" -eq 1 ] || [ -z "$PLUGIN_SRC" ] \
                || [ ! -x "$PLUGIN_SRC/.orbit-plugin/bin/office-bin" ]; then
                if [ -z "$SCRIPT_DIR" ] || [ ! -e "$SCRIPT_DIR/.git" ]; then
                    die "--from-source needs a clone; otherwise set ORBIT_OFFICE_PLUGIN_BUNDLE"
                fi
                _source="$DATA_DIR/plugins/office"
                # Reuse a completed export when add failed on an earlier run.
                if ! state_has "plugin.office.exported"; then
                    office_export || die "could not build the office plugin from $SCRIPT_DIR"
                    state_mark "plugin.office.exported"
                fi
            else
                _source="$PLUGIN_SRC"
            fi
            if [ "$_plugin" = office ] && [ ! -x "$_source/.orbit-plugin/bin/office-bin" ]; then
                die "office plugin export lacks executable bin/office-bin: $_source"
            fi
            orbit plugin add "$_source"
            state_mark "$_added"
            say "plugin: added $_plugin from $_source"
        fi
    done
}

step_secrets() {
    step "6 secrets"
    set_secret "meta_api_key" "${ORBIT_OFFICE_META_API_KEY:-${META_API_KEY:-}}" 0
    # STD-01@2 §R35: the retired names stay accepted for one release as
    # aliases of their replacements, each use warning on stderr. The
    # warning names the variable only; its value is never echoed.
    _open_weights_token="${ORBIT_OFFICE_OPEN_WEIGHTS_TOKEN:-${OPEN_WEIGHTS_TOKEN:-}}"
    if [ -n "${ORBIT_OFFICE_BASETEN_TOKEN:-}" ]; then
        warn "ORBIT_OFFICE_BASETEN_TOKEN is deprecated; use ORBIT_OFFICE_OPEN_WEIGHTS_TOKEN instead"
        if [ -z "$_open_weights_token" ]; then
            _open_weights_token="$ORBIT_OFFICE_BASETEN_TOKEN"
        fi
    fi
    if [ -n "${BASETEN_TOKEN:-}" ]; then
        warn "BASETEN_TOKEN is deprecated; use OPEN_WEIGHTS_TOKEN instead"
        if [ -z "$_open_weights_token" ]; then
            _open_weights_token="$BASETEN_TOKEN"
        fi
    fi
    set_secret "open_weights_token" "$_open_weights_token" 1
    _open_weights_token=""
}

# Register the Orbit MCP entry with the agent CLI. Node is a proven
# prerequisite, so JSON edits go through it; the old file is backed up
# once to <settings>.bak. The entry is "already done" only when it
# matches the desired command, args and PATH. A marker from an older
# install is not enough: that install had no tools/bin on PATH.
# JSON.parse validates the document; the range parser below locates entries
# so edits splice only their bytes. Never round-trip unrelated user settings.
# Receipts contain only owned items: claude-settings.json holds added deny
# rule names and newly created containers; claude-mcp.json holds whether
# the MCP entry in .claude.json is installer-owned and its new containers.

# ---------------------------------------------------------------------------
# App, uninstall and main
# ---------------------------------------------------------------------------

# A provisioned app is only usable when the service's entrypoint exists,
# is non-empty and parses under node. The login service is never written
# or enabled for an app that fails this check.
app_entrypoint_ok() {
    _entry="$1/packages/server/dist/main.js"
    [ -f "$_entry" ] && [ -s "$_entry" ] || return 1
    command -v node >/dev/null 2>&1 || return 1
    node --check "$_entry" >/dev/null 2>&1
}

step_app() {
    step "9 app"
    compute_agent_path
    validate_agent_path
    if ! _node_bin=$(stable_node_path); then
        _raw_node=$(command -v node 2>/dev/null || printf 'node')
        die "app: node resolves to a per-shell shim ($_raw_node) under a fnm_multishells directory, which fnm removes when its shell exits, so the login service would not start; install a stable Node.js 20+ (a fnm default install, nvm, or the nodejs.org tarball) and put its bin directory, not a fnm_multishells path, first on PATH, then re-run"
    fi
    if [ -n "$APP_SRC" ]; then
        _app_home="$APP_SRC"
        if app_entrypoint_ok "$APP_SRC"; then
            say "app: build already done"
        else
            npm --prefix "$APP_SRC" run build >/dev/null
            app_entrypoint_ok "$APP_SRC" \
                || die "app: build did not produce a runnable entrypoint at $APP_SRC/packages/server/dist/main.js; run 'npm --prefix $APP_SRC run build' to see the failure, then re-run"
            state_mark "app_built"
            say "app: built ($APP_SRC)"
        fi
    elif [ -n "${ORBIT_OFFICE_APP_BUNDLE:-}" ]; then
        _bundle="${ORBIT_OFFICE_APP_BUNDLE%/}"
        [ -d "$_bundle" ] \
            || die "app: ORBIT_OFFICE_APP_BUNDLE is not a directory: ${ORBIT_OFFICE_APP_BUNDLE}; supply an unpacked app bundle containing packages/server/dist/main.js, or run install.sh from a clone beside app/package.json, then re-run"
        # The bundle is the app root itself; a bundle that wraps it in a
        # single app/ folder is accepted as the same layout.
        if ! app_entrypoint_ok "$_bundle" && app_entrypoint_ok "$_bundle/app"; then
            _bundle="$_bundle/app"
        fi
        app_entrypoint_ok "$_bundle" \
            || die "app: ORBIT_OFFICE_APP_BUNDLE (${ORBIT_OFFICE_APP_BUNDLE}) has no runnable packages/server/dist/main.js; supply an unpacked app bundle containing packages/server/dist/main.js and its runtime files, or run install.sh from a clone beside app/package.json, then re-run"
        _app_home="$DATA_DIR/app"
        if app_entrypoint_ok "$_app_home"; then
            _app_already=1
        else
            _app_already=0
        fi
        if [ "$_bundle" != "$_app_home" ]; then
            mkdir -p "$_app_home"
            cp -R "$_bundle/." "$_app_home/" \
                || die "app: could not copy the app bundle from $_bundle to $_app_home"
        fi
        app_entrypoint_ok "$_app_home" \
            || die "app: the app bundle did not provision a runnable entrypoint at $_app_home/packages/server/dist/main.js; supply a complete app bundle, then re-run"
        if [ "$_app_already" -eq 1 ]; then
            say "app: build already done (app at $_app_home)"
        else
            say "app: installed from ORBIT_OFFICE_APP_BUNDLE ($_bundle -> $_app_home)"
        fi
    else
        _app_home="$DATA_DIR/app"
        if app_entrypoint_ok "$_app_home"; then
            say "app: build already done (app at $_app_home)"
        else
            die "app: no local app tree and no app bundle: missing $_app_home/packages/server/dist/main.js; run install.sh from a clone (beside app/package.json) or set ORBIT_OFFICE_APP_BUNDLE to an unpacked app bundle containing packages/server/dist/main.js, then re-run"
        fi
    fi
    app_entrypoint_ok "$_app_home" \
        || die "app: missing or unrunnable entrypoint at $_app_home/packages/server/dist/main.js; provision the app (clone build or ORBIT_OFFICE_APP_BUNDLE) before installing the service, then re-run"
    _server_js="$_app_home/packages/server/dist/main.js"
    # Workspace binding for the app (ORBIT_OFFICE_WORKSPACE): the absolute
    # workspace path recorded as workspace_path. It names the workspace
    # only; it grants no operator capability (see client.ts authority note).
    # The value expands inside unquoted heredocs, so escape it for the
    # target format: XML for the plist, and for systemd a double-quoted
    # Environment= assignment with backslash, quote and percent escaped
    # (percent is doubled to escape systemd specifiers).
    _workspace_xml=$(printf '%s' "$WORKSPACE" | sed -e 's/&/\&amp;/g' -e 's/</\&lt;/g' -e 's/>/\&gt;/g')
    _workspace_systemd=$(printf '%s' "$WORKSPACE" | sed -e 's/\\/\\\\/g' -e 's/%/%%/g' -e 's/"/\\"/g')
    _service_log="$SERVICE_LOG"
    _service_log_xml=$(printf '%s' "$_service_log" | sed -e 's/&/\&amp;/g' -e 's/</\&lt;/g' -e 's/>/\&gt;/g')
    if [ "$OS" = "macos" ]; then
        mkdir -p "$DATA_DIR"
        [ -f "$_service_log" ] || (umask 077; : > "$_service_log")
        chmod 600 "$_service_log"
        if write_if_changed "$LAUNCHD_PLIST" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>com.orbit-office.app</string>
  <key>ProgramArguments</key>
  <array><string>$_node_bin</string><string>$_server_js</string></array>
  <key>EnvironmentVariables</key>
  <dict><key>PORT</key><string>$PORT</string><key>HOST</key><string>127.0.0.1</string><key>PATH</key><string>$AGENT_PATH</string><key>ORBIT_OFFICE_WORKSPACE</key><string>$_workspace_xml</string></dict>
  <key>WorkingDirectory</key><string>$_workspace_xml</string>
  <key>StandardOutPath</key><string>$_service_log_xml</string>
  <key>StandardErrorPath</key><string>$_service_log_xml</string>
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
            if ! command -v launchctl >/dev/null 2>&1; then
                state_clear "service_installed"
                die "service: launchctl is not available; cannot install the login agent $LAUNCHD_PLIST (re-run on macOS with launchd)"
            fi
            launchctl unload "$LAUNCHD_PLIST" >/dev/null 2>&1 || true
            if ! launchctl load "$LAUNCHD_PLIST" >/dev/null 2>&1; then
                state_clear "service_installed"
                die "service: launchctl load $LAUNCHD_PLIST failed; fix launchd and re-run to retry"
            fi
            if ! launchctl list com.orbit-office.app >/dev/null 2>&1; then
                state_clear "service_installed"
                die "service: launchd agent com.orbit-office.app is not loaded after load; fix launchd and re-run to retry"
            fi
            state_mark "service_installed"
            say "app: service installed (login agent com.orbit-office.app)"
        else
            if ! command -v launchctl >/dev/null 2>&1; then
                state_clear "service_installed"
                die "service: launchctl is not available; cannot verify the login agent $LAUNCHD_PLIST (re-run on macOS with launchd)"
            fi
            if launchctl list com.orbit-office.app >/dev/null 2>&1; then
                say "app: service already done ($LAUNCHD_PLIST)"
            else
                launchctl unload "$LAUNCHD_PLIST" >/dev/null 2>&1 || true
                if ! launchctl load "$LAUNCHD_PLIST" >/dev/null 2>&1; then
                    state_clear "service_installed"
                    die "service: launchctl load $LAUNCHD_PLIST failed; fix launchd and re-run to retry"
                fi
                if ! launchctl list com.orbit-office.app >/dev/null 2>&1; then
                    state_clear "service_installed"
                    die "service: launchd agent com.orbit-office.app is not loaded after load; fix launchd and re-run to retry"
                fi
                say "app: service re-enabled (login agent com.orbit-office.app)"
            fi
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
Environment=PATH=$AGENT_PATH
Environment="ORBIT_OFFICE_WORKSPACE=$_workspace_systemd"
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
            if ! command -v systemctl >/dev/null 2>&1; then
                state_clear "service_installed"
                die "service: systemctl is not available; cannot install the user unit $SYSTEMD_UNIT (re-run with systemd --user)"
            fi
            if ! systemctl --user daemon-reload >/dev/null 2>&1; then
                state_clear "service_installed"
                die "service: systemctl --user daemon-reload failed; fix systemd and re-run to retry"
            fi
            if ! systemctl --user enable "$SERVICE_NAME.service" >/dev/null 2>&1; then
                state_clear "service_installed"
                die "service: systemctl --user enable $SERVICE_NAME.service failed; fix systemd and re-run to retry"
            fi
            if ! systemctl --user restart "$SERVICE_NAME.service" >/dev/null 2>&1; then
                if ! systemctl --user start "$SERVICE_NAME.service" >/dev/null 2>&1; then
                    state_clear "service_installed"
                    die "service: systemctl --user start $SERVICE_NAME.service failed; fix systemd and re-run to retry"
                fi
            fi
            if ! systemctl --user is-active "$SERVICE_NAME.service" >/dev/null 2>&1; then
                state_clear "service_installed"
                die "service: $SERVICE_NAME.service is not active after start; fix systemd and re-run to retry"
            fi
            state_mark "service_installed"
            say "app: service installed (login service $SERVICE_NAME)"
        else
            if ! command -v systemctl >/dev/null 2>&1; then
                state_clear "service_installed"
                die "service: systemctl is not available; cannot verify the user unit $SYSTEMD_UNIT (re-run with systemd --user)"
            fi
            if systemctl --user is-active "$SERVICE_NAME.service" >/dev/null 2>&1; then
                say "app: service already enabled ($SERVICE_NAME)"
            else
                if ! systemctl --user restart "$SERVICE_NAME.service" >/dev/null 2>&1; then
                    if ! systemctl --user start "$SERVICE_NAME.service" >/dev/null 2>&1; then
                        state_clear "service_installed"
                        die "service: systemctl --user start $SERVICE_NAME.service failed; fix systemd and re-run to retry"
                    fi
                fi
                if ! systemctl --user is-active "$SERVICE_NAME.service" >/dev/null 2>&1; then
                    state_clear "service_installed"
                    die "service: $SERVICE_NAME.service is not active after start; fix systemd and re-run to retry"
                fi
                say "app: service re-enabled (login service $SERVICE_NAME)"
            fi
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
    # Prove ownership before any teardown. A refusal must leave the
    # service, plugin, tools, workspace and data folder in place.
    if [ "$_purge" -eq 1 ]; then
        refuse_protected_purge
        if ! is_office_workspace; then
            die "refusing to purge $WORKSPACE: it is not an office workspace this installer owns"
        fi
    fi
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
        muse) remove_mcp_muse ;;
    esac
    # Skills this installer copied in, proven by the receipt, never by
    # a same-named directory in the current plugin tree.
    remove_recorded_skills
    # Uninstall also finds optional plugins from an earlier install without
    # requiring --with-cloudcli again.
    for _plugin in office gateway cloudcli; do
        _added=$(plugin_marker "$_plugin" added)
        _owned=0
        if state_has "$_added" || state_has "$(plugin_marker "$_plugin" enabled)"; then
            _owned=1
        elif [ "$_plugin" = office ] && plugin_is_listed office; then
            _owned=1
        fi
        if [ "$_owned" -eq 1 ]; then
            command -v orbit >/dev/null 2>&1 || die "uninstall: Orbit is required to remove $_plugin"
            if plugin_is_listed "$_plugin"; then
                orbit plugin remove "$_plugin" --yes >/dev/null 2>&1 \
                    || die "uninstall: could not remove $_plugin; retry when Orbit is available"
                say "uninstall: removed the $_plugin plugin"
            else
                _list_status=$?
                [ "$_list_status" -eq 1 ] \
                    || die "uninstall: could not list Orbit plugins; retry when Orbit is available"
                say "uninstall: $_plugin plugin already removed"
            fi
        fi
        state_clear "$_added"
        state_clear "$(plugin_marker "$_plugin" enabled)"
        if state_has "plugin.$_plugin.exported"; then
            rm -rf "${DATA_DIR:?}/plugins/$_plugin"
            state_clear "plugin.$_plugin.exported"
        fi
    done
    remove_recorded_launcher "$HOME/.local/bin/orbit-office" uninstall
    remove_recorded_launcher "$LEGACY_LAUNCHER" uninstall
    # tools/ is not the data folder. On Linux the data folder is its
    # parent; on macOS they are different trees. Remove only tools/.
    case "$TOOLS_DIR" in
        "$HOME/.local/share/orbit-office/tools")
            if [ -e "$TOOLS_DIR" ]; then
                rm -rf "$TOOLS_DIR"
                say "uninstall: removed tools ($TOOLS_DIR)"
            else
                say "uninstall: tools already removed"
            fi
            ;;
        *) die "refusing to remove unexpected tools path $TOOLS_DIR" ;;
    esac
    state_clear "plugin_added"
    state_clear "plugin_enabled"
    state_clear "service_installed"
    state_clear "app_built"
    state_clear "agent_registered"
    if [ "$_purge" -eq 1 ]; then
        # The check above already ran. Repeat the protected-path guard
        # immediately before the deletes so HOME, / and an empty path
        # cannot be removed even if that earlier check is bypassed or
        # teardown created a path that now resolves to one of them.
        refuse_protected_purge
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

INSTALL_PLUGINS="office gateway"
[ "$WITH_CLOUDCLI" -eq 0 ] || INSTALL_PLUGINS="$INSTALL_PLUGINS cloudcli"

check_workspace_adoptable
step_preflight
step_python
step_orbit
step_launcher
step_plugin
step_workspace_init
step_secrets
step_agent
step_app

step "summary"
say "summary: launcher   $LAUNCHER (add $HOME/.local/bin to PATH if needed)"
say "summary: workspace  $WORKSPACE"
say "summary: library    $WORKSPACE/library"
say "summary: data       $DATA_DIR"
say "summary: tools      $TOOLS_DIR"
say "summary: plugins    $INSTALL_PLUGINS"
if [ "$AGENT" = "claude" ]; then
    say "summary: deny rules Bash(curl *), Bash(wget *), Bash(http *), Bash(aws *), Bash(gcloud *)"
elif [ "$AGENT" = "muse" ]; then
    say "summary: deny rules unavailable for Muse Code shell calls; it has no per-command deny settings to write"
else
    say "summary: deny rules unavailable for sandboxed Codex shell calls; its execpolicy rules apply outside the sandbox only"
fi
say "summary: app        http://localhost:$PORT/"
say "summary: uninstall  install.sh --uninstall (keeps $WORKSPACE)"
say "summary: purge      install.sh --purge (also removes $WORKSPACE)"
