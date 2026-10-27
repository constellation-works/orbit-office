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
  --from-source    Install office from the pinned git+ source (needs
                   cargo) instead of a local/prebuilt export
  --agent NAME     Agent CLI to register: claude (default) or codex
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
  ORBIT_OFFICE_META_API_KEY  Meta API key (used instead of prompting)
  ORBIT_OFFICE_OPEN_WEIGHTS_TOKEN  Private-route endpoint token (used
                             instead of prompting; optional)
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
AGENT_PATH="$TOOLS_BIN:$_path_rest"
_nl='
'
case "$AGENT_PATH" in
    *\"*|*"$_nl"*|*\\*) die "agent PATH cannot be encoded: $AGENT_PATH" ;;
esac
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
    say "The plugins $INSTALL_PLUGINS request these grants:"
    say "  office: fs {{workspace}}, {{plugin_state}}; network unscoped; orbit_tools task, friction and search"
    say "  gateway: fs {{workspace}}/.orbit-gateway, {{plugin_state}}; network unscoped; orbit_tools task add/artifact/show/update"
    if [ "$WITH_CLOUDCLI" -eq 1 ]; then
        say "  cloudcli: fs {{plugin_state}}; network unscoped; programs python3, aws, gcloud"
    fi
    if [ "$ASSUME_YES" -eq 1 ]; then
        say "Consent: --yes given, enabling $INSTALL_PLUGINS."
        return 0
    fi
    if [ -t 0 ]; then
        printf 'Enable plugins %s with these grants? [y/N] ' "$INSTALL_PLUGINS"
        _answer=""
        read -r _answer || _answer=""
        case "$_answer" in
            y|Y|yes|YES) return 0 ;;
            *) die "plugin enable declined; re-run with --yes to consent" ;;
        esac
    fi
    say "Consent: non-interactive run, enabling $INSTALL_PLUGINS (pass --yes to confirm explicitly)."
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

step_workspace() {
    step "4 workspace"
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
    EXPORT_REPO="$SCRIPT_DIR" EXPORT_OUT="$DATA_DIR/plugins/$1" EXPORT_NAME="$1" node <<'NODE'
const fs = require("fs");
const path = require("path");
const repo = process.env.EXPORT_REPO;
const out = process.env.EXPORT_OUT;
const name = process.env.EXPORT_NAME;
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

step_plugin() {
    step "5 plugin"
    if [ -n "${ORBIT_OFFICE_PLUGIN_BUNDLE:-}" ]; then
        for _plugin in $INSTALL_PLUGINS; do
            [ -f "$ORBIT_OFFICE_PLUGIN_BUNDLE/$_plugin/.orbit-plugin/plugin.yaml" ] \
                || die "missing $_plugin export in ORBIT_OFFICE_PLUGIN_BUNDLE"
        done
    fi
    _need_consent=0
    for _plugin in $INSTALL_PLUGINS; do
        if ! state_has "$(plugin_marker "$_plugin" enabled)"; then _need_consent=1; fi
    done
    if [ "$_need_consent" -eq 1 ]; then consent_enable_plugin; fi
    for _plugin in $INSTALL_PLUGINS; do
        _added=$(plugin_marker "$_plugin" added)
        _enabled=$(plugin_marker "$_plugin" enabled)
        if plugin_is_listed "$_plugin" || state_has "$_added"; then
            say "plugin: already added ($_plugin)"
        else
            _allow_build=0
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
            elif [ "$FROM_SOURCE" -eq 1 ] || [ -z "$PLUGIN_SRC" ]; then
                _ref=$(git -C "$SCRIPT_DIR" rev-parse HEAD 2>/dev/null) \
                    || die "--from-source needs a clone; otherwise set ORBIT_OFFICE_PLUGIN_BUNDLE"
                _source="git+$REPO_GIT_URL#$_ref"
                _allow_build=1
            else
                _source="$PLUGIN_SRC"
            fi
            if [ "$_allow_build" -eq 1 ]; then
                orbit plugin add "$_source" --allow-build
            else
                orbit plugin add "$_source"
            fi
            state_mark "$_added"
            say "plugin: added $_plugin from $_source"
        fi
        if state_has "$_enabled"; then
            say "plugin: already enabled ($_plugin)"
        else
            case "$_plugin" in
                office) PATH="$TOOLS_BIN:$PATH" orbit plugin enable office --grant 'fs={{workspace}},{{plugin_state}}' --grant network --grant orbit_tools ;;
                gateway) PATH="$TOOLS_BIN:$PATH" orbit plugin enable gateway --grant 'fs={{workspace}}/.orbit-gateway,{{plugin_state}}' --grant network --grant orbit_tools ;;
                cloudcli) PATH="$TOOLS_BIN:$PATH" orbit plugin enable cloudcli --grant 'fs={{plugin_state}}' --grant network ;;
            esac
            state_mark "$_enabled"
            say "plugin: enabled ($_plugin)"
        fi
    done
}

