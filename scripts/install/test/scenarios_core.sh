#!/bin/sh
# scenarios_core.sh - checks 1-7 for test-install.sh (sourced by the entry point; not run directly)
scenario_1() {
# ---------------------------------------------------------------------------
# 1. Install twice: second run changes nothing, reports already done
# ---------------------------------------------------------------------------

HOME1="$ROOT/home1"
mkdir -p "$HOME1"

OUT1="$ROOT/out1.txt"
run_install "$HOME1" -- > "$OUT1" 2>&1 || { cat "$OUT1" >&2; fail "first install exits 0"; }
ok "first install exits 0"
SNAP1="$ROOT/snap1.txt"; snapshot "$HOME1" > "$SNAP1"
check "installer creates the standalone office launcher" test -x "$HOME1/.local/bin/orbit-office"
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
check "office init names the workspace office-<folder>" sh -c 'grep -F "workspace init --preset " "$1" | grep -qF -- "--name office-$2" && grep -F "workspace init --force --preset " "$1" | grep -qF -- "--name office-$2"' sh "$STUB_STATE/orbit.log" "$(basename "$WS2")"
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
check "office init help names init usage" grep -qF "Usage: orbit-office init" "$ROOT/office-init-help"
run_office "$HOME1" open --help > "$ROOT/office-open-help" 2>&1
check "office open help names open usage" grep -qF "Usage: orbit-office open" "$ROOT/office-open-help"
run_office "$HOME1" status --help > "$ROOT/office-status-help" 2>&1
check "office status help names status usage" grep -qF "Usage: orbit-office status" "$ROOT/office-status-help"
if run_office "$HOME1" bogus > "$ROOT/office-usage" 2>&1; then fail "office rejects unknown commands";
else _usage_rc=$?; check "office usage exits 2" test "$_usage_rc" -eq 2; fi
}

scenario_2() {
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
}

