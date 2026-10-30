#!/bin/sh
# lib.sh - assert/run helpers for test-install.sh (sourced by the entry point; not run directly)
ok() { PASS=$((PASS + 1)); printf 'ok %s - %s\n' "$PASS" "$1"; }
fail() { printf 'FAIL - %s\n' "$1" >&2; exit 1; }
check() { # check <label> <condition...>
    _label="$1"; shift
    if "$@" >/dev/null 2>&1; then ok "$_label"; else fail "$_label"; fi
}
snapshot() { # snapshot <home> : sorted relative paths + file hashes
    (cd "$1" && find . -type f | sort | while IFS= read -r _f; do
        if command -v sha256sum >/dev/null 2>&1; then
            _hash=$(sha256sum "$_f")
        else
            _hash=$(shasum -a 256 "$_f")
        fi
        printf '%s  %s\n' "${_hash%% *}" "$_f"
    done)
    (cd "$1" && find . -type d | sort)
}
AGENT_CLI_TS="$SCRIPT_DIR/../../app/packages/server/src/features/settings/agent-cli.ts"
NODE_BIN=$(command -v node)
cat > "$ROOT/agent-status.mjs" <<'EOF'
const m = await import(new URL("file://" + process.argv[2]));
process.stdout.write(JSON.stringify(m.agentCliStatus()));
EOF
agent_cli_status() { # agent_cli_status <home> [claude_config_dir]
    if [ -n "${2:-}" ]; then
        env -i HOME="$1" CLAUDE_CONFIG_DIR="$2" "$NODE_BIN" --experimental-strip-types "$ROOT/agent-status.mjs" "$AGENT_CLI_TS" 2>/dev/null
    else
        env -i HOME="$1" "$NODE_BIN" --experimental-strip-types "$ROOT/agent-status.mjs" "$AGENT_CLI_TS" 2>/dev/null
    fi
}
run_install() { # run_install <home> <extra env...> -- <args...>
    _home="$1"; shift
    if [ ! -f "$STUB_STATE/current-home" ] || [ "$(cat "$STUB_STATE/current-home")" != "$_home" ]; then
        rm -f "$STUB_STATE"/plugin-* "$STUB_STATE"/secret.*
        printf '%s' "$_home" > "$STUB_STATE/current-home"
    fi
    _envs=""
    _uv_url="$FIXTURE"
    while [ $# -gt 0 ] && [ "$1" != "--" ]; do
        case "$1" in
            ORBIT_OFFICE_UV_URL=none) _uv_url="" ;;
            ORBIT_OFFICE_UV_URL=*) _uv_url=${1#ORBIT_OFFICE_UV_URL=} ;;
            *) _envs="$_envs $1" ;;
        esac
        shift
    done
    [ $# -gt 0 ] && shift
    _uv_env=""
    if [ -n "$_uv_url" ]; then
        _uv_env="ORBIT_OFFICE_UV_URL=$_uv_url"
    fi
    _py_env=""
    if [ -n "${ORBIT_OFFICE_TEST_PYTHON_VERSION:-}" ]; then
        _py_env="ORBIT_OFFICE_TEST_PYTHON_VERSION=$ORBIT_OFFICE_TEST_PYTHON_VERSION"
    fi
    # --yes unless the caller sets NO_YES=1 (the non-interactive case).
    _yes="--yes"
    [ -z "${NO_YES:-}" ] || _yes=""
    # INSTALL_FROM_CWD=1 runs the documented `sh install.sh` from the clone
    # root, so $0 carries no directory.
    _script="$INSTALL_SH"; _run_dir="."
    if [ "${INSTALL_FROM_CWD:-0}" = 1 ]; then _script=install.sh; _run_dir=$(dirname -- "$INSTALL_SH"); fi
    # shellcheck disable=SC2086
    (cd "$_run_dir" && env -i PATH="${TEST_PATH:-${TEST_BIN:-$BIN}:$NODE_DIR:/usr/bin:/bin}" HOME="$_home" STUB_STATE="$STUB_STATE" \
        STUB_MANIFEST="$MANIFEST" \
        STUB_GATEWAY_MANIFESTS="$SCRIPT_DIR/../../gateway/plugins" \
        STUB_ORBIT_VERSION="${STUB_ORBIT_VERSION:-0.37.0}" \
        STUB_UV_ARCHIVE="$FIXTURE" \
        STUB_CURL_MODE="${STUB_CURL_MODE:-ok}" \
        STUB_INIT_MODE="${STUB_INIT_MODE:-ok}" \
        STUB_INIT_FIXTURES="$INIT_FIXTURES" \
        STUB_ORBIT_SRC="$BIN/orbit" \
        ORBIT_OFFICE_OS=linux \
        $_uv_env $_py_env $_envs \
        sh "$_script" $_yes "$@" < /dev/null)
}
# Run the documented curl-to-sh form: install.sh itself is the shell's
# stdin (a pipe), so a secret read from stdin would consume script text.
# The shell runs in $PIPED_CWD, or in a fresh empty temporary working
# directory when PIPED_CWD is unset, never in this harness's clone.
# PIPED_NO_PLUGIN_BUNDLE=1 omits ORBIT_OFFICE_PLUGIN_BUNDLE.
# run_install_piped <home> <sh|bash-posix> <output>
run_install_piped() {
    _home="$1"; _kind="$2"; _out="$3"
    printf '%s' "$_home" > "$STUB_STATE/current-home"
    rm -f "$STUB_STATE"/plugin-* "$STUB_STATE"/secret.*
    _cwd="${PIPED_CWD:-}"
    if [ -z "$_cwd" ]; then
        _cwd=$(mktemp -d "$ROOT/piped-cwd.XXXXXX")
    fi
    mkdir -p "$_cwd"
    _plugin_bundle_env=""
    if [ "${PIPED_NO_PLUGIN_BUNDLE:-0}" != 1 ]; then
        _plugin_bundle_env="ORBIT_OFFICE_PLUGIN_BUNDLE=$PLUGIN_BUNDLE"
    fi
    if [ "$_kind" = bash-posix ]; then
        # shellcheck disable=SC2086
        (cd "$_cwd" && cat "$INSTALL_SH" | env -i PATH="$BIN:$NODE_DIR:/usr/bin:/bin" HOME="$_home" \
            STUB_STATE="$STUB_STATE" STUB_MANIFEST="$MANIFEST" \
            STUB_GATEWAY_MANIFESTS="$SCRIPT_DIR/../../gateway/plugins" \
            STUB_ORBIT_VERSION=0.37.0 STUB_UV_ARCHIVE="$FIXTURE" \
            STUB_CURL_MODE=ok STUB_INIT_MODE=ok \
            STUB_INIT_FIXTURES="$INIT_FIXTURES" STUB_ORBIT_SRC="$BIN/orbit" \
            ORBIT_OFFICE_OS=linux ORBIT_OFFICE_UV_URL="$FIXTURE" \
            $_plugin_bundle_env \
            ORBIT_OFFICE_APP_BUNDLE="$APP_BUNDLE" \
            bash --posix -s -- --yes > "$_out" 2>&1)
    else
        # shellcheck disable=SC2086
        (cd "$_cwd" && cat "$INSTALL_SH" | env -i PATH="$BIN:$NODE_DIR:/usr/bin:/bin" HOME="$_home" \
            STUB_STATE="$STUB_STATE" STUB_MANIFEST="$MANIFEST" \
            STUB_GATEWAY_MANIFESTS="$SCRIPT_DIR/../../gateway/plugins" \
            STUB_ORBIT_VERSION=0.37.0 STUB_UV_ARCHIVE="$FIXTURE" \
            STUB_CURL_MODE=ok STUB_INIT_MODE=ok \
            STUB_INIT_FIXTURES="$INIT_FIXTURES" STUB_ORBIT_SRC="$BIN/orbit" \
            ORBIT_OFFICE_OS=linux ORBIT_OFFICE_UV_URL="$FIXTURE" \
            $_plugin_bundle_env \
            ORBIT_OFFICE_APP_BUNDLE="$APP_BUNDLE" \
            sh -s -- --yes > "$_out" 2>&1)
    fi
}
# Like run_install, but runs a copy of install.sh that sits alone in a
# temp directory: no adjacent app/package.json and no plugin tree, which
# is the standalone (curl-pipe style) layout.
run_install_standalone() { # run_install_standalone <home> <env...> -- <args...>
    _home="$1"; shift
    if [ ! -f "$STUB_STATE/current-home" ] || [ "$(cat "$STUB_STATE/current-home")" != "$_home" ]; then
        rm -f "$STUB_STATE"/plugin-* "$STUB_STATE"/secret.*
        printf '%s' "$_home" > "$STUB_STATE/current-home"
    fi
    _envs=""
    _uv_url="$FIXTURE"
    while [ $# -gt 0 ] && [ "$1" != "--" ]; do
        case "$1" in
            ORBIT_OFFICE_UV_URL=none) _uv_url="" ;;
            ORBIT_OFFICE_UV_URL=*) _uv_url=${1#ORBIT_OFFICE_UV_URL=} ;;
            *) _envs="$_envs $1" ;;
        esac
        shift
    done
    [ $# -gt 0 ] && shift
    _uv_env=""
    if [ -n "$_uv_url" ]; then
        _uv_env="ORBIT_OFFICE_UV_URL=$_uv_url"
    fi
    _yes="--yes"
    [ -z "${NO_YES:-}" ] || _yes=""
    # shellcheck disable=SC2086
    env -i PATH="${TEST_PATH:-${TEST_BIN:-$BIN}:$NODE_DIR:/usr/bin:/bin}" HOME="$_home" STUB_STATE="$STUB_STATE" \
        STUB_MANIFEST="$MANIFEST" \
        STUB_GATEWAY_MANIFESTS="$SCRIPT_DIR/../../gateway/plugins" \
        STUB_ORBIT_VERSION="${STUB_ORBIT_VERSION:-0.37.0}" \
        STUB_UV_ARCHIVE="$FIXTURE" \
        STUB_CURL_MODE="${STUB_CURL_MODE:-ok}" \
        STUB_INIT_MODE="${STUB_INIT_MODE:-ok}" \
        STUB_INIT_FIXTURES="$INIT_FIXTURES" \
        STUB_ORBIT_SRC="$BIN/orbit" \
        ORBIT_OFFICE_OS=linux \
        $_uv_env $_envs \
        sh "$ROOT/standalone-src/install.sh" $_yes "$@" < /dev/null
}
TOOLS_REL=".local/share/orbit-office/tools"
assert_home_scope() { # files stay under the workspace, data/tools, service, MCP
    _home="$1"
    _bad="$ROOT/scope-bad.txt"
    : > "$_bad"
    (cd "$_home" && find . \( -type f -o -type l \) | sort) | while IFS= read -r _f; do
        case "$_f" in
            ./orbit|./orbit/*) ;;
            ./.local/bin/orbit-office) ;;
            ./.local/share/orbit-office|./.local/share/orbit-office/*) ;;
            ./.config/systemd|./.config/systemd/*) ;;
            ./.claude|./.claude/*) ;;
            ./.claude.json|./.claude.json.bak) ;;
            ./.codex|./.codex/*) ;;
            ./.config/muse|./.config/muse/*) ;;
            ./.agents|./.agents/*) ;;
            ./Library/LaunchAgents|./Library/LaunchAgents/*) ;;
            ./Library/Application\ Support/orbit-office|./Library/Application\ Support/orbit-office/*) ;;
            *) printf '%s\n' "$_f" >> "$_bad" ;;
        esac
    done
    if [ -s "$_bad" ]; then
        cat "$_bad" >&2
        fail "files outside the workspace, data folder and tools"
    fi
    ok "no files outside the workspace, data folder and tools"
}
assert_profiles_untouched() {
    _home="$1"
    for _p in .bashrc .bash_profile .profile .zshrc .zprofile .zlogin .cshrc .bash_login; do
        if [ -e "$_home/$_p" ]; then
            fail "shell profile was created: $_p"
        fi
    done
    for _tool in uv python python3; do
        if [ -e "$_home/.local/bin/$_tool" ]; then fail "system $_tool path was created"; fi
    done
    ok "shell profiles and system uv/python paths untouched"
}
# The installed launcher is the real user entry point, independent of install.sh.
run_office() { # run_office <home> <args...>
    _office_home="$1"; shift
    env -i HOME="$_office_home" PATH="$BIN:$NODE_DIR:/usr/bin:/bin" \
        STUB_STATE="$STUB_STATE" STUB_MANIFEST="$MANIFEST" \
        STUB_GATEWAY_MANIFESTS="$SCRIPT_DIR/../../gateway/plugins" \
        STUB_INIT_MODE="${STUB_INIT_MODE:-ok}" STUB_INIT_FIXTURES="$INIT_FIXTURES" \
        ORBIT_OFFICE_OS="${OFFICE_TEST_OS:-linux}" ORBIT_OFFICE_HOME="${OFFICE_TEST_HOME:-}" "$_office_home/.local/bin/orbit-office" "$@" < /dev/null
}
# check_protected_purge <label> <home> <expected line> <run_install args...>
check_protected_purge() {
    _cp_label="$1"; _cp_home="$2"; _cp_expect="$3"; shift 3
    _cp_out="$ROOT/out-protected-alias.txt"
    rm -f "$STUB_STATE/rm-guard.log"
    snapshot "$_cp_home" > "$ROOT/snap-alias-before.txt"
    if (TEST_PATH="$RM_GUARD_BIN:$BIN:$NODE_DIR:/usr/bin:/bin" \
        run_install "$_cp_home" "$@") > "$_cp_out" 2>&1; then
        cat "$_cp_out" >&2
        fail "$_cp_label: purge is refused"
    fi
    ok "$_cp_label: purge is refused"
    check "$_cp_label: refusal names the protected path" \
        grep -qxF "error: $_cp_expect" "$_cp_out"
    snapshot "$_cp_home" > "$ROOT/snap-alias-after.txt"
    check "$_cp_label: refusal leaves every file unchanged" \
        cmp -s "$ROOT/snap-alias-before.txt" "$ROOT/snap-alias-after.txt"
    check "$_cp_label: nothing tried to delete HOME or /" \
        test ! -e "$STUB_STATE/rm-guard.log"
}
pin_of() {
    awk -F'"' -v key="$1" 'index($0, key "=\"") == 1 { print $2; exit }' "$INSTALL_SH"
}
check_runtime_path() { # check_runtime_path <label> <path-value> <home>
    _label="$1"; _path_value="$2"; _tools_bin="$3/$TOOLS_REL/bin"
    [ -n "$_path_value" ] || fail "$_label: generated PATH is empty"
    [ "${_path_value%%:*}" = "$_tools_bin" ] \
        || fail "$_label: tools/bin is not first in generated PATH: $_path_value"
    ok "$_label: tools/bin first in generated PATH"
    _resolved=$(env -i PATH="$_path_value" /bin/sh -c 'command -v orbit' 2>/dev/null) \
        || fail "$_label: orbit is not resolvable under the generated PATH"
    [ "$_resolved" = "$CUSTOM_BIN/orbit" ] \
        || fail "$_label: generated PATH resolves orbit to $_resolved, not $CUSTOM_BIN/orbit"
    ok "$_label: generated PATH resolves orbit in the non-system prefix"
    _resolved_node=$(env -i PATH="$_path_value" /bin/sh -c 'command -v node' 2>/dev/null) \
        || fail "$_label: node is not resolvable under the generated PATH"
    [ "$_resolved_node" = "$CUSTOM_BIN/node" ] \
        || fail "$_label: generated PATH resolves node to $_resolved_node, not $CUSTOM_BIN/node"
    ok "$_label: generated PATH resolves node in the non-system prefix"
    if env -i PATH="$_path_value" /bin/sh -c 'orbit --version >/dev/null 2>&1 && node --version >/dev/null 2>&1'; then
        ok "$_label: generated PATH executes orbit and node"
    else
        fail "$_label: generated PATH executes orbit and node"
    fi
}