step_secrets() {
    step "6 secrets"
    set_secret "meta_api_key" "${ORBIT_OFFICE_META_API_KEY:-${META_API_KEY:-}}" 0
    set_secret "open_weights_token" "${ORBIT_OFFICE_OPEN_WEIGHTS_TOKEN:-${OPEN_WEIGHTS_TOKEN:-}}" 1
}

# Prints "ok" when office.init succeeded, otherwise the office code, or
# "unreadable". Arguments are the exit status, stdout and stderr.
#
# `orbit tool run --json --full` exits 0 and prints the tool output object
# on stdout. That object has no top-level ok. An {ok:true} envelope (the
# direct office binary) is also success. An {ok:false} envelope names its
# code even when the exit status is 0. A non-zero exit puts the code in a
# JSON object on stderr, which a WARN line may precede.
init_envelope_status() {
    ORBIT_OFFICE_INIT_RC="$1" \
    ORBIT_OFFICE_INIT_OUT="$2" \
    ORBIT_OFFICE_INIT_ERR="$3" \
        node <<'NODE' || printf 'unreadable'
function tryParse(text) {
  try {
    return JSON.parse(text);
  } catch (_) {
    return undefined;
  }
}

function parseLoose(text) {
  if (text == null) return undefined;
  var trimmed = String(text).trim();
  if (!trimmed) return undefined;
  var direct = tryParse(trimmed);
  if (direct !== undefined) return direct;
  var lines = trimmed.split(/\r?\n/);
  var i;
  for (i = lines.length - 1; i >= 0; i--) {
    var line = lines[i].trim();
    if (line.charAt(0) !== "{") continue;
    var parsed = tryParse(line);
    if (parsed !== undefined) return parsed;
    var block = tryParse(lines.slice(i).join("\n").trim());
    if (block !== undefined) return block;
  }
  var start = trimmed.indexOf("{");
  var end = trimmed.lastIndexOf("}");
  if (start !== -1 && end > start) {
    var sliced = tryParse(trimmed.slice(start, end + 1));
    if (sliced !== undefined) return sliced;
  }
  return undefined;
}

function asObject(text) {
  var value = parseLoose(text);
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  return value;
}

function codeOf(obj) {
  if (!obj) return "";
  var nested = obj.error;
  if (nested && typeof nested === "object" && !Array.isArray(nested) &&
      typeof nested.code === "string" && nested.code) {
    return nested.code;
  }
  if (typeof obj.code === "string" && obj.code) return obj.code;
  return "";
}

var rc = Number(process.env.ORBIT_OFFICE_INIT_RC || "0");
var out = asObject(process.env.ORBIT_OFFICE_INIT_OUT || "");
if (!isFinite(rc)) {
  process.stdout.write("unreadable");
} else if (rc === 0) {
  if (!out) {
    process.stdout.write("unreadable");
  } else if (out.ok === true || !Object.prototype.hasOwnProperty.call(out, "ok")) {
    process.stdout.write("ok");
  } else if (out.ok === false) {
    process.stdout.write(codeOf(out) || "unreadable");
  } else {
    process.stdout.write("unreadable");
  }
} else {
  var err = asObject(process.env.ORBIT_OFFICE_INIT_ERR || "");
  process.stdout.write(codeOf(err) || codeOf(out) || "unreadable");
}
NODE
}

