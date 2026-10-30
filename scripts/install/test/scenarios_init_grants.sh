#!/bin/sh
# scenarios_init_grants.sh - checks 11-13 for test-install.sh (sourced by the entry point; not run directly)
scenario_11() {
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
}

scenario_12() {
# ---------------------------------------------------------------------------
# 12. Non-interactive setup needs new explicit consent, including agent verbs.
# ---------------------------------------------------------------------------
HOME12N="$ROOT/home12n"; mkdir -p "$HOME12N"
: > "$STUB_STATE/orbit.log"
OUT_NOYES="$ROOT/out-noyes.txt"
if (NO_YES=1 run_install "$HOME12N" -- > "$OUT_NOYES" 2>&1); then
    fail "non-interactive run without --yes refuses missing consent"
fi
ok "non-interactive run without --yes refuses missing consent"
check "refusal asks for explicit --yes" grep -qF "plugin consent required: re-run with --yes" "$OUT_NOYES"
check "no enable before consent" sh -c '! grep -qF "plugin enable office" "$1"' sh "$STUB_STATE/orbit.log"
check "printed consent names exact agent verbs" grep -qF "office.attach, office.capture, office.proposal-create, office.selection-get" "$OUT_NOYES"
check "printed network grant is unscoped" grep -qF "network unscoped" "$OUT_NOYES"
run_install "$HOME12N" -- > "$ROOT/out-yes.txt" 2>&1 || { cat "$ROOT/out-yes.txt" >&2; fail "explicit consent enables plugin"; }
check "explicit consent includes complete grant set" grep -qF "plugin enable office --grant fs={{workspace}},{{plugin_state}} --grant network --grant orbit_tools --grant agent_tools" "$STUB_STATE/orbit.log"
check "plugin enabled after consent" test -f "$STUB_STATE/plugin-active"

# Simulate an existing install with all old grants but no agent consent.
# This tests installer repair, not Orbit authorization (the real fixture does).
: > "$STUB_STATE/missing-agent-consent"
: > "$STUB_STATE/narrow-fs"
printf 'custom Library setting\n' > "$HOME12N/orbit/library/custom.txt"
cp "$HOME12N/orbit/library/custom.txt" "$ROOT/custom-before"
: > "$STUB_STATE/orbit.log"
if (NO_YES=1 run_install "$HOME12N" -- > "$ROOT/repair-no.txt" 2>&1); then fail "repair must ask for fresh consent"; fi
check "repair does not enable before fresh consent" sh -c '! grep -qF "plugin enable office" "$1"' sh "$STUB_STATE/orbit.log"
run_install "$HOME12N" -- > "$ROOT/repair-yes.txt" 2>&1 || { cat "$ROOT/repair-yes.txt" >&2; fail "repair with consent succeeds"; }
check "repair preserves narrower fs and other grants" grep -qF "plugin enable office --grant fs={{workspace}}/library,{{plugin_state}} --grant network --grant orbit_tools --grant agent_tools" "$STUB_STATE/orbit.log"
check "repair preserves custom Library bytes" cmp -s "$HOME12N/orbit/library/custom.txt" "$ROOT/custom-before"
# Simulated readback now reflects the newly recorded consent.
# Markers are specific fixture files, not host state.
unlink "$STUB_STATE/missing-agent-consent"
: > "$STUB_STATE/orbit.log"
run_install "$HOME12N" -- > "$ROOT/repair-repeat.txt" 2>&1 || { cat "$ROOT/repair-repeat.txt" >&2; fail "repair rerun succeeds"; }
check "repair rerun does not replace grants" sh -c '! grep -qF "plugin enable office" "$1"' sh "$STUB_STATE/orbit.log"
if run_install "$HOME12N" STUB_AGENT_VERBS=attach,capture,proposal-create,selection-get,transcribe -- > "$ROOT/wider.txt" 2>&1; then fail "installer refuses unreviewed widened verbs"; fi
check "widened verbs name review and upgrade" grep -qF "installed office agent verbs differ" "$ROOT/wider.txt"
unlink "$STUB_STATE/narrow-fs"

}

scenario_13() {
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
check "fresh clone builds office at the clone HEAD" grep -qxF "build $(git -C "$CLONE" rev-parse HEAD)" "$STUB_STATE/cargo.log"
check "fresh clone adds the built office export" grep -F "plugin add $CLONE_HOME/.local/share/orbit-office/plugins/office" "$STUB_STATE/orbit.log"
check "fresh clone export carries an executable backend" test -x "$CLONE_HOME/.local/share/orbit-office/plugins/office/.orbit-plugin/bin/office-bin"
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
check "source office install builds the actual worktree commit" \
    grep -qxF "build $(git -C "$SCRIPT_DIR/../.." rev-parse HEAD)" "$STUB_STATE/cargo.log"

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
}

