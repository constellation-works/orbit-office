#!/bin/sh
# test-install.sh - acceptance harness for ../../install.sh
#
# Runs install.sh against a temp HOME with stub orbit, npm, curl, tar
# and service managers on PATH (stub state and logs live outside HOME,
# so a HOME snapshot only sees installer effects). A decoy `uv` on PATH
# records any use and must stay unused. Checks:
#   1. two runs: the second changes nothing and reports already done,
#      including the pinned uv/Python step
#   2. older Orbit stops with the upgrade command; newer warns on
#   3. --uninstall removes tools/ and keeps ~/orbit; --purge removes an
#      owned workspace, refuses any other folder and refuses a data
#      folder or workspace that is HOME or /, however it is spelled
#      (trailing or repeated slashes, dot segments, a symlink), before
#      any teardown and with every file left unchanged
#   4. a non-empty unrelated workspace folder is refused, including by
#      --purge --home
#   5. missing Orbit is installed from npm
#   6. a uv archive checksum mismatch stops before unpack
#   7. python3 --version must print the pinned minor
#   8. launchd, systemd and the MCP entry put tools/bin first on PATH
#   9. a Codex MCP entry uses an inline env table; uninstall removes it
#  10. an older Claude MCP entry gains tools/bin on PATH
#  11. a real office.init error (exit 1, WARN then JSON on stderr)
#      fails the step, names the office code and leaves no marker;
#      an {ok:false} stdout envelope with exit 0 still does too
#  12. a non-interactive run without --yes prints the grants, enables the
#      plugin with them and does not ask
#  13. enable grants every permission plugin.yaml requests: the plugin is
#      active and office.init is not refused with policy_denied
#  14. the documented curl-to-sh form (install.sh itself on a piped stdin,
#      under sh and under bash --posix, from an empty temporary working
#      directory, with prepared plugin and app bundles) stores no secret
#      from script text and still runs the scaffold and agent steps
#  20. a piped install with no plugin bundle and no clone fails with the
#      bundle error from an empty working directory and from a decoy git
#      checkout carrying a gateway/ tree: it pins no git HEAD from the
#      working directory and copies no gateway export from it
#  15. a pre-existing same-named user skill survives install and
#      uninstall byte-for-byte for claude and codex; uninstall removes
#      only installer-copied skills still unmodified, keeps a skill the
#      user edited after copying, and is safe to repeat
#  16. Orbit and Node in a temporary non-system prefix: the PATH values
#      read back from the generated Claude, Codex, launchd and systemd
#      files resolve and execute that Orbit (and Node) under env -i,
#      with tools/bin first, and a second run changes nothing
#  17. standalone install (install.sh copied alone, no adjacent app tree):
#      without an app bundle it fails before any service is written and
#      names ORBIT_OFFICE_APP_BUNDLE; an invalid bundle fails the same
#      way; a valid bundle provisions a runnable entrypoint under the
#      data folder, a rerun is idempotent, and deleting the entrypoint
#      is repaired by the bundle (and named when no bundle is given)

#  19. Claude MCP registration lands in ${CLAUDE_CONFIG_DIR:-$HOME}/.claude.json
#      and the existing agentCliStatus API reports Claude Code connected,
#      including with CLAUDE_CONFIG_DIR set; uninstall removes only an
#      installer-owned entry and preserves unrelated / user-owned entries,
#      and a misplaced entry from an earlier install is migrated.
#  18. Failed service activation is never marked complete: a failing or
#      missing systemctl/launchctl stops the step with a service error and
#      leaves no service_installed marker, and the next run with the
#      manager restored retries activation; an installed but inactive
#      service is re-enabled on the next run. Stubs only; no host service
#      is touched.
#  21. office status/open: healthy plain and --json reports, one missing
#      prerequisite at a time (Library, plugin, Meta key, stopped app)
#      naming its fix, no secret value in output, open starting a stopped
#      installed service via stub managers and a stub browser, and the
#      missing-service install.sh error (linux and macos service shapes).
# Usage: sh scripts/install/test-install.sh
set -eu

SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd) || exit 1
INSTALL_SH="$SCRIPT_DIR/../../install.sh"
FIXTURE="$SCRIPT_DIR/fixtures/uv-test-fixture.tar.gz"
INIT_FIXTURES="$SCRIPT_DIR/fixtures/office-init"
MANIFEST="$SCRIPT_DIR/../../plugin/.orbit-plugin/plugin.yaml"
[ -f "$INSTALL_SH" ] || { printf 'missing %s\n' "$INSTALL_SH" >&2; exit 1; }
[ -f "$FIXTURE" ] || { printf 'missing %s\n' "$FIXTURE" >&2; exit 1; }
for _need in success.stdout success.exit already.stdout already.exit \
    error.stdout error.stderr error.exit; do
    [ -f "$INIT_FIXTURES/$_need" ] || {
        printf 'missing %s\n' "$INIT_FIXTURES/$_need" >&2
        exit 1
    }
done
[ -f "$MANIFEST" ] || { printf 'missing %s\n' "$MANIFEST" >&2; exit 1; }
REAL_TAR=$(command -v tar) || { printf 'tar is required\n' >&2; exit 1; }

mkdir -p "$SCRIPT_DIR/../../.orbit/tmp"
ROOT=$(mktemp -d "$SCRIPT_DIR/../../.orbit/tmp/install-test.XXXXXX")
ROOT=$(CDPATH='' cd -- "$ROOT" && pwd -P)
trap 'rm -rf "$ROOT"' EXIT INT TERM

PASS=0
ok() { PASS=$((PASS + 1)); printf 'ok %s - %s\n' "$PASS" "$1"; }
fail() { printf 'FAIL - %s\n' "$1" >&2; exit 1; }
check() { # check <label> <condition...>
    _label="$1"; shift
    if "$@" >/dev/null 2>&1; then ok "$_label"; else fail "$_label"; fi
}

# The success fixture is the real CLI object. A wrapped {ok:true} envelope
# is the stub that hid this bug, so the harness fails if that is what
# success stdout is (STD-04 R5: the message names the escape).
if ! node -e '
const fs = require("fs");
const path = require("path");
const dir = process.argv[1];
function load(name) { return fs.readFileSync(path.join(dir, name), "utf8"); }
const success = JSON.parse(load("success.stdout"));
if (!success || typeof success !== "object" || Array.isArray(success) ||
    Object.prototype.hasOwnProperty.call(success, "ok") ||
    success.already_initialized !== false || !Array.isArray(success.created)) {
  process.stderr.write("success fixture must be the CLI output object without ok\n");
  process.exit(1);
}
const already = JSON.parse(load("already.stdout"));
if (Object.prototype.hasOwnProperty.call(already, "ok") ||
    already.already_initialized !== true ||
    !Array.isArray(already.created) || already.created.length !== 0) {
  process.stderr.write("already fixture must be already_initialized without ok\n");
  process.exit(1);
}
const errText = load("error.stderr");
try {
  JSON.parse(errText);
  process.stderr.write("error stderr must not be bare JSON\n");
  process.exit(1);
} catch (_) {}
const lines = errText.split(/\n/);
if (!lines[0].includes("WARN")) {
  process.stderr.write("error stderr must start with a WARN line\n");
  process.exit(1);
}
const err = JSON.parse(lines.slice(1).join("\n"));
if (err.code !== "library_dir_outside_workspace") process.exit(1);
if (load("error.exit").trim() !== "1") process.exit(1);
if (load("success.exit").trim() !== "0" || load("already.exit").trim() !== "0") process.exit(1);
if (load("error.stdout") !== "") process.exit(1);
' "$INIT_FIXTURES"; then
    fail "recorded office.init fixtures match the real CLI"
fi
ok "recorded office.init fixtures match the real CLI"

# ---------------------------------------------------------------------------
# Stubs (bin/ and state/ live outside the temp HOME)
# ---------------------------------------------------------------------------

NODE_DIR=$(dirname -- "$(command -v node)")
BIN="$ROOT/bin"
STUB_STATE="$ROOT/stub-state"
mkdir -p "$BIN" "$STUB_STATE"
export STUB_STATE

cat > "$BIN/orbit" <<'STUB'
#!/bin/sh
state=${STUB_STATE:?}
mkdir -p "$state"
printf '%s\n' "$*" >> "$state/orbit.log"
case "${1:-} ${2:-}" in
    "--version ")
        printf 'orbit %s\n' "${STUB_ORBIT_VERSION:-0.37.0}" ;;
    "plugin list")
        case " $* " in
            *" --json "*)
                node - "$state" "$PWD" <<'JSON'
const fs = require("fs"), path = require("path");
const [state, cwd] = process.argv.slice(2);
const present = (name) => fs.existsSync(path.join(state, name));
const rows = [];
for (const name of ["office", "gateway", "cloudcli"]) {
  const suffix = name === "office" ? "" : "." + name;
  if (!present("plugin-added" + suffix)) continue;
  const host = present("plugin-active" + suffix);
  const toggle = fs.existsSync(path.join(cwd, ".orbit", "enabled-" + name));
  rows.push({name, install_path: path.dirname(process.env.STUB_MANIFEST), host_enabled: host, workspace_toggle: toggle || null,
    permissions: ["fs", "network", ...(name === "cloudcli" ? [] : ["orbit_tools"])].map(grant => ({grant, granted: host}))});
}
process.stdout.write(JSON.stringify(rows));
JSON
                ;;
            *)
                for _plugin in office gateway cloudcli; do
                    _suffix=".$_plugin"
                    [ "$_plugin" != office ] || _suffix=""
                    if [ -f "$state/plugin-added$_suffix" ]; then
                        printf '%s\t0.1.0\tenabled\tenabled\t3\tok\n' "$_plugin"
                    fi
                done ;;
        esac ;;
    "plugin add")
        _source=${3:?}
        case "$_source" in
            git+*)
                case " $* " in *" --allow-build "*) ;; *) exit 1 ;; esac
                [ "${STUB_BUILD_FAIL:-0}" != 1 ] || { printf "stub: build failed\n" >&2; exit 1; }
                _plugin=office ;;
            *)
                if [ ! -f "$_source/.orbit-plugin/plugin.yaml" ]; then
                    printf 'stub: local plugin source must be an export\n' >&2; exit 1
                fi
                _plugin=$(awk '/^  name:/ {print $2; exit}' "$_source/.orbit-plugin/plugin.yaml") ;;
        esac
        if [ "$_plugin" = office ]; then
            case "$_source" in
                git+*) ;;
                *) [ -x "$_source/.orbit-plugin/bin/office-bin" ] || { printf "stub: office backend missing\n" >&2; exit 1; } ;;
            esac
        fi
        _suffix=".$_plugin"
        [ "$_plugin" != office ] || _suffix=""
        if [ "$_plugin" != office ] && [ ! -f "$_source/.orbit-plugin/lib/orbit_gateway/envelope.py" ]; then
            printf 'stub: gateway export lacks shared package\n' >&2; exit 1
        fi
        : > "$state/plugin-added$_suffix"; printf 'added\n' ;;
    "plugin enable")
        # Like Orbit: enable records the grants given, and the plugin is
        # active only when every permission the manifest requests is granted.
        _plugin=${3:?}
        printf '%s: %s\n' "$PWD" "$*" >> "$state/enable-workspaces.log"
        case " $* " in
            *" --scope workspace "*)
                case " $* " in *" --grant "*) printf 'workspace scope refuses grants\n' >&2; exit 1 ;; esac
                _suffix=".$_plugin"; [ "$_plugin" != office ] || _suffix=""
                [ -f "$state/plugin-active$_suffix" ] || exit 1
                [ -d .orbit ] || { printf 'enable must select a registered workspace\n' >&2; exit 1; }
                : > ".orbit/enabled-$_plugin"
                exit 0 ;;
        esac
        if [ "$_plugin" != office ] && [ "$(command -v python3)" != "$HOME/.local/share/orbit-office/tools/bin/python3" ]; then
            printf 'plugin enable must resolve the managed Python\n' >&2; exit 1
        fi
        _suffix=".$_plugin"
        [ "$_plugin" != office ] || _suffix=""
        _manifest=${STUB_MANIFEST:?}
        if [ "$_plugin" != office ]; then
            _manifest="${STUB_GATEWAY_MANIFESTS:?}/$_plugin/.orbit-plugin/plugin.yaml"
        fi
        : > "$state/plugin-enabled$_suffix"
        rm -f "$state/plugin-active$_suffix" "$state/plugin-grants$_suffix"
        _prev=""
        for _a in "$@"; do
            if [ "$_prev" = "--grant" ]; then
                printf '%s\n' "$_a" | tr ',' '\n' | sed 's/=.*//' >> "$state/plugin-grants$_suffix"
            fi
            _prev=$_a
        done
        _missing=""
        for _p in $(awk '/^  permissions:/ { p = 1; next } /^  [^ #]/ { p = 0 } p && /^    [a-z_]+:/ { sub(/:.*/, ""); sub(/^ +/, ""); print }' "$_manifest"); do
            grep -qxF "$_p" "$state/plugin-grants$_suffix" || _missing="$_missing $_p"
        done
        if [ -n "$_missing" ]; then
            printf "Plugin is inactive: plugin 'office' requests%s but this host has not granted it\n" "$_missing"
        else
            : > "$state/plugin-active$_suffix"; printf 'enabled\n'
        fi ;;
    "plugin remove")
        _suffix=".${3:?}"
        [ "$3" != office ] || _suffix=""
        if [ ! -f "$state/plugin-added$_suffix" ]; then
            printf 'stub: plugin record is already gone\n' >&2
            exit 1
        fi
        rm -f "$state/plugin-added$_suffix" "$state/plugin-enabled$_suffix" "$state/plugin-active$_suffix"
        printf 'removed\n' ;;
    "plugin disable")
        rm -f "$state/plugin-enabled" "$state/plugin-active"; printf 'disabled\n' ;;
    "plugin secret")
        case "${3:-}" in
            list)
                for n in meta_api_key open_weights_token; do
                    if [ -f "$state/secret.$n" ]; then
                        printf '%s set\n' "$n"
                    else
                        printf '%s unset\n' "$n"
                    fi
                done ;;
            set)
                cat > "$state/secret.${5:-}.value"
                [ "${4:-}" = office ] || exit 1
                case "${5:-}" in meta_api_key|open_weights_token) ;; *) exit 1 ;; esac
                : > "$state/secret.$5" ;;
            *) exit 1 ;;
        esac ;;
    "workspace init")
        mkdir -p .orbit
        grep -qxF "$PWD" "$state/workspace-roots" 2>/dev/null || printf '%s\n' "$PWD" >> "$state/workspace-roots"
        printf 'initialized\n' ;;
    "workspace list")
        case " $* " in
            *" --json "*) node - "$state/workspace-roots" <<'JSON'
const fs = require("fs"), file = process.argv[2];
const roots = fs.existsSync(file) ? fs.readFileSync(file, "utf8").trim().split("\n").filter(Boolean) : [];
process.stdout.write(JSON.stringify(roots.map(repo_root => ({repo_root, status: "active"}))));
JSON
                ;;
            *) if [ -n "${STUB_WORKSPACE_LIST:-}" ]; then printf '%s\n' "$STUB_WORKSPACE_LIST"; fi ;;
        esac ;;
    "tool run")
        if [ "${3:-}" = "office.status" ]; then
            _meta=unset; [ -f "$state/secret.meta_api_key" ] && _meta=set
            _ow=unset; [ -f "$state/secret.open_weights_token" ] && _ow=set
            if [ -f library/.office/config.yaml ]; then _init=true; _docs=3; else _init=false; _docs=0; fi
            printf '{"version":"0.1.0","secrets":{"meta_api_key":"%s","open_weights_token":"%s","baseten_token":"%s"},"library":{"initialized":%s,"documents":%s,"documents_by_kind":{"source":%s,"domain":0,"runbook":0,"artifact":0},"pages_off_template":0,"template_invalid":[]},"open_weights":null}\n' "$_meta" "$_ow" "$_ow" "$_init" "$_docs" "$_docs"
            exit 0
        fi
        # office.init uses recorded `orbit tool run --json --full` bytes.
        # success and already are stdout objects with no top-level ok and
        # exit 0. error is exit 1, empty stdout, and a WARN line before
        # the JSON object on stderr. ok-false is the direct office
        # binary's {ok:false} envelope on stdout with exit 0.
        if [ "${3:-}" = "office.init" ]; then
            # Orbit refuses a tool of a plugin that is not active: stderr, exit 1.
            if [ ! -f "$state/plugin-active" ]; then
                printf '%s\n' "{\"code\":\"policy_denied\",\"message\":\"policy denied: plugin 'office' requests orbit_tools (spec.permissions.orbit_tools) but this host has not granted it\"}" >&2
                exit 1
            fi
            _mode="${STUB_INIT_MODE:-ok}"
            _fix="${STUB_INIT_FIXTURES:?}"
            case "$_mode" in
                ok-false)
                    printf '%s\n' '{"ok":false,"error":{"code":"library_dir_outside_workspace","message":"library_dir is outside the workspace","retryable":false}}'
                    exit 0
                    ;;
                error) _name="error" ;;
                already) _name="already" ;;
                ok|success) _name="success" ;;
                *)
                    printf 'stub orbit: unknown STUB_INIT_MODE %s\n' "$_mode" >&2
                    exit 1
                    ;;
            esac
            if [ "$_name" = "success" ]; then
                mkdir -p library/.office library/.git
                printf 'config\n' > library/.office/config.yaml
            fi
            if [ -s "$_fix/$_name.stdout" ]; then
                cat "$_fix/$_name.stdout"
            fi
            if [ -s "$_fix/$_name.stderr" ]; then
                cat "$_fix/$_name.stderr" >&2
            fi
            _rc=0
            if [ -f "$_fix/$_name.exit" ]; then
                _rc=$(tr -d '[:space:]' < "$_fix/$_name.exit")
            fi
            exit "$_rc"
        fi ;;
    "mcp remove"|"mcp init")
        printf 'mcp ok\n' ;;
    *) printf 'stub orbit: %s\n' "$*" ;;
esac
exit 0
STUB

cat > "$BIN/npm" <<'STUB'
#!/bin/sh
state=${STUB_STATE:?}
mkdir -p "$state"
printf '%s\n' "$*" >> "$state/npm.log"
# `npm --prefix DIR run build`: fake the build output.
_prev=""
_prefix=""
for _a in "$@"; do
    if [ "$_prev" = "--prefix" ]; then _prefix="$_a"; fi
    _prev="$_a"
