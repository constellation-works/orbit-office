#!/bin/sh
# build-install.sh - assemble the distributed install.sh from its modules.
#
# The hand-maintained sources live in scripts/install/lib/; install.sh at
# the repository root is generated, self-contained output. It is never
# split into files it sources at runtime, so the piped (curl | sh) and
# standalone-copy forms keep working with no adjacent files.
#
# Usage:
#   sh scripts/install/build-install.sh           regenerate install.sh
#   sh scripts/install/build-install.sh --check   fail if install.sh differs
set -eu

SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
LIB_DIR="$SCRIPT_DIR/lib"
ROOT_DIR=$(CDPATH='' cd -- "$SCRIPT_DIR/../.." && pwd)
OUTPUT="$ROOT_DIR/install.sh"
MODULES="helpers python agent steps"

usage() {
    printf '%s\n' "Usage: build-install.sh [--check]"
}

MODE="build"
case "${1:-}" in
    "") ;;
    --check) MODE="check" ;;
    -h|--help) usage; exit 0 ;;
    *) printf 'error: unknown option: %s\n' "$1" >&2; usage >&2; exit 2 ;;
esac
[ $# -le 1 ] || { printf 'error: too many arguments\n' >&2; usage >&2; exit 2; }

for _module in $MODULES; do
    [ -f "$LIB_DIR/$_module.sh" ] || {
        printf 'error: missing install module: %s\n' "$LIB_DIR/$_module.sh" >&2
        exit 1
    }
done

assemble() {
    printf '%s\n' '#!/bin/sh'
    printf '%s\n' '# GENERATED FILE - do not edit directly.'
    printf '%s\n' '# Sources: scripts/install/lib/helpers.sh, scripts/install/lib/python.sh,'
    printf '%s\n' '#          scripts/install/lib/agent.sh, scripts/install/lib/steps.sh'
    printf '%s\n' '# Regenerate: sh scripts/install/build-install.sh'
    printf '%s\n' '# Check:      sh scripts/install/build-install.sh --check'
    for _module in $MODULES; do
        printf '\n'
        printf '%s\n' '# ---------------------------------------------------------------------------'
        printf '# Module: scripts/install/lib/%s.sh\n' "$_module"
        printf '%s\n' '# ---------------------------------------------------------------------------'
        printf '\n'
        cat "$LIB_DIR/$_module.sh"
    done
}

TMP_OUTPUT="$OUTPUT.tmp.$$"
trap 'rm -f "$TMP_OUTPUT"' EXIT INT TERM
assemble > "$TMP_OUTPUT"
chmod 755 "$TMP_OUTPUT"

if [ "$MODE" = "check" ]; then
    if cmp -s "$TMP_OUTPUT" "$OUTPUT"; then
        printf '%s\n' "install.sh is up to date"
        exit 0
    fi
    printf '%s\n' "error: install.sh is not identical to scripts/install/build-install.sh output" >&2
    printf '%s\n' "run: sh scripts/install/build-install.sh" >&2
    exit 1
fi

mv "$TMP_OUTPUT" "$OUTPUT"
trap - EXIT INT TERM
printf '%s\n' "wrote $OUTPUT"
