#!/bin/sh
# scenarios_distribution.sh - piped checks 14/20 and standalone 17 for test-install.sh (sourced by the entry point; not run directly)
scenario_14() {
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
}

scenario_20_piped() {
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
}

scenario_17() {
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
}