done
case " $* " in
    *" run build "*)
        if [ -n "$_prefix" ]; then
            mkdir -p "$_prefix/packages/server/dist"
            printf '// stub build\n' > "$_prefix/packages/server/dist/main.js"
        fi ;;
    *" install -g "*)
        # Simulate installing Orbit: drop the orbit stub next to npm.
        _bindir=$(dirname -- "$0")
        if [ ! -f "$_bindir/orbit" ]; then
            cp "${STUB_ORBIT_SRC:?}" "$_bindir/orbit"
            chmod +x "$_bindir/orbit"
        fi ;;
    *) ;;
esac
exit 0
STUB

cat > "$BIN/systemctl" <<'STUB'
#!/bin/sh
state=${STUB_STATE:?}
mkdir -p "$state"
printf '%s\n' "$*" >> "$state/systemctl.log"
if [ "${STUB_SYSTEMCTL_FAIL:-0}" = 1 ]; then
    exit 1
fi
case " $* " in
    *" is-enabled "*) [ -f "$state/svc-enabled" ]; exit ;;
    *" is-active "*) [ -f "$state/svc-active" ]; exit ;;
    *" enable "*) : > "$state/svc-enabled" ;;
    *" disable "*) rm -f "$state/svc-enabled" "$state/svc-active" ;;
    *" start "*|*" restart "*) : > "$state/svc-active" ;;
    *" stop "*) rm -f "$state/svc-active" ;;
    *) ;;
esac
exit 0
STUB

cat > "$BIN/launchctl" <<'STUB'
#!/bin/sh
state=${STUB_STATE:?}
mkdir -p "$state"
printf '%s\n' "$*" >> "$state/launchctl.log"
if [ "${STUB_LAUNCHCTL_FAIL:-0}" = 1 ]; then
    exit 1
fi
case " $* " in
    *" list "*)
        [ -f "$state/launchd-loaded" ]; exit ;;
    *" load "*)
        : > "$state/launchd-loaded" ;;
    *" unload "*)
        rm -f "$state/launchd-loaded" ;;
    *) ;;
esac
exit 0
STUB

cat > "$BIN/open" <<'STUB'
#!/bin/sh
state=${STUB_STATE:?}
mkdir -p "$state"
printf '%s\n' "$*" >> "$state/browser.log"
exit "${STUB_BROWSER_EXIT:-0}"
STUB

cat > "$BIN/xdg-open" <<'STUB'
#!/bin/sh
state=${STUB_STATE:?}
mkdir -p "$state"
printf '%s\n' "$*" >> "$state/browser.log"
exit "${STUB_BROWSER_EXIT:-0}"
STUB

printf '#!/bin/sh\nexit 0\n' > "$BIN/ffmpeg"

