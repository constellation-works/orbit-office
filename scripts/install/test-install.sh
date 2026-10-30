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
#  12. a non-interactive run without --yes prints grants and refuses;
#      --yes records explicit consent, including the exact agent verbs
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
#      prerequisite at a time (Library, plugin, Meta key, stopped app,
#      app bound to another workspace) naming its fix, no secret value
#      in output, open starting a stopped installed service via stub
#      managers and a stub browser, and the missing-service install.sh
#      error (linux and macos service shapes).
#  23. Muse Code (--agent muse): the Orbit MCP entry lands in
#      ${XDG_CONFIG_HOME:-~/.config}/muse/settings.json with tools/bin
#      first on PATH, other settings and servers preserved, skills in
#      ~/.agents/skills; a second run changes nothing; uninstall removes
#      only the installer-owned entry and keeps a pre-existing identical
#      one; invalid JSON refuses byte-identically.
#  22. an fnm_multishells node symlink on PATH resolves to the stable
#      node in both service units, with no fnm_multishells entry in the
#      unit PATH; a non-symlink fnm_multishells wrapper as the only node
#      fails naming the per-shell cause and writes neither unit.
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
. "$SCRIPT_DIR/test/lib.sh"


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
        printf 'orbit %s\n' "${STUB_ORBIT_VERSION:-0.39.0}" ;;
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
    permissions: ["fs", "network", ...(name === "cloudcli" ? [] : ["orbit_tools"]), ...(name === "office" ? ["agent_tools"] : [])].map(grant => ({grant,
      requested: grant === "agent_tools" ? (process.env.STUB_AGENT_VERBS || "attach,capture,proposal-create,selection-get") : null,
      granted_roots: grant === "fs" && present("narrow-fs") ? ["{{workspace}}/library", "{{plugin_state}}"] : null,
      granted: host && !(grant === "agent_tools" && present("missing-agent-consent"))}))});
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
                # Real Orbit needs .orbit-plugin/ at the repository root; this
                # repository keeps it under plugin/, so a git+ source fails.
                printf 'stub: git+ source has no plugin at its root\n' >&2; exit 1 ;;
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
        if [ "$_plugin" = office ]; then
            grep -qxF agent_tools "$state/plugin-grants$_suffix" || _missing="$_missing agent_tools"
            if [ -z "$_missing" ]; then : > "$state/agent-consented"; fi
        fi
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
        # STUB_LIST_UNBOUND_FAILS=1 mimics an Orbit host where a read-only
        # command from a folder with no .orbit fails before registration.
        if [ "${STUB_LIST_UNBOUND_FAILS:-0}" = 1 ] && [ ! -d .orbit ]; then
            printf '{"error":"workspace error: read-only generation join needs an existing task-registry binding","code":"workspace_error"}\n'; exit 1
        fi
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
            # Like Orbit, a mutating plugin tool from a caller without a TTY
            # needs the audited operator override.
            if [ "${ORBIT_OPERATOR:-}" != 1 ]; then
                printf '%s\n' '{"error":"capability denied: operation '"'"'plugin.tool.mutating'"'"' requires the `operator or runner` capability","code":"capability_denied"}' >&2
                exit 1
            fi
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

# cargo stub: records the commit it builds and writes a fake office binary
# into --target-dir, so the clone path is exercised without compiling Rust.
cat > "$BIN/cargo" <<'STUB'
#!/bin/sh
[ "${STUB_BUILD_FAIL:-0}" != 1 ] || { printf "stub: build failed\n" >&2; exit 1; }
_target=""
while [ $# -gt 0 ]; do
    case "$1" in --target-dir) _target="$2"; shift 2 ;; *) shift ;; esac
done
[ -n "$_target" ] || exit 1
printf 'build %s\n' "$(git rev-parse HEAD)" >> "$STUB_STATE/cargo.log"
mkdir -p "$_target/release"
printf '#!/bin/sh\nexit 0\n' > "$_target/release/office"
chmod +x "$_target/release/office"
STUB
chmod +x "$BIN/cargo"
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


. "$SCRIPT_DIR/test/scenarios_core.sh"
. "$SCRIPT_DIR/test/scenarios_distribution.sh"
. "$SCRIPT_DIR/test/scenarios_init_grants.sh"
. "$SCRIPT_DIR/test/scenarios_services_mcp.sh"
. "$SCRIPT_DIR/test/scenarios_status_skills.sh"

scenario_1
scenario_21
scenario_14
scenario_20_piped
scenario_2
scenario_3
scenario_4
scenario_5
scenario_6
scenario_7
scenario_8
scenario_9
scenario_10
scenario_11
scenario_12
scenario_13
scenario_15
scenario_16
scenario_17
scenario_18
scenario_19
scenario_20_retired
scenario_22
scenario_23

printf 'PASS: %s checks\n' "$PASS"
