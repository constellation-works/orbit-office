#!/bin/sh
# scenarios_status_skills.sh - checks 21, 15, 16 and retired-token 20 for test-install.sh (sourced by the entry point; not run directly)
scenario_21() {
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
check "status names Library fix" grep -F "fix: orbit-office init $HOME1/orbit" "$ROOT/status-no-library"
mv "$ROOT/saved-healthy-library" "$HOME1/orbit/library"

rm -f "$HOME1/orbit/.orbit/enabled-office"
if run_office "$HOME1" status > "$ROOT/status-plugin-off" 2>&1; then fail "status with plugin disabled exits non-zero"; else ok "status with plugin disabled exits non-zero"; fi
check "status names disabled office plugin" grep -F "plugin 'office' is not enabled" "$ROOT/status-plugin-off"
check "status names plugin fix" grep -F "fix: orbit-office init $HOME1/orbit" "$ROOT/status-plugin-off"
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
check "status names stopped fix" grep -F "fix: orbit-office open $HOME1/orbit" "$ROOT/status-stopped"
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
# Bound-workspace mismatch: the service is bound to ~/orbit, so status for
# a different registered workspace must fail naming the bound workspace.
grep -qxF "$WS2" "$STUB_STATE/workspace-roots" 2>/dev/null || printf '%s\n' "$WS2" >> "$STUB_STATE/workspace-roots"
: > "$WS2/.orbit/enabled-office"
: > "$WS2/.orbit/enabled-gateway"
: > "$STUB_STATE/secret.meta_api_key"
: > "$STUB_STATE/svc-active"
if run_office "$HOME1" status "$WS2" > "$ROOT/status-mismatch" 2>&1; then fail "office status for another workspace exits non-zero"; else ok "office status for another workspace exits non-zero"; fi
check "status mismatch names bound workspace" grep -F "app service serves $HOME1/orbit not $WS2" "$ROOT/status-mismatch"
check "status mismatch names install.sh --home fix" grep -F "fix: install.sh --home $WS2" "$ROOT/status-mismatch"
check "status mismatch is not OK" sh -c '! grep -qxF "Status: OK" "$1"' sh "$ROOT/status-mismatch"
if run_office "$HOME1" status --json "$WS2" > "$ROOT/status-mismatch.json" 2> "$ROOT/status-mismatch.json.err"; then fail "office status --json for another workspace exits non-zero"; else ok "office status --json for another workspace exits non-zero"; fi
check "status --json mismatch stderr is empty" test ! -s "$ROOT/status-mismatch.json.err"
node - "$ROOT/status-mismatch.json" "$HOME1/orbit" "$WS2" <<'NODE' || fail "office status --json mismatch shape"
const fs = require("fs");
const [file, bound, other] = process.argv.slice(2);
const lines = fs.readFileSync(file, "utf8").trim().split("\n");
if (lines.length !== 1) process.exit(1);
const r = JSON.parse(lines[0]);
if (r.workspace !== other || r.workspace_registered !== true) process.exit(1);
if (r.plugins.office !== true || r.plugins.gateway !== true) process.exit(1);
if (!r.library.initialized) process.exit(1);
if (r.meta_api_key !== "set") process.exit(1);
if (!r.app_service.installed || !r.app_service.running) process.exit(1);
if (r.app_service.workspace !== bound) process.exit(1);
if (r.ok !== false || !Array.isArray(r.problems)) process.exit(1);
const row = r.problems.find((entry) => entry && entry.check === "app_service");
if (!row) process.exit(1);
if (!String(row.problem).includes(bound) || !String(row.problem).includes(other)) process.exit(1);
if (row.fix !== "install.sh --home " + other) process.exit(1);
if (fs.readFileSync(file, "utf8").includes("stub-secret-value")) process.exit(1);
NODE
ok "office status --json mismatch shape"
run_office "$HOME1" status "$HOME1/orbit" > "$ROOT/status-bound" 2>&1 || { cat "$ROOT/status-bound" >&2; fail "office status for bound workspace stays OK"; }
ok "office status for bound workspace stays OK"
check "status for bound workspace prints Status OK" grep -qxF "Status: OK" "$ROOT/status-bound"
# macOS service shape: launchd plist + launchctl stubs.
mkdir -p "$HOME1/Library/LaunchAgents"
printf '%s\n' '<?xml version="1.0"?>' '<plist><dict><key>Label</key><string>com.orbit-office.app</string><key>EnvironmentVariables</key>' "<dict><key>PORT</key><string>4317</string><key>HOST</key><string>127.0.0.1</string><key>ORBIT_OFFICE_WORKSPACE</key><string>$HOME1/orbit</string></dict></dict></plist>" > "$HOME1/Library/LaunchAgents/com.orbit-office.app.plist"
: > "$STUB_STATE/launchd-loaded"
OFFICE_TEST_OS=macos run_office "$HOME1" status > "$ROOT/status-macos" 2>&1 || { cat "$ROOT/status-macos" >&2; fail "office status reads launchd service"; }
ok "office status reads launchd service (macos)"
check "status macos reports running URL" grep -F "App service: running at http://localhost:4317/" "$ROOT/status-macos"
check "status macos prints the service log path" grep -F "App service log: $HOME1/Library/Application Support/orbit-office/app.log" "$ROOT/status-macos"
rm -f "$STUB_STATE/launchd-loaded" "$STUB_STATE/browser.log"
OFFICE_TEST_OS=macos run_office "$HOME1" open > "$ROOT/open-macos" 2>&1 || { cat "$ROOT/open-macos" >&2; fail "office open loads launchd service"; }
ok "office open loads launchd service (macos)"
check "office open macos loaded the agent" test -f "$STUB_STATE/launchd-loaded"
check "office open macos opened URL" grep -qxF "http://localhost:4317/" "$STUB_STATE/browser.log"
rm -f "$HOME1/Library/LaunchAgents/com.orbit-office.app.plist"
check "office launcher is recorded as created" grep -qxF "launcher_path=$HOME1/.local/bin/orbit-office" "$HOME1/.local/share/orbit-office/install-state"
}

scenario_15() {
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
}

scenario_16() {
# ---------------------------------------------------------------------------
# 16. Orbit and Node in a temporary non-system prefix: the generated
#     runtime environments must still resolve and execute them.
#     The custom orbit is a Node script (as an npm-installed Orbit is),
#     so it needs both its own directory and node's on PATH.
# ---------------------------------------------------------------------------

CUSTOM_PREFIX="$ROOT/custom-prefix"
CUSTOM_BIN="$CUSTOM_PREFIX/bin"
mkdir -p "$CUSTOM_BIN"
for _stub in npm systemctl launchctl ffmpeg curl tar uv cargo; do
    cp "$BIN/$_stub" "$CUSTOM_BIN/$_stub"
done
chmod +x "$CUSTOM_BIN"/*
ln -s "$(command -v node)" "$CUSTOM_BIN/node"
cat > "$CUSTOM_BIN/orbit" <<STUB
#!/usr/bin/env node
if (process.argv[2] === "--version") { process.stdout.write("orbit 0.39.0\n"); process.exit(0); }
const { spawnSync } = require("child_process");
const r = spawnSync("/bin/sh", ["$BIN/orbit", ...process.argv.slice(2)], { stdio: "inherit" });
process.exit(r.status === null ? 1 : r.status);
STUB
chmod +x "$CUSTOM_BIN/orbit"
cp "$CUSTOM_BIN/orbit" "$ROOT/custom-orbit-before"


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
}

scenario_20_retired() {
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
}