# Serves the recorded fixture, or different bytes when the checksum
# mismatch case asks for them. Never touches the network.
cat > "$BIN/curl" <<'STUB'
#!/bin/sh
state=${STUB_STATE:?}
mkdir -p "$state"
printf '%s\n' "$*" >> "$state/curl.log"
out=""
prev=""
for a in "$@"; do
    if [ "$prev" = "-o" ] || [ "$prev" = "--output" ]; then
        out=$a
    fi
    case "$a" in
        --output=*) out=${a#--output=} ;;
        -o*)
            if [ "$a" != "-o" ]; then
                out=${a#-o}
            fi
            ;;
    esac
    prev=$a
done
[ -n "$out" ] || { printf 'curl stub: no output path\n' >&2; exit 2; }
mkdir -p "$(dirname -- "$out")"
if [ "${STUB_CURL_MODE:-ok}" = "mismatch" ]; then
    printf 'not-the-uv-archive\n' > "$out"
else
    cp "${STUB_UV_ARCHIVE:?}" "$out"
fi
exit 0
STUB

# Proves the installer did not unpack. Exec the real tar otherwise.
cat > "$BIN/tar" <<STUB
#!/bin/sh
state=\${STUB_STATE:?}
mkdir -p "\$state"
printf '%s\n' "\$*" >> "\$state/tar.log"
exec $REAL_TAR "\$@"
STUB

# A uv already on PATH. The installer must not execute this.
cat > "$BIN/uv" <<'STUB'
#!/bin/sh
state=${STUB_STATE:?}
mkdir -p "$state"
printf '%s\n' "$*" >> "$state/uv-on-path.log"
printf 'uv on PATH must not be used\n' >&2
exit 99
STUB

chmod +x "$BIN/orbit" "$BIN/npm" "$BIN/systemctl" "$BIN/launchctl" "$BIN/ffmpeg" \
    "$BIN/curl" "$BIN/tar" "$BIN/uv" "$BIN/open" "$BIN/xdg-open"

# A prepared app bundle for installs with no adjacent app tree (the piped
# form and the standalone section): a runnable entrypoint plus a runtime file.
APP_BUNDLE="$ROOT/app-bundle"
mkdir -p "$APP_BUNDLE/packages/server/dist" "$APP_BUNDLE/packages/contracts/dist"
printf '{"name":"orbit-office-app","type":"module"}\n' > "$APP_BUNDLE/package.json"
printf 'console.log("orbit-office standalone stub app");\n' > "$APP_BUNDLE/packages/server/dist/main.js"
printf '// app runtime file\n' > "$APP_BUNDLE/packages/contracts/dist/index.js"

# A prepared plugin bundle for the piped form: office and gateway exports,
# as a release bundle carries them.
PLUGIN_BUNDLE="$ROOT/plugin-bundle"
mkdir -p "$PLUGIN_BUNDLE"
cp -R "$SCRIPT_DIR/../../plugin" "$PLUGIN_BUNDLE/office"
printf '#!/bin/sh\nexit 0\n' > "$PLUGIN_BUNDLE/office/.orbit-plugin/bin/office-bin"
chmod +x "$PLUGIN_BUNDLE/office/.orbit-plugin/bin/office-bin"
python3 -I "$SCRIPT_DIR/../../gateway/scripts/export.py" gateway "$PLUGIN_BUNDLE/gateway" > /dev/null

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
        $_uv_env $_py_env $_envs \
        sh "$INSTALL_SH" $_yes "$@" < /dev/null
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
            ./.local/bin/office) ;;
            ./.local/share/orbit-office|./.local/share/orbit-office/*) ;;
            ./.config/systemd|./.config/systemd/*) ;;
            ./.claude|./.claude/*) ;;
            ./.claude.json|./.claude.json.bak) ;;
            ./.codex|./.codex/*) ;;
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

# ---------------------------------------------------------------------------
# 1. Install twice: second run changes nothing, reports already done
# ---------------------------------------------------------------------------

HOME1="$ROOT/home1"
mkdir -p "$HOME1"

OUT1="$ROOT/out1.txt"
run_install "$HOME1" -- > "$OUT1" 2>&1 || { cat "$OUT1" >&2; fail "first install exits 0"; }
ok "first install exits 0"
SNAP1="$ROOT/snap1.txt"; snapshot "$HOME1" > "$SNAP1"
check "installer creates the standalone office launcher" test -x "$HOME1/.local/bin/office"
check "workspace is a git repo" test -d "$HOME1/orbit/.git"
check "workspace registered (.orbit)" test -d "$HOME1/orbit/.orbit"
check "library scaffolded" test -f "$HOME1/orbit/library/.office/config.yaml"
check "first run records the scaffolded marker" \
    grep -qx scaffolded "$HOME1/.local/share/orbit-office/install-state"
check "systemd unit written" test -f "$HOME1/.config/systemd/user/orbit-office.service"
check "MCP entry written" grep -q '"orbit"' "$HOME1/.claude.json"
check "MCP entry not written to settings.json" sh -c '! grep -q "\"orbit\"" "$1"' sh "$HOME1/.claude/settings.json"
check "plugin added (stub)" test -f "$STUB_STATE/plugin-added"
check "plugin enabled (stub)" test -f "$STUB_STATE/plugin-enabled"
check "gateway is enabled with office" grep -qF "plugin enable gateway " "$STUB_STATE/orbit.log"
check "plugin active (stub): every manifest permission granted" test -f "$STUB_STATE/plugin-active"
check "step 2 is numbered" grep -q '==> 2 python' "$OUT1"
for _n in 1 3 4 5 6 7 8 9; do
    check "step $_n is numbered" grep -q "==> $_n " "$OUT1"
done
TOOLS1="$HOME1/$TOOLS_REL"
check "uv installed under tools/bin" test -x "$TOOLS1/bin/uv"
check "python3 linked under tools/bin" test -L "$TOOLS1/bin/python3"
check "python linked under tools/bin" test -L "$TOOLS1/bin/python"
check "python3 link points into tools/python" \
    sh -c 'case "$(readlink "$1")" in "$2"/python/*) exit 0 ;; *) exit 1 ;; esac' \
    sh "$TOOLS1/bin/python3" "$TOOLS1"
check "python3 --version prints 3.12" \
    sh -c '"$1" --version | grep -q "Python 3\\.12"' sh "$TOOLS1/bin/python3"
check "systemd PATH starts with tools/bin" \
    grep -F "Environment=PATH=$TOOLS1/bin:" "$HOME1/.config/systemd/user/orbit-office.service"
check "MCP PATH starts with tools/bin" \
    grep -F "\"PATH\": \"$TOOLS1/bin:" "$HOME1/.claude.json"
check "uv install passed --no-bin" grep -F "python install --no-bin" "$STUB_STATE/uv-fixture.log"
check "uv cache stays under tools" \
    grep -F "UV_CACHE_DIR=$TOOLS1/cache" "$STUB_STATE/uv-fixture.log"
check "uv python dir stays under tools" \
    grep -F "UV_PYTHON_INSTALL_DIR=$TOOLS1/python" "$STUB_STATE/uv-fixture.log"
check "PATH uv was not executed" test ! -e "$STUB_STATE/uv-on-path.log"
assert_home_scope "$HOME1"
assert_profiles_untouched "$HOME1"
CURL_AFTER_FIRST=$(wc -l < "$STUB_STATE/curl.log")
TAR_AFTER_FIRST=$(wc -l < "$STUB_STATE/tar.log")

OUT2="$ROOT/out2.txt"
run_install "$HOME1" -- > "$OUT2" 2>&1 || { cat "$OUT2" >&2; fail "second install exits 0"; }
ok "second install exits 0"
SNAP2="$ROOT/snap2.txt"; snapshot "$HOME1" > "$SNAP2"
check "second run changes nothing in HOME" cmp -s "$SNAP1" "$SNAP2"
check "second run does not download uv again" \
    sh -c '[ "$(wc -l < "$1")" -eq "$2" ]' sh "$STUB_STATE/curl.log" "$CURL_AFTER_FIRST"
check "second run does not unpack uv again" \
    sh -c '[ "$(wc -l < "$1")" -eq "$2" ]' sh "$STUB_STATE/tar.log" "$TAR_AFTER_FIRST"
for _word in python orbit workspace plugin scaffold agent service; do
    if grep -i "already" "$OUT2" | grep -qi "$_word"; then
        ok "second run reports $_word as already done"
    else
        cat "$OUT2" >&2; fail "second run reports $_word as already done"
    fi
done

# The installed launcher is the real user entry point, independent of install.sh.
run_office() { # run_office <home> <args...>
    _office_home="$1"; shift
    env -i HOME="$_office_home" PATH="$BIN:$NODE_DIR:/usr/bin:/bin" \
        STUB_STATE="$STUB_STATE" STUB_MANIFEST="$MANIFEST" \
        STUB_GATEWAY_MANIFESTS="$SCRIPT_DIR/../../gateway/plugins" \
        STUB_INIT_MODE="${STUB_INIT_MODE:-ok}" STUB_INIT_FIXTURES="$INIT_FIXTURES" \
        ORBIT_OFFICE_OS="${OFFICE_TEST_OS:-linux}" ORBIT_OFFICE_HOME="${OFFICE_TEST_HOME:-}" "$_office_home/.local/bin/office" "$@" < /dev/null
}
WS2="$ROOT/second workspace"
run_office "$HOME1" init "$WS2" --yes > "$ROOT/office-second" 2>&1 \
    || { cat "$ROOT/office-second" >&2; fail "office init creates a second workspace"; }
check "office init creates first workspace commit" git -C "$WS2" rev-parse --verify HEAD
check "office init creates main branch" sh -c '[ "$(git -C "$1" branch --show-current)" = main ]' sh "$WS2"
check "office init ignores the Library" grep -qxF /library/ "$WS2/.gitignore"
check "office init fills repo-local git name" git -C "$WS2" config --local user.name
check "office init fills repo-local git email" git -C "$WS2" config --local user.email
check "office init registers selected folder" grep -qxF "$WS2" "$STUB_STATE/workspace-roots"
check "office init applies workspace presets" sh -c 'grep -F "workspace init --preset " "$1" >/dev/null && grep -F "workspace init --force --preset " "$1" >/dev/null' sh "$STUB_STATE/orbit.log"
check "office init enables office in selected folder" test -f "$WS2/.orbit/enabled-office"
check "office init enables gateway in selected folder" test -f "$WS2/.orbit/enabled-gateway"
check "office init scaffolds the selected Library" test -f "$WS2/library/.office/config.yaml"
snapshot "$WS2" > "$ROOT/office-ws-before"
snapshot "$HOME1" > "$ROOT/office-home-before"
(STUB_INIT_MODE=already run_office "$HOME1" init "$WS2") > "$ROOT/office-already" 2>&1 \
    || { cat "$ROOT/office-already" >&2; fail "office init accepts already_initialized"; }
snapshot "$WS2" > "$ROOT/office-ws-after"
snapshot "$HOME1" > "$ROOT/office-home-after"
check "office init repeat preserves workspace bytes" cmp -s "$ROOT/office-ws-before" "$ROOT/office-ws-after"
check "office init repeat preserves install bytes" cmp -s "$ROOT/office-home-before" "$ROOT/office-home-after"
check "office init repeat does not ask consent" sh -c '! grep -q "Consent:" "$1"' sh "$ROOT/office-already"
run_office "$HOME1" init > "$ROOT/office-default" 2>&1 \
    || { cat "$ROOT/office-default" >&2; fail "office init uses default ~/orbit"; }
check "office init default names ~/orbit" grep -F "$HOME1/orbit/library" "$ROOT/office-default"
(OFFICE_TEST_HOME="$WS2" run_office "$HOME1" init) > "$ROOT/office-env-default" 2>&1 \
    || { cat "$ROOT/office-env-default" >&2; fail "office init honors ORBIT_OFFICE_HOME"; }
check "office env default names selected Library" grep -F "$WS2/library" "$ROOT/office-env-default"
(cd "$ROOT" && run_office "$HOME1" init './second workspace') > "$ROOT/office-relative" 2>&1 \
    || { cat "$ROOT/office-relative" >&2; fail "office init accepts relative path"; }
check "relative office init names resolved Library" grep -F "$WS2/library" "$ROOT/office-relative"
# A stale .orbit directory and ownership receipt are insufficient registration.
# The old global scaffold marker is also insufficient when the Library is missing.
: > "$STUB_STATE/workspace-roots"
mv "$WS2/library" "$ROOT/saved-second-library"
run_office "$HOME1" init "$WS2" > "$ROOT/office-repair" 2>&1 \
    || { cat "$ROOT/office-repair" >&2; fail "office init repairs missing registration and Library"; }
check "office init repairs registration despite receipt" grep -qxF "$WS2" "$STUB_STATE/workspace-roots"
check "office init repairs Library despite global marker" test -f "$WS2/library/.office/config.yaml"
REFUSED="$ROOT/unrelated-office"; mkdir -p "$REFUSED/.orbit" "$REFUSED/library"
printf 'keep\n' > "$REFUSED/sentinel"
snapshot "$REFUSED" > "$ROOT/office-refused-before"
snapshot "$HOME1" > "$ROOT/office-refused-home-before"
if run_office "$HOME1" init "$REFUSED" --yes > "$ROOT/office-refused" 2>&1; then
    fail "office init refuses unrelated non-empty folder"
else ok "office init refuses unrelated non-empty folder"; fi
check "office init refusal names folder and reason" grep -F "refusing to adopt non-empty folder $REFUSED: it is not an office workspace" "$ROOT/office-refused"
snapshot "$REFUSED" > "$ROOT/office-refused-after"
snapshot "$HOME1" > "$ROOT/office-refused-home-after"
check "office init refusal preserves folder bytes" cmp -s "$ROOT/office-refused-before" "$ROOT/office-refused-after"
check "office init refusal preserves install bytes" cmp -s "$ROOT/office-refused-home-before" "$ROOT/office-refused-home-after"
for _mode in error ok-false; do
    if (STUB_INIT_MODE="$_mode" run_office "$HOME1" init "$WS2") > "$ROOT/office-error-$_mode" 2>&1; then
        fail "office init $_mode must fail"
    else ok "office init $_mode fails"; fi
    check "office init $_mode prints office error code" grep -F 'code: library_dir_outside_workspace' "$ROOT/office-error-$_mode"
done
run_office "$HOME1" --help > "$ROOT/office-help" 2>&1
if [ "${UPDATE_GOLDENS:-0}" = 1 ]; then cp "$ROOT/office-help" "$SCRIPT_DIR/fixtures/office-help.stdout"; fi
check "office help matches its shipped golden" cmp -s "$ROOT/office-help" "$SCRIPT_DIR/fixtures/office-help.stdout"
run_office "$HOME1" init --help > "$ROOT/office-init-help" 2>&1
check "office init help names init usage" grep -qF "Usage: office init" "$ROOT/office-init-help"
run_office "$HOME1" open --help > "$ROOT/office-open-help" 2>&1
check "office open help names open usage" grep -qF "Usage: office open" "$ROOT/office-open-help"
run_office "$HOME1" status --help > "$ROOT/office-status-help" 2>&1
check "office status help names status usage" grep -qF "Usage: office status" "$ROOT/office-status-help"
if run_office "$HOME1" bogus > "$ROOT/office-usage" 2>&1; then fail "office rejects unknown commands";
else _usage_rc=$?; check "office usage exits 2" test "$_usage_rc" -eq 2; fi

# ---------------------------------------------------------------------------
# 21. office status and office open (fixtures; stub browser/service, no host)
# ---------------------------------------------------------------------------
# Healthy: meta key set (presence only), Library scaffolded, plugins on,
# service running, agent CLI registered.
printf 'stub-secret-value-must-not-leak\n' > "$STUB_STATE/secret.meta_api_key.value"
grep -qxF "$HOME1/orbit" "$STUB_STATE/workspace-roots" 2>/dev/null || printf '%s\n' "$HOME1/orbit" >> "$STUB_STATE/workspace-roots"
: > "$STUB_STATE/secret.meta_api_key"
: > "$STUB_STATE/svc-active"
rm -f "$STUB_STATE/browser.log"
run_office "$HOME1" status > "$ROOT/status-healthy" 2>&1 || { cat "$ROOT/status-healthy" >&2; fail "office status healthy exits 0"; }
ok "office status healthy exits 0"
check "status reports workspace" grep -F "Workspace: $HOME1/orbit" "$ROOT/status-healthy"
check "status reports plugins enabled" grep -F "Plugins: office enabled; gateway enabled" "$ROOT/status-healthy"
check "status reports Library health" grep -F "Library: initialized" "$ROOT/status-healthy"
check "status reports Meta key presence" grep -F "Meta API key: set" "$ROOT/status-healthy"
check "status reports private route" grep -F "Private route:" "$ROOT/status-healthy"
check "status reports app URL" grep -F "App service: running at http://localhost:4317/" "$ROOT/status-healthy"
check "status reports agent CLI" grep -F "Agent CLI: registered" "$ROOT/status-healthy"
check "status healthy prints Status OK" grep -qxF "Status: OK" "$ROOT/status-healthy"
check "status never leaks secret value" sh -c '! grep -qF "stub-secret-value-must-not-leak" "$1"' sh "$ROOT/status-healthy"
check "status does not launch browser" test ! -e "$STUB_STATE/browser.log"
run_office "$HOME1" status --json > "$ROOT/status-healthy.json" 2> "$ROOT/status-healthy.json.err" || { cat "$ROOT/status-healthy.json" >&2; fail "office status --json healthy exits 0"; }
ok "office status --json healthy exits 0"
check "status --json stderr is empty" test ! -s "$ROOT/status-healthy.json.err"
node - "$ROOT/status-healthy.json" "$HOME1/orbit" <<'NODE' || fail "office status --json shape"
const fs = require("fs");
const [file, ws] = process.argv.slice(2);
const lines = fs.readFileSync(file, "utf8").trim().split("\n");
if (lines.length !== 1) process.exit(1);
const r = JSON.parse(lines[0]);
if (r.workspace !== ws || r.workspace_registered !== true) process.exit(1);
if (r.plugins.office !== true || r.plugins.gateway !== true) process.exit(1);
if (!r.library.initialized || typeof r.library.documents !== "number") process.exit(1);
if (r.meta_api_key !== "set" || r.secrets.meta_api_key !== "set") process.exit(1);
if (r.private_route !== null) process.exit(1);
if (!r.app_service.installed || !r.app_service.running || r.app_service.url !== "http://localhost:4317/") process.exit(1);
if (r.agent_cli.registered !== true || r.ok !== true || !Array.isArray(r.problems) || r.problems.length !== 0) process.exit(1);
if (fs.readFileSync(file, "utf8").includes("stub-secret-value")) process.exit(1);
NODE
ok "office status --json shape"

# One missing prerequisite at a time.
mv "$HOME1/orbit/library" "$ROOT/saved-healthy-library"
if run_office "$HOME1" status > "$ROOT/status-no-library" 2>&1; then fail "status without Library exits non-zero"; else ok "status without Library exits non-zero"; fi
check "status names missing Library" grep -F "Library is not initialized" "$ROOT/status-no-library"
check "status names Library fix" grep -F "fix: office init $HOME1/orbit" "$ROOT/status-no-library"
mv "$ROOT/saved-healthy-library" "$HOME1/orbit/library"

rm -f "$HOME1/orbit/.orbit/enabled-office"
if run_office "$HOME1" status > "$ROOT/status-plugin-off" 2>&1; then fail "status with plugin disabled exits non-zero"; else ok "status with plugin disabled exits non-zero"; fi
check "status names disabled office plugin" grep -F "plugin 'office' is not enabled" "$ROOT/status-plugin-off"
check "status names plugin fix" grep -F "fix: office init $HOME1/orbit" "$ROOT/status-plugin-off"
: > "$HOME1/orbit/.orbit/enabled-office"

rm -f "$STUB_STATE/secret.meta_api_key"
if run_office "$HOME1" status > "$ROOT/status-no-key" 2>&1; then fail "status without Meta key exits non-zero"; else ok "status without Meta key exits non-zero"; fi
check "status names unset Meta key" grep -F "Meta API key is not set" "$ROOT/status-no-key"
check "status names Meta key fix" grep -F "fix: orbit plugin secret set office meta_api_key" "$ROOT/status-no-key"
check "status without key leaks no secret" sh -c '! grep -qF "stub-secret-value-must-not-leak" "$1"' sh "$ROOT/status-no-key"
: > "$STUB_STATE/secret.meta_api_key"

rm -f "$STUB_STATE/svc-active"
if run_office "$HOME1" status > "$ROOT/status-stopped" 2>&1; then fail "status with app stopped exits non-zero"; else ok "status with app stopped exits non-zero"; fi
check "status names stopped app service" grep -F "app service is not running" "$ROOT/status-stopped"
check "status names stopped fix" grep -F "fix: office open $HOME1/orbit" "$ROOT/status-stopped"
check "status does not start stopped service" test ! -e "$STUB_STATE/svc-active"

# open starts a stopped but installed service, then opens the URL.
run_office "$HOME1" open > "$ROOT/open-stopped" 2>&1 || { cat "$ROOT/open-stopped" >&2; fail "office open starts stopped service"; }
ok "office open starts stopped service and exits 0"
check "office open started the service" test -f "$STUB_STATE/svc-active"
check "office open opened app URL via stub browser" grep -qxF "http://localhost:4317/" "$STUB_STATE/browser.log"
check "office open prints app URL" grep -F "app: http://localhost:4317/" "$ROOT/open-stopped"

# open with no service installed fails naming install.sh, opens nothing.
mv "$HOME1/.config/systemd/user/orbit-office.service" "$ROOT/saved-unit"
rm -f "$STUB_STATE/browser.log"
if run_office "$HOME1" open > "$ROOT/open-no-service" 2>&1; then fail "office open without service exits non-zero"; else ok "office open without service exits non-zero"; fi
check "office open names install.sh when no service" grep -F "install.sh" "$ROOT/open-no-service"
check "office open without service opens nothing" test ! -e "$STUB_STATE/browser.log"
mv "$ROOT/saved-unit" "$HOME1/.config/systemd/user/orbit-office.service"
if run_office "$HOME1" status > "$ROOT/status-restored" 2>&1; then ok "office status healthy again after open fixtures"; else cat "$ROOT/status-restored" >&2; fail "office status healthy again after open fixtures"; fi
run_office "$HOME1" status "$HOME1/orbit" > "$ROOT/status-explicit" 2>&1 || { cat "$ROOT/status-explicit" >&2; fail "office status accepts explicit path"; }
ok "office status accepts explicit path"
rm -f "$STUB_STATE/browser.log"
run_office "$HOME1" open "$HOME1/orbit" > "$ROOT/open-explicit" 2>&1 || { cat "$ROOT/open-explicit" >&2; fail "office open accepts explicit path"; }
ok "office open accepts explicit path"
check "office open explicit path opens URL" grep -qxF "http://localhost:4317/" "$STUB_STATE/browser.log"
if run_office "$HOME1" status --json "$HOME1/orbit" extra > "$ROOT/status-usage2" 2>&1; then fail "office status rejects two paths"; else _rc=$?; check "office status two paths exits 2" test "$_rc" -eq 2; fi
# macOS service shape: launchd plist + launchctl stubs.
mkdir -p "$HOME1/Library/LaunchAgents"
printf '%s\n' '<?xml version="1.0"?>' '<plist><dict><key>Label</key><string>com.orbit-office.app</string><key>EnvironmentVariables</key>' "<dict><key>PORT</key><string>4317</string><key>HOST</key><string>127.0.0.1</string><key>ORBIT_OFFICE_WORKSPACE</key><string>$HOME1/orbit</string></dict></dict></plist>" > "$HOME1/Library/LaunchAgents/com.orbit-office.app.plist"
: > "$STUB_STATE/launchd-loaded"
OFFICE_TEST_OS=macos run_office "$HOME1" status > "$ROOT/status-macos" 2>&1 || { cat "$ROOT/status-macos" >&2; fail "office status reads launchd service"; }
ok "office status reads launchd service (macos)"
check "status macos reports running URL" grep -F "App service: running at http://localhost:4317/" "$ROOT/status-macos"
rm -f "$STUB_STATE/launchd-loaded" "$STUB_STATE/browser.log"
OFFICE_TEST_OS=macos run_office "$HOME1" open > "$ROOT/open-macos" 2>&1 || { cat "$ROOT/open-macos" >&2; fail "office open loads launchd service"; }
ok "office open loads launchd service (macos)"
check "office open macos loaded the agent" test -f "$STUB_STATE/launchd-loaded"
check "office open macos opened URL" grep -qxF "http://localhost:4317/" "$STUB_STATE/browser.log"
rm -f "$HOME1/Library/LaunchAgents/com.orbit-office.app.plist"
check "office launcher is recorded as created" grep -qxF "launcher_path=$HOME1/.local/bin/office" "$HOME1/.local/share/orbit-office/install-state"

# ---------------------------------------------------------------------------
# 14. Piped curl-to-sh: no secret is stored from the script text itself
# ---------------------------------------------------------------------------

for _piped_kind in sh bash-posix; do
    PIPED_HOME="$ROOT/piped-$_piped_kind"; mkdir -p "$PIPED_HOME"
    PIPED_OUT="$ROOT/out-piped-$_piped_kind.txt"
    run_install_piped "$PIPED_HOME" "$_piped_kind" "$PIPED_OUT" \
        || { cat "$PIPED_OUT" >&2; fail "piped install ($_piped_kind) exits 0"; }
    ok "piped install ($_piped_kind) exits 0"
    check "piped ($_piped_kind): no meta_api_key secret stored" \
        test ! -e "$STUB_STATE/secret.meta_api_key"
    check "piped ($_piped_kind): no meta_api_key value recorded from script text" \
        test ! -e "$STUB_STATE/secret.meta_api_key.value"
    check "piped ($_piped_kind): no open_weights_token secret stored" \
        test ! -e "$STUB_STATE/secret.open_weights_token"
    check "piped ($_piped_kind): no open_weights_token value recorded from script text" \
        test ! -e "$STUB_STATE/secret.open_weights_token.value"
    check "piped ($_piped_kind): required meta_api_key skip prints the set-later command" \
        grep -qxF "secrets: meta_api_key skipped (no input; set it later with: orbit plugin secret set office meta_api_key)" "$PIPED_OUT"
    check "piped ($_piped_kind): optional open_weights_token skip prints no set command" \
        grep -qxF "secrets: open_weights_token skipped (optional)" "$PIPED_OUT"
    check "piped ($_piped_kind): scaffold step still runs" \
        test -f "$PIPED_HOME/orbit/library/.office/config.yaml"
    check "piped ($_piped_kind): agent step still runs" \
        grep -q '"orbit"' "$PIPED_HOME/.claude.json"
    check "piped ($_piped_kind): app bundle provisions the service entrypoint" \
        test -f "$PIPED_HOME/.local/share/orbit-office/app/packages/server/dist/main.js"
done

# ---------------------------------------------------------------------------
# 20. Piped with no plugin bundle and no clone: bundle error from any cwd
# ---------------------------------------------------------------------------

PIPED_EMPTY_CWD=$(mktemp -d "$ROOT/piped-empty.XXXXXX")
NO_BUNDLE_HOME="$ROOT/piped-no-bundle-home"; mkdir -p "$NO_BUNDLE_HOME"
OUT_NO_BUNDLE="$ROOT/out-piped-no-bundle.txt"
PIPED_NO_PLUGIN_BUNDLE=1
PIPED_CWD="$PIPED_EMPTY_CWD"
if run_install_piped "$NO_BUNDLE_HOME" sh "$OUT_NO_BUNDLE"; then
    PIPED_NO_PLUGIN_BUNDLE=0; PIPED_CWD=""
    cat "$OUT_NO_BUNDLE" >&2; fail "piped install with no bundle from an empty working directory fails"
fi
PIPED_NO_PLUGIN_BUNDLE=0; PIPED_CWD=""
ok "piped install with no bundle from an empty working directory fails"
check "piped no-bundle (empty cwd) fails with the bundle error" \
    grep -F "otherwise set ORBIT_OFFICE_PLUGIN_BUNDLE" "$OUT_NO_BUNDLE"
check "piped no-bundle (empty cwd) adds no plugin" \
    test ! -e "$STUB_STATE/plugin-added"
check "piped no-bundle (empty cwd) creates no gateway export" \
    test ! -e "$NO_BUNDLE_HOME/.local/share/orbit-office/plugins/gateway"

# A decoy checkout: a git repository whose HEAD is not this clone's, with
# a gateway/ tree that must not be read as the export source.
DECOY="$ROOT/piped-decoy-checkout"
mkdir -p "$DECOY"
cp -R "$SCRIPT_DIR/../../gateway" "$DECOY/gateway"
printf 'decoy\n' > "$DECOY/README"
env HOME="$ROOT" GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null \
    git -C "$DECOY" init -q -b main
env HOME="$ROOT" GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null \
    git -C "$DECOY" add README gateway
env HOME="$ROOT" GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null \
    git -C "$DECOY" -c user.name=decoy -c user.email=decoy@localhost commit -qm decoy
DECOY_HEAD=$(git -C "$DECOY" rev-parse HEAD)
DECOY_HOME="$ROOT/piped-decoy-home"; mkdir -p "$DECOY_HOME"
OUT_DECOY="$ROOT/out-piped-decoy.txt"
: > "$STUB_STATE/orbit.log"
PIPED_NO_PLUGIN_BUNDLE=1
PIPED_CWD="$DECOY"
if run_install_piped "$DECOY_HOME" sh "$OUT_DECOY"; then
    PIPED_NO_PLUGIN_BUNDLE=0; PIPED_CWD=""
    cat "$OUT_DECOY" >&2; fail "piped install with no bundle from a decoy checkout fails"
fi
PIPED_NO_PLUGIN_BUNDLE=0; PIPED_CWD=""
ok "piped install with no bundle from a decoy checkout fails"
check "piped no-bundle (decoy cwd) fails with the bundle error" \
    grep -F "otherwise set ORBIT_OFFICE_PLUGIN_BUNDLE" "$OUT_DECOY"
check "piped no-bundle (decoy cwd) pins no git HEAD from the working directory" \
    sh -c '! grep -F "$2" "$1"' sh "$STUB_STATE/orbit.log" "git+https://github.com/constellation-works/orbit-office.git#$DECOY_HEAD"
check "piped no-bundle (decoy cwd) adds no plugin" \
    test ! -e "$STUB_STATE/plugin-added"
check "piped no-bundle (decoy cwd) creates no gateway export" \
    test ! -e "$DECOY_HOME/.local/share/orbit-office/plugins/gateway"

# ---------------------------------------------------------------------------
# 2. Older Orbit stops; newer warns and continues
# ---------------------------------------------------------------------------

HOME2="$ROOT/home2"; mkdir -p "$HOME2"
OUT_OLD="$ROOT/out-old.txt"
if run_install "$HOME2" STUB_ORBIT_VERSION=0.33.0 -- > "$OUT_OLD" 2>&1; then
    fail "older Orbit stops the script"
else
    ok "older Orbit stops the script"
fi
check "older Orbit prints the upgrade command" \
    grep -q "npm install -g @orbit-tools/cli@0.37.0" "$OUT_OLD"
check "older Orbit: no workspace created" test ! -e "$HOME2/orbit"

HOME3="$ROOT/home3"; mkdir -p "$HOME3"
# Reset plugin stub state so this fresh HOME installs cleanly.
rm -f "$STUB_STATE/plugin-added" "$STUB_STATE/plugin-enabled"
OUT_NEW="$ROOT/out-new.txt"
run_install "$HOME3" STUB_ORBIT_VERSION=0.38.0 -- > "$OUT_NEW" 2>&1 \
    || { cat "$OUT_NEW" >&2; fail "newer Orbit continues"; }
ok "newer Orbit continues"
check "newer Orbit warns" grep -qi "newer" "$OUT_NEW"

# ---------------------------------------------------------------------------
# 3. Uninstall keeps ~/orbit; purge removes it
# ---------------------------------------------------------------------------

OUT_UN="$ROOT/out-un.txt"
# Simulate someone removing the Orbit plugin record while the install-state
# ownership marker remains. The stub's plugin remove now fails in this case.
check "office ownership marker exists before missing-record uninstall" \
    grep -qx plugin_added "$HOME1/.local/share/orbit-office/install-state"
rm -f "$STUB_STATE/plugin-added" "$STUB_STATE/plugin-enabled" "$STUB_STATE/plugin-active"
run_install "$HOME1" -- --uninstall > "$OUT_UN" 2>&1 \
    || { cat "$OUT_UN" >&2; fail "uninstall exits 0"; }
ok "uninstall exits 0"
check "missing Orbit plugin record still clears office ownership markers" \
    sh -c '! grep -Eq "^plugin_(added|enabled)$" "$1"' sh "$HOME1/.local/share/orbit-office/install-state"
check "uninstall removes the service unit" \
    test ! -e "$HOME1/.config/systemd/user/orbit-office.service"
if grep -q '"orbit"' "$HOME1/.claude.json" 2>/dev/null; then
    fail "uninstall removes the MCP entry"
else
    ok "uninstall removes the MCP entry"
fi
check "uninstall removes the plugin (stub)" test ! -e "$STUB_STATE/plugin-added"
check "uninstall keeps the workspace" test -d "$HOME1/orbit"
check "uninstall keeps the Library" test -d "$HOME1/orbit/library"
check "uninstall removes tools" test ! -e "$HOME1/$TOOLS_REL"
check "uninstall removes installer-owned launcher" test ! -e "$HOME1/.local/bin/office"

OUT_PURGE="$ROOT/out-purge.txt"
run_install "$HOME1" -- --purge > "$OUT_PURGE" 2>&1 \
    || { cat "$OUT_PURGE" >&2; fail "purge exits 0"; }
ok "purge exits 0"
check "purge removes the workspace" test ! -e "$HOME1/orbit"

# A pre-existing identical launcher is reusable but never owned for uninstall.
UNOWNED_HOME="$ROOT/unowned-launcher-home"
mkdir -p "$UNOWNED_HOME/.local/bin"
# Copy a launcher from an installed home (HOME3 is the newer-Orbit installation).
cp "$HOME3/.local/bin/office" "$UNOWNED_HOME/.local/bin/office"
run_install "$UNOWNED_HOME" -- > "$ROOT/unowned-install" 2>&1 \
    || { cat "$ROOT/unowned-install" >&2; fail "install reuses identical unowned launcher"; }
check "installer does not claim pre-existing launcher" sh -c '! grep -q "^launcher_path=" "$1"' sh "$UNOWNED_HOME/.local/share/orbit-office/install-state"
run_install "$UNOWNED_HOME" -- --uninstall > "$ROOT/unowned-uninstall" 2>&1 \
    || { cat "$ROOT/unowned-uninstall" >&2; fail "uninstall with unowned launcher succeeds"; }
check "uninstall keeps unrecorded launcher" test -x "$UNOWNED_HOME/.local/bin/office"
MODIFIED_HOME="$ROOT/modified-launcher-home"; mkdir -p "$MODIFIED_HOME"
run_install "$MODIFIED_HOME" -- > "$ROOT/modified-install" 2>&1 \
    || { cat "$ROOT/modified-install" >&2; fail "install before launcher edit"; }
printf '// user addition\n' >> "$MODIFIED_HOME/.local/bin/office"
cp "$MODIFIED_HOME/.local/bin/office" "$ROOT/modified-office-before"
if run_install "$MODIFIED_HOME" -- > "$ROOT/modified-reinstall" 2>&1; then fail "reinstall must preserve modified launcher";
else ok "reinstall refuses to overwrite modified launcher"; fi
run_install "$MODIFIED_HOME" -- --uninstall > "$ROOT/modified-uninstall" 2>&1 \
    || { cat "$ROOT/modified-uninstall" >&2; fail "uninstall keeps edited launcher"; }
check "uninstall preserves edited launcher bytes" cmp -s "$MODIFIED_HOME/.local/bin/office" "$ROOT/modified-office-before"


# ---------------------------------------------------------------------------
# 4. A non-empty unrelated folder is refused
# ---------------------------------------------------------------------------

HOME4="$ROOT/home4"
mkdir -p "$HOME4/orbit"
printf 'precious\n' > "$HOME4/orbit/notes.txt"
OUT_REF="$ROOT/out-refuse.txt"
if run_install "$HOME4" -- > "$OUT_REF" 2>&1; then
    fail "unrelated non-empty workspace is refused"
else
    ok "unrelated non-empty workspace is refused"
fi
check "unrelated folder is untouched" grep -q precious "$HOME4/orbit/notes.txt"

# A library directory by itself does not prove that this installer owns the
# workspace and must not make the default install adopt it.
HOME4LIB="$ROOT/home4-library"
LIBRARY_ONLY="$HOME4LIB/orbit"
mkdir -p "$LIBRARY_ONLY/library"
printf 'precious\n' > "$LIBRARY_ONLY/library/keep.txt"
OUT_ADOPT_LIB="$ROOT/out-adopt-library.txt"
if run_install "$HOME4LIB" -- > "$OUT_ADOPT_LIB" 2>&1; then
    fail "library-only workspace is refused during adoption"
else
    ok "library-only workspace is refused during adoption"
fi
check "library-only adoption refusal message is unchanged" \
    grep -qxF "error: refusing to adopt non-empty folder $LIBRARY_ONLY: it is not an office workspace (move it aside or pass --home DIR)" "$OUT_ADOPT_LIB"
check "library-only adoption leaves the sentinel" \
    grep -qx precious "$LIBRARY_ONLY/library/keep.txt"
check "library-only adoption creates no git repository" test ! -e "$LIBRARY_ONLY/.git"
check "library-only adoption creates no .gitignore" test ! -e "$LIBRARY_ONLY/.gitignore"
check "library-only adoption creates no .orbit marker" test ! -e "$LIBRARY_ONLY/.orbit"
check "library-only adoption creates no office library" test ! -e "$LIBRARY_ONLY/library/.office"

# A registered workspace path that only shares the default workspace's prefix
# is not proof that the default workspace itself is owned.
HOME4PREFIX="$ROOT/home4-prefix"
PREFIX_ONLY="$HOME4PREFIX/orbit"
REGISTERED_PREFIX="$HOME4PREFIX/orbit-archive"
mkdir -p "$PREFIX_ONLY"
printf 'precious\n' > "$PREFIX_ONLY/notes.txt"
OUT_ADOPT_PREFIX="$ROOT/out-adopt-prefix.txt"
if run_install "$HOME4PREFIX" "STUB_WORKSPACE_LIST=$REGISTERED_PREFIX" -- \
    > "$OUT_ADOPT_PREFIX" 2>&1; then
    fail "prefix-only workspace-list match is refused during adoption"
else
    ok "prefix-only workspace-list match is refused during adoption"
fi
check "prefix-only adoption refusal message is unchanged" \
    grep -qxF "error: refusing to adopt non-empty folder $PREFIX_ONLY: it is not an office workspace (move it aside or pass --home DIR)" "$OUT_ADOPT_PREFIX"
check "prefix-only adoption leaves the sentinel" \
    grep -qx precious "$PREFIX_ONLY/notes.txt"
check "prefix-only adoption creates no git repository" test ! -e "$PREFIX_ONLY/.git"
check "prefix-only adoption creates no .gitignore" test ! -e "$PREFIX_ONLY/.gitignore"
check "prefix-only adoption creates no .orbit marker" test ! -e "$PREFIX_ONLY/.orbit"

# --purge --home at an unrelated non-empty folder must not delete it.
HOME4P="$ROOT/home4-purge"
UNRELATED="$HOME4P/Documents"
mkdir -p "$UNRELATED" "$HOME4P/.local/share/orbit-office"
printf 'precious\n' > "$UNRELATED/notes.txt"
printf 'keep-data\n' > "$HOME4P/.local/share/orbit-office/keep.txt"
OUT_PURGE_REF="$ROOT/out-purge-refuse.txt"
if run_install "$HOME4P" -- --purge --home "$UNRELATED" > "$OUT_PURGE_REF" 2>&1; then
    fail "purge of an unrelated folder is refused"
else
    ok "purge of an unrelated folder is refused"
fi
check "purge refusal names the path" grep -F "$UNRELATED" "$OUT_PURGE_REF"
check "purge leaves the unrelated folder" test -d "$UNRELATED"
check "purge leaves the sentinel" grep -qx precious "$UNRELATED/notes.txt"
check "purge leaves the data folder" grep -qx keep-data "$HOME4P/.local/share/orbit-office/keep.txt"

# A registered workspace whose path merely contains --home is not proof.
# The old check used `grep -Fq` on `orbit workspace list`.
HOME4L="$ROOT/home4-list"
LISTED="$HOME4L/Documents"
mkdir -p "$LISTED"
printf 'precious\n' > "$LISTED/notes.txt"
OUT_LIST="$ROOT/out-purge-list.txt"
if run_install "$HOME4L" "STUB_WORKSPACE_LIST=$LISTED/notes" -- --purge --home "$LISTED" \
    > "$OUT_LIST" 2>&1; then
    fail "purge refuses a path that is only a prefix of a registered workspace"
else
    ok "purge refuses a path that is only a prefix of a registered workspace"
fi
check "prefix refusal names the path" grep -F "refusing to purge $LISTED" "$OUT_LIST"
check "prefix refusal leaves the sentinel" grep -qx precious "$LISTED/notes.txt"

# .orbit and library are markers a user's own project can carry. With
# extra files they are not the partial tree this installer writes, and
# without the state record they are not ownership.
HOME4M="$ROOT/home4-markers"
MARKED="$HOME4M/notes"
mkdir -p "$MARKED/.orbit" "$MARKED/.git" "$MARKED/library"
printf '/library/\n' > "$MARKED/.gitignore"
printf 'precious\n' > "$MARKED/notes.txt"
printf 'keep\n' > "$MARKED/library/keep.txt"
OUT_MARKED="$ROOT/out-purge-markers.txt"
if run_install "$HOME4M" -- --purge --home "$MARKED" > "$OUT_MARKED" 2>&1; then
    fail "purge refuses a folder that only looks like an office workspace"
else
    ok "purge refuses a folder that only looks like an office workspace"
fi
check "marker refusal names the path" grep -F "$MARKED" "$OUT_MARKED"
check "purge leaves the marked folder" test -d "$MARKED"
check "purge leaves the marked sentinel" grep -qx precious "$MARKED/notes.txt"
check "purge leaves the library file" grep -qx keep "$MARKED/library/keep.txt"

# The install-state line is enough on its own, even with no .orbit.
HOME4S="$ROOT/home4-state"
OWNED="$HOME4S/orbit"
mkdir -p "$OWNED" "$HOME4S/.local/share/orbit-office"
printf 'workspace_path=%s\n' "$OWNED" > "$HOME4S/.local/share/orbit-office/install-state"
printf 'gone\n' > "$OWNED/sentinel.txt"
OUT_OWNED="$ROOT/out-purge-owned.txt"
run_install "$HOME4S" -- --purge --home "$OWNED" > "$OUT_OWNED" 2>&1 \
    || { cat "$OUT_OWNED" >&2; fail "purge of a recorded workspace exits 0"; }
ok "purge of a recorded workspace exits 0"
check "purge removes a recorded workspace" test ! -e "$OWNED"
check "purge removes the data folder of a recorded workspace" \
    test ! -e "$HOME4S/.local/share/orbit-office"

# --purge with ORBIT_OFFICE_DATA_DIR set to the home folder is refused before
# any teardown. The workspace is recorded as owned, so the data-folder guard
# is the only thing between this purge and the deletes. "/" and the other
# spellings run below, behind an rm guard.
HOME4X="$ROOT/home4-protected-data"
OWNED_X="$HOME4X/orbit"
mkdir -p "$OWNED_X"
printf 'workspace_path=%s\n' "$OWNED_X" > "$HOME4X/install-state"
printf 'gone\n' > "$OWNED_X/sentinel.txt"
printf 'keep\n' > "$HOME4X/keep.txt"
SNAP_BEFORE_X="$ROOT/snap-protected-before.txt"
SNAP_AFTER_X="$ROOT/snap-protected-after.txt"
snapshot "$HOME4X" > "$SNAP_BEFORE_X"
OUT_PROTECTED="$ROOT/out-purge-protected-data.txt"
if run_install "$HOME4X" "ORBIT_OFFICE_DATA_DIR=$HOME4X" -- --purge > "$OUT_PROTECTED" 2>&1; then
    fail "purge refuses a data folder that is the home folder"
else
    ok "purge refuses a data folder that is the home folder"
fi
check "protected data refusal names the path" \
    grep -F "refusing to purge the data folder $HOME4X" "$OUT_PROTECTED"
snapshot "$HOME4X" > "$SNAP_AFTER_X"
check "protected data refusal removes nothing" cmp -s "$SNAP_BEFORE_X" "$SNAP_AFTER_X"
check "protected data refusal leaves the workspace sentinel" \
    grep -qx gone "$OWNED_X/sentinel.txt"

# An ordinary data folder override is still purged.
CUSTOM_X="$HOME4X/custom-data"
mkdir -p "$CUSTOM_X"
printf 'workspace_path=%s\n' "$OWNED_X" > "$CUSTOM_X/install-state"
OUT_CUSTOM="$ROOT/out-purge-custom-data.txt"
run_install "$HOME4X" "ORBIT_OFFICE_DATA_DIR=$CUSTOM_X" -- --purge > "$OUT_CUSTOM" 2>&1 \
    || { cat "$OUT_CUSTOM" >&2; fail "purge of an ordinary data folder override exits 0"; }
ok "purge of an ordinary data folder override exits 0"
check "purge removes the ordinary data folder override" test ! -e "$CUSTOM_X"
check "purge removes the workspace recorded under the override" test ! -e "$OWNED_X"
check "purge keeps the home folder" grep -qx keep "$HOME4X/keep.txt"

# Equivalent spellings of HOME and / are the same protected paths:
# trailing and repeated slashes, `.` and `..` segments and a symlink.
# Each purge must be refused before any teardown, for the data folder
# and for the workspace, with every file in the temp home unchanged.
# These runs put an `rm` guard first on PATH: it refuses, and logs, any
# operand that resolves to / or HOME, so a regressed installer guard
# fails the check instead of deleting the host root or the temp home.
RM_GUARD_BIN="$ROOT/bin-rm-guard"
mkdir -p "$RM_GUARD_BIN"
REAL_RM=$(command -v rm) || fail "rm is on PATH"
cat > "$RM_GUARD_BIN/rm" <<STUB
#!/bin/sh
_home=\$(CDPATH='' cd -P -- "\$HOME" 2>/dev/null && pwd -P) || _home=/
for _op in "\$@"; do
    case "\$_op" in -*) continue ;; *) ;; esac
    _phys=\$(CDPATH='' cd -P -- "\$_op" 2>/dev/null && pwd -P) || continue
    case "\$_phys" in
        /|//|"\$_home")
            printf '%s\n' "\$_op" >> "\${STUB_STATE:?}/rm-guard.log"
            exit 1 ;;
        *) ;;
    esac
done
exec "$REAL_RM" "\$@"
STUB
chmod +x "$RM_GUARD_BIN/rm"

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

HOME4A="$ROOT/home4-alias"
OWNED_A="$HOME4A/orbit"
DATA_A="$HOME4A/.local/share/orbit-office"
mkdir -p "$OWNED_A" "$DATA_A/tools/bin" "$HOME4A/.config/systemd/user"
printf 'gone\n' > "$OWNED_A/sentinel.txt"
printf 'keep\n' > "$HOME4A/keep.txt"
printf 'tool\n' > "$DATA_A/tools/bin/keep"
printf '[Unit]\n' > "$HOME4A/.config/systemd/user/orbit-office.service"
# Data folder aliases read install-state from HOME itself, which records
# the default workspace as owned: the protected-path guard alone refuses.
printf 'workspace_path=%s\n' "$OWNED_A" > "$HOME4A/install-state"
HOME_LINK_A="$ROOT/home4-alias-link"
ln -s "$HOME4A" "$HOME_LINK_A"
for _alias in "$HOME4A/" "$HOME4A//" "$HOME4A/." "$HOME4A/./" \
    "$HOME4A/orbit/.." "$HOME_LINK_A" "$HOME_LINK_A/"; do
    check_protected_purge "data folder $_alias" "$HOME4A" \
        "refusing to purge the data folder $_alias: it is a protected path" \
        "ORBIT_OFFICE_DATA_DIR=$_alias" -- --purge
done
for _alias in / // /. /./ //.; do
    check_protected_purge "data folder $_alias" "$HOME4A" \
        "refusing to purge the data folder $_alias: it is a protected path" \
        "ORBIT_OFFICE_DATA_DIR=$_alias" -- --purge
done

# Workspace aliases are recorded as owned in the default data folder,
# spelled as the installer sees them (one trailing slash stripped).
for _ws in "$HOME4A/." "$HOME4A/" "$HOME4A/orbit/.." "$HOME_LINK_A" \
    / /. //.; do
    printf 'workspace_path=%s\n' "$_ws" >> "$DATA_A/install-state"
done
for _pair in "$HOME4A/.|$HOME4A/." "$HOME4A//|$HOME4A/" \
    "$HOME4A/./|$HOME4A/." "$HOME4A/orbit/..|$HOME4A/orbit/.." \
    "$HOME_LINK_A/|$HOME_LINK_A" "//|/" "/.|/." "//.|//." "/./|/."; do
    _given=${_pair%%|*}; _seen=${_pair#*|}
    check_protected_purge "workspace $_given" "$HOME4A" \
        "refusing to purge $_seen: it is a protected path" \
        -- --purge --home "$_given"
done
check "alias refusals keep the owned workspace" grep -qx gone "$OWNED_A/sentinel.txt"

# The partial tree step 4 writes before the state line is the same proof
# adoption uses, so purge removes it and a later install still adopts it.
HOME4D="$ROOT/home4-digest"
DIGEST="$HOME4D/partial"
mkdir -p "$DIGEST/.git" "$DIGEST/.orbit"
printf '/library/\n' > "$DIGEST/.gitignore"
printf 'partial\n' > "$DIGEST/.git/HEAD"
OUT_DIG="$ROOT/out-purge-digest.txt"
run_install "$HOME4D" -- --purge --home "$DIGEST" > "$OUT_DIG" 2>&1 \
    || { cat "$OUT_DIG" >&2; fail "purge of the partial install tree exits 0"; }
ok "purge of the partial install tree exits 0"
check "purge removes the partial install tree" test ! -e "$DIGEST"

HOME4R="$ROOT/home4-repair"
REPAIR="$HOME4R/wip"
mkdir -p "$REPAIR/.orbit"
git -C "$REPAIR" init -b main >/dev/null
printf '/library/\n' > "$REPAIR/.gitignore"
OUT_ADOPT="$ROOT/out-adopt-partial.txt"
run_install "$HOME4R" -- --home "$REPAIR" > "$OUT_ADOPT" 2>&1 \
    || { cat "$OUT_ADOPT" >&2; fail "partial install tree is adopted"; }
ok "partial install tree is adopted"
check "adoption records workspace_path" \
    grep -qxF "workspace_path=$REPAIR" "$HOME4R/.local/share/orbit-office/install-state"
check "adoption keeps the partial .orbit" test -d "$REPAIR/.orbit"

# ---------------------------------------------------------------------------
# 5. Missing Orbit is installed from npm at the tested version
# ---------------------------------------------------------------------------

BIN2="$ROOT/bin-no-orbit"
mkdir -p "$BIN2"
for _stub in npm systemctl launchctl ffmpeg curl tar uv; do cp "$BIN/$_stub" "$BIN2/$_stub"; done
chmod +x "$BIN2"/*
HOME5="$ROOT/home5"; mkdir -p "$HOME5"
OUT_MISS="$ROOT/out-missing.txt"
(TEST_BIN="$BIN2" run_install "$HOME5" -- > "$OUT_MISS" 2>&1) \
    || { cat "$OUT_MISS" >&2; fail "missing Orbit install exits 0"; }
ok "missing Orbit: npm installs it and the install completes"
check "missing Orbit: npm got the tested package" \
    grep -q "install -g @orbit-tools/cli@0.37.0" "$STUB_STATE/npm.log"
check "missing Orbit: stub orbit created by npm" test -x "$BIN2/orbit"
check "missing Orbit: tools/bin/uv installed from the archive" \
    test -x "$HOME5/$TOOLS_REL/bin/uv"
check "missing Orbit: PATH uv was not executed" test ! -e "$STUB_STATE/uv-on-path.log"

# ---------------------------------------------------------------------------
# 6. Checksum mismatch stops before unpack; a digest override is ignored
# ---------------------------------------------------------------------------

pin_of() {
    awk -F'"' -v key="$1" 'index($0, key "=\"") == 1 { print $2; exit }' "$INSTALL_SH"
}

FIXTURE_SHA=$(pin_of UV_SHA256_FIXTURE)
LINUX_SHA=$(pin_of UV_SHA256_LINUX_X86_64)
[ -n "$FIXTURE_SHA" ] || fail "fixture pin missing from install.sh"
GOT_FIXTURE=$(sha256sum "$FIXTURE" | awk '{print $1}')
if [ "$GOT_FIXTURE" = "$FIXTURE_SHA" ]; then
    ok "fixture archive matches the recorded SHA-256"
else
    fail "fixture archive SHA-256 drifted from install.sh"
fi
for _pair in \
    "UV_SHA256_MACOS_ARM64 60fbd5c8a1ed681845eea28d9d622fcde03c51e91a43465b64a1e17db1ed3472" \
    "UV_SHA256_MACOS_X86_64 63ba17fdda55ad1a230d4b37fc6017ffa10c23688c83818bd3bfdf914c652a02" \
    "UV_SHA256_LINUX_X86_64 f29c3e94ffabd7e6f3ba521dba169a9b7ec12100bf3257087f0f24f6123f3645" \
    "UV_SHA256_LINUX_AARCH64 229e194255c7958d86d56e8790d969a6f74736592133f0dfd0076f754c642c1d"
do
    _name=${_pair%% *}
    _want=${_pair#* }
    _got=$(pin_of "$_name")
    if [ "$_got" = "$_want" ]; then
        ok "recorded $_name"
    else
        fail "recorded $_name ($_got)"
    fi
done

HOME6="$ROOT/home6"; mkdir -p "$HOME6"
rm -f "$STUB_STATE/curl.log" "$STUB_STATE/tar.log" "$STUB_STATE/uv-on-path.log"
OUT_BAD="$ROOT/out-bad.txt"
BAD_SHA=$(printf 'not-the-uv-archive\n' | sha256sum | awk '{print $1}')
if run_install "$HOME6" STUB_CURL_MODE=mismatch "ORBIT_OFFICE_UV_SHA256=$BAD_SHA" -- \
    > "$OUT_BAD" 2>&1; then
    fail "checksum mismatch stops the script"
else
    ok "checksum mismatch stops the script"
fi
check "checksum mismatch names the digest" grep -F "$FIXTURE_SHA" "$OUT_BAD"
check "checksum mismatch says mismatch" grep -F "SHA-256 mismatch" "$OUT_BAD"
check "checksum mismatch leaves no tools/bin/uv" test ! -e "$HOME6/$TOOLS_REL/bin/uv"
check "checksum mismatch does not unpack" test ! -e "$STUB_STATE/tar.log"
check "checksum mismatch does not use PATH uv" test ! -e "$STUB_STATE/uv-on-path.log"

# Default URL (no seam) is the pinned GitHub asset and still refuses
# bytes that do not match that asset's recorded digest.
HOME6B="$ROOT/home6b"; mkdir -p "$HOME6B"
rm -f "$STUB_STATE/curl.log" "$STUB_STATE/tar.log" "$STUB_STATE/uv-on-path.log"
OUT_PIN="$ROOT/out-pin.txt"
if run_install "$HOME6B" ORBIT_OFFICE_UV_URL=none ORBIT_OFFICE_ARCH=x86_64 -- \
    > "$OUT_PIN" 2>&1; then
    fail "unpinned bytes against the platform digest stop the script"
else
    ok "unpinned bytes against the platform digest stop the script"
fi
check "platform digest is named" grep -F "$LINUX_SHA" "$OUT_PIN"
check "platform digest says mismatch" grep -F "SHA-256 mismatch" "$OUT_PIN"
check "download URL is the pinned uv asset" \
    grep -F "github.com/astral-sh/uv/releases/download/0.13.6/uv-x86_64-unknown-linux-gnu.tar.gz" \
    "$STUB_STATE/curl.log"
check "platform mismatch leaves no tools/bin/uv" test ! -e "$HOME6B/$TOOLS_REL/bin/uv"
check "platform mismatch does not unpack" test ! -e "$STUB_STATE/tar.log"
check "platform mismatch does not use PATH uv" test ! -e "$STUB_STATE/uv-on-path.log"

# ---------------------------------------------------------------------------
# 7. python3 --version must print the pinned minor
# ---------------------------------------------------------------------------

HOME7="$ROOT/home7"; mkdir -p "$HOME7"
rm -f "$STUB_STATE/uv-on-path.log"
OUT_VER="$ROOT/out-ver.txt"
if run_install "$HOME7" ORBIT_OFFICE_TEST_PYTHON_VERSION=3.11.2 -- \
    > "$OUT_VER" 2>&1; then
    fail "wrong python minor stops the step"
else
    ok "wrong python minor stops the step"
fi
if ! grep -F "pinned minor version 3.12" "$OUT_VER" >/dev/null; then
    cat "$OUT_VER" >&2
    fail "version failure names the pinned minor"
fi
ok "version failure names the pinned minor"
check "version failure does not use PATH uv" test ! -e "$STUB_STATE/uv-on-path.log"

# ---------------------------------------------------------------------------
# 8. launchd plist PATH, and tools stay off DATA_DIR
# ---------------------------------------------------------------------------

HOME8="$ROOT/home8"; mkdir -p "$HOME8"
OUT_MAC="$ROOT/out-mac.txt"
run_install "$HOME8" ORBIT_OFFICE_OS=macos ORBIT_OFFICE_DATA_DIR="$HOME8/custom-data" -- \
    > "$OUT_MAC" 2>&1 || { cat "$OUT_MAC" >&2; fail "macos install exits 0"; }
ok "macos install exits 0"
PLIST="$HOME8/Library/LaunchAgents/com.orbit-office.app.plist"
MAC_TOOLS="$HOME8/$TOOLS_REL"
check "launchd plist written" test -f "$PLIST"
check "launchd PATH starts with tools/bin" grep -F "<key>PATH</key><string>$MAC_TOOLS/bin:" "$PLIST"
check "macos tools path is the literal share path" test -x "$MAC_TOOLS/bin/uv"
check "macos data dir override is honoured" test -f "$HOME8/custom-data/install-state"
check "macos data dir is not the tools dir" test ! -e "$HOME8/custom-data/tools"
check "no systemd unit on the macos run" \
    test ! -e "$HOME8/.config/systemd/user/orbit-office.service"
check "macos MCP PATH starts with tools/bin" \
    grep -F "\"PATH\": \"$MAC_TOOLS/bin:" "$HOME8/.claude.json"
assert_profiles_untouched "$HOME8"

# ---------------------------------------------------------------------------
# 9. Codex MCP entry uses an inline env table; uninstall removes it
# ---------------------------------------------------------------------------

HOME9="$ROOT/home9"; mkdir -p "$HOME9/.codex"
cat > "$HOME9/.codex/config.toml" <<'EOF'
[other]
keep = 1

[mcp_servers.orbit]
command = "orbit"
args = ["mcp", "serve"]
EOF
OUT_CODEX="$ROOT/out-codex.txt"
run_install "$HOME9" -- --agent codex > "$OUT_CODEX" 2>&1 \
    || { cat "$OUT_CODEX" >&2; fail "codex install exits 0"; }
ok "codex install exits 0"
CODEX_CFG="$HOME9/.codex/config.toml"
check "codex orbit table present" grep -q '^\[mcp_servers\.orbit\]$' "$CODEX_CFG"
check "codex env is inline and starts with tools/bin" \
    grep -F "env = { PATH = \"$HOME9/$TOOLS_REL/bin:" "$CODEX_CFG"
check "codex has no separate env table" \
    sh -c '! grep -q "^\\[mcp_servers\\.orbit\\.env\\]" "$1"' sh "$CODEX_CFG"
check "codex kept the other table" grep -q '^keep = 1$' "$CODEX_CFG"
OUT_CODEX_UN="$ROOT/out-codex-un.txt"
run_install "$HOME9" -- --agent codex --uninstall > "$OUT_CODEX_UN" 2>&1 \
    || { cat "$OUT_CODEX_UN" >&2; fail "codex uninstall exits 0"; }
ok "codex uninstall exits 0"
check "codex uninstall removes the orbit table" \
    sh -c '! grep -q "mcp_servers.orbit" "$1"' sh "$CODEX_CFG"
check "codex uninstall keeps the other table" grep -q '^keep = 1$' "$CODEX_CFG"
check "codex uninstall removes tools" test ! -e "$HOME9/$TOOLS_REL"
check "codex uninstall keeps the workspace" test -d "$HOME9/orbit"

# ---------------------------------------------------------------------------
# 10. An older Claude MCP entry gains tools/bin on PATH
# ---------------------------------------------------------------------------

HOME10="$ROOT/home10"
mkdir -p "$HOME10/.claude" "$HOME10/.local/share/orbit-office"
cat > "$HOME10/.claude/settings.json" <<'EOF'
{
  "mcpServers": {
    "orbit": {
      "command": "orbit",
      "args": ["mcp", "serve"]
    }
  }
}
EOF
printf 'agent_registered\n' > "$HOME10/.local/share/orbit-office/install-state"
OUT_REPAIR="$ROOT/out-repair.txt"
run_install "$HOME10" -- > "$OUT_REPAIR" 2>&1 \
    || { cat "$OUT_REPAIR" >&2; fail "claude repair install exits 0"; }
ok "claude repair install exits 0"
check "repaired MCP PATH starts with tools/bin" \
    grep -F "\"PATH\": \"$HOME10/$TOOLS_REL/bin:" "$HOME10/.claude.json"
check "misplaced MCP entry migrated out of settings.json" \
    sh -c '! grep -q "\"orbit\"" "$1"' sh "$HOME10/.claude/settings.json"
SNAP_REPAIR="$ROOT/snap-repair.txt"
snapshot "$HOME10" > "$SNAP_REPAIR"
OUT_REPAIR2="$ROOT/out-repair2.txt"
run_install "$HOME10" -- > "$OUT_REPAIR2" 2>&1 \
    || { cat "$OUT_REPAIR2" >&2; fail "claude repair second run exits 0"; }
ok "claude repair second run exits 0"
SNAP_REPAIR2="$ROOT/snap-repair2.txt"
snapshot "$HOME10" > "$SNAP_REPAIR2"
check "repaired MCP entry stays unchanged" cmp -s "$SNAP_REPAIR" "$SNAP_REPAIR2"

# ---------------------------------------------------------------------------
# 11. office.init failure names the office code and leaves no marker.
#     The error fixture is a non-zero exit: empty stdout, a WARN line,
#     then the JSON object on stderr. An {ok:false} envelope with exit 0
#     is still a failure.
# ---------------------------------------------------------------------------

HOME11="$ROOT/home11"; mkdir -p "$HOME11"
STATE11="$HOME11/.local/share/orbit-office/install-state"
OUT_INIT="$ROOT/out-init.txt"
if run_install "$HOME11" STUB_INIT_MODE=error -- > "$OUT_INIT" 2>&1; then
    fail "office.init error stops the install"
else
    ok "office.init error stops the install"
fi
check "office.init error names its code in the die message" \
    grep -F "scaffold: office.init failed (code: library_dir_outside_workspace, exit: 1)" "$OUT_INIT"
check "office.init error leaves no scaffolded marker" \
    sh -c '! grep -qx scaffolded "$1"' sh "$STATE11"
OUT_INIT2="$ROOT/out-init2.txt"
run_install "$HOME11" -- > "$OUT_INIT2" 2>&1 \
    || { cat "$OUT_INIT2" >&2; fail "next run retries office.init"; }
ok "next run retries office.init"
check "next run scaffolds the Library" test -f "$HOME11/orbit/library/.office/config.yaml"
check "next run records the scaffolded marker" grep -qx scaffolded "$STATE11"

HOME11B="$ROOT/home11b"; mkdir -p "$HOME11B"
STATE11B="$HOME11B/.local/share/orbit-office/install-state"
OUT_ENV="$ROOT/out-init-envelope.txt"
if run_install "$HOME11B" STUB_INIT_MODE=ok-false -- > "$OUT_ENV" 2>&1; then
    fail "ok:false envelope stops the install"
else
    ok "ok:false envelope stops the install"
fi
check "ok:false envelope names its code" \
    grep -F "scaffold: office.init failed (code: library_dir_outside_workspace, exit: 0)" "$OUT_ENV"
check "ok:false envelope leaves no scaffolded marker" \
    sh -c '! grep -qx scaffolded "$1"' sh "$STATE11B"

HOME12="$ROOT/home12"; mkdir -p "$HOME12"
OUT_ALREADY="$ROOT/out-already.txt"
run_install "$HOME12" STUB_INIT_MODE=already -- > "$OUT_ALREADY" 2>&1 \
    || { cat "$OUT_ALREADY" >&2; fail "already_initialized object exits 0"; }
ok "already_initialized object exits 0"
check "already_initialized object records the scaffolded marker" \
    grep -qx scaffolded "$HOME12/.local/share/orbit-office/install-state"

# ---------------------------------------------------------------------------
# 12. A non-interactive run without --yes prints the grants, enables the
#     plugin with the same grants and does not ask
# ---------------------------------------------------------------------------

HOME12N="$ROOT/home12n"; mkdir -p "$HOME12N"
rm -f "$STUB_STATE/plugin-added" "$STUB_STATE/plugin-enabled" "$STUB_STATE/plugin-active"
: > "$STUB_STATE/orbit.log"
OUT_NOYES="$ROOT/out-noyes.txt"
(NO_YES=1 run_install "$HOME12N" -- > "$OUT_NOYES" 2>&1) \
    || { cat "$OUT_NOYES" >&2; fail "non-interactive run without --yes exits 0"; }
ok "non-interactive run without --yes exits 0"
check "non-interactive run does not ask the question" \
    sh -c '! grep -qF "[y/N]" "$1"' sh "$OUT_NOYES"
check "non-interactive run says it proceeds" \
    grep -qF "non-interactive run, enabling office" "$OUT_NOYES"
check "printed network grant is unscoped" \
    grep -qF "network unscoped" "$OUT_NOYES"
check "printed grants include orbit_tools" \
    grep -qF "orbit_tools" "$OUT_NOYES"
check "plugin enabled with the fs, network and orbit_tools grants" \
    grep -qF "plugin enable office --grant fs={{workspace}},{{plugin_state}} --grant network --grant orbit_tools" "$STUB_STATE/orbit.log"
check "plugin enabled (stub, non-interactive)" test -f "$STUB_STATE/plugin-enabled"

# ---------------------------------------------------------------------------
# 13. Enable grants every permission plugin.yaml requests. A grant left out
#     leaves the plugin inactive, and then office.init is refused with
#     policy_denied, so the install must not get that far.
# ---------------------------------------------------------------------------

HOME13="$ROOT/home13"; mkdir -p "$HOME13"
rm -f "$STUB_STATE/plugin-added" "$STUB_STATE/plugin-enabled" "$STUB_STATE/plugin-active"
OUT_GRANTS="$ROOT/out-grants.txt"
run_install "$HOME13" -- > "$OUT_GRANTS" 2>&1 \
    || { cat "$OUT_GRANTS" >&2; fail "install with the manifest grants exits 0"; }
ok "install with the manifest grants exits 0"
check "plugin is active after enable" test -f "$STUB_STATE/plugin-active"
check "office.init is not refused with policy_denied" \
    sh -c '! grep -q policy_denied "$1"' sh "$OUT_GRANTS"

# Gateway consent, optional cloudcli and real export contents.
HOME14="$ROOT/home14"; mkdir -p "$HOME14"
OUT_GATEWAY="$ROOT/out-gateway.txt"
: > "$STUB_STATE/orbit.log"
run_install "$HOME14" -- > "$OUT_GATEWAY" 2>&1 \
    || { cat "$OUT_GATEWAY" >&2; fail "gateway install exits 0"; }
check "gateway export contains its shared backend package" \
    test -f "$HOME14/.local/share/orbit-office/plugins/gateway/.orbit-plugin/lib/orbit_gateway/envelope.py"
check "gateway is active with all manifest grants" test -f "$STUB_STATE/plugin-active.gateway"
check "default consent names office and gateway together" grep -F "Consent: --yes given, enabling office gateway." "$OUT_GATEWAY"
check "one consent covers both plugins" sh -c '[ "$(grep -c "Consent:" "$1")" -eq 1 ]' sh "$OUT_GATEWAY"
check "cloudcli is absent by default" test ! -e "$STUB_STATE/plugin-added.cloudcli"
check "cloudcli has no export by default" test ! -e "$HOME14/.local/share/orbit-office/plugins/cloudcli"
run_install "$HOME14" -- --with-cloudcli > "$ROOT/out-cloudcli.txt" 2>&1 \
    || { cat "$ROOT/out-cloudcli.txt" >&2; fail "cloudcli opt-in exits 0"; }
check "cloudcli opt-in adds and enables it" test -f "$STUB_STATE/plugin-active.cloudcli"
check "cloudcli consent lists its programs" grep -F "programs python3, aws, gcloud" "$ROOT/out-cloudcli.txt"
snapshot "$HOME14" > "$ROOT/cloudcli-snapshot1"
run_install "$HOME14" -- --with-cloudcli > "$ROOT/out-cloudcli2.txt" 2>&1 \
    || { cat "$ROOT/out-cloudcli2.txt" >&2; fail "cloudcli repeat exits 0"; }
snapshot "$HOME14" > "$ROOT/cloudcli-snapshot2"
check "cloudcli second run changes nothing" cmp -s "$ROOT/cloudcli-snapshot1" "$ROOT/cloudcli-snapshot2"
check "cloudcli second run needs no new consent" sh -c '! grep -q "Consent:" "$1"' sh "$ROOT/out-cloudcli2.txt"
run_install "$HOME14" -- --uninstall > "$ROOT/out-cloudcli-un.txt" 2>&1 \
    || { cat "$ROOT/out-cloudcli-un.txt" >&2; fail "cloudcli uninstall exits 0"; }
check "uninstall finds optional cloudcli without flag" test ! -e "$STUB_STATE/plugin-added.cloudcli"
check "uninstall removes gateway" test ! -e "$STUB_STATE/plugin-added.gateway"
check "uninstall removes owned exports" test ! -e "$HOME14/.local/share/orbit-office/plugins/gateway"

# A supplied release bundle is used directly and survives uninstall.
BUNDLE="$ROOT/bundle"; mkdir -p "$BUNDLE"
cp -R "$SCRIPT_DIR/../../plugin" "$BUNDLE/office"
printf '#!/bin/sh\nexit 0\n' > "$BUNDLE/office/.orbit-plugin/bin/office-bin"
chmod +x "$BUNDLE/office/.orbit-plugin/bin/office-bin"
python3 -I "$SCRIPT_DIR/../../gateway/scripts/export.py" gateway "$BUNDLE/gateway" > /dev/null
python3 -I "$SCRIPT_DIR/../../gateway/scripts/export.py" cloudcli "$BUNDLE/cloudcli" > /dev/null
BUNDLE_HOME="$ROOT/bundle-home"; mkdir -p "$BUNDLE_HOME"
run_install "$BUNDLE_HOME" "ORBIT_OFFICE_PLUGIN_BUNDLE=$BUNDLE" -- --with-cloudcli > "$ROOT/bundle-out" 2>&1 \
    || { cat "$ROOT/bundle-out" >&2; fail "release bundle install exits 0"; }
check "release bundle enables all three plugins" test -f "$STUB_STATE/plugin-active.cloudcli"
check "release bundle creates no local export" test ! -e "$BUNDLE_HOME/.local/share/orbit-office/plugins"
snapshot "$BUNDLE_HOME" > "$ROOT/bundle-snapshot"
run_install "$BUNDLE_HOME" "ORBIT_OFFICE_PLUGIN_BUNDLE=$BUNDLE" -- --with-cloudcli > "$ROOT/bundle-out2" 2>&1 \
    || { cat "$ROOT/bundle-out2" >&2; fail "release bundle repeat exits 0"; }
snapshot "$BUNDLE_HOME" > "$ROOT/bundle-snapshot2"
check "release bundle repeat changes nothing" cmp -s "$ROOT/bundle-snapshot" "$ROOT/bundle-snapshot2"
run_install "$BUNDLE_HOME" -- --uninstall > "$ROOT/bundle-un" 2>&1 \
    || { cat "$ROOT/bundle-un" >&2; fail "bundle uninstall exits 0"; }
check "uninstall preserves the supplied bundle" test -f "$BUNDLE/cloudcli/.orbit-plugin/lib/orbit_gateway/envelope.py"
# Exercise the installer from a fresh clone with no generated backend. Copy
# the candidate installer into the clone while retaining its pinned HEAD.
CLONE="$ROOT/fresh-clone"
env HOME="$ROOT" GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null \
    git clone --quiet --no-hardlinks --no-checkout "$SCRIPT_DIR/../.." "$CLONE"
env HOME="$ROOT" GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null \
    git -C "$CLONE" checkout --quiet --detach "$(git -C "$SCRIPT_DIR/../.." rev-parse HEAD)"
cp "$INSTALL_SH" "$CLONE/install.sh"
check "fresh clone has no backend" test ! -e "$CLONE/plugin/.orbit-plugin/bin/office-bin"
CLONE_HOME="$ROOT/clone-home"; mkdir -p "$CLONE_HOME"
: > "$STUB_STATE/orbit.log"
SAVED_INSTALL_SH="$INSTALL_SH"; INSTALL_SH="$CLONE/install.sh"
run_install "$CLONE_HOME" -- > "$ROOT/clone-out" 2>&1 \
    || { cat "$ROOT/clone-out" >&2; fail "fresh clone install exits 0"; }
INSTALL_SH="$SAVED_INSTALL_SH"
check "fresh clone uses pinned build path" grep -F "plugin add git+https://github.com/constellation-works/orbit-office.git#$(git -C "$CLONE" rev-parse HEAD) --allow-build" "$STUB_STATE/orbit.log"
check "fresh clone reaches scaffold" test -f "$CLONE_HOME/orbit/library/.office/config.yaml"

# An explicitly supplied local source must be a prepared export, and remains
# usable on a repeated run. A missing backend must stop before any markers.
MISSING_HOME="$ROOT/missing-backend-home"; mkdir -p "$MISSING_HOME"
if run_install "$MISSING_HOME" "ORBIT_OFFICE_PLUGIN_SOURCE=$CLONE/plugin" -- > "$ROOT/missing-backend-out" 2>&1; then
    fail "missing backend stops install"
fi
check "missing backend leaves office unadded" test ! -e "$STUB_STATE/plugin-added"
check "missing backend leaves office unenabled" test ! -e "$STUB_STATE/plugin-enabled"
check "missing backend leaves no scaffold" sh -c '! grep -Eq "^(plugin_added|plugin_enabled|scaffolded)$" "$1"' sh "$MISSING_HOME/.local/share/orbit-office/install-state"
EXPLICIT_HOME="$ROOT/explicit-home"; mkdir -p "$EXPLICIT_HOME"
run_install "$EXPLICIT_HOME" "ORBIT_OFFICE_PLUGIN_SOURCE=$BUNDLE/office" -- > "$ROOT/explicit-out" 2>&1 \
    || { cat "$ROOT/explicit-out" >&2; fail "explicit export install exits 0"; }
check "explicit export selected" grep -F "plugin add $BUNDLE/office" "$STUB_STATE/orbit.log"
snapshot "$EXPLICIT_HOME" > "$ROOT/explicit-snapshot"
run_install "$EXPLICIT_HOME" "ORBIT_OFFICE_PLUGIN_SOURCE=$BUNDLE/office" -- > "$ROOT/explicit-out2" 2>&1 \
    || { cat "$ROOT/explicit-out2" >&2; fail "explicit export repeat exits 0"; }
snapshot "$EXPLICIT_HOME" > "$ROOT/explicit-snapshot2"
check "explicit export repeat changes nothing" cmp -s "$ROOT/explicit-snapshot" "$ROOT/explicit-snapshot2"
FAIL_HOME="$ROOT/build-fail-home"; mkdir -p "$FAIL_HOME"
INSTALL_SH="$CLONE/install.sh"
if run_install "$FAIL_HOME" STUB_BUILD_FAIL=1 -- > "$ROOT/build-fail-out" 2>&1; then
    fail "failed build stops install"
fi
INSTALL_SH="$SAVED_INSTALL_SH"
check "failed build leaves office unadded" test ! -e "$STUB_STATE/plugin-added"
check "failed build leaves office unenabled" test ! -e "$STUB_STATE/plugin-enabled"
check "failed build leaves no scaffold" sh -c '! grep -Eq "^(plugin_added|plugin_enabled|scaffolded)$" "$1"' sh "$FAIL_HOME/.local/share/orbit-office/install-state"
SOURCE_HOME="$ROOT/source-home"; mkdir -p "$SOURCE_HOME"
run_install "$SOURCE_HOME" -- --from-source > "$ROOT/source-out" 2>&1 \
    || { cat "$ROOT/source-out" >&2; fail "from-source install exits 0"; }
check "source office install binds the actual worktree commit" \
    grep -F "git+https://github.com/constellation-works/orbit-office.git#$(git -C "$SCRIPT_DIR/../.." rev-parse HEAD) --allow-build" "$STUB_STATE/orbit.log"

snapshot "$SOURCE_HOME" > "$ROOT/source-snapshot"
run_install "$SOURCE_HOME" -- --from-source > "$ROOT/source-out2" 2>&1 \
    || { cat "$ROOT/source-out2" >&2; fail "from-source repeat exits 0"; }
snapshot "$SOURCE_HOME" > "$ROOT/source-snapshot2"
check "from-source repeat changes nothing" cmp -s "$ROOT/source-snapshot" "$ROOT/source-snapshot2"

# Non-canonical JSON: preserve every byte outside inserted entries. The
# pre-existing curl deny belongs to the user and must survive uninstall.
HOME15="$ROOT/home15"; mkdir -p "$HOME15/.claude"
CFG15="$HOME15/.claude/settings.json"
cat > "$CFG15" <<'JSON'
{ "sentinel" : "é } , [ \\\"", "numeric": 1e+02,
 "mcpServers" : { "other" : {"command":"kept"} },
 "permissions" : {"allow" : [ "Read(/allowed)" ], "deny" : [ "Bash(curl *)", "Read(/keep)" ]}, "tail":true }
JSON
cp "$CFG15" "$ROOT/original15.json"
run_install "$HOME15" -- > "$ROOT/out-settings15.txt" 2>&1 \
    || { cat "$ROOT/out-settings15.txt" >&2; fail "non-canonical settings install exits 0"; }
check "backup is the exact original settings" cmp -s "$CFG15.bak" "$ROOT/original15.json"
if ! node - "$CFG15" "$ROOT/original15.json" "$HOME15/.local/share/orbit-office/claude-settings.json" "$HOME15/.claude.json" <<'NODE'
const fs = require("fs");
const [file, original, receipt, mcpFile] = process.argv.slice(2);
const text = fs.readFileSync(file, "utf8");
const data = JSON.parse(text);
const expected = ["Bash(curl *)", "Read(/keep)", "Bash(wget *)", "Bash(http *)", "Bash(aws *)", "Bash(gcloud *)"];
if (JSON.stringify(data.permissions.deny) !== JSON.stringify(expected)) process.exit(1);
if (data.mcpServers.orbit) process.exit(1);
if (JSON.stringify(data.mcpServers.other) !== JSON.stringify({command:"kept"})) process.exit(1);
const mcp = JSON.parse(fs.readFileSync(mcpFile, "utf8"));
if (!mcp.mcpServers.orbit || mcp.mcpServers.orbit.command !== "orbit") process.exit(1);
const owned = JSON.parse(fs.readFileSync(receipt, "utf8")).added;
if (owned.length !== 4 || owned.includes("Bash(curl *)")) process.exit(1);
// Remove precisely the inserted deny entries from the observed file. All
// other whitespace, escaped strings, Unicode and numeric spellings match.
let stripped = text;
for (const rule of expected.slice(2)) stripped = stripped.replace("," + JSON.stringify(rule), "");
if (stripped !== fs.readFileSync(original, "utf8")) process.exit(1);
// Simulate a user adding another rule after installation.
fs.writeFileSync(file, text.replace('"Bash(gcloud *)"', '"Bash(gcloud *)","Bash(user *)"'));
NODE
then fail "settings preserve original bytes and record only added rules"; fi
ok "settings preserve original bytes and record only added rules"
run_install "$HOME15" -- --uninstall > "$ROOT/out-settings15-un.txt" 2>&1 \
    || { cat "$ROOT/out-settings15-un.txt" >&2; fail "settings uninstall exits 0"; }
check "uninstall keeps pre-existing and subsequently added user rules byte-for-byte" \
    node -e 'const fs=require("fs"); const want=fs.readFileSync(process.argv[2],"utf8").replace("\"Read(/keep)\"", "\"Read(/keep)\",\"Bash(user *)\""); process.exit(fs.readFileSync(process.argv[1],"utf8")===want?0:1)' "$CFG15" "$ROOT/original15.json"
check "uninstall keeps original backup" cmp -s "$CFG15.bak" "$ROOT/original15.json"
check "uninstall clears ownership receipt" test ! -e "$HOME15/.local/share/orbit-office/claude-settings.json"
check "uninstall clears MCP ownership receipt" test ! -e "$HOME15/.local/share/orbit-office/claude-mcp.json"
check "uninstall removes owned MCP entry from .claude.json" sh -c '! grep -q "\"orbit\"" "$1"' sh "$HOME15/.claude.json"

# Missing, empty and complete rule containers are all reversible. Existing
# all-five deny rules must never become installer-owned.
for _variant in absent empty complete; do
    _case_home="$ROOT/settings-$_variant"; mkdir -p "$_case_home/.claude"
    _case_file="$_case_home/.claude/settings.json"
    case "$_variant" in
        absent) printf '{ "kept" : 1e+02 }\n' > "$_case_file" ;;
        empty) printf '{"permissions" : {"deny" : [  ]}, "kept" : true}\n' > "$_case_file" ;;
        complete) printf '{"permissions":{"deny":["Bash(curl *)","Bash(wget *)","Bash(http *)","Bash(aws *)","Bash(gcloud *)"]}}\n' > "$_case_file" ;;
    esac
    cp "$_case_file" "$ROOT/$_variant-original"
    run_install "$_case_home" -- > "$ROOT/$_variant-out" 2>&1 \
        || { cat "$ROOT/$_variant-out" >&2; fail "settings $_variant install exits 0"; }
    check "settings $_variant has exactly five deny rules" node -e 'const d=require(process.argv[1]).permissions.deny; process.exit(d.length===5&&new Set(d).size===5?0:1)' "$_case_file"
    run_install "$_case_home" -- --uninstall > "$ROOT/$_variant-un" 2>&1 \
        || { cat "$ROOT/$_variant-un" >&2; fail "settings $_variant uninstall exits 0"; }
    check "settings $_variant round-trip preserves every original byte" cmp -s "$_case_file" "$ROOT/$_variant-original"
done

# Fail closed on malformed/ambiguous JSON and wrong field types without
# replacing the file, creating a backup or exposing parser fragments.
for _bad_json in '{invalid' '{"permissions":{"deny":"wrong"}}' '{"permissions":{},"permissions":{}}'; do
    _bad_home="$ROOT/bad-settings"; mkdir -p "$_bad_home/.claude"
    printf '%s' "$_bad_json" > "$_bad_home/.claude/settings.json"
    cp "$_bad_home/.claude/settings.json" "$ROOT/bad-original"
    if run_install "$_bad_home" -- > "$ROOT/bad-out" 2>&1; then fail "invalid settings must refuse"; fi
    check "invalid settings remain byte-identical" cmp -s "$_bad_home/.claude/settings.json" "$ROOT/bad-original"
    check "invalid settings have no backup or receipt" sh -c '[ ! -e "$1.bak" ] && [ ! -e "$2" ]' sh "$_bad_home/.claude/settings.json" "$_bad_home/.local/share/orbit-office/claude-settings.json"
done
check "Codex summary describes its sandbox deny limitation" grep -F "deny rules unavailable for sandboxed Codex shell calls" "$OUT_CODEX"

# Ordinary settings files: an empty file, a group-writable (664) file and
# a symlinked file must install (proceed), and an unusable settings file
# must not abort --uninstall partway.
EMPTY_SET_HOME="$ROOT/home-empty-settings"; mkdir -p "$EMPTY_SET_HOME/.claude"
: > "$EMPTY_SET_HOME/.claude/settings.json"
run_install "$EMPTY_SET_HOME" -- > "$ROOT/out-empty-settings.txt" 2>&1 \
    || { cat "$ROOT/out-empty-settings.txt" >&2; fail "empty settings install exits 0"; }
ok "empty settings install exits 0"
check "empty settings gains exactly five deny rules" \
    node -e 'const d=require(process.argv[1]).permissions.deny; process.exit(d.length===5&&new Set(d).size===5?0:1)' "$EMPTY_SET_HOME/.claude/settings.json"
check "empty settings install writes the MCP entry" grep -q '"orbit"' "$EMPTY_SET_HOME/.claude.json"

GROUP_SET_HOME="$ROOT/home-group-settings"; mkdir -p "$GROUP_SET_HOME/.claude"
printf '{"kept":true}\n' > "$GROUP_SET_HOME/.claude/settings.json"
chmod 664 "$GROUP_SET_HOME/.claude/settings.json"
run_install "$GROUP_SET_HOME" -- > "$ROOT/out-group-settings.txt" 2>&1 \
    || { cat "$ROOT/out-group-settings.txt" >&2; fail "group-writable settings install exits 0"; }
ok "group-writable settings install exits 0"
check "group-writable settings keeps user keys and gains five deny rules" \
    node -e 'const d=require(process.argv[1]); process.exit(d.kept===true&&d.permissions.deny.length===5?0:1)' "$GROUP_SET_HOME/.claude/settings.json"

LINK_SET_HOME="$ROOT/home-link-settings"; mkdir -p "$LINK_SET_HOME/.claude"
printf '{"kept":true}\n' > "$LINK_SET_HOME/.claude/real-settings.json"
ln -s real-settings.json "$LINK_SET_HOME/.claude/settings.json"
run_install "$LINK_SET_HOME" -- > "$ROOT/out-link-settings.txt" 2>&1 \
    || { cat "$ROOT/out-link-settings.txt" >&2; fail "symlinked settings install exits 0"; }
ok "symlinked settings install exits 0"
check "symlinked settings remains a symlink" test -L "$LINK_SET_HOME/.claude/settings.json"
check "symlinked settings target keeps user keys and gains five deny rules" \
    node -e 'const d=require(process.argv[1]); process.exit(d.kept===true&&d.permissions.deny.length===5?0:1)' "$LINK_SET_HOME/.claude/real-settings.json"

UNUSABLE_HOME="$ROOT/home-unusable-settings"
run_install "$UNUSABLE_HOME" -- > "$ROOT/out-unusable-install.txt" 2>&1 \
    || { cat "$ROOT/out-unusable-install.txt" >&2; fail "unusable-settings setup install exits 0"; }
ok "unusable-settings setup install exits 0"
printf '{invalid' > "$UNUSABLE_HOME/.claude/settings.json"
cp "$UNUSABLE_HOME/.claude/settings.json" "$ROOT/unusable-original"
run_install "$UNUSABLE_HOME" -- --uninstall > "$ROOT/out-unusable-un.txt" 2>&1 \
    || { cat "$ROOT/out-unusable-un.txt" >&2; fail "uninstall with unusable settings exits 0"; }
ok "uninstall with unusable settings exits 0"
check "uninstall with unusable settings warns instead of dying" \
    grep -qi 'warning:' "$ROOT/out-unusable-un.txt"
check "uninstall with unusable settings leaves the file byte-identical" \
    cmp -s "$UNUSABLE_HOME/.claude/settings.json" "$ROOT/unusable-original"
check "uninstall with unusable settings removes the service unit" \
    test ! -e "$UNUSABLE_HOME/.config/systemd/user/orbit-office.service"
check "uninstall with unusable settings removes the plugin (stub)" \
    test ! -e "$STUB_STATE/plugin-added"
check "uninstall with unusable settings removes tools" \
    test ! -e "$UNUSABLE_HOME/$TOOLS_REL"
check "uninstall with unusable settings keeps the workspace" \
    test -d "$UNUSABLE_HOME/orbit"

# Drive the real interactive entry point over a PTY. The stty wrapper emits
# a synchronization marker only AFTER the actual terminal has disabled echo;
# input is sent at that marker, so no sleep or timing race is involved.
REAL_STTY=$(command -v stty)
cat > "$BIN/stty" <<STTY
#!/bin/sh
"$REAL_STTY" "\$@" || exit 1
if [ "\${1:-}" = -echo ]; then printf '<echo-disabled>\\n'; fi
STTY
chmod +x "$BIN/stty"
PTY_HOME="$ROOT/pty-home"; mkdir -p "$PTY_HOME"
# Initialize stub registry isolation for this HOME before invoking Python.
printf '%s' "$PTY_HOME" > "$STUB_STATE/current-home"
rm -f "$STUB_STATE"/plugin-* "$STUB_STATE"/secret.*
if ! python3 - "$INSTALL_SH" "$PTY_HOME" "$BIN:$NODE_DIR:/usr/bin:/bin" "$STUB_STATE" "$MANIFEST" "$SCRIPT_DIR/../../gateway/plugins" "$FIXTURE" "$INIT_FIXTURES" <<'PY'
import errno
import os
import pty
import select
import subprocess
import sys
import termios
import time

script, home, path, state, manifest, gateway, fixture, init = sys.argv[1:]
env = dict(HOME=home, PATH=path, STUB_STATE=state, STUB_MANIFEST=manifest,
           STUB_GATEWAY_MANIFESTS=gateway, STUB_UV_ARCHIVE=fixture,
           STUB_INIT_FIXTURES=init, ORBIT_OFFICE_OS="linux", ORBIT_OFFICE_UV_URL=fixture)
master, slave = pty.openpty()
child = subprocess.Popen(["sh", script], env=env, stdin=slave, stdout=slave, stderr=slave)
transcript = b""
consented = False
sent = 0
values = [b"synthetic-meta-key", b"synthetic-private-token"]
deadline = time.monotonic() + 30
try:
    while time.monotonic() < deadline:
        ready, _, _ = select.select([master], [], [], max(0, deadline-time.monotonic()))
        if not ready:
            raise AssertionError("interactive installer did not finish")
        try:
            block = os.read(master, 65536)
        except OSError as error:
            if error.errno == errno.EIO:
                break
            raise
        if not block:
            break
        transcript += block
        if not consented and b"[y/N]" in transcript:
            assert b"office gateway" in transcript
            os.write(master, b"yes\n")
            consented = True
        markers = transcript.count(b"<echo-disabled>")
        while sent < markers:
            assert sent < 2
            assert not termios.tcgetattr(slave)[3] & termios.ECHO
            os.write(master, values[sent] + b"\n")
            sent += 1
        # The slave stays open in the parent for echo inspection; after exit
        # the final summary provides the deterministic completion marker.
        if b"summary: purge" in transcript:
            break
    assert child.wait(timeout=5) == 0
    assert consented and transcript.count(b"[y/N]") == 1 and sent == 2
    assert b"Enter open_weights_token (hidden" in transcript
    assert b"baseten_token" not in transcript
    assert all(value not in transcript for value in values)
    assert termios.tcgetattr(slave)[3] & termios.ECHO
    with open(os.path.join(state, "orbit.log")) as log:
        assert "plugin secret set office open_weights_token\n" in log.read()
    assert os.path.isfile(os.path.join(state, "secret.open_weights_token"))
except Exception:
    sys.stderr.buffer.write(transcript)
    raise
finally:
    if child.poll() is None:
        child.kill()
        child.wait()
    os.close(master)
    os.close(slave)
PY
then fail "interactive consent and private token remain hidden"; fi
ok "one interactive consent enables office and gateway; open_weights_token is hidden and stored"

# ---------------------------------------------------------------------------
# 15. Pre-existing same-named skills survive install and uninstall for
#     both agents; only recorded, unmodified copies are removed.
# ---------------------------------------------------------------------------

for _agent in claude codex; do
    SK_HOME="$ROOT/home-skill-$_agent"
    SK_DIR="$SK_HOME/.$_agent/skills"
    mkdir -p "$SK_DIR/library-conventions"
    printf 'user-authored %s library-conventions\nunique bytes\n' "$_agent" \
        > "$SK_DIR/library-conventions/SKILL.md"
    printf 'user extra file %s\n' "$_agent" > "$SK_DIR/library-conventions/extra.txt"
    cp -R "$SK_DIR/library-conventions" "$ROOT/user-skill-$_agent"
    run_install "$SK_HOME" -- --agent "$_agent" > "$ROOT/out-skill-$_agent.txt" 2>&1 \
        || { cat "$ROOT/out-skill-$_agent.txt" >&2; fail "skill $_agent install exits 0"; }
    ok "skill $_agent install exits 0"
    check "$_agent pre-existing skill bytes unchanged by install" \
        sh -c 'cmp -s "$1/SKILL.md" "$2/SKILL.md" && cmp -s "$1/extra.txt" "$2/extra.txt"' \
        sh "$ROOT/user-skill-$_agent" "$SK_DIR/library-conventions"
    check "$_agent plugin skill installed beside the user skill" \
        test -f "$SK_DIR/orchestrator/SKILL.md"
    printf 'user edit after install\n' >> "$SK_DIR/meetings/SKILL.md"
    printf 'user added file\n' > "$SK_DIR/meetings/user-added.txt"
    cp "$SK_DIR/meetings/SKILL.md" "$ROOT/modified-skill-$_agent.md"
    run_install "$SK_HOME" -- --agent "$_agent" --uninstall > "$ROOT/out-skill-$_agent-un.txt" 2>&1 \
        || { cat "$ROOT/out-skill-$_agent-un.txt" >&2; fail "skill $_agent uninstall exits 0"; }
    ok "skill $_agent uninstall exits 0"
    check "$_agent pre-existing skill bytes survive uninstall" \
        sh -c 'cmp -s "$1/SKILL.md" "$2/SKILL.md" && cmp -s "$1/extra.txt" "$2/extra.txt"' \
        sh "$ROOT/user-skill-$_agent" "$SK_DIR/library-conventions"
    check "$_agent unmodified installed skill removed by uninstall" \
        test ! -e "$SK_DIR/orchestrator"
    check "$_agent user-modified installed skill preserved by uninstall" \
        cmp -s "$SK_DIR/meetings/SKILL.md" "$ROOT/modified-skill-$_agent.md"
    check "$_agent file added to an installed skill preserved by uninstall" \
        grep -qx "user added file" "$SK_DIR/meetings/user-added.txt"
    check "$_agent uninstall says the modified skill was kept" \
        grep -F "kept modified skill meetings" "$ROOT/out-skill-$_agent-un.txt"
    run_install "$SK_HOME" -- --agent "$_agent" --uninstall > "$ROOT/out-skill-$_agent-un2.txt" 2>&1 \
        || { cat "$ROOT/out-skill-$_agent-un2.txt" >&2; fail "skill $_agent repeated uninstall exits 0"; }
    ok "skill $_agent repeated uninstall exits 0"
    check "$_agent repeated uninstall still preserves the user skill" \
        sh -c 'cmp -s "$1/SKILL.md" "$2/SKILL.md" && cmp -s "$1/extra.txt" "$2/extra.txt"' \
        sh "$ROOT/user-skill-$_agent" "$SK_DIR/library-conventions"
    check "$_agent repeated uninstall still preserves the modified skill" \
        cmp -s "$SK_DIR/meetings/SKILL.md" "$ROOT/modified-skill-$_agent.md"
done

# ---------------------------------------------------------------------------
# 16. Orbit and Node in a temporary non-system prefix: the generated
#     runtime environments must still resolve and execute them.
#     The custom orbit is a Node script (as an npm-installed Orbit is),
#     so it needs both its own directory and node's on PATH.
# ---------------------------------------------------------------------------

CUSTOM_PREFIX="$ROOT/custom-prefix"
CUSTOM_BIN="$CUSTOM_PREFIX/bin"
mkdir -p "$CUSTOM_BIN"
for _stub in npm systemctl launchctl ffmpeg curl tar uv; do
    cp "$BIN/$_stub" "$CUSTOM_BIN/$_stub"
done
chmod +x "$CUSTOM_BIN"/*
ln -s "$(command -v node)" "$CUSTOM_BIN/node"
cat > "$CUSTOM_BIN/orbit" <<STUB
#!/usr/bin/env node
if (process.argv[2] === "--version") { process.stdout.write("orbit 0.37.0\n"); process.exit(0); }
const { spawnSync } = require("child_process");
const r = spawnSync("/bin/sh", ["$BIN/orbit", ...process.argv.slice(2)], { stdio: "inherit" });
process.exit(r.status === null ? 1 : r.status);
STUB
chmod +x "$CUSTOM_BIN/orbit"
cp "$CUSTOM_BIN/orbit" "$ROOT/custom-orbit-before"

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

CUSTOM_HOME="$ROOT/home-custom-prefix"; mkdir -p "$CUSTOM_HOME"
OUT_CUSTOM_RT="$ROOT/out-custom-runtime.txt"
(TEST_PATH="$CUSTOM_BIN:/usr/bin:/bin" run_install "$CUSTOM_HOME" -- > "$OUT_CUSTOM_RT" 2>&1) \
    || { cat "$OUT_CUSTOM_RT" >&2; fail "custom-prefix install exits 0"; }
ok "custom-prefix install exits 0"
CLAUDE_RT_PATH=$(node -e 'const fs=require("fs"); const d=JSON.parse(fs.readFileSync(process.argv[1],"utf8")); process.stdout.write(d.mcpServers.orbit.env.PATH)' "$CUSTOM_HOME/.claude.json") \
    || fail "custom-prefix: Claude MCP PATH is readable"
check_runtime_path "Claude MCP (linux, custom prefix)" "$CLAUDE_RT_PATH" "$CUSTOM_HOME"
SYSTEMD_RT_PATH=$(sed -n 's/^Environment=PATH=//p' "$CUSTOM_HOME/.config/systemd/user/orbit-office.service" | head -n 1)
check_runtime_path "systemd service (custom prefix)" "$SYSTEMD_RT_PATH" "$CUSTOM_HOME"
snapshot "$CUSTOM_HOME" > "$ROOT/snap-custom1.txt"
(TEST_PATH="$CUSTOM_BIN:/usr/bin:/bin" run_install "$CUSTOM_HOME" -- > "$ROOT/out-custom-runtime2.txt" 2>&1) \
    || { cat "$ROOT/out-custom-runtime2.txt" >&2; fail "custom-prefix second install exits 0"; }
ok "custom-prefix second install exits 0"
snapshot "$CUSTOM_HOME" > "$ROOT/snap-custom2.txt"
check "custom-prefix second run changes nothing in HOME" cmp -s "$ROOT/snap-custom1.txt" "$ROOT/snap-custom2.txt"

CUSTOM_MAC_HOME="$ROOT/home-custom-prefix-mac"; mkdir -p "$CUSTOM_MAC_HOME"
OUT_CUSTOM_MAC="$ROOT/out-custom-runtime-mac.txt"
(TEST_PATH="$CUSTOM_BIN:/usr/bin:/bin" run_install "$CUSTOM_MAC_HOME" ORBIT_OFFICE_OS=macos -- --agent codex > "$OUT_CUSTOM_MAC" 2>&1) \
    || { cat "$OUT_CUSTOM_MAC" >&2; fail "custom-prefix macos codex install exits 0"; }
ok "custom-prefix macos codex install exits 0"
LAUNCHD_RT_PATH=$(sed -n 's/.*<key>PATH<\/key><string>\([^<]*\)<\/string>.*/\1/p' "$CUSTOM_MAC_HOME/Library/LaunchAgents/com.orbit-office.app.plist" | head -n 1)
check_runtime_path "launchd service (custom prefix)" "$LAUNCHD_RT_PATH" "$CUSTOM_MAC_HOME"
CODEX_RT_PATH=$(sed -n 's/^env = { PATH = "\(.*\)" }$/\1/p' "$CUSTOM_MAC_HOME/.codex/config.toml" | head -n 1)
check_runtime_path "Codex MCP (macos, custom prefix)" "$CODEX_RT_PATH" "$CUSTOM_MAC_HOME"
snapshot "$CUSTOM_MAC_HOME" > "$ROOT/snap-custom-mac1.txt"
(TEST_PATH="$CUSTOM_BIN:/usr/bin:/bin" run_install "$CUSTOM_MAC_HOME" ORBIT_OFFICE_OS=macos -- --agent codex > "$ROOT/out-custom-runtime-mac2.txt" 2>&1) \
    || { cat "$ROOT/out-custom-runtime-mac2.txt" >&2; fail "custom-prefix macos second install exits 0"; }
