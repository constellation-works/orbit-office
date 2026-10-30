#!/bin/sh
# scenarios_services_mcp.sh - checks 8-10, 18, 19, 22, 23 for test-install.sh (sourced by the entry point; not run directly)
scenario_8() {
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
check "launchd WorkingDirectory is the bound workspace" grep -F "<key>WorkingDirectory</key><string>$HOME8/orbit</string>" "$PLIST"
check "launchd StandardOutPath is the service log" grep -F "<key>StandardOutPath</key><string>$HOME8/custom-data/app.log</string>" "$PLIST"
check "launchd StandardErrorPath is the service log" grep -F "<key>StandardErrorPath</key><string>$HOME8/custom-data/app.log</string>" "$PLIST"
check "service log file exists under the data dir" test -f "$HOME8/custom-data/app.log"
check "service log file is owner-only 0600" sh -c '[ "$(stat -c %a "$1" 2>/dev/null || stat -f %Lp "$1" 2>/dev/null)" = "600" ]' sh "$HOME8/custom-data/app.log"
check "macos tools path is the literal share path" test -x "$MAC_TOOLS/bin/uv"
check "macos data dir override is honoured" test -f "$HOME8/custom-data/install-state"
check "macos data dir is not the tools dir" test ! -e "$HOME8/custom-data/tools"
check "no systemd unit on the macos run" \
    test ! -e "$HOME8/.config/systemd/user/orbit-office.service"
check "macos MCP PATH starts with tools/bin" \
    grep -F "\"PATH\": \"$MAC_TOOLS/bin:" "$HOME8/.claude.json"
assert_profiles_untouched "$HOME8"
}

scenario_9() {
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
}

scenario_10() {
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
}

scenario_18() {
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
for _stub in orbit npm launchctl ffmpeg curl tar uv cargo; do cp "$BIN/$_stub" "$BIN_NOSYS/$_stub"; done
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
for _stub in orbit npm systemctl ffmpeg curl tar uv cargo; do cp "$BIN/$_stub" "$BIN_NOLAUNCH/$_stub"; done
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
}

scenario_19() {
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
}

scenario_22() {
# ---------------------------------------------------------------------------
# 22. fnm_multishells node: stable path in both units; wrapper-only fails
# ---------------------------------------------------------------------------

STABLE_NODE=$("$(command -v node)" -e 'console.log(require("fs").realpathSync(process.argv[1]))' "$(command -v node)") \
    || fail "stable node path is resolvable"
FNM_BIN="$ROOT/fnm_multishells/82380_1793236905367/bin"
mkdir -p "$FNM_BIN"
ln -s "$(command -v node)" "$FNM_BIN/node"

FNM_HOME="$ROOT/home-fnm-systemd"; mkdir -p "$FNM_HOME"
OUT_FNM="$ROOT/out-fnm-systemd.txt"
(TEST_PATH="$FNM_BIN:$BIN:$NODE_DIR:/usr/bin:/bin" run_install "$FNM_HOME" -- > "$OUT_FNM" 2>&1) \
    || { cat "$OUT_FNM" >&2; fail "fnm_multishells symlink install (systemd) exits 0"; }
ok "fnm_multishells symlink install (systemd) exits 0"
check "systemd ExecStart uses the stable node" \
    grep -F "ExecStart=$STABLE_NODE " "$FNM_HOME/.config/systemd/user/orbit-office.service"
check "systemd unit has no fnm_multishells entry" \
    sh -c '! grep -q fnm_multishells "$1"' sh "$FNM_HOME/.config/systemd/user/orbit-office.service"

FNM_MAC_HOME="$ROOT/home-fnm-launchd"; mkdir -p "$FNM_MAC_HOME"
OUT_FNM_MAC="$ROOT/out-fnm-launchd.txt"
(TEST_PATH="$FNM_BIN:$BIN:$NODE_DIR:/usr/bin:/bin" run_install "$FNM_MAC_HOME" ORBIT_OFFICE_OS=macos -- > "$OUT_FNM_MAC" 2>&1) \
    || { cat "$OUT_FNM_MAC" >&2; fail "fnm_multishells symlink install (launchd) exits 0"; }
ok "fnm_multishells symlink install (launchd) exits 0"
check "launchd ProgramArguments uses the stable node" \
    grep -F "<string>$STABLE_NODE</string>" "$FNM_MAC_HOME/Library/LaunchAgents/com.orbit-office.app.plist"
check "launchd plist has no fnm_multishells entry" \
    sh -c '! grep -q fnm_multishells "$1"' sh "$FNM_MAC_HOME/Library/LaunchAgents/com.orbit-office.app.plist"

FNM_ONLY_BIN="$ROOT/fnm-only/fnm_multishells/99999_1/bin"
mkdir -p "$FNM_ONLY_BIN"
printf '#!/bin/sh\nexec "%s" "$@"\n' "$STABLE_NODE" > "$FNM_ONLY_BIN/node"
chmod +x "$FNM_ONLY_BIN/node"
FNM_ONLY_HOME="$ROOT/home-fnm-only"; mkdir -p "$FNM_ONLY_HOME"
OUT_FNM_ONLY="$ROOT/out-fnm-only.txt"
if (TEST_PATH="$FNM_ONLY_BIN:$BIN:/usr/bin:/bin" run_install "$FNM_ONLY_HOME" -- > "$OUT_FNM_ONLY" 2>&1); then
    cat "$OUT_FNM_ONLY" >&2; fail "fnm_multishells wrapper-only install stops"
else
    ok "fnm_multishells wrapper-only install stops"
fi
check "wrapper-only failure names the per-shell fnm_multishells cause" \
    grep -F "fnm_multishells" "$OUT_FNM_ONLY"
check "wrapper-only failure writes no systemd unit" \
    test ! -e "$FNM_ONLY_HOME/.config/systemd/user/orbit-office.service"
check "wrapper-only failure writes no launchd plist" \
    test ! -e "$FNM_ONLY_HOME/Library/LaunchAgents/com.orbit-office.app.plist"
}