scenario_3() {
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
check "uninstall removes installer-owned launcher" test ! -e "$HOME1/.local/bin/orbit-office"

OUT_PURGE="$ROOT/out-purge.txt"
run_install "$HOME1" -- --purge > "$OUT_PURGE" 2>&1 \
    || { cat "$OUT_PURGE" >&2; fail "purge exits 0"; }
ok "purge exits 0"
check "purge removes the workspace" test ! -e "$HOME1/orbit"

# A pre-existing identical launcher is reusable but never owned for uninstall.
UNOWNED_HOME="$ROOT/unowned-launcher-home"
mkdir -p "$UNOWNED_HOME/.local/bin"
# Copy a launcher from an installed home (HOME3 is the newer-Orbit installation).
cp "$HOME3/.local/bin/orbit-office" "$UNOWNED_HOME/.local/bin/orbit-office"
run_install "$UNOWNED_HOME" -- > "$ROOT/unowned-install" 2>&1 \
    || { cat "$ROOT/unowned-install" >&2; fail "install reuses identical unowned launcher"; }
check "installer does not claim pre-existing launcher" sh -c '! grep -q "^launcher_path=" "$1"' sh "$UNOWNED_HOME/.local/share/orbit-office/install-state"
run_install "$UNOWNED_HOME" -- --uninstall > "$ROOT/unowned-uninstall" 2>&1 \
    || { cat "$ROOT/unowned-uninstall" >&2; fail "uninstall with unowned launcher succeeds"; }
check "uninstall keeps unrecorded launcher" test -x "$UNOWNED_HOME/.local/bin/orbit-office"
MODIFIED_HOME="$ROOT/modified-launcher-home"; mkdir -p "$MODIFIED_HOME"
run_install "$MODIFIED_HOME" -- > "$ROOT/modified-install" 2>&1 \
    || { cat "$ROOT/modified-install" >&2; fail "install before launcher edit"; }
printf '// user addition\n' >> "$MODIFIED_HOME/.local/bin/orbit-office"
cp "$MODIFIED_HOME/.local/bin/orbit-office" "$ROOT/modified-office-before"
if run_install "$MODIFIED_HOME" -- > "$ROOT/modified-reinstall" 2>&1; then fail "reinstall must preserve modified launcher";
else ok "reinstall refuses to overwrite modified launcher"; fi
run_install "$MODIFIED_HOME" -- --uninstall > "$ROOT/modified-uninstall" 2>&1 \
    || { cat "$ROOT/modified-uninstall" >&2; fail "uninstall keeps edited launcher"; }
check "uninstall preserves edited launcher bytes" cmp -s "$MODIFIED_HOME/.local/bin/orbit-office" "$ROOT/modified-office-before"
# An install from before the rename recorded ~/.local/bin/office; reinstall
# moves ownership to orbit-office and removes the old file only if unchanged.
LEGACY_HOME="$ROOT/legacy-launcher-home"; mkdir -p "$LEGACY_HOME"
run_install "$LEGACY_HOME" -- > "$ROOT/legacy-install" 2>&1 \
    || { cat "$ROOT/legacy-install" >&2; fail "install before legacy launcher simulation"; }
_legacy_state="$LEGACY_HOME/.local/share/orbit-office/install-state"
mv "$LEGACY_HOME/.local/bin/orbit-office" "$LEGACY_HOME/.local/bin/office"
sed "s#^launcher_path=.*#launcher_path=$LEGACY_HOME/.local/bin/office#" "$_legacy_state" > "$_legacy_state.tmp" && mv "$_legacy_state.tmp" "$_legacy_state"
run_install "$LEGACY_HOME" -- > "$ROOT/legacy-reinstall" 2>&1 \
    || { cat "$ROOT/legacy-reinstall" >&2; fail "reinstall over legacy launcher"; }
check "reinstall removes recorded legacy launcher" test ! -e "$LEGACY_HOME/.local/bin/office"
check "reinstall installs orbit-office launcher" test -x "$LEGACY_HOME/.local/bin/orbit-office"
check "reinstall records orbit-office launcher" grep -qxF "launcher_path=$LEGACY_HOME/.local/bin/orbit-office" "$_legacy_state"
check "reinstall forgets legacy launcher path" sh -c '! grep -qF "launcher_path=$1" "$2"' sh "$LEGACY_HOME/.local/bin/office" "$_legacy_state"
# `sh install.sh` from the clone root finds the clone's plugin sources.
CWD_HOME="$ROOT/cwd-install-home"; mkdir -p "$CWD_HOME"
INSTALL_FROM_CWD=1 run_install "$CWD_HOME" -- > "$ROOT/cwd-install" 2>&1 \
    || { cat "$ROOT/cwd-install" >&2; fail "sh install.sh from the clone root exits 0"; }
ok "sh install.sh from the clone root exits 0"
check "sh install.sh from the clone root adds the office plugin" test -e "$STUB_STATE/plugin-added"
check "sh install.sh from the clone root does not ask for a clone" sh -c '! grep -qF "needs a clone" "$1"' sh "$ROOT/cwd-install"
# Inside a managed Orbit run the launcher must not add the operator override.
MANAGED_HOME="$ROOT/managed-run-home"; mkdir -p "$MANAGED_HOME"
if run_install "$MANAGED_HOME" ORBIT_MANAGED_RUN_CONTEXT=probe -- > "$ROOT/managed-install" 2>&1; then
    fail "install inside a managed run must not scaffold as operator"
else ok "install inside a managed run is refused at scaffold"; fi
check "managed-run refusal is the capability denial" grep -qF "capability_denied" "$ROOT/managed-install"
# A host where `orbit workspace list` fails from an unregistered folder.
UNBOUND_HOME="$ROOT/unbound-list-home"; mkdir -p "$UNBOUND_HOME"
run_install "$UNBOUND_HOME" STUB_LIST_UNBOUND_FAILS=1 -- > "$ROOT/unbound-install" 2>&1 \
    || { cat "$ROOT/unbound-install" >&2; fail "install where workspace list needs a binding exits 0"; }
ok "install where workspace list needs a binding exits 0"
check "install where workspace list needs a binding registers the workspace" grep -qxF "$UNBOUND_HOME/orbit" "$STUB_STATE/workspace-roots"
}

scenario_4() {
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
}

scenario_5() {
# ---------------------------------------------------------------------------
# 5. Missing Orbit is installed from npm at the tested version
# ---------------------------------------------------------------------------

BIN2="$ROOT/bin-no-orbit"
mkdir -p "$BIN2"
for _stub in npm systemctl launchctl ffmpeg curl tar uv cargo; do cp "$BIN/$_stub" "$BIN2/$_stub"; done
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
}

scenario_6() {
# ---------------------------------------------------------------------------
# 6. Checksum mismatch stops before unpack; a digest override is ignored
# ---------------------------------------------------------------------------


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
}

scenario_7() {
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
}