ok "custom-prefix macos second install exits 0"
snapshot "$CUSTOM_MAC_HOME" > "$ROOT/snap-custom-mac2.txt"
check "custom-prefix macos second run changes nothing in HOME" \
    cmp -s "$ROOT/snap-custom-mac1.txt" "$ROOT/snap-custom-mac2.txt"
check "existing Orbit in the non-system prefix is left unchanged" \
    cmp -s "$CUSTOM_BIN/orbit" "$ROOT/custom-orbit-before"
check "existing Node in the non-system prefix is left unchanged" test -L "$CUSTOM_BIN/node"

# ---------------------------------------------------------------------------
# 17. Standalone install: no adjacent app tree
# ---------------------------------------------------------------------------

STANDALONE_SRC="$ROOT/standalone-src"; mkdir -p "$STANDALONE_SRC"
cp "$INSTALL_SH" "$STANDALONE_SRC/install.sh"
APP_BUNDLE="$ROOT/app-bundle"
mkdir -p "$APP_BUNDLE/packages/server/dist" "$APP_BUNDLE/packages/contracts/dist"
printf '{"name":"orbit-office-app","type":"module"}\n' > "$APP_BUNDLE/package.json"
printf 'console.log("orbit-office standalone stub app");\n' > "$APP_BUNDLE/packages/server/dist/main.js"
printf '// app runtime file\n' > "$APP_BUNDLE/packages/contracts/dist/index.js"
INVALID_APP_BUNDLE="$ROOT/app-bundle-invalid"
mkdir -p "$INVALID_APP_BUNDLE/packages/server/dist"