scenario_23() {
# ---------------------------------------------------------------------------
# 23. Muse Code MCP entry: written, idempotent, and removed on uninstall
# ---------------------------------------------------------------------------

HOME_MUSE="$ROOT/home-muse"; mkdir -p "$HOME_MUSE/.config/muse"
MUSE_CFG="$HOME_MUSE/.config/muse/settings.json"
cat > "$MUSE_CFG" <<'EOF'
{ "kept": true, "mcpServers": { "other": {"command":"kept"} } }
EOF
cp "$MUSE_CFG" "$ROOT/muse-original.json"
OUT_MUSE="$ROOT/out-muse.txt"
run_install "$HOME_MUSE" -- --agent muse > "$OUT_MUSE" 2>&1 \
    || { cat "$OUT_MUSE" >&2; fail "muse install exits 0"; }
ok "muse install exits 0"
check "muse MCP entry written with tools/bin first on PATH" \
    node -e 'const fs=require("fs"); const d=JSON.parse(fs.readFileSync(process.argv[1],"utf8")); const o=d.mcpServers&&d.mcpServers.orbit; if(!o||o.command!=="orbit"||JSON.stringify(o.args)!==JSON.stringify(["mcp","serve"])) process.exit(1); if(!o.env||!o.env.PATH.startsWith(process.argv[2]+":")) process.exit(1); if(d.kept!==true||!d.mcpServers.other||d.mcpServers.other.command!=="kept") process.exit(1)' "$MUSE_CFG" "$HOME_MUSE/$TOOLS_REL/bin"
check "muse backup is the exact original settings" cmp -s "$MUSE_CFG.bak" "$ROOT/muse-original.json"
check "muse ownership receipt records the owned entry" \
    grep -q '"owned":true' "$HOME_MUSE/.local/share/orbit-office/muse-mcp.json"
check "muse skills linked under .agents/skills with office- names" \
    sh -c 'test -L "$1/office-orchestrator" && test -f "$1/office-orchestrator/SKILL.md" && test ! -e "$1/orchestrator"' sh "$HOME_MUSE/.agents/skills"
check "muse summary reports no deny rules" \
    grep -F "deny rules unavailable for Muse Code shell calls" "$OUT_MUSE"
snapshot "$HOME_MUSE" > "$ROOT/muse-snap1.txt"
run_install "$HOME_MUSE" -- --agent muse > "$ROOT/out-muse2.txt" 2>&1 \
    || { cat "$ROOT/out-muse2.txt" >&2; fail "muse second install exits 0"; }
ok "muse second install exits 0"
snapshot "$HOME_MUSE" > "$ROOT/muse-snap2.txt"
check "muse second run changes nothing" cmp -s "$ROOT/muse-snap1.txt" "$ROOT/muse-snap2.txt"
run_install "$HOME_MUSE" -- --agent muse --uninstall > "$ROOT/out-muse-un.txt" 2>&1 \
    || { cat "$ROOT/out-muse-un.txt" >&2; fail "muse uninstall exits 0"; }
ok "muse uninstall exits 0"
check "muse uninstall removes the owned orbit entry" \
    node -e 'const fs=require("fs"); const d=JSON.parse(fs.readFileSync(process.argv[1],"utf8")); if(d.mcpServers&&d.mcpServers.orbit) process.exit(1); if(d.kept!==true||!d.mcpServers.other) process.exit(1)' "$MUSE_CFG"
check "muse uninstall clears the ownership receipt" \
    test ! -e "$HOME_MUSE/.local/share/orbit-office/muse-mcp.json"
check "muse uninstall removes the plugin skill link" \
    sh -c 'test ! -e "$1/office-orchestrator" && test ! -L "$1/office-orchestrator"' sh "$HOME_MUSE/.agents/skills"
check "muse uninstall keeps the workspace" test -d "$HOME_MUSE/orbit"

# XDG_CONFIG_HOME selects the Muse settings file; a pre-existing
# identical entry (rediscovered without a receipt) is not claimed.
HOME_MUSE_X="$ROOT/home-muse-xdg"; mkdir -p "$HOME_MUSE_X"
run_install "$HOME_MUSE_X" "XDG_CONFIG_HOME=$HOME_MUSE_X/xdg" -- --agent muse > "$ROOT/out-muse-xdg.txt" 2>&1 \
    || { cat "$ROOT/out-muse-xdg.txt" >&2; fail "muse XDG install exits 0"; }
ok "muse XDG install exits 0"
check "muse XDG settings file holds the entry" grep -q '"orbit"' "$HOME_MUSE_X/xdg/muse/settings.json"
rm -f "$HOME_MUSE_X/.local/share/orbit-office/muse-mcp.json"
run_install "$HOME_MUSE_X" "XDG_CONFIG_HOME=$HOME_MUSE_X/xdg" -- --agent muse > "$ROOT/out-muse-xdg2.txt" 2>&1 \
    || { cat "$ROOT/out-muse-xdg2.txt" >&2; fail "muse XDG reinstall exits 0"; }
check "pre-existing identical muse entry is not claimed as owned" \
    grep -q '"owned":false' "$HOME_MUSE_X/.local/share/orbit-office/muse-mcp.json"
run_install "$HOME_MUSE_X" "XDG_CONFIG_HOME=$HOME_MUSE_X/xdg" -- --agent muse --uninstall > "$ROOT/out-muse-xdg-un.txt" 2>&1 \
    || { cat "$ROOT/out-muse-xdg-un.txt" >&2; fail "muse XDG uninstall exits 0"; }
check "muse uninstall keeps a user-owned identical entry" grep -q '"orbit"' "$HOME_MUSE_X/xdg/muse/settings.json"

# Invalid Muse settings refuse install and stay byte-identical.
HOME_MUSE_BAD="$ROOT/home-muse-bad"; mkdir -p "$HOME_MUSE_BAD/.config/muse"
printf '{invalid' > "$HOME_MUSE_BAD/.config/muse/settings.json"
cp "$HOME_MUSE_BAD/.config/muse/settings.json" "$ROOT/muse-bad-original"
if run_install "$HOME_MUSE_BAD" -- --agent muse > "$ROOT/out-muse-bad.txt" 2>&1; then fail "invalid muse settings must refuse"; fi
ok "invalid muse settings refuses install"
check "invalid muse settings remain byte-identical" cmp -s "$HOME_MUSE_BAD/.config/muse/settings.json" "$ROOT/muse-bad-original"
check "invalid muse settings create no receipt" \
    test ! -e "$HOME_MUSE_BAD/.local/share/orbit-office/muse-mcp.json"
}

