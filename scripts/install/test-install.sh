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
#      owned workspace and refuses any other folder
#   4. a non-empty unrelated workspace folder is refused, including by
#      --purge --home
#   5. missing Orbit is installed from npm
#   6. a uv archive checksum mismatch stops before unpack
#   7. python3 --version must print the pinned minor
#   8. launchd, systemd and the MCP entry put tools/bin first on PATH
#   9. a Codex MCP entry uses an inline env table; uninstall removes it
#  10. an older Claude MCP entry gains tools/bin on PATH
#  11. an office.init error envelope fails the step and leaves no marker
#  12. a non-interactive run without --yes prints the grants, enables the
#      plugin with them and does not ask
# Usage: sh scripts/install/test-install.sh
set -eu

SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd) || exit 1
INSTALL_SH="$SCRIPT_DIR/../../install.sh"
FIXTURE="$SCRIPT_DIR/fixtures/uv-test-fixture.tar.gz"
[ -f "$INSTALL_SH" ] || { printf 'missing %s\n' "$INSTALL_SH" >&2; exit 1; }
[ -f "$FIXTURE" ] || { printf 'missing %s\n' "$FIXTURE" >&2; exit 1; }
REAL_TAR=$(command -v tar) || { printf 'tar is required\n' >&2; exit 1; }

ROOT=$(mktemp -d)
trap 'rm -rf "$ROOT"' EXIT INT TERM

PASS=0
ok() { PASS=$((PASS + 1)); printf 'ok %s - %s\n' "$PASS" "$1"; }
fail() { printf 'FAIL - %s\n' "$1" >&2; exit 1; }
check() { # check <label> <condition...>
    _label="$1"; shift
    if "$@" >/dev/null 2>&1; then ok "$_label"; else fail "$_label"; fi
}

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
        printf 'orbit %s\n' "${STUB_ORBIT_VERSION:-0.34.0}" ;;
    "plugin list")
        if [ -f "$state/plugin-added" ]; then
            printf 'office\t0.1.0\tenabled\tenabled\t3\tok\n'
        fi ;;
    "plugin add")
        : > "$state/plugin-added"; printf 'added\n' ;;
    "plugin enable")
        : > "$state/plugin-enabled"; printf 'enabled\n' ;;
    "plugin remove")
        rm -f "$state/plugin-added" "$state/plugin-enabled"
        printf 'removed\n' ;;
    "plugin disable")
        rm -f "$state/plugin-enabled"; printf 'disabled\n' ;;
    "plugin secret")
        case "${3:-}" in
            list)
                for n in meta_api_key baseten_token; do
                    if [ -f "$state/secret.$n" ]; then
                        printf '%s set\n' "$n"
                    else
                        printf '%s unset\n' "$n"
                    fi
                done ;;
            set)
                cat > /dev/null
                : > "$state/secret.$4" ;;
            *) exit 1 ;;
        esac ;;
    "workspace init")
        mkdir -p .orbit; printf 'initialized\n' ;;
    "workspace list")
        if [ -n "${STUB_WORKSPACE_LIST:-}" ]; then printf '%s\n' "$STUB_WORKSPACE_LIST"; fi ;;
    "tool run")
        # Replies with the office response envelope. STUB_INIT_MODE picks
        # the case: ok (default), already (ok, already_initialized) or error.
        if [ "${3:-}" = "office.init" ]; then
            case "${STUB_INIT_MODE:-ok}" in
                error)
                    printf '%s\n' '{"ok":false,"error":{"code":"library_dir_outside_workspace","message":"library_dir is outside the workspace","retryable":false}}' ;;
                already)
                    printf '{"ok":true,"output":{"already_initialized":true,"created":[]}}\n' ;;
                *)
                    mkdir -p library/.office library/.git
                    printf 'config\n' > library/.office/config.yaml
                    printf '{"ok":true,"output":{"already_initialized":false,"created":["library/.office/config.yaml"]}}\n' ;;
            esac
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
case " $* " in
    *" is-enabled "*) [ -f "$state/svc-enabled" ] ;;
    *" is-active "*) [ -f "$state/svc-active" ] ;;
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
exit 0
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
    "$BIN/curl" "$BIN/tar" "$BIN/uv"

snapshot() { # snapshot <home> : sorted relative paths + file hashes
    (cd "$1" && find . -type f | sort | while IFS= read -r _f; do
        printf '%s  %s\n' "$(sha256sum "$_f" 2>/dev/null | awk '{print $1}')" "$_f"
    done)
    (cd "$1" && find . -type d | sort)
}