SA_NOAPP="$ROOT/home-standalone-noapp"; mkdir -p "$SA_NOAPP"
OUT_SA_NOAPP="$ROOT/out-standalone-noapp.txt"
if run_install_standalone "$SA_NOAPP" "ORBIT_OFFICE_PLUGIN_BUNDLE=$BUNDLE" -- > "$OUT_SA_NOAPP" 2>&1; then
    fail "standalone without an app bundle stops the install"
else
    ok "standalone without an app bundle stops the install"
fi
check "standalone failure names ORBIT_OFFICE_APP_BUNDLE" \
    grep -F "ORBIT_OFFICE_APP_BUNDLE" "$OUT_SA_NOAPP"
check "standalone failure names the missing entrypoint" \
    grep -F "packages/server/dist/main.js" "$OUT_SA_NOAPP"
check "standalone failure writes no systemd unit" \
    test ! -e "$SA_NOAPP/.config/systemd/user/orbit-office.service"
check "standalone failure claims no app success URL" \
    sh -c '! grep -qF "app: http://localhost" "$1"' sh "$OUT_SA_NOAPP"
check "standalone failure leaves no service marker" \
    sh -c '! grep -qx service_installed "$1" 2>/dev/null' sh "$SA_NOAPP/.local/share/orbit-office/install-state"