step_scaffold() {
    step "7 scaffold"
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
    # Exit 0 with the tool output object (no top-level ok) is success, as
    # is an {ok:true} envelope. Anything else fails and the next run asks
    # office.init again. Stderr is kept: a WARN line can precede the JSON
    # error object, and the office code is in that object.
    _init_err_file="$DATA_DIR/office-init.stderr"
    mkdir -p "$DATA_DIR"
    : > "$_init_err_file"
    _init_rc=0
    _init_out=$(
        cd "$WORKSPACE" && orbit tool run office.init --json --full --input '{}' 2>"$_init_err_file"
    ) || _init_rc=$?
    if [ -s "$_init_err_file" ]; then
        cat "$_init_err_file" >&2
    fi
    _init_err=$(cat "$_init_err_file")
    rm -f "$_init_err_file"
    _init_status=$(init_envelope_status "$_init_rc" "$_init_out" "$_init_err")
    if [ "$_init_rc" -eq 0 ] && [ "$_init_status" = "ok" ]; then
        state_mark "scaffolded"
        say "scaffold: ran office.init ($WORKSPACE/library)"
        return 0
    fi
    die "scaffold: office.init failed (code: $_init_status, exit: $_init_rc)"
}

# Register the Orbit MCP entry with the agent CLI. Node is a proven
# prerequisite, so JSON edits go through it; the old file is backed up
# once to <settings>.bak. The entry is "already done" only when it
# matches the desired command, args and PATH. A marker from an older
# install is not enough: that install had no tools/bin on PATH.
# JSON.parse validates the document; the range parser below locates entries
# so edits splice only their bytes. Never round-trip unrelated user settings.
# The receipt contains only owned rule names and newly created containers.
edit_claude_settings() {
    SETTINGS_FILE="$AGENT_SETTINGS" SETTINGS_RECEIPT="$DATA_DIR/claude-settings.json" \
    SETTINGS_MODE="$1" ORBIT_OFFICE_AGENT_PATH="$AGENT_PATH" node <<'NODE'
const fs = require("fs");
const path = require("path");
const crypto = require("crypto");
const file = process.env.SETTINGS_FILE;
const receiptFile = process.env.SETTINGS_RECEIPT;
const installing = process.env.SETTINGS_MODE === "install";
const rules = ["curl", "wget", "http", "aws", "gcloud"].map((p) => `Bash(${p} *)`);
const object = (v) => v !== null && typeof v === "object" && !Array.isArray(v);
function regular(p) {
  try {
    const stat = fs.lstatSync(p);
    if (!stat.isFile() || stat.uid !== process.getuid() || (stat.mode & 0o022) !== 0) {
      throw new Error("expected an owned regular file without group/world write access");
    }
    return true;
  } catch (e) {
    if (e.code === "ENOENT") return false;
    throw e;
  }
}
function write(p, text) {
  regular(p);
  fs.mkdirSync(path.dirname(p), { recursive: true, mode: 0o700 });
  const tmp = p + "." + crypto.randomBytes(12).toString("hex");
  try {
    const fd = fs.openSync(tmp, "wx", 0o600);
    try { fs.writeFileSync(fd, text); fs.fsyncSync(fd); }
    finally { fs.closeSync(fd); }
    fs.renameSync(tmp, p);
  } finally {
    if (fs.existsSync(tmp)) fs.unlinkSync(tmp);
  }
}
try {
  let receipt = { added: [], containers: [] };
  if (regular(receiptFile)) receipt = JSON.parse(fs.readFileSync(receiptFile, "utf8"));
  if (!object(receipt) || !Array.isArray(receipt.added) || !Array.isArray(receipt.containers) ||
      receipt.added.some((r) => !rules.includes(r)) ||
      receipt.containers.some((p) => !["permissions", "permissions.deny", "mcpServers"].includes(p))) {
    throw new Error("invalid receipt");
  }
  const existed = regular(file);
  if (!existed && !installing) {
    if (regular(receiptFile)) fs.unlinkSync(receiptFile);
    process.exit(0);
  }
  const original = existed ? fs.readFileSync(file, "utf8") : "{}\n";
  let text = original;
  // A validated JSON tree with source ranges, including property names.
  // Duplicate keys are refused: two values for one setting are ambiguous.
  function tree() {
    if (!object(JSON.parse(text))) throw new Error("settings must be an object");
    let pos = 0;
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
            key = string(); ws(); pos++; // colon, already JSON-validated
            if (keys.has(key)) throw new Error("duplicate key");
            keys.add(key);
          }
          const node = value();
          entries.push({ key, start: entryStart, end: node.end, node });
          ws();
          if (text[pos] === close) break;
          pos++; ws(); // comma
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
  }
  function locate(keys) {
    let node = tree();
    for (const key of keys) {
      if (node.kind !== "{") throw new Error("expected object");
      const entry = node.entries.find((e) => e.key === key);
      if (!entry) return undefined;
      node = entry.node;
    }
    return node;
  }
  function splice(start, end, added) { text = text.slice(0, start) + added + text.slice(end); }
  function append(node, encoded) {
    const last = node.entries.at(-1);
    const offset = last ? last.end : node.start + 1;
    splice(offset, offset, (last ? "," : "") + encoded);
  }
  function ensureObject(keys) {
    const node = locate(keys);
    if (node) {
      if (node.kind !== "{") throw new Error("expected object");
      return;
    }
    set(keys, {});
    receipt.containers.push(keys.join("."));
  }
  function set(keys, value) {
    const parent = locate(keys.slice(0, -1));
    const key = keys.at(-1);
    const entry = parent.entries.find((e) => e.key === key);
    const encoded = JSON.stringify(value, null, 2);
    if (entry) splice(entry.node.start, entry.node.end, encoded);
    else append(parent, JSON.stringify(key) + ": " + encoded);
  }
  function removeEntry(parent, index) {
    const entry = parent.entries[index];
    if (index > 0) splice(parent.entries[index - 1].end, entry.end, "");
    else if (parent.entries.length > 1) splice(entry.start, parent.entries[1].start, "");
    else splice(entry.start, entry.end, "");
  }
  function remove(keys) {
    const parent = locate(keys.slice(0, -1));
    if (!parent) return;
    const index = parent.entries.findIndex((e) => e.key === keys.at(-1));
    if (index >= 0) removeEntry(parent, index);
  }
  tree(); // refuse malformed/ambiguous JSON even if no rule needs adding
  if (installing) {
    ensureObject(["mcpServers"]);
    const desired = { command: "orbit", args: ["mcp", "serve"], env: { PATH: process.env.ORBIT_OFFICE_AGENT_PATH } };
    const orbit = locate(["mcpServers", "orbit"]);
    if (!orbit || JSON.stringify(JSON.parse(text.slice(orbit.start, orbit.end))) !== JSON.stringify(desired)) {
      set(["mcpServers", "orbit"], desired);
    }
    ensureObject(["permissions"]);
    if (!locate(["permissions", "deny"])) {
      set(["permissions", "deny"], []);
      receipt.containers.push("permissions.deny");
    }
    let deny = locate(["permissions", "deny"]);
    if (deny.kind !== "[") throw new Error("deny must be an array");
    const existing = JSON.parse(text.slice(deny.start, deny.end));
    if (existing.some((r) => typeof r !== "string")) throw new Error("deny rules must be strings");
    for (const rule of rules) {
      if (existing.includes(rule)) continue;
      deny = locate(["permissions", "deny"]);
      append(deny, JSON.stringify(rule));
      if (!receipt.added.includes(rule)) receipt.added.push(rule);
    }
  } else {
    for (const rule of receipt.added) {
      const deny = locate(["permissions", "deny"]);
      if (!deny) continue;
      if (deny.kind !== "[") throw new Error("deny must be an array");
      const index = deny.entries.findIndex((e) => JSON.parse(text.slice(e.start, e.end)) === rule);
      if (index >= 0) removeEntry(deny, index);
    }
    remove(["mcpServers", "orbit"]);
    // Only remove containers we created, and only when still empty.
    for (const key of [...receipt.containers].reverse()) {
      const keys = key.split(".");
      const node = locate(keys);
      if (node && node.entries.length === 0) remove(keys);
    }
  }
  // Validate all edits before the first write. Back up the exact original
  // once. Persist ownership before settings so a retry can undo a partial write.
  tree();
  const serializedReceipt = JSON.stringify(receipt) + "\n";
  if (installing && (!regular(receiptFile) || fs.readFileSync(receiptFile, "utf8") !== serializedReceipt)) {
    write(receiptFile, serializedReceipt);
  }
  if (text !== original || !existed) {
    if (existed && !regular(file + ".bak")) {
      fs.writeFileSync(file + ".bak", original, { flag: "wx", mode: 0o600 });
    }
    write(file, text);
  }
  if (!installing && regular(receiptFile)) fs.unlinkSync(receiptFile);
} catch (_) {
  // JSON parser errors can include the user's setting values; never echo them.
  process.stderr.write("agent settings update refused; existing JSON and ownership receipt must be valid regular files\n");
  process.exit(1);
}
NODE
}

