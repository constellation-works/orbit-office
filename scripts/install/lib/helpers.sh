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