SA_BAD="$ROOT/home-standalone-badapp"; mkdir -p "$SA_BAD"
OUT_SA_BAD="$ROOT/out-standalone-badapp.txt"
if run_install_standalone "$SA_BAD" "ORBIT_OFFICE_PLUGIN_BUNDLE=$BUNDLE" "ORBIT_OFFICE_APP_BUNDLE=$INVALID_APP_BUNDLE" -- > "$OUT_SA_BAD" 2>&1; then
    fail "standalone with an invalid app bundle stops the install"
else
    ok "standalone with an invalid app bundle stops the install"
fi
check "invalid bundle failure names the missing entrypoint" \
    grep -F "packages/server/dist/main.js" "$OUT_SA_BAD"
check "invalid bundle writes no systemd unit" \
    test ! -e "$SA_BAD/.config/systemd/user/orbit-office.service"

SA_HOME="$ROOT/home-standalone-app"; mkdir -p "$SA_HOME"
OUT_SA="$ROOT/out-standalone-app.txt"
run_install_standalone "$SA_HOME" "ORBIT_OFFICE_PLUGIN_BUNDLE=$BUNDLE" "ORBIT_OFFICE_APP_BUNDLE=$APP_BUNDLE" -- > "$OUT_SA" 2>&1 \
    || { cat "$OUT_SA" >&2; fail "standalone with an app bundle exits 0"; }