register_mcp_claude() {
    edit_claude_settings install || die "agent: could not update $AGENT_SETTINGS (check JSON, types and file permissions)"
    state_mark "agent_registered"
    say "agent: already configured (Orbit MCP entry and gateway deny rules in $AGENT_SETTINGS)"
}

# True when the Codex orbit table already puts tools/bin on PATH.
# The env map is inline on that table so uninstall, which stops at the
# next [header], removes it with the section.
codex_orbit_path_current() {
    [ -f "$AGENT_SETTINGS" ] || return 1
    awk -v bin="$TOOLS_BIN" '
        /^\[mcp_servers\.orbit\]$/ { in_s = 1; next }
        /^\[/ { in_s = 0 }
        in_s && index($0, "PATH") && index($0, bin) { found = 1 }
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
    edit_claude_settings uninstall || die "agent: could not remove owned entries from $AGENT_SETTINGS"
    say "agent: removed the Orbit MCP entry and recorded deny rules from $AGENT_SETTINGS"
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

step_agent() {
    step "8 agent"
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
    step "9 app"
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
  <dict><key>PORT</key><string>$PORT</string><key>HOST</key><string>127.0.0.1</string><key>PATH</key><string>$AGENT_PATH</string></dict>
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
Environment=PATH=$AGENT_PATH
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
    # Prove ownership before any teardown. A refusal must leave the
    # service, plugin, tools, workspace and data folder in place.
    if [ "$_purge" -eq 1 ]; then
        case "$WORKSPACE" in
            "$HOME"|"/"|"") die "refusing to purge $WORKSPACE" ;;
            *) ;;
        esac
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
            orbit plugin remove "$_plugin" --yes >/dev/null 2>&1 \
                || die "uninstall: could not remove $_plugin; retry when Orbit is available"
            say "uninstall: removed the $_plugin plugin"
        fi
        state_clear "$_added"
        state_clear "$(plugin_marker "$_plugin" enabled)"
        if state_has "plugin.$_plugin.exported"; then
            rm -rf "${DATA_DIR:?}/plugins/$_plugin"
            state_clear "plugin.$_plugin.exported"
        fi
    done
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
        # immediately before the delete so HOME, / and an empty path
        # cannot be removed even if that earlier check is bypassed.
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

INSTALL_PLUGINS="office gateway"
[ "$WITH_CLOUDCLI" -eq 0 ] || INSTALL_PLUGINS="$INSTALL_PLUGINS cloudcli"

check_workspace_adoptable
step_preflight
step_python
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
say "summary: tools      $TOOLS_DIR"
say "summary: plugins    $INSTALL_PLUGINS"
if [ "$AGENT" = "claude" ]; then
    say "summary: deny rules Bash(curl *), Bash(wget *), Bash(http *), Bash(aws *), Bash(gcloud *)"
else
    say "summary: deny rules unavailable for sandboxed Codex shell calls; its execpolicy rules apply outside the sandbox only"
fi
say "summary: app        http://localhost:$PORT/"
say "summary: uninstall  install.sh --uninstall (keeps $WORKSPACE)"
say "summary: purge      install.sh --purge (also removes $WORKSPACE)"
