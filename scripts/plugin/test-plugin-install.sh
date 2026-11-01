#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
tmp=$(mktemp -d "${TMPDIR:-/tmp}/office-plugin-install-test.XXXXXX")
trap 'rm -rf "$tmp"' EXIT HUP INT TERM
mkdir -p "$tmp/bin" "$tmp/home" "$tmp/work"
export HOME=$tmp/home
export TMPDIR=$tmp/work
log=$tmp/orbit.log
: > "$log"
export ORBIT_STUB_LOG=$log
unset ORBIT_MANAGED_RUN_CONTEXT ORBIT_TASK_ACTOR_KIND 2>/dev/null || :

cat > "$tmp/bin/orbit" <<'STUB'
#!/bin/sh
echo "args:$* ORBIT_OPERATOR=${ORBIT_OPERATOR:-<unset>}" >> "$ORBIT_STUB_LOG"
if [ "$1 $2" = "plugin show" ]; then
    echo "office plugin"
    exit 0
fi
if [ "$1 $2" = "tool run" ]; then
    case "${STUB_INIT_MODE:-ok}" in
        ok) printf '%s\n' '{"already_initialized":true,"created":[]}'; exit 0 ;;
        ok-false) printf '%s\n' '{"ok":false,"error":{"code":"stub_init_failed"}}'; exit 0 ;;
        error) printf '%s\n' '{"error":{"code":"stub_init_failed"}}' >&2; exit 1 ;;
        *) echo "unknown STUB_INIT_MODE" >&2; exit 2 ;;
    esac
fi
exit 0
STUB
chmod +x "$tmp/bin/orbit"
export PATH="$tmp/bin:$PATH"

run_install() {
    make -s -C "$root" plugin-install MAKE=/bin/true "$@"
}

# With a workspace, office.init runs in that workspace after enable.
: > "$log"
STUB_INIT_MODE=ok run_install PLUGIN_WORKSPACE=test-ws > "$tmp/ok.log" 2>&1 \
    || { cat "$tmp/ok.log" >&2; echo 'plugin-install failed on ok office.init' >&2; exit 1; }
grep -F 'tool run office.init' "$log" >/dev/null \
    || { echo 'office.init was not called' >&2; cat "$log" >&2; exit 1; }
grep -F -- '--workspace test-ws' "$log" >/dev/null \
    || { echo 'office.init was not called with the workspace' >&2; cat "$log" >&2; exit 1; }
grep -F 'ORBIT_OPERATOR=1' "$log" >/dev/null \
    || { echo 'office.init was not called as operator' >&2; cat "$log" >&2; exit 1; }

# An {ok:false} envelope on stdout with exit 0 still fails the target.
: > "$log"
if STUB_INIT_MODE=ok-false run_install PLUGIN_WORKSPACE=test-ws > "$tmp/false.log" 2>&1; then
    echo 'plugin-install succeeded on an ok:false office.init envelope' >&2; exit 1
fi

# A non-zero exit from office.init fails the target.
: > "$log"
if STUB_INIT_MODE=error run_install PLUGIN_WORKSPACE=test-ws > "$tmp/error.log" 2>&1; then
    echo 'plugin-install succeeded when office.init exited non-zero' >&2; exit 1
fi

# Without a workspace, office.init is not called.
: > "$log"
STUB_INIT_MODE=ok run_install > "$tmp/nows.log" 2>&1 \
    || { cat "$tmp/nows.log" >&2; echo 'plugin-install failed without a workspace' >&2; exit 1; }
if grep -F 'tool run office.init' "$log" >/dev/null; then
    echo 'office.init was called without PLUGIN_WORKSPACE' >&2; exit 1
fi

# Inside a managed run the operator override is never added.
: > "$log"
STUB_INIT_MODE=ok ORBIT_TASK_ACTOR_KIND=agent run_install PLUGIN_WORKSPACE=test-ws > "$tmp/managed.log" 2>&1 \
    || { cat "$tmp/managed.log" >&2; echo 'plugin-install failed in managed mode' >&2; exit 1; }
if grep -F 'tool run office.init' "$log" | grep -F 'ORBIT_OPERATOR=1' >/dev/null; then
    echo 'office.init got the operator override inside a managed run' >&2; exit 1
fi

printf 'plugin install office.init: passed\n'