ok "standalone with an app bundle exits 0"
SA_ENTRY="$SA_HOME/.local/share/orbit-office/app/packages/server/dist/main.js"
check "standalone provisions the service entrypoint" test -f "$SA_ENTRY"
check "standalone provisions required runtime files" \
    test -f "$SA_HOME/.local/share/orbit-office/app/packages/contracts/dist/index.js"
check "standalone entrypoint is runnable under node" \
    sh -c '"$1" "$2" | grep -q "orbit-office standalone stub app"' sh "$(command -v node)" "$SA_ENTRY"
check "standalone systemd unit points at the provisioned entrypoint" \
    grep -F "$SA_ENTRY" "$SA_HOME/.config/systemd/user/orbit-office.service"
snapshot "$SA_HOME" > "$ROOT/snap-standalone1.txt"
OUT_SA2="$ROOT/out-standalone-app2.txt"
run_install_standalone "$SA_HOME" "ORBIT_OFFICE_PLUGIN_BUNDLE=$BUNDLE" -- > "$OUT_SA2" 2>&1 \
    || { cat "$OUT_SA2" >&2; fail "standalone rerun without a bundle exits 0 on a complete install"; }
ok "standalone rerun without a bundle exits 0 on a complete install"
snapshot "$SA_HOME" > "$ROOT/snap-standalone2.txt"
check "standalone complete rerun changes nothing" cmp -s "$ROOT/snap-standalone1.txt" "$ROOT/snap-standalone2.txt"
OUT_SA3="$ROOT/out-standalone-app3.txt"
run_install_standalone "$SA_HOME" "ORBIT_OFFICE_PLUGIN_BUNDLE=$BUNDLE" "ORBIT_OFFICE_APP_BUNDLE=$APP_BUNDLE" -- > "$OUT_SA3" 2>&1 \
    || { cat "$OUT_SA3" >&2; fail "standalone rerun with the same bundle exits 0"; }
snapshot "$SA_HOME" > "$ROOT/snap-standalone3.txt"
check "standalone bundle rerun changes nothing" cmp -s "$ROOT/snap-standalone1.txt" "$ROOT/snap-standalone3.txt"

rm -f "$SA_ENTRY"
OUT_SA_PARTIAL="$ROOT/out-standalone-partial.txt"
if run_install_standalone "$SA_HOME" "ORBIT_OFFICE_PLUGIN_BUNDLE=$BUNDLE" -- > "$OUT_SA_PARTIAL" 2>&1; then
    fail "standalone partial install without a bundle stops the install"
else
    ok "standalone partial install without a bundle stops the install"
fi
check "partial failure names the concrete missing entrypoint" \
    grep -F "$SA_ENTRY" "$OUT_SA_PARTIAL"
OUT_SA_REPAIR="$ROOT/out-standalone-repair.txt"
run_install_standalone "$SA_HOME" "ORBIT_OFFICE_PLUGIN_BUNDLE=$BUNDLE" "ORBIT_OFFICE_APP_BUNDLE=$APP_BUNDLE" -- > "$OUT_SA_REPAIR" 2>&1 \
    || { cat "$OUT_SA_REPAIR" >&2; fail "standalone partial install is repaired by the bundle"; }
ok "standalone partial install is repaired by the bundle"
check "repaired entrypoint exists and runs" \
    sh -c '"$1" "$2" | grep -q "orbit-office standalone stub app"' sh "$(command -v node)" "$SA_ENTRY"

# 18. Failed service activation is never marked complete; the next run
#     retries, and an inactive installed service is re-enabled.
#     Stub managers only; no host service manager is contacted.
# ---------------------------------------------------------------------------