run_install() { # run_install <home> <extra env...> -- <args...>
    _home="$1"; shift
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
    env -i PATH="${TEST_BIN:-$BIN}:$NODE_DIR:/usr/bin:/bin" HOME="$_home" STUB_STATE="$STUB_STATE" \
        STUB_ORBIT_VERSION="${STUB_ORBIT_VERSION:-0.34.0}" \
        STUB_UV_ARCHIVE="$FIXTURE" \
        STUB_CURL_MODE="${STUB_CURL_MODE:-ok}" \
        STUB_INIT_MODE="${STUB_INIT_MODE:-ok}" \
        STUB_ORBIT_SRC="$BIN/orbit" \
        ORBIT_OFFICE_OS=linux \
        $_uv_env $_py_env $_envs \
        sh "$INSTALL_SH" $_yes "$@" < /dev/null
}

TOOLS_REL=".local/share/orbit-office/tools"

assert_home_scope() { # files stay under the workspace, data/tools, service, MCP
    _home="$1"
    _bad="$ROOT/scope-bad.txt"
    : > "$_bad"
    (cd "$_home" && find . \( -type f -o -type l \) | sort) | while IFS= read -r _f; do
        case "$_f" in
            ./orbit|./orbit/*) ;;
            ./.local/share/orbit-office|./.local/share/orbit-office/*) ;;
            ./.config/systemd|./.config/systemd/*) ;;
            ./.claude|./.claude/*) ;;
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
    if [ -e "$_home/.local/bin" ]; then
        fail "~/.local/bin was created (system uv/python path)"
    fi
    ok "shell profiles and ~/.local/bin untouched"
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
check "workspace is a git repo" test -d "$HOME1/orbit/.git"
check "workspace registered (.orbit)" test -d "$HOME1/orbit/.orbit"
check "library scaffolded" test -f "$HOME1/orbit/library/.office/config.yaml"
check "systemd unit written" test -f "$HOME1/.config/systemd/user/orbit-office.service"
check "MCP entry written" grep -q '"orbit"' "$HOME1/.claude/settings.json"
check "plugin added (stub)" test -f "$STUB_STATE/plugin-added"
check "plugin enabled (stub)" test -f "$STUB_STATE/plugin-enabled"
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
    grep -F "\"PATH\": \"$TOOLS1/bin:" "$HOME1/.claude/settings.json"
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

# ---------------------------------------------------------------------------
# 2. Older Orbit stops; newer warns and continues
# ---------------------------------------------------------------------------

HOME2="$ROOT/home2"; mkdir -p "$HOME2"
OUT_OLD="$ROOT/out-old.txt"
if STUB_ORBIT_VERSION="0.33.0" run_install "$HOME2" -- > "$OUT_OLD" 2>&1; then
    fail "older Orbit stops the script"
else
    ok "older Orbit stops the script"
fi
check "older Orbit prints the upgrade command" \
    grep -q "npm install -g @orbit-tools/cli@0.34.0" "$OUT_OLD"
check "older Orbit: no workspace created" test ! -e "$HOME2/orbit"

HOME3="$ROOT/home3"; mkdir -p "$HOME3"
# Reset plugin stub state so this fresh HOME installs cleanly.
rm -f "$STUB_STATE/plugin-added" "$STUB_STATE/plugin-enabled"
OUT_NEW="$ROOT/out-new.txt"
STUB_ORBIT_VERSION="0.35.0" run_install "$HOME3" -- > "$OUT_NEW" 2>&1 \
    || { cat "$OUT_NEW" >&2; fail "newer Orbit continues"; }
ok "newer Orbit continues"
check "newer Orbit warns" grep -qi "newer" "$OUT_NEW"

# ---------------------------------------------------------------------------
# 3. Uninstall keeps ~/orbit; purge removes it
# ---------------------------------------------------------------------------

OUT_UN="$ROOT/out-un.txt"
run_install "$HOME1" -- --uninstall > "$OUT_UN" 2>&1 \
    || { cat "$OUT_UN" >&2; fail "uninstall exits 0"; }
ok "uninstall exits 0"
check "uninstall removes the service unit" \
    test ! -e "$HOME1/.config/systemd/user/orbit-office.service"
if grep -q '"orbit"' "$HOME1/.claude/settings.json" 2>/dev/null; then
    fail "uninstall removes the MCP entry"
else
    ok "uninstall removes the MCP entry"
fi
check "uninstall removes the plugin (stub)" test ! -e "$STUB_STATE/plugin-added"
check "uninstall keeps the workspace" test -d "$HOME1/orbit"
check "uninstall keeps the Library" test -d "$HOME1/orbit/library"
check "uninstall removes tools" test ! -e "$HOME1/$TOOLS_REL"

OUT_PURGE="$ROOT/out-purge.txt"
run_install "$HOME1" -- --purge > "$OUT_PURGE" 2>&1 \
    || { cat "$OUT_PURGE" >&2; fail "purge exits 0"; }
ok "purge exits 0"
check "purge removes the workspace" test ! -e "$HOME1/orbit"

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
TEST_BIN="$BIN2" run_install "$HOME5" -- > "$OUT_MISS" 2>&1 \
    || { cat "$OUT_MISS" >&2; fail "missing Orbit install exits 0"; }
ok "missing Orbit: npm installs it and the install completes"
check "missing Orbit: npm got the tested package" \
    grep -q "install -g @orbit-tools/cli@0.34.0" "$STUB_STATE/npm.log"
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
if STUB_CURL_MODE=mismatch run_install "$HOME6" "ORBIT_OFFICE_UV_SHA256=$BAD_SHA" -- \
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
if ORBIT_OFFICE_TEST_PYTHON_VERSION=3.11.2 run_install "$HOME7" -- \
    > "$OUT_VER" 2>&1; then
    fail "wrong python minor stops the step"
else
    ok "wrong python minor stops the step"
fi
check "version failure names the pinned minor" grep -F "pinned minor version 3.12" "$OUT_VER"
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
    grep -F "\"PATH\": \"$MAC_TOOLS/bin:" "$HOME8/.claude/settings.json"
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
    grep -F "\"PATH\": \"$HOME10/$TOOLS_REL/bin:" "$HOME10/.claude/settings.json"
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
# 11. An office.init error envelope fails the step and leaves no marker
# ---------------------------------------------------------------------------

HOME11="$ROOT/home11"; mkdir -p "$HOME11"
STATE11="$HOME11/.local/share/orbit-office/install-state"
OUT_INIT="$ROOT/out-init.txt"
if STUB_INIT_MODE=error run_install "$HOME11" -- > "$OUT_INIT" 2>&1; then
    fail "office.init error envelope stops the install"
else
    ok "office.init error envelope stops the install"
fi
check "office.init error names its code" grep -F "library_dir_outside_workspace" "$OUT_INIT"
check "office.init error leaves no scaffolded marker" \
    sh -c '! grep -qx scaffolded "$1"' sh "$STATE11"
OUT_INIT2="$ROOT/out-init2.txt"
run_install "$HOME11" -- > "$OUT_INIT2" 2>&1 \
    || { cat "$OUT_INIT2" >&2; fail "next run retries office.init"; }
ok "next run retries office.init"
check "next run scaffolds the Library" test -f "$HOME11/orbit/library/.office/config.yaml"
check "next run records the scaffolded marker" grep -qx scaffolded "$STATE11"

HOME12="$ROOT/home12"; mkdir -p "$HOME12"
OUT_ALREADY="$ROOT/out-already.txt"
STUB_INIT_MODE=already run_install "$HOME12" -- > "$OUT_ALREADY" 2>&1 \
    || { cat "$OUT_ALREADY" >&2; fail "already_initialized envelope exits 0"; }
ok "already_initialized envelope exits 0"
check "already_initialized envelope records the scaffolded marker" \
    grep -qx scaffolded "$HOME12/.local/share/orbit-office/install-state"

# ---------------------------------------------------------------------------
# 12. A non-interactive run without --yes prints the grants, enables the
#     plugin with the same grants and does not ask
# ---------------------------------------------------------------------------

HOME12N="$ROOT/home12n"; mkdir -p "$HOME12N"
rm -f "$STUB_STATE/plugin-added" "$STUB_STATE/plugin-enabled"
: > "$STUB_STATE/orbit.log"
OUT_NOYES="$ROOT/out-noyes.txt"
NO_YES=1 run_install "$HOME12N" -- > "$OUT_NOYES" 2>&1 \
    || { cat "$OUT_NOYES" >&2; fail "non-interactive run without --yes exits 0"; }
ok "non-interactive run without --yes exits 0"
check "non-interactive run does not ask the question" \
    sh -c '! grep -qF "[y/N]" "$1"' sh "$OUT_NOYES"
check "non-interactive run says it proceeds" \
    grep -qF "non-interactive run, enabling office" "$OUT_NOYES"
check "printed network grant is unscoped" \
    grep -qF "network unscoped" "$OUT_NOYES"
check "plugin enabled with the fs and network grants" \
    grep -qF "plugin enable office --grant fs={{workspace}},{{plugin_state}} --grant network" "$STUB_STATE/orbit.log"
check "plugin enabled (stub, non-interactive)" test -f "$STUB_STATE/plugin-enabled"

printf 'PASS: %s checks\n' "$PASS"