# --- systemd: failing manager stops, leaves no marker, retry completes ---
HOME_SVC_FAIL="$ROOT/home-svc-fail"; mkdir -p "$HOME_SVC_FAIL"
rm -f "$STUB_STATE/svc-enabled" "$STUB_STATE/svc-active" "$STUB_STATE/launchd-loaded"
: > "$STUB_STATE/systemctl.log"; : > "$STUB_STATE/launchctl.log"
OUT_SVC_FAIL="$ROOT/out-svc-fail.txt"
if run_install "$HOME_SVC_FAIL" STUB_SYSTEMCTL_FAIL=1 -- > "$OUT_SVC_FAIL" 2>&1; then
    cat "$OUT_SVC_FAIL" >&2; fail "failing systemctl stops the install"
else
    ok "failing systemctl stops the install"
fi
check "failing systemctl reports the service" grep -F "service:" "$OUT_SVC_FAIL"
check "failing systemctl leaves no service_installed marker" \
    sh -c '! grep -qx service_installed "$1"' sh "$HOME_SVC_FAIL/.local/share/orbit-office/install-state"
check "failing systemctl does not report service success" \
    sh -c '! grep -F "service installed" "$1" && ! grep -F "service already enabled" "$1"' sh "$OUT_SVC_FAIL"
check "failing systemctl attempted activation" test -s "$STUB_STATE/systemctl.log"
SVC_CALLS_FAIL=$(wc -l < "$STUB_STATE/systemctl.log")
OUT_SVC_RETRY="$ROOT/out-svc-retry.txt"
run_install "$HOME_SVC_FAIL" -- > "$OUT_SVC_RETRY" 2>&1 \
    || { cat "$OUT_SVC_RETRY" >&2; fail "systemd retry after failure exits 0"; }
ok "systemd retry after failure exits 0"
check "systemd retry records the marker" \
    grep -qx service_installed "$HOME_SVC_FAIL/.local/share/orbit-office/install-state"
check "systemd retry reports service installed" grep -F "service installed" "$OUT_SVC_RETRY"
check "systemd retry made new manager calls" \
    sh -c '[ "$(wc -l < "$1")" -gt "$2" ]' sh "$STUB_STATE/systemctl.log" "$SVC_CALLS_FAIL"
check "systemd retry leaves the service active" test -f "$STUB_STATE/svc-active"

# --- systemd: already installed but inactive is re-enabled ---
rm -f "$STUB_STATE/svc-active"
OUT_SVC_REPAIR="$ROOT/out-svc-repair.txt"
run_install "$HOME_SVC_FAIL" -- > "$OUT_SVC_REPAIR" 2>&1 \
    || { cat "$OUT_SVC_REPAIR" >&2; fail "inactive systemd service is repaired"; }
ok "inactive systemd service is repaired"
check "systemd repair re-enables the service" grep -F "service re-enabled" "$OUT_SVC_REPAIR"
check "systemd repair restores the active state" test -f "$STUB_STATE/svc-active"
check "systemd repair keeps the marker" \
    grep -qx service_installed "$HOME_SVC_FAIL/.local/share/orbit-office/install-state"

# --- systemd: missing manager stops, leaves no marker, retry completes ---
# The host may provide a real systemctl, so hide every host manager behind
# a tool dir of symlinks; the stub dir then decides what is visible.
HIDE_BASE="$ROOT/hide-base"; mkdir -p "$HIDE_BASE"
if [ ! -e "$HIDE_BASE/.done" ]; then
    for _d in /usr/bin /bin; do
        for _f in "$_d"/*; do
            [ -e "$_f" ] || continue
            _b=$(basename -- "$_f")
            case "$_b" in systemctl|launchctl) continue ;; esac
            [ -e "$HIDE_BASE/$_b" ] || ln -s "$_f" "$HIDE_BASE/$_b"
        done
    done
    : > "$HIDE_BASE/.done"
fi
BIN_NOSYS="$ROOT/bin-no-sys"; mkdir -p "$BIN_NOSYS"
for _stub in orbit npm launchctl ffmpeg curl tar uv; do cp "$BIN/$_stub" "$BIN_NOSYS/$_stub"; done
chmod +x "$BIN_NOSYS"/*
HOME_SVC_MISSING="$ROOT/home-svc-missing"; mkdir -p "$HOME_SVC_MISSING"
rm -f "$STUB_STATE/svc-enabled" "$STUB_STATE/svc-active"
: > "$STUB_STATE/systemctl.log"
OUT_SVC_MISSING="$ROOT/out-svc-missing.txt"
if (TEST_PATH="$BIN_NOSYS:$NODE_DIR:$HIDE_BASE" \
    run_install "$HOME_SVC_MISSING" -- > "$OUT_SVC_MISSING" 2>&1); then
    cat "$OUT_SVC_MISSING" >&2; fail "missing systemctl stops the install"
else
    ok "missing systemctl stops the install"
fi
check "missing systemctl names the manager" grep -F "systemctl is not available" "$OUT_SVC_MISSING"
check "missing systemctl leaves no marker" \
    sh -c '! grep -qx service_installed "$1"' sh "$HOME_SVC_MISSING/.local/share/orbit-office/install-state"
check "missing systemctl does not report service success" \
    sh -c '! grep -F "service installed" "$1" && ! grep -F "service already enabled" "$1"' sh "$OUT_SVC_MISSING"
OUT_SVC_MISSING_RETRY="$ROOT/out-svc-missing-retry.txt"
run_install "$HOME_SVC_MISSING" -- > "$OUT_SVC_MISSING_RETRY" 2>&1 \
    || { cat "$OUT_SVC_MISSING_RETRY" >&2; fail "systemd retry after missing manager exits 0"; }
ok "systemd retry after missing manager exits 0"
check "systemd retry after missing manager records the marker" \
    grep -qx service_installed "$HOME_SVC_MISSING/.local/share/orbit-office/install-state"

# --- launchd: failing manager stops, leaves no marker, retry completes ---
# macOS keeps its data folder under Library/Application Support, so pin a
# space-free override for the state assertions below.
HOME_L_FAIL="$ROOT/home-launchd-fail"; mkdir -p "$HOME_L_FAIL"
L_FAIL_DATA="$HOME_L_FAIL/custom-data"
rm -f "$STUB_STATE/launchd-loaded" "$STUB_STATE/svc-enabled" "$STUB_STATE/svc-active"
: > "$STUB_STATE/launchctl.log"
OUT_L_FAIL="$ROOT/out-launchd-fail.txt"
if run_install "$HOME_L_FAIL" ORBIT_OFFICE_OS=macos "ORBIT_OFFICE_DATA_DIR=$L_FAIL_DATA" STUB_LAUNCHCTL_FAIL=1 -- \
    > "$OUT_L_FAIL" 2>&1; then
    cat "$OUT_L_FAIL" >&2; fail "failing launchctl stops the install"
else
    ok "failing launchctl stops the install"
fi
check "failing launchctl reports the service" grep -F "service:" "$OUT_L_FAIL"
check "failing launchctl leaves no marker" \
    sh -c '! grep -qx service_installed "$1"' sh "$L_FAIL_DATA/install-state"
check "failing launchctl does not report service success" \
    sh -c '! grep -F "service installed" "$1" && ! grep -F "service already done" "$1"' sh "$OUT_L_FAIL"
check "failing launchctl attempted activation" test -s "$STUB_STATE/launchctl.log"
L_CALLS_FAIL=$(wc -l < "$STUB_STATE/launchctl.log")
OUT_L_RETRY="$ROOT/out-launchd-retry.txt"
run_install "$HOME_L_FAIL" ORBIT_OFFICE_OS=macos "ORBIT_OFFICE_DATA_DIR=$L_FAIL_DATA" -- > "$OUT_L_RETRY" 2>&1 \
    || { cat "$OUT_L_RETRY" >&2; fail "launchd retry after failure exits 0"; }
ok "launchd retry after failure exits 0"
check "launchd retry records the marker" \
    grep -qx service_installed "$L_FAIL_DATA/install-state"
check "launchd retry reports service installed" grep -F "service installed" "$OUT_L_RETRY"
check "launchd retry made new manager calls" \
    sh -c '[ "$(wc -l < "$1")" -gt "$2" ]' sh "$STUB_STATE/launchctl.log" "$L_CALLS_FAIL"
check "launchd retry leaves the agent loaded" test -f "$STUB_STATE/launchd-loaded"

# --- launchd: already installed but unloaded is re-enabled ---
rm -f "$STUB_STATE/launchd-loaded"
OUT_L_REPAIR="$ROOT/out-launchd-repair.txt"
run_install "$HOME_L_FAIL" ORBIT_OFFICE_OS=macos "ORBIT_OFFICE_DATA_DIR=$L_FAIL_DATA" -- > "$OUT_L_REPAIR" 2>&1 \
    || { cat "$OUT_L_REPAIR" >&2; fail "unloaded launchd agent is repaired"; }
ok "unloaded launchd agent is repaired"
check "launchd repair re-enables the agent" grep -F "service re-enabled" "$OUT_L_REPAIR"
check "launchd repair restores the loaded state" test -f "$STUB_STATE/launchd-loaded"

# --- launchd: missing manager stops, leaves no marker, retry completes ---
BIN_NOLAUNCH="$ROOT/bin-no-launch"; mkdir -p "$BIN_NOLAUNCH"
for _stub in orbit npm systemctl ffmpeg curl tar uv; do cp "$BIN/$_stub" "$BIN_NOLAUNCH/$_stub"; done
chmod +x "$BIN_NOLAUNCH"/*
HOME_L_MISSING="$ROOT/home-launchd-missing"; mkdir -p "$HOME_L_MISSING"
L_MISSING_DATA="$HOME_L_MISSING/custom-data"
rm -f "$STUB_STATE/launchd-loaded"
: > "$STUB_STATE/launchctl.log"
OUT_L_MISSING="$ROOT/out-launchd-missing.txt"
if (TEST_PATH="$BIN_NOLAUNCH:$NODE_DIR:$HIDE_BASE" \
    run_install "$HOME_L_MISSING" ORBIT_OFFICE_OS=macos "ORBIT_OFFICE_DATA_DIR=$L_MISSING_DATA" -- > "$OUT_L_MISSING" 2>&1); then
    cat "$OUT_L_MISSING" >&2; fail "missing launchctl stops the install"
else
    ok "missing launchctl stops the install"
fi
check "missing launchctl names the manager" grep -F "launchctl is not available" "$OUT_L_MISSING"
check "missing launchctl leaves no marker" \
    sh -c '! grep -qx service_installed "$1"' sh "$L_MISSING_DATA/install-state"
OUT_L_MISSING_RETRY="$ROOT/out-launchd-missing-retry.txt"
run_install "$HOME_L_MISSING" ORBIT_OFFICE_OS=macos "ORBIT_OFFICE_DATA_DIR=$L_MISSING_DATA" -- > "$OUT_L_MISSING_RETRY" 2>&1 \
    || { cat "$OUT_L_MISSING_RETRY" >&2; fail "launchd retry after missing manager exits 0"; }
ok "launchd retry after missing manager exits 0"
check "launchd retry after missing manager records the marker" \
    grep -qx service_installed "$L_MISSING_DATA/install-state"

# ---------------------------------------------------------------------------
# 19. Claude MCP registration is read back through agentCliStatus, honours
#     CLAUDE_CONFIG_DIR, and uninstall removes only installer-owned entries
# ---------------------------------------------------------------------------

HOME19="$ROOT/home19"; mkdir -p "$HOME19"
run_install "$HOME19" -- > "$ROOT/out19.txt" 2>&1 \
    || { cat "$ROOT/out19.txt" >&2; fail "claude .claude.json install exits 0"; }
ok "claude .claude.json install exits 0"
_status19=$(agent_cli_status "$HOME19") || fail "agentCliStatus runs for home19"
[ "$_status19" = '{"cli":"Claude Code","connected":true}' ] \
    || { printf '%s\n' "$_status19" >&2; fail "agentCliStatus connected (default)"; }
ok "agentCliStatus connected (default)"

HOME19B="$ROOT/home19b"; mkdir -p "$HOME19B"
CLAUDE_DIR19="$HOME19B/custom-claude"
run_install "$HOME19B" "CLAUDE_CONFIG_DIR=$CLAUDE_DIR19" -- > "$ROOT/out19b.txt" 2>&1 \
    || { cat "$ROOT/out19b.txt" >&2; fail "claude CLAUDE_CONFIG_DIR install exits 0"; }
ok "claude CLAUDE_CONFIG_DIR install exits 0"
check "CLAUDE_CONFIG_DIR registration lands in its .claude.json" \
    grep -q '"orbit"' "$CLAUDE_DIR19/.claude.json"
check "CLAUDE_CONFIG_DIR install writes no default .claude.json" \
    test ! -e "$HOME19B/.claude.json"
_status19b=$(agent_cli_status "$HOME19B" "$CLAUDE_DIR19") || fail "agentCliStatus runs for override"
[ "$_status19b" = '{"cli":"Claude Code","connected":true}' ] \
    || { printf '%s\n' "$_status19b" >&2; fail "agentCliStatus connected (CLAUDE_CONFIG_DIR)"; }
ok "agentCliStatus connected (CLAUDE_CONFIG_DIR)"
run_install "$HOME19B" "CLAUDE_CONFIG_DIR=$CLAUDE_DIR19" -- --uninstall > "$ROOT/out19b-un.txt" 2>&1 \
    || { cat "$ROOT/out19b-un.txt" >&2; fail "claude CLAUDE_CONFIG_DIR uninstall exits 0"; }
check "CLAUDE_CONFIG_DIR uninstall removes owned entry" \
    sh -c '! grep -q "\"orbit\"" "$1"' sh "$CLAUDE_DIR19/.claude.json"

# A user-owned exact entry is not claimed: capture the installer entry,
# restore it as a pre-existing file with an unrelated key, reinstall, and
# uninstall must keep both.
HOME19C="$ROOT/home19c"; mkdir -p "$HOME19C"
run_install "$HOME19C" -- > "$ROOT/out19c.txt" 2>&1 \
    || { cat "$ROOT/out19c.txt" >&2; fail "user-owned setup install exits 0"; }
cp "$HOME19C/.claude.json" "$ROOT/user-owned-claude.json"
run_install "$HOME19C" -- --uninstall > "$ROOT/out19c-un.txt" 2>&1 \
    || { cat "$ROOT/out19c-un.txt" >&2; fail "user-owned setup uninstall exits 0"; }
node -e 'const fs=require("fs"); const d=JSON.parse(fs.readFileSync(process.argv[1],"utf8")); d.unrelated={"keep":true}; fs.writeFileSync(process.argv[1], JSON.stringify(d,null,2)+"\n")' "$ROOT/user-owned-claude.json"
cp "$ROOT/user-owned-claude.json" "$HOME19C/.claude.json"
run_install "$HOME19C" -- > "$ROOT/out19c2.txt" 2>&1 \
    || { cat "$ROOT/out19c2.txt" >&2; fail "user-owned reinstall exits 0"; }
check "pre-existing exact MCP entry is not claimed as owned" \
    grep -q '"owned":false' "$HOME19C/.local/share/orbit-office/claude-mcp.json"
run_install "$HOME19C" -- --uninstall > "$ROOT/out19c2-un.txt" 2>&1 \
    || { cat "$ROOT/out19c2-un.txt" >&2; fail "user-owned uninstall exits 0"; }
check "uninstall keeps a user-owned MCP entry" grep -q '"orbit"' "$HOME19C/.claude.json"
check "uninstall keeps unrelated .claude.json settings" grep -q '"unrelated"' "$HOME19C/.claude.json"

# Invalid .claude.json fails closed and is left byte-identical.
HOME19D="$ROOT/home19d"; mkdir -p "$HOME19D"
printf '{invalid' > "$HOME19D/.claude.json"
cp "$HOME19D/.claude.json" "$ROOT/bad-claude-json"
if run_install "$HOME19D" -- > "$ROOT/out19d.txt" 2>&1; then fail "invalid .claude.json must refuse"; fi
ok "invalid .claude.json refuses install"
check "invalid .claude.json remains byte-identical" cmp -s "$HOME19D/.claude.json" "$ROOT/bad-claude-json"
check "invalid .claude.json creates no MCP receipt" \
    test ! -e "$HOME19D/.local/share/orbit-office/claude-mcp.json"

# ---------------------------------------------------------------------------
# 20. Retired token env vars warn and still store the token (STD-01 R35)
# ---------------------------------------------------------------------------

HOME_OLD1="$ROOT/home-old-token-1"; mkdir -p "$HOME_OLD1"
run_install "$HOME_OLD1" ORBIT_OFFICE_BASETEN_TOKEN=old-token-must-not-echo-1 -- > "$ROOT/out-old1.txt" 2>&1 \
    || { cat "$ROOT/out-old1.txt" >&2; fail "ORBIT_OFFICE_BASETEN_TOKEN install exits 0"; }
ok "ORBIT_OFFICE_BASETEN_TOKEN install exits 0"
check "ORBIT_OFFICE_BASETEN_TOKEN warns naming the old variable and its replacement" \
    grep -F "warning: ORBIT_OFFICE_BASETEN_TOKEN is deprecated; use ORBIT_OFFICE_OPEN_WEIGHTS_TOKEN instead" "$ROOT/out-old1.txt"
check "ORBIT_OFFICE_BASETEN_TOKEN warning never echoes its value" \
    sh -c '! grep -q "old-token-must-not-echo-1" "$1"' sh "$ROOT/out-old1.txt"
check "ORBIT_OFFICE_BASETEN_TOKEN is stored as open_weights_token" \
    test -f "$STUB_STATE/secret.open_weights_token"

HOME_OLD2="$ROOT/home-old-token-2"; mkdir -p "$HOME_OLD2"
run_install "$HOME_OLD2" BASETEN_TOKEN=old-token-must-not-echo-2 -- > "$ROOT/out-old2.txt" 2>&1 \
    || { cat "$ROOT/out-old2.txt" >&2; fail "BASETEN_TOKEN install exits 0"; }
ok "BASETEN_TOKEN install exits 0"
check "BASETEN_TOKEN warns naming the old variable and its replacement" \
    grep -F "warning: BASETEN_TOKEN is deprecated; use OPEN_WEIGHTS_TOKEN instead" "$ROOT/out-old2.txt"
check "BASETEN_TOKEN warning never echoes its value" \
    sh -c '! grep -q "old-token-must-not-echo-2" "$1"' sh "$ROOT/out-old2.txt"
check "BASETEN_TOKEN is stored as open_weights_token" \
    test -f "$STUB_STATE/secret.open_weights_token"

printf 'PASS: %s checks\n' "$PASS"
