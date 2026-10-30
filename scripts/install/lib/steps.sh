# steps.sh — install steps, app, uninstall and main dispatch.
# Fragment of the generated install.sh: assembled by scripts/install/build-install.sh,
# never sourced at runtime. Edit this file, then regenerate install.sh.

# ---------------------------------------------------------------------------
# Install steps
# ---------------------------------------------------------------------------

step_preflight() {
    step "1 preflight"
    case "$OS" in
        macos|linux) say "preflight: already done (platform $OS)" ;;
        *) die "unsupported platform '$OS_RAW': macOS or Linux required (Windows: use WSL2)" ;;
    esac
    command -v node >/dev/null 2>&1 \
        || die "Node.js 20+ is required and is not installed: get it from https://nodejs.org/ (or your package manager), then re-run"
    _node_version=$(node --version 2>/dev/null || printf '')
    _node_major=$(extract_version "$_node_version")
    _node_major=${_node_major%%.*}
    case "$_node_major" in
        ''|*[!0-9]*) die "could not read the Node.js version from '$_node_version'; install Node.js 20+ from https://nodejs.org/" ;;
        *) ;;
    esac
    [ "$_node_major" -ge 20 ] \
        || die "Node.js 20+ is required (found $_node_version): get it from https://nodejs.org/, then re-run"
    command -v npm >/dev/null 2>&1 \
        || die "npm is required and is not installed: it ships with Node.js 20+ from https://nodejs.org/, then re-run"
    say "preflight: node $_node_version and npm present"
    if command -v git >/dev/null 2>&1; then
        say "preflight: git present"
    elif [ "$OS" = "macos" ]; then
        warn "git is missing; starting the Xcode command line tools install"
        xcode-select --install 2>/dev/null || true
        die "git is being installed by xcode-select; re-run this script when it finishes"
    else
        die "git is required and is not installed: install it with your package manager (e.g. apt-get install git), then re-run"
    fi
    if command -v ffmpeg >/dev/null 2>&1; then
        say "preflight: ffmpeg present"
    else
        warn "ffmpeg is missing; trying to install it"
        if command -v brew >/dev/null 2>&1; then
            brew install ffmpeg 2>/dev/null || true
        elif command -v apt-get >/dev/null 2>&1; then
            apt-get install -y ffmpeg 2>/dev/null || true
        elif command -v dnf >/dev/null 2>&1; then
            dnf install -y ffmpeg 2>/dev/null || true
        elif command -v apk >/dev/null 2>&1; then
            apk add ffmpeg 2>/dev/null || true
        fi
        if command -v ffmpeg >/dev/null 2>&1; then
            say "preflight: ffmpeg installed"
        elif [ "${ORBIT_OFFICE_REQUIRE_FFMPEG:-0}" = "1" ]; then
            die "ffmpeg is required: install it with Homebrew (brew install ffmpeg) or your package manager, then re-run"
        else
            warn "ffmpeg is still missing: install it with Homebrew or your package manager; media tools will fail until then"
            say "preflight: ffmpeg missing (warning only, continuing)"
        fi
    fi
}

step_orbit() {
    step "3 orbit"
    if command -v orbit >/dev/null 2>&1; then
        _raw=$(orbit --version 2>/dev/null || printf '')
        _version=$(extract_version "$_raw")
        if [ -z "$_version" ]; then
            warn "could not read the Orbit version from '$_raw'; continuing without changing it"
            say "orbit: already installed (version unreadable, left unchanged)"
            return 0
        fi
        _cmp=$(version_cmp "$_version" "$TESTED_ORBIT_VERSION")
        if [ "$_cmp" -lt 0 ]; then
            printf 'error: Orbit %s is older than the tested %s; upgrade it yourself with:\n  npm install -g %s\nthen re-run this script. This script never changes an existing Orbit.\n' \
                "$_version" "$TESTED_ORBIT_VERSION" "$ORBIT_NPM_PACKAGE" >&2
            exit 1
        fi
        if [ "$_cmp" -gt 0 ]; then
            warn "Orbit $_version is newer than the tested $TESTED_ORBIT_VERSION; continuing, leaving it unchanged"
        fi
        say "orbit: already installed (orbit $_version, left unchanged)"
        return 0
    fi
    say "orbit: installing the tested release with npm"
    npm install -g "$ORBIT_NPM_PACKAGE"
    command -v orbit >/dev/null 2>&1 \
        || die "npm installed $ORBIT_NPM_PACKAGE but 'orbit' is still not on PATH"
    say "orbit: installed ($ORBIT_NPM_PACKAGE)"
}

# The same standalone launcher ships in clone and curl-to-sh installs.
launcher_digest() {
    node -e 'const fs = require("fs"), crypto = require("crypto"); process.stdout.write(crypto.createHash("sha256").update(fs.readFileSync(process.argv[1])).digest("hex"));' "$1"
}

# Before the rename the launcher was ~/.local/bin/office, which shadowed or was
# shadowed by the plugin's `office` exec backend binary on PATH.
LEGACY_LAUNCHER="$HOME/.local/bin/office"

# Remove a launcher only when install-state recorded it and its bytes still
# match the recorded digest; a changed or unrecorded file is kept.
remove_recorded_launcher() {
    _rl_path="$1"; _rl_label="$2"
    if state_has "launcher_path=$_rl_path"; then
        if [ -f "$_rl_path" ] && [ ! -L "$_rl_path" ] \
            && _rl_digest=$(launcher_digest "$_rl_path" 2>/dev/null) \
            && state_has "launcher_digest=$_rl_digest"; then
            rm -f "$_rl_path"
            state_clear "launcher_digest=$_rl_digest"
            say "$_rl_label: removed the office launcher ($_rl_path)"
        elif [ -e "$_rl_path" ] || [ -L "$_rl_path" ]; then
            warn "$_rl_label: kept changed launcher path $_rl_path"
        fi
        state_clear "launcher_path=$_rl_path"
    elif [ "$_rl_label" = uninstall ] && [ -e "$_rl_path" ]; then
        say "uninstall: kept any unrecorded office launcher ($_rl_path)"
    fi
}

step_launcher() {
    remove_recorded_launcher "$LEGACY_LAUNCHER" launcher
    LAUNCHER="$HOME/.local/bin/orbit-office"
    mkdir -p "$DATA_DIR"
    _candidate="$DATA_DIR/office-launcher.$$"
    _launcher_created=0
    trap 'rm -f "$_candidate" "$LAUNCHER.install-$$"' EXIT HUP INT TERM
    cat > "$_candidate" <<'OFFICE_LAUNCHER'
#!/usr/bin/env node
// Human workspace setup. The Rust office-bin remains Orbit's exec backend.
"use strict";
const fs = require("node:fs");
const path = require("node:path");
const os = require("node:os");
const { spawnSync } = require("node:child_process");
const help = `Usage: orbit-office <command> [path] [options]

Commands:
  init [path] [--yes]   Create or repair an office workspace and its Library
  open [path]           Open the app for a workspace in the default browser,
                        starting its installed app service first if stopped
  status [path] [--json]
                        Report workspace, plugin, Library, secret, private
                        route, app service and agent CLI health

Default path: ~/orbit (or ORBIT_OFFICE_HOME; for open/status, the workspace
the installed app service is bound to when known). Plugins must already be
installed for init.

Options:
  --yes       (init) Enable plugins without the consent question
  --json      (status) Print one JSON object instead of plain words
  -h, --help  Show this help (or one command's help, e.g. orbit-office status --help)

Exit codes: 0 success, 1 setup/health/tool failure, 2 usage error.

Examples:
  orbit-office init
  orbit-office init ~/another-office --yes
  orbit-office open
  orbit-office status
  orbit-office status ~/another-office --json
`;
const initHelp = `Usage: orbit-office init [path] [--yes]

Create or repair an office workspace and its Library.
Default path: ~/orbit (or ORBIT_OFFICE_HOME). Plugins must already be installed.

Options:
  --yes       Enable plugins without the consent question (default: ask on a TTY)
  -h, --help  Show this help

Non-interactive stdin prints the grants and proceeds without reading input.
Existing non-empty folders are refused unless owned by office setup.
Exit codes: 0 success, 1 setup/tool failure, 2 usage error.

Examples:
  orbit-office init
  orbit-office init ~/another-office --yes
`;
const openHelp = `Usage: orbit-office open [path]

Open the app for a workspace in the default browser. Starts the installed
app service first when it is stopped. Without a service, exits 1 naming
install.sh. Default path: the installed workspace (see orbit-office --help).

Options:
  -h, --help  Show this help

Exit codes: 0 opened, 1 no service/mismatch/start or browser failure, 2 usage.
`;
const statusHelp = `Usage: orbit-office status [path] [--json]

Report workspace registration, office/gateway enablement, Library health
(from office.status), Meta API key presence, the private route, the app
service and its URL, its log path, and agent CLI registration. Changes nothing.

Options:
  --json      Print one JSON object with the same facts
  -h, --help  Show this help

Exit codes: 0 healthy, 1 something needed is missing, 2 usage error.
`;
const say = (message) => process.stderr.write(message + "\n");
function fail(message, code = 1) {
  say("error: " + message);
  process.exit(code);
}
const args = process.argv.slice(2);
if (args.length === 1 && ["--help", "-h"].includes(args[0])) {
  process.stdout.write(help);
  process.exit(0);
}
const command = args.shift();
if (!["init", "open", "status"].includes(command)) fail("expected 'orbit-office init', 'orbit-office open' or 'orbit-office status'; use --help", 2);
let yes = false;
let jsonOutput = false;
let target;
for (const arg of args) {
  if (command === "init" && arg === "--yes") yes = true;
  else if (command === "status" && arg === "--json") jsonOutput = true;
  else if (["--help", "-h"].includes(arg)) {
    process.stdout.write(command === "init" ? initHelp : command === "open" ? openHelp : statusHelp);
    process.exit(0);
  } else if (arg.startsWith("-")) fail("unknown option: " + arg + "; use --help", 2);
  else if (target !== undefined) fail("expected one workspace path; use --help", 2);
  else target = arg;
}
if (!process.env.HOME) fail("HOME is not set");
if (target === "") fail("workspace path must not be empty", 2);
process.umask(0o077);
// office.init is a mutating plugin tool, which Orbit runs only for an
// operator. The person running orbit-office init (or install.sh) is that
// operator, but the launcher's piped child has no TTY and resolves as an
// agent, so pass Orbit's audited override. Inside a managed Orbit run the
// caller is an agent, so the override is never added there.
function operatorEnv() {
  const managed = process.env.ORBIT_MANAGED_RUN_CONTEXT || process.env.ORBIT_TASK_ACTOR_KIND === "agent";
  return managed ? {} : { ORBIT_OPERATOR: "1" };
}
function officeWorkspaceName(workspace) {
  return "office-" + path.basename(workspace);
}
function exists(file) {
  try { return fs.lstatSync(file); }
  catch (error) { if (error.code === "ENOENT") return null; throw error; }
}
function resolvePhysical(file) {
  const absolute = path.resolve(file);
  if (exists(absolute)) return fs.realpathSync(absolute);
  const parent = path.dirname(absolute);
  return path.join(resolvePhysical(parent), path.basename(absolute));
}
function run(command, argv, cwd, required = true, extraEnv = {}) {
  const result = spawnSync(command, argv, {
    cwd, encoding: "utf8", timeout: 300000, maxBuffer: 8 * 1024 * 1024,
    stdio: ["ignore", "pipe", "pipe"],
    env: { ...process.env, ...extraEnv, PATH: path.join(process.env.HOME, ".local/share/orbit-office/tools/bin") + path.delimiter + process.env.PATH },
  });
  if (required && (result.error || result.status !== 0)) {
    if (result.stderr) process.stderr.write(result.stderr);
    fail(command + " " + argv.join(" ") + " failed" + (result.error ? ": " + result.error.message : " (exit: " + result.status + ")"));
  }
  return result;
}
function json(command, argv, cwd) {
  const result = run(command, argv, cwd);
  try {
    const rows = JSON.parse(result.stdout);
    if (!Array.isArray(rows)) throw new Error("expected an array");
    return rows;
  } catch (_) { fail("could not read JSON from " + command + " " + argv.join(" ")); }
}
function parseLoose(text) {
  function parse(value) { try { return JSON.parse(value); } catch (_) { return undefined; } }
  const trimmed = String(text || "").trim();
  let value = parse(trimmed);
  if (value !== undefined) return value;
  const lines = trimmed.split(/\r?\n/);
  for (let i = lines.length - 1; i >= 0; i--) {
    if (!lines[i].trim().startsWith("{")) continue;
    value = parse(lines[i].trim());
    if (value !== undefined) return value;
    value = parse(lines.slice(i).join("\n").trim());
    if (value !== undefined) return value;
  }
  return parse(trimmed.slice(trimmed.indexOf("{"), trimmed.lastIndexOf("}") + 1));
}
function initStatus(result) {
  const object = (value) => value && typeof value === "object" && !Array.isArray(value) ? value : null;
  const code = (value) => {
    if (!value) return "";
    if (object(value.error) && typeof value.error.code === "string" && value.error.code) return value.error.code;
    return typeof value.code === "string" ? value.code : "";
  };
  const out = object(parseLoose(result.stdout));
  if (!result.error && result.status === 0) {
    if (!out) return "unreadable";
    if (out.ok === true || !Object.hasOwn(out, "ok")) return "ok";
    return out.ok === false ? code(out) || "unreadable" : "unreadable";
  }
  return code(object(parseLoose(result.stderr))) || code(out) || "unreadable";
}
function writeAtomic(file, content) {
  const temp = file + ".office-init-" + process.pid;
  const fd = fs.openSync(temp, "wx", 0o600);
  try {
    fs.writeFileSync(fd, content);
    fs.fsyncSync(fd);
  } finally { fs.closeSync(fd); }
  try {
    fs.renameSync(temp, file);
    const parent = fs.openSync(path.dirname(file), "r");
    try { fs.fsyncSync(parent); } finally { fs.closeSync(parent); }
  } finally { if (exists(temp)) fs.unlinkSync(temp); }
}
function isMacos() {
  return ["macos", "macOS", "Darwin", "darwin"].includes(process.env.ORBIT_OFFICE_OS || "") || (!process.env.ORBIT_OFFICE_OS && process.platform === "darwin");
}
function dataDir() {
  return path.resolve(process.env.ORBIT_OFFICE_DATA_DIR || (isMacos() ? path.join(process.env.HOME, "Library/Application Support/orbit-office") : path.join(process.env.XDG_DATA_HOME || path.join(process.env.HOME, ".local/share"), "orbit-office")));
}
function readInstallState() {
  try { return fs.readFileSync(path.join(dataDir(), "install-state"), "utf8").split("\n"); }
  catch (_) { return []; }
}
function runOptional(commandName, argv, cwd) {
  return spawnSync(commandName, argv, {
    cwd, encoding: "utf8", timeout: 300000, maxBuffer: 8 * 1024 * 1024,
    stdio: ["ignore", "pipe", "pipe"],
    env: { ...process.env, PATH: path.join(process.env.HOME, ".local/share/orbit-office/tools/bin") + path.delimiter + (process.env.PATH || "") },
  });
}
function xmlUnescape(value) {
  return value.replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&quot;/g, '"').replace(/&apos;/g, "'").replace(/&amp;/g, "&");
}
function serviceInfo() {
  const macos = isMacos();
  const file = macos ? path.join(process.env.HOME, "Library/LaunchAgents/com.orbit-office.app.plist") : path.join(process.env.HOME, ".config/systemd/user/orbit-office.service");
  const installed = Boolean(exists(file) && exists(file).isFile());
  let port = Number(process.env.ORBIT_OFFICE_PORT || 4317) || 4317;
  let bound = null;
  let logPath = null;
  if (installed) {
    let text = "";
    try { text = fs.readFileSync(file, "utf8"); } catch (_) {}
    if (macos) {
      const pm = /<key>PORT<\/key>\s*<string>([^<]+)<\/string>/.exec(text);
      if (pm && Number(pm[1])) port = Number(pm[1]);
      const wm = /<key>ORBIT_OFFICE_WORKSPACE<\/key>\s*<string>([^<]*)<\/string>/.exec(text);
      if (wm) bound = xmlUnescape(wm[1]);
      const lm = /<key>Standard(?:Out|Error)Path<\/key>\s*<string>([^<]*)<\/string>/.exec(text);
      logPath = lm ? xmlUnescape(lm[1]) : path.join(dataDir(), "app.log");
    } else {
      const pm = /Environment="?PORT=([0-9]+)"?/.exec(text);
      if (pm) port = Number(pm[1]);
      const wm = /Environment="?ORBIT_OFFICE_WORKSPACE=([^"\n]+)"?/.exec(text);
      if (wm) bound = wm[1].replace(/\\"/g, '"').replace(/%%/g, "%").replace(/\\\\/g, "\\");
    }
  }
  let running = false;
  if (installed) {
    const result = macos ? runOptional("launchctl", ["list", "com.orbit-office.app"], process.env.HOME) : runOptional("systemctl", ["--user", "is-active", "orbit-office.service"], process.env.HOME);
    running = !result.error && result.status === 0;
  }
  return { installed, running, file, macos, port, boundWorkspace: bound ? resolvePhysical(bound) : null, url: "http://localhost:" + port + "/", logPath, log_path: logPath, log_file: logPath };
}
function defaultWorkspace() {
  if (target) return path.resolve(target);
  if (process.env.ORBIT_OFFICE_HOME) return path.resolve(process.env.ORBIT_OFFICE_HOME);
  const svc = serviceInfo();
  if (svc.boundWorkspace) return svc.boundWorkspace;
  const recorded = readInstallState().filter((line) => line.startsWith("workspace_path=")).map((line) => line.slice("workspace_path=".length)).filter(Boolean);
  if (recorded.length) return path.resolve(recorded[recorded.length - 1]);
  return path.join(process.env.HOME, "orbit");
}
function resolveWorkspace() {
  if (target === "") fail("workspace path must not be empty", 2);
  return resolvePhysical(defaultWorkspace());
}
function startService(svc) {
  const result = svc.macos ? runOptional("launchctl", ["load", svc.file], process.env.HOME) : runOptional("systemctl", ["--user", "start", "orbit-office.service"], process.env.HOME);
  return !result.error && result.status === 0;
}
function agentCliRegistered() {
  const mentions = (value) => typeof value === "string" && value.toLowerCase().includes("orbit");
  const checkJson = (file) => {
    try {
      const parsed = JSON.parse(fs.readFileSync(file, "utf8"));
      const servers = parsed && typeof parsed === "object" ? parsed.mcpServers : null;
      if (!servers || typeof servers !== "object") return false;
      return Object.entries(servers).some(([name, entry]) => mentions(name) || mentions(entry && entry.command) || (Array.isArray(entry && entry.args) && entry.args.some(mentions)) || mentions(entry && entry.url));
    } catch (_) { return false; }
  };
  const claudeDir = process.env.CLAUDE_CONFIG_DIR || process.env.HOME;
  if (checkJson(path.join(claudeDir, ".claude.json"))) return true;
  if (checkJson(path.join(process.env.CLAUDE_CONFIG_DIR ? path.join(process.env.CLAUDE_CONFIG_DIR, "settings.json") : path.join(process.env.HOME, ".claude", "settings.json")))) return true;
  const museDir = process.env.XDG_CONFIG_HOME || path.join(process.env.HOME, ".config");
  if (checkJson(path.join(museDir, "muse", "settings.json"))) return true;
  try {
    const toml = fs.readFileSync(path.join(process.env.CODEX_HOME || path.join(process.env.HOME, ".codex"), "config.toml"), "utf8");
    if (/\[mcp_servers[.\]]/i.test(toml) && toml.toLowerCase().includes("orbit")) return true;
  } catch (_) {}
  return false;
}
function collectStatus(workspace) {
  const problems = [];
  const add = (check, problem, fix) => problems.push({ check, problem, fix });
  const cwd = exists(workspace) ? workspace : process.env.HOME;
  let registered = false;
  // As in init: no <workspace>/.orbit means not registered.
  const wsResult = exists(path.join(workspace, ".orbit")) ? runOptional("orbit", ["workspace", "list", "--json"], cwd) : { status: 1 };
  if (!wsResult.error && wsResult.status === 0) {
    try {
      const rows = JSON.parse(wsResult.stdout);
      registered = Array.isArray(rows) && rows.some((row) => row && typeof row.repo_root === "string" && resolvePhysical(row.repo_root) === workspace && row.status === "active");
    } catch (_) {}
  }
  if (!registered) add("workspace", "workspace is not registered: " + workspace, "orbit-office init " + workspace);
  const enabled = { office: false, gateway: false };
  const pluginResult = runOptional("orbit", ["plugin", "list", "--json", "--workspace", workspace], cwd);
  if (!pluginResult.error && pluginResult.status === 0) {
    try {
      const rows = JSON.parse(pluginResult.stdout);
      if (Array.isArray(rows)) for (const name of ["office", "gateway"]) {
        const row = rows.find((plugin) => plugin && plugin.name === name);
        enabled[name] = Boolean(row && row.host_enabled === true && row.workspace_toggle === true);
      }
    } catch (_) {}
  }
  for (const name of ["office", "gateway"]) if (!enabled[name]) add("plugin_" + name, "plugin '" + name + "' is not enabled in this workspace", "orbit-office init " + workspace);
  let tool = null;
  const toolResult = runOptional("orbit", ["tool", "run", "office.status", "--json", "--input", "{}", "--workspace", workspace], cwd);
  if (!toolResult.error && toolResult.status === 0) {
    const parsed = parseLoose(toolResult.stdout);
    if (parsed && typeof parsed === "object" && !Array.isArray(parsed) && parsed.library) tool = parsed;
  }
  const library = tool && tool.library ? {
    initialized: tool.library.initialized === true,
    documents: typeof tool.library.documents === "number" ? tool.library.documents : 0,
    ...(tool.library.documents_by_kind ? { documents_by_kind: tool.library.documents_by_kind } : {}),
    ...(tool.library.pages_off_template !== undefined ? { pages_off_template: tool.library.pages_off_template } : {}),
    ...(tool.library.template_invalid !== undefined ? { template_invalid: tool.library.template_invalid } : {}),
  } : { initialized: false, documents: 0 };
  if (!library.initialized) add("library", "Library is not initialized (not scaffolded)", "orbit-office init " + workspace);
  let meta = tool && tool.secrets && typeof tool.secrets.meta_api_key === "string" ? tool.secrets.meta_api_key : null;
  if (meta !== "set" && meta !== "unset") {
    meta = "unset";
    const secretResult = runOptional("orbit", ["plugin", "secret", "list", "office"], cwd);
    if (!secretResult.error && secretResult.status === 0) {
      for (const line of String(secretResult.stdout || "").split("\n")) {
        const parts = line.trim().split(/\s+/);
        if (parts[0] === "meta_api_key" && parts.includes("set") && !parts.includes("unset")) meta = "set";
      }
    }
  } else if (meta !== "set") {
    const secretResult = runOptional("orbit", ["plugin", "secret", "list", "office"], cwd);
    if (!secretResult.error && secretResult.status === 0) {
      for (const line of String(secretResult.stdout || "").split("\n")) {
        const parts = line.trim().split(/\s+/);
        if (parts[0] === "meta_api_key" && parts.includes("set") && !parts.includes("unset")) meta = "set";
      }
    }
  }
  if (meta !== "set") add("meta_api_key", "Meta API key is not set", "orbit plugin secret set office meta_api_key");
  const privateRoute = tool && tool.open_weights !== undefined ? tool.open_weights : null;
  const svc = serviceInfo();
  if (!svc.installed) add("app_service", "app service is not installed", "install.sh");
  else if (svc.boundWorkspace && svc.boundWorkspace !== workspace) add("app_service", "app service serves " + svc.boundWorkspace + " not " + workspace, "install.sh --home " + workspace);
  else if (!svc.running) add("app_service", "app service is not running", "orbit-office open " + workspace);
  const agentRegistered = agentCliRegistered();
  if (!agentRegistered) add("agent_cli", "agent CLI registration is missing (no Orbit MCP entry)", "install.sh");
  return {
    workspace, workspace_registered: registered,
    plugins: { office: enabled.office, gateway: enabled.gateway },
    library, secrets: { meta_api_key: meta }, meta_api_key: meta,
    private_route: privateRoute,
    app_service: { installed: svc.installed, running: svc.running, url: svc.url, port: svc.port, workspace: svc.boundWorkspace, log_path: svc.logPath, log_file: svc.logPath, service_log: svc.logPath },
    app: { installed: svc.installed, running: svc.running, url: svc.url },
    agent_cli: { registered: agentRegistered },
    ok: problems.length === 0, problems,
  };
}
function statusMain() {
  if (!process.env.HOME) fail("HOME is not set");
  const workspace = resolveWorkspace();
  const report = collectStatus(workspace);
  if (jsonOutput) {
    process.stdout.write(JSON.stringify(report) + "\n");
  } else {
    const lines = [];
    lines.push("Workspace: " + report.workspace + " — " + (report.workspace_registered ? "registered" : "not registered"));
    lines.push("Plugins: office " + (report.plugins.office ? "enabled" : "not enabled") + "; gateway " + (report.plugins.gateway ? "enabled" : "not enabled"));
    lines.push("Library: " + (report.library.initialized ? "initialized (" + report.library.documents + " documents)" : "not initialized"));
    lines.push("Meta API key: " + report.meta_api_key);
    if (report.private_route && typeof report.private_route === "object") {
      lines.push("Private route: configured (" + (report.private_route.endpoint || "") + (report.private_route.model ? ", model " + report.private_route.model : "") + ")");
    } else lines.push("Private route: not configured (optional)");
    if (!report.app_service.installed) lines.push("App service: not installed");
    else if (report.app_service.running) lines.push("App service: running at " + report.app_service.url);
    else lines.push("App service: stopped (installed; not running) — " + report.app_service.url);
    if (report.app_service.log_path) lines.push("App service log: " + report.app_service.log_path);
    else if (report.app_service.installed) lines.push("App service log: journalctl --user -u orbit-office.service");
    lines.push("Agent CLI: " + (report.agent_cli.registered ? "registered" : "not registered"));
    if (report.ok) lines.push("Status: OK");
    else {
      lines.push("Problems:");
      for (const problem of report.problems) lines.push("- " + problem.problem + " — fix: " + problem.fix);
    }
    process.stdout.write(lines.join("\n") + "\n");
  }
  process.exit(report.ok ? 0 : 1);
}
function openMain() {
  if (!process.env.HOME) fail("HOME is not set");
  if (target === "") fail("workspace path must not be empty", 2);
  const workspace = resolveWorkspace();
  let svc = serviceInfo();
  if (!svc.installed) fail("app service is not installed; run install.sh to install it");
  if (svc.boundWorkspace && svc.boundWorkspace !== workspace) fail("app service serves " + svc.boundWorkspace + " not " + workspace + "; run install.sh --home " + workspace + " to install the service for this workspace");
  if (!svc.running) {
    startService(svc);
    svc = serviceInfo();
    if (!svc.running) fail("app service did not start; fix the login service and re-run, or run install.sh");
  }
  const browser = process.env.ORBIT_OFFICE_BROWSER || (svc.macos ? "open" : "xdg-open");
  const result = spawnSync(browser, [svc.url], { stdio: "ignore", env: process.env });
  if (result.error || result.status !== 0) fail("could not open " + svc.url + " with " + browser + (result.error ? ": " + result.error.message : " (exit: " + result.status + ")"));
  say("app: " + svc.url);
}
function main() {
  const requested = path.resolve(target || process.env.ORBIT_OFFICE_HOME || path.join(process.env.HOME, "orbit"));
  const entry = exists(requested);
  if (entry && (entry.isSymbolicLink() || !entry.isDirectory())) fail("workspace path " + requested + " exists and is not a folder or is a symlink");
  const workspace = resolvePhysical(requested);
  if ([resolvePhysical(process.env.HOME), path.parse(workspace).root].includes(workspace)) fail("refusing to use " + workspace + " as the workspace");
  const macos = ["macos", "macOS", "Darwin", "darwin"].includes(process.env.ORBIT_OFFICE_OS || "") || (!process.env.ORBIT_OFFICE_OS && process.platform === "darwin");
  const data = path.resolve(process.env.ORBIT_OFFICE_DATA_DIR || (macos ? path.join(process.env.HOME, "Library/Application Support/orbit-office") : path.join(process.env.XDG_DATA_HOME || path.join(process.env.HOME, ".local/share"), "orbit-office")));
  const stateFile = path.join(data, "install-state");
  for (let current = stateFile; current !== path.dirname(current); current = path.dirname(current)) {
    const stat = exists(current);
    if (stat && stat.isSymbolicLink()) fail("refusing symlink install state path: " + current);
  }
  const stateStat = exists(stateFile);
  if (stateStat && (!stateStat.isFile() || stateStat.uid !== process.getuid() || (stateStat.mode & 0o022))) fail("unsafe install state: " + stateFile);
  let state = stateStat ? fs.readFileSync(stateFile, "utf8") : "";
  const has = (line) => state.split("\n").includes(line);
  function mark(line) {
    if (has(line)) return;
    fs.mkdirSync(data, { recursive: true, mode: 0o700 });
    const next = state + (state && !state.endsWith("\n") ? "\n" : "") + line + "\n";
    writeAtomic(stateFile, next);
    state = next;
  }
  const names = entry ? fs.readdirSync(workspace) : [];
  for (const name of [".git", ".gitignore", ".orbit"]) {
    const stat = exists(path.join(workspace, name));
    if (stat && stat.isSymbolicLink()) fail("refusing symlink workspace entry: " + path.join(workspace, name));
  }
  const ignore = path.join(workspace, ".gitignore");
  const ignoreStat = exists(ignore);
  const partial = exists(path.join(workspace, ".git"))?.isDirectory() && ignoreStat?.isFile() &&
    fs.readFileSync(ignore, "utf8").split("\n").includes("/library/") && names.every((name) => [".git", ".gitignore", ".orbit"].includes(name));
  if (names.length && !has("workspace_path=" + workspace) && !partial) fail("refusing to adopt non-empty folder " + workspace + ": it is not an office workspace (move it aside or choose another path)");
  run("git", ["--version"]);
  const installed = json("orbit", ["plugin", "list", "--json"]);
  for (const name of ["office", "gateway"]) {
    if (!installed.some((plugin) => plugin.name === name)) fail("plugin '" + name + "' is not installed; run install.sh first");
  }
  const officePlugin = installed.find((plugin) => plugin.name === "office");
  if (!officePlugin?.install_path) fail("office plugin has no installed path; reinstall it");
  const presetDir = path.join(officePlugin.install_path, "presets");
  const bootstrapPreset = path.join(presetDir, "office-bootstrap.toml");
  const officePreset = path.join(presetDir, "office.toml");
  for (const preset of [bootstrapPreset, officePreset]) if (!exists(preset)?.isFile()) fail("office preset missing: " + preset + "; reinstall the office plugin");
  const plugins = ["office", "gateway", ...(installed.some((plugin) => plugin.name === "cloudcli") ? ["cloudcli"] : [])];
  say("==> 4 workspace");
  fs.mkdirSync(workspace, { recursive: true, mode: 0o700 });
  if (!exists(path.join(workspace, ".git"))) {
    const result = run("git", ["init", "-b", "main"], workspace, false);
    if (result.status !== 0) {
      run("git", ["init"], workspace);
      run("git", ["symbolic-ref", "HEAD", "refs/heads/main"], workspace);
    }
    say("workspace: ran git init -b main (" + workspace + ")");
  } else say("workspace: git repository already done (" + workspace + ")");
  const oldIgnore = exists(ignore) ? fs.readFileSync(ignore, "utf8") : "";
  if (!oldIgnore.split("\n").includes("/library/")) writeAtomic(ignore, oldIgnore + (oldIgnore && !oldIgnore.endsWith("\n") ? "\n" : "") + "/library/\n");
  const login = os.userInfo().username || "orbit";
  for (const [key, fallback] of [["user.name", login], ["user.email", login + "@localhost"]]) {
    if (!run("git", ["config", key], workspace, false).stdout.trim()) run("git", ["config", key, fallback], workspace);
  }
  if (run("git", ["rev-parse", "--verify", "HEAD"], workspace, false).status !== 0) {
    run("git", ["add", ".gitignore"], workspace);
    run("git", ["commit", "-m", "Initial commit"], workspace);
    say("workspace: made the first commit");
  } else say("workspace: first commit already done");
  // `orbit workspace init` creates <workspace>/.orbit, so a folder without
  // one is not registered. Asking Orbit from such a folder can fail before
  // registration (a read-only join with no binding), so skip the query.
  const registrations = exists(path.join(workspace, ".orbit")) ? json("orbit", ["workspace", "list", "--json"], workspace) : [];
  if (!registrations.some((row) => typeof row.repo_root === "string" && resolvePhysical(row.repo_root) === workspace && row.status === "active")) {
    mark("preset_pending=" + workspace);
    // Orbit names a workspace after its folder, so ~/orbit would claim
    // `ws_orbit` and collide with a code workspace of that name. Office
    // workspaces get their own prefix.
    run("orbit", ["workspace", "init", "--preset", bootstrapPreset, "--name", officeWorkspaceName(workspace)], workspace);
    say("workspace: registered with office seeding policy");
  } else say("workspace: already registered (" + workspace + ")");
  mark("workspace_path=" + workspace);
  say("==> 5 workspace plugins");
  const rows = json("orbit", ["plugin", "list", "--json", "--workspace", workspace], workspace);
  const grants = {
    office: ["fs={{workspace}},{{plugin_state}}", "network", "orbit_tools"],
    gateway: ["fs={{workspace}}/.orbit-gateway,{{plugin_state}}", "network", "orbit_tools"],
    cloudcli: ["fs={{plugin_state}}", "network"],
  };
  const needsHost = (row, name) => row.host_enabled !== true || grants[name].some((grant) => !row.permissions?.some((permission) => permission.grant === grant.split("=")[0] && permission.granted === true));
  if (plugins.some((name) => { const row = rows.find((plugin) => plugin.name === name); return !row || needsHost(row, name) || row.workspace_toggle !== true; })) {
    say("The plugins " + plugins.join(" ") + " request these grants:");
    say("  office: fs {{workspace}}, {{plugin_state}}; network unscoped; orbit_tools task, friction, search and auto-task");
    say("  gateway: fs {{workspace}}/.orbit-gateway, {{plugin_state}}; network unscoped; orbit_tools task add/artifact/show/update");
    if (plugins.includes("cloudcli")) say("  cloudcli: fs {{plugin_state}}; network unscoped; programs python3, aws, gcloud");
    if (yes) say("Consent: --yes given, enabling " + plugins.join(" ") + ".");
    else if (require("node:tty").isatty(0)) {
      process.stderr.write("Enable plugins " + plugins.join(" ") + " with these grants? [y/N] ");
      let answer = "";
      const byte = Buffer.alloc(1);
      while (fs.readSync(0, byte, 0, 1, null) && byte[0] !== 10) answer += byte.toString();
      if (!["y", "yes"].includes(answer.trim().toLowerCase())) fail("plugin enable declined; re-run with --yes to consent");
    } else say("Consent: non-interactive run, enabling " + plugins.join(" ") + " (pass --yes to confirm explicitly).");
  }
  for (const name of plugins) {
    const row = rows.find((plugin) => plugin.name === name);
    if (!row) fail("plugin disappeared during setup: " + name);
    if (needsHost(row, name)) run("orbit", ["plugin", "enable", name, ...grants[name].flatMap((grant) => ["--grant", grant]), "--workspace", workspace], workspace);
    if (row.workspace_toggle !== true) run("orbit", ["plugin", "enable", name, "--scope", "workspace", "--workspace", workspace], workspace);
    mark(name === "office" ? "plugin_enabled" : "plugin." + name + ".enabled");
    say("plugin: already enabled or repaired (" + name + ", " + workspace + ")");
  }
  if (has("preset_pending=" + workspace) && !has("preset_applied=" + workspace)) {
    run("orbit", ["workspace", "init", "--force", "--preset", officePreset, "--name", officeWorkspaceName(workspace)], workspace);
    mark("preset_applied=" + workspace);
    say("workspace: office preset applied");
  }
  say("==> 7 scaffold");
  const result = run("orbit", ["tool", "run", "office.init", "--json", "--full", "--input", "{}", "--workspace", workspace], workspace, false, operatorEnv());
  if (result.stderr) process.stderr.write(result.stderr);
  const status = initStatus(result);
  if (result.error || result.status !== 0 || status !== "ok") fail("scaffold: office.init failed (code: " + status + ", exit: " + result.status + ")");
  mark("scaffolded");
  say("scaffold: already initialized or repaired (office.init, " + workspace + "/library)");
}
try { if (command === "open") openMain(); else if (command === "status") statusMain(); else main(); } catch (error) { fail(error.message); }
OFFICE_LAUNCHER
    if [ -e "$LAUNCHER" ] || [ -L "$LAUNCHER" ]; then
        [ -f "$LAUNCHER" ] && [ ! -L "$LAUNCHER" ] \
            || die "refusing to replace unrelated launcher $LAUNCHER"
        if state_has "launcher_path=$LAUNCHER"; then
            _old_digest=$(launcher_digest "$LAUNCHER")
            state_has "launcher_digest=$_old_digest" \
                || die "refusing to replace modified launcher $LAUNCHER (move it aside and re-run)"
        else
            cmp -s "$_candidate" "$LAUNCHER" && [ -x "$LAUNCHER" ] \
                || die "refusing to replace unrelated launcher $LAUNCHER (move it aside and re-run)"
        fi
    else
        mkdir -p "$(dirname -- "$LAUNCHER")"
        _launcher_created=1
    fi
    if [ ! -f "$LAUNCHER" ] || ! cmp -s "$_candidate" "$LAUNCHER"; then
        # Stage next to the destination so replacement is atomic.
        cp "$_candidate" "$LAUNCHER.install-$$"
        chmod 755 "$LAUNCHER.install-$$"
        mv "$LAUNCHER.install-$$" "$LAUNCHER"
    fi
    if [ "$_launcher_created" -eq 1 ] || state_has "launcher_path=$LAUNCHER"; then
        _new_digest=$(launcher_digest "$LAUNCHER")
        state_mark "launcher_digest=$_new_digest"
        state_mark "launcher_path=$LAUNCHER"
        if [ -n "${_old_digest:-}" ] && [ "$_old_digest" != "$_new_digest" ]; then
            state_clear "launcher_digest=$_old_digest"
        fi
    fi
    rm -f "$_candidate"
    trap - EXIT HUP INT TERM
    say "office: launcher already installed or updated ($LAUNCHER)"
}

step_workspace_init() {
    if [ "$ASSUME_YES" -eq 1 ]; then
        PATH="$TOOLS_BIN:$PATH" "$LAUNCHER" init "$WORKSPACE" --yes
    else
        PATH="$TOOLS_BIN:$PATH" "$LAUNCHER" init "$WORKSPACE"
    fi
}

plugin_is_listed() {
    command -v orbit >/dev/null 2>&1 || return 2
    _list=$(orbit plugin list 2>/dev/null) || return 2
    printf '%s\n' "$_list" | awk -v name="$1" '$1 == name { found=1 } END { exit !found }'
}

plugin_marker() {
    # Keep the original office markers for upgrades from older installs.
    if [ "$1" = "office" ]; then printf 'plugin_%s' "$2";
    else printf 'plugin.%s.%s' "$1" "$2"; fi
}

# gateway and cloudcli have no build step. In a clone, assemble the same
# export as gateway/scripts/export.py, including the shared Python package.
# A release bundle already has this layout and is used without rebuilding.
gateway_export() {
    if [ -z "$SCRIPT_DIR" ]; then
        printf '%s\n' "plugin export unavailable; run from a clone or set ORBIT_OFFICE_PLUGIN_BUNDLE" >&2
        return 1
    fi
    EXPORT_REPO="$SCRIPT_DIR" EXPORT_OUT="$DATA_DIR/plugins/$1" EXPORT_NAME="$1" node <<'NODE'
const fs = require("fs");
const path = require("path");
const repo = process.env.EXPORT_REPO;
const out = process.env.EXPORT_OUT;
const name = process.env.EXPORT_NAME;
if (!repo) {
  process.stderr.write("plugin export unavailable; run from a clone or set ORBIT_OFFICE_PLUGIN_BUNDLE\n");
  process.exit(1);
}
const source = path.join(repo, "gateway", "plugins", name, ".orbit-plugin");
const shared = path.join(repo, "gateway", "src", "orbit_gateway");
if (!fs.existsSync(path.join(source, "plugin.yaml")) || !fs.existsSync(shared)) {
  process.stderr.write("plugin export unavailable; run from a clone or set ORBIT_OFFICE_PLUGIN_BUNDLE\n");
  process.exit(1);
}
if (fs.existsSync(out)) {
  process.stderr.write("incomplete plugin export; move aside " + out + " and retry\n");
  process.exit(1);
}
const filter = (p) => !["__pycache__", ".DS_Store"].includes(path.basename(p)) && !p.endsWith(".pyc");
fs.mkdirSync(out, { recursive: true, mode: 0o700 });
try {
  fs.cpSync(source, path.join(out, ".orbit-plugin"), { recursive: true, filter });
  fs.cpSync(shared, path.join(out, ".orbit-plugin", "lib", "orbit_gateway"), { recursive: true, filter });
} catch (_) {
  fs.rmSync(out, { recursive: true, force: true });
  process.stderr.write("could not assemble plugin export\n");
  process.exit(1);
}
NODE
}

# Build office from the clone's HEAD into the same layout `make -C plugin
# export` produces. Orbit's git+ sources need `.orbit-plugin/` at the
# repository root, and this repository keeps it under plugin/, so office is
# built here and added as a directory. A throwaway local clone pins HEAD and
# keeps uncommitted and untracked files out of the export.
office_export() {
    _oe_out="$DATA_DIR/plugins/office"
    command -v cargo >/dev/null 2>&1 \
        || die "building the office plugin needs cargo (install Rust from https://rustup.rs), or set ORBIT_OFFICE_PLUGIN_BUNDLE"
    [ ! -e "$_oe_out" ] || die "incomplete plugin export; move aside $_oe_out and retry"
    _oe_rev=$(git -C "$SCRIPT_DIR" rev-parse --verify 'HEAD^{commit}' 2>/dev/null) \
        || die "--from-source needs a clone; otherwise set ORBIT_OFFICE_PLUGIN_BUNDLE"
    _oe_work=$(mktemp -d "${TMPDIR:-/tmp}/office-plugin-build.XXXXXX") || return 1
    say "plugin: building office from $_oe_rev (cargo build --release; this takes a few minutes)"
    if git clone --quiet --no-hardlinks --no-checkout "$SCRIPT_DIR" "$_oe_work/source" \
        && git -C "$_oe_work/source" checkout --quiet --detach "$_oe_rev" \
        && (cd "$_oe_work/source" && cargo build --release --locked -p office --target-dir "$_oe_work/target") \
        && mkdir -p "$DATA_DIR/plugins" \
        && cp -R "$_oe_work/source/plugin" "$_oe_out.build-$$" \
        && cp "$_oe_work/target/release/office" "$_oe_out.build-$$/.orbit-plugin/bin/office-bin" \
        && chmod 755 "$_oe_out.build-$$/.orbit-plugin/bin/office-bin" \
        && mv "$_oe_out.build-$$" "$_oe_out"; then
        rm -rf "$_oe_work"
        return 0
    fi
    rm -rf "$_oe_work" "$_oe_out.build-$$"
    return 1
}

step_plugin() {
    step "5 plugin"
    if [ -n "${ORBIT_OFFICE_PLUGIN_BUNDLE:-}" ]; then
        for _plugin in $INSTALL_PLUGINS; do
            [ -f "$ORBIT_OFFICE_PLUGIN_BUNDLE/$_plugin/.orbit-plugin/plugin.yaml" ] \
                || die "missing $_plugin export in ORBIT_OFFICE_PLUGIN_BUNDLE"
        done
    fi
    for _plugin in $INSTALL_PLUGINS; do
        _added=$(plugin_marker "$_plugin" added)
        if plugin_is_listed "$_plugin" || state_has "$_added"; then
            say "plugin: already added ($_plugin)"
        else
            if [ -n "${ORBIT_OFFICE_PLUGIN_BUNDLE:-}" ]; then
                _source="$ORBIT_OFFICE_PLUGIN_BUNDLE/$_plugin"
                [ -f "$_source/.orbit-plugin/plugin.yaml" ] || die "missing $_plugin export in ORBIT_OFFICE_PLUGIN_BUNDLE"
            elif [ "$_plugin" != "office" ]; then
                _source="$DATA_DIR/plugins/$_plugin"
                # Reuse a completed export when add failed on an earlier run.
                if ! state_has "plugin.$_plugin.exported"; then
                    gateway_export "$_plugin" || die "could not prepare $_plugin export"
                    state_mark "plugin.$_plugin.exported"
                fi
            elif [ -n "${ORBIT_OFFICE_PLUGIN_SOURCE:-}" ]; then
                _source="$ORBIT_OFFICE_PLUGIN_SOURCE"
            elif [ "$FROM_SOURCE" -eq 1 ] || [ -z "$PLUGIN_SRC" ] \
                || [ ! -x "$PLUGIN_SRC/.orbit-plugin/bin/office-bin" ]; then
                if [ -z "$SCRIPT_DIR" ] || [ ! -e "$SCRIPT_DIR/.git" ]; then
                    die "--from-source needs a clone; otherwise set ORBIT_OFFICE_PLUGIN_BUNDLE"
                fi
                _source="$DATA_DIR/plugins/office"
                # Reuse a completed export when add failed on an earlier run.
                if ! state_has "plugin.office.exported"; then
                    office_export || die "could not build the office plugin from $SCRIPT_DIR"
                    state_mark "plugin.office.exported"
                fi
            else
                _source="$PLUGIN_SRC"
            fi
            if [ "$_plugin" = office ] && [ ! -x "$_source/.orbit-plugin/bin/office-bin" ]; then
                die "office plugin export lacks executable bin/office-bin: $_source"
            fi
            orbit plugin add "$_source"
            state_mark "$_added"
            say "plugin: added $_plugin from $_source"
        fi
    done
}

step_secrets() {
    step "6 secrets"
    set_secret "meta_api_key" "${ORBIT_OFFICE_META_API_KEY:-${META_API_KEY:-}}" 0
    # STD-01@2 §R35: the retired names stay accepted for one release as
    # aliases of their replacements, each use warning on stderr. The
    # warning names the variable only; its value is never echoed.
    _open_weights_token="${ORBIT_OFFICE_OPEN_WEIGHTS_TOKEN:-${OPEN_WEIGHTS_TOKEN:-}}"
    if [ -n "${ORBIT_OFFICE_BASETEN_TOKEN:-}" ]; then
        warn "ORBIT_OFFICE_BASETEN_TOKEN is deprecated; use ORBIT_OFFICE_OPEN_WEIGHTS_TOKEN instead"
        if [ -z "$_open_weights_token" ]; then
            _open_weights_token="$ORBIT_OFFICE_BASETEN_TOKEN"
        fi
    fi
    if [ -n "${BASETEN_TOKEN:-}" ]; then
        warn "BASETEN_TOKEN is deprecated; use OPEN_WEIGHTS_TOKEN instead"
        if [ -z "$_open_weights_token" ]; then
            _open_weights_token="$BASETEN_TOKEN"
        fi
    fi
    set_secret "open_weights_token" "$_open_weights_token" 1
    _open_weights_token=""
}

# Register the Orbit MCP entry with the agent CLI. Node is a proven
# prerequisite, so JSON edits go through it; the old file is backed up
# once to <settings>.bak. The entry is "already done" only when it
# matches the desired command, args and PATH. A marker from an older
# install is not enough: that install had no tools/bin on PATH.
# JSON.parse validates the document; the range parser below locates entries
# so edits splice only their bytes. Never round-trip unrelated user settings.
# Receipts contain only owned items: claude-settings.json holds added deny
# rule names and newly created containers; claude-mcp.json holds whether
# the MCP entry in .claude.json is installer-owned and its new containers.

# ---------------------------------------------------------------------------
# App, uninstall and main
# ---------------------------------------------------------------------------

# A provisioned app is only usable when the service's entrypoint exists,
# is non-empty and parses under node. The login service is never written
# or enabled for an app that fails this check.
app_entrypoint_ok() {
    _entry="$1/packages/server/dist/main.js"
    [ -f "$_entry" ] && [ -s "$_entry" ] || return 1
    command -v node >/dev/null 2>&1 || return 1
    node --check "$_entry" >/dev/null 2>&1
}

step_app() {
    step "9 app"
    compute_agent_path
    validate_agent_path
    if ! _node_bin=$(stable_node_path); then
        _raw_node=$(command -v node 2>/dev/null || printf 'node')
        die "app: node resolves to a per-shell shim ($_raw_node) under a fnm_multishells directory, which fnm removes when its shell exits, so the login service would not start; install a stable Node.js 20+ (a fnm default install, nvm, or the nodejs.org tarball) and put its bin directory, not a fnm_multishells path, first on PATH, then re-run"
    fi
    if [ -n "$APP_SRC" ]; then
        _app_home="$APP_SRC"
        if app_entrypoint_ok "$APP_SRC"; then
            say "app: build already done"
        else
            npm --prefix "$APP_SRC" run build >/dev/null
            app_entrypoint_ok "$APP_SRC" \
                || die "app: build did not produce a runnable entrypoint at $APP_SRC/packages/server/dist/main.js; run 'npm --prefix $APP_SRC run build' to see the failure, then re-run"
            state_mark "app_built"
            say "app: built ($APP_SRC)"
        fi
    elif [ -n "${ORBIT_OFFICE_APP_BUNDLE:-}" ]; then
        _bundle="${ORBIT_OFFICE_APP_BUNDLE%/}"
        [ -d "$_bundle" ] \
            || die "app: ORBIT_OFFICE_APP_BUNDLE is not a directory: ${ORBIT_OFFICE_APP_BUNDLE}; supply an unpacked app bundle containing packages/server/dist/main.js, or run install.sh from a clone beside app/package.json, then re-run"
        # The bundle is the app root itself; a bundle that wraps it in a
        # single app/ folder is accepted as the same layout.
        if ! app_entrypoint_ok "$_bundle" && app_entrypoint_ok "$_bundle/app"; then
            _bundle="$_bundle/app"
        fi
        app_entrypoint_ok "$_bundle" \
            || die "app: ORBIT_OFFICE_APP_BUNDLE (${ORBIT_OFFICE_APP_BUNDLE}) has no runnable packages/server/dist/main.js; supply an unpacked app bundle containing packages/server/dist/main.js and its runtime files, or run install.sh from a clone beside app/package.json, then re-run"
        _app_home="$DATA_DIR/app"
        if app_entrypoint_ok "$_app_home"; then
            _app_already=1
        else
            _app_already=0
        fi
        if [ "$_bundle" != "$_app_home" ]; then
            mkdir -p "$_app_home"
            cp -R "$_bundle/." "$_app_home/" \
                || die "app: could not copy the app bundle from $_bundle to $_app_home"
        fi
        app_entrypoint_ok "$_app_home" \
            || die "app: the app bundle did not provision a runnable entrypoint at $_app_home/packages/server/dist/main.js; supply a complete app bundle, then re-run"
        if [ "$_app_already" -eq 1 ]; then
            say "app: build already done (app at $_app_home)"
        else
            say "app: installed from ORBIT_OFFICE_APP_BUNDLE ($_bundle -> $_app_home)"
        fi
    else
        _app_home="$DATA_DIR/app"
        if app_entrypoint_ok "$_app_home"; then
            say "app: build already done (app at $_app_home)"
        else
            die "app: no local app tree and no app bundle: missing $_app_home/packages/server/dist/main.js; run install.sh from a clone (beside app/package.json) or set ORBIT_OFFICE_APP_BUNDLE to an unpacked app bundle containing packages/server/dist/main.js, then re-run"
        fi
    fi
    app_entrypoint_ok "$_app_home" \
        || die "app: missing or unrunnable entrypoint at $_app_home/packages/server/dist/main.js; provision the app (clone build or ORBIT_OFFICE_APP_BUNDLE) before installing the service, then re-run"
    _server_js="$_app_home/packages/server/dist/main.js"
    # Workspace binding for the app (ORBIT_OFFICE_WORKSPACE): the absolute
    # workspace path recorded as workspace_path. It names the workspace
    # only; it grants no operator capability (see client.ts authority note).
    # The value expands inside unquoted heredocs, so escape it for the
    # target format: XML for the plist, and for systemd a double-quoted
    # Environment= assignment with backslash, quote and percent escaped
    # (percent is doubled to escape systemd specifiers).
    _workspace_xml=$(printf '%s' "$WORKSPACE" | sed -e 's/&/\&amp;/g' -e 's/</\&lt;/g' -e 's/>/\&gt;/g')
    _workspace_systemd=$(printf '%s' "$WORKSPACE" | sed -e 's/\\/\\\\/g' -e 's/%/%%/g' -e 's/"/\\"/g')
    _service_log="$SERVICE_LOG"
    _service_log_xml=$(printf '%s' "$_service_log" | sed -e 's/&/\&amp;/g' -e 's/</\&lt;/g' -e 's/>/\&gt;/g')
    if [ "$OS" = "macos" ]; then
        mkdir -p "$DATA_DIR"
        [ -f "$_service_log" ] || (umask 077; : > "$_service_log")
        chmod 600 "$_service_log"
        if write_if_changed "$LAUNCHD_PLIST" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>com.orbit-office.app</string>
  <key>ProgramArguments</key>
  <array><string>$_node_bin</string><string>$_server_js</string></array>
  <key>EnvironmentVariables</key>
  <dict><key>PORT</key><string>$PORT</string><key>HOST</key><string>127.0.0.1</string><key>PATH</key><string>$AGENT_PATH</string><key>ORBIT_OFFICE_WORKSPACE</key><string>$_workspace_xml</string></dict>
  <key>WorkingDirectory</key><string>$_workspace_xml</string>
  <key>StandardOutPath</key><string>$_service_log_xml</string>
  <key>StandardErrorPath</key><string>$_service_log_xml</string>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
</dict>
</plist>
EOF
        then
            say "app: wrote launchd agent $LAUNCHD_PLIST"
            _svc_changed=1
        else
            _svc_changed=0
            say "app: service already done ($LAUNCHD_PLIST)"
        fi
        if [ "$_svc_changed" -eq 1 ] || ! state_has "service_installed"; then
            if ! command -v launchctl >/dev/null 2>&1; then
                state_clear "service_installed"
                die "service: launchctl is not available; cannot install the login agent $LAUNCHD_PLIST (re-run on macOS with launchd)"
            fi
            launchctl unload "$LAUNCHD_PLIST" >/dev/null 2>&1 || true
            if ! launchctl load "$LAUNCHD_PLIST" >/dev/null 2>&1; then
                state_clear "service_installed"
                die "service: launchctl load $LAUNCHD_PLIST failed; fix launchd and re-run to retry"
            fi
            if ! launchctl list com.orbit-office.app >/dev/null 2>&1; then
                state_clear "service_installed"
                die "service: launchd agent com.orbit-office.app is not loaded after load; fix launchd and re-run to retry"
            fi
            state_mark "service_installed"
            say "app: service installed (login agent com.orbit-office.app)"
        else
            if ! command -v launchctl >/dev/null 2>&1; then
                state_clear "service_installed"
                die "service: launchctl is not available; cannot verify the login agent $LAUNCHD_PLIST (re-run on macOS with launchd)"
            fi
            if launchctl list com.orbit-office.app >/dev/null 2>&1; then
                say "app: service already done ($LAUNCHD_PLIST)"
            else
                launchctl unload "$LAUNCHD_PLIST" >/dev/null 2>&1 || true
                if ! launchctl load "$LAUNCHD_PLIST" >/dev/null 2>&1; then
                    state_clear "service_installed"
                    die "service: launchctl load $LAUNCHD_PLIST failed; fix launchd and re-run to retry"
                fi
                if ! launchctl list com.orbit-office.app >/dev/null 2>&1; then
                    state_clear "service_installed"
                    die "service: launchd agent com.orbit-office.app is not loaded after load; fix launchd and re-run to retry"
                fi
                say "app: service re-enabled (login agent com.orbit-office.app)"
            fi
        fi
    else
        if write_if_changed "$SYSTEMD_UNIT" <<EOF
[Unit]
Description=orbit-office app
After=network.target

[Service]
Type=simple
WorkingDirectory=$_app_home
ExecStart=$_node_bin $_server_js
Environment=PORT=$PORT
Environment=HOST=127.0.0.1
Environment=PATH=$AGENT_PATH
Environment="ORBIT_OFFICE_WORKSPACE=$_workspace_systemd"
Restart=on-failure

[Install]
WantedBy=default.target
EOF
        then
            say "app: wrote systemd user unit $SYSTEMD_UNIT"
            _svc_changed=1
        else
            _svc_changed=0
            say "app: service already done ($SYSTEMD_UNIT)"
        fi
        if [ "$_svc_changed" -eq 1 ] || ! state_has "service_installed"; then
            if ! command -v systemctl >/dev/null 2>&1; then
                state_clear "service_installed"
                die "service: systemctl is not available; cannot install the user unit $SYSTEMD_UNIT (re-run with systemd --user)"
            fi
            if ! systemctl --user daemon-reload >/dev/null 2>&1; then
                state_clear "service_installed"
                die "service: systemctl --user daemon-reload failed; fix systemd and re-run to retry"
            fi
            if ! systemctl --user enable "$SERVICE_NAME.service" >/dev/null 2>&1; then
                state_clear "service_installed"
                die "service: systemctl --user enable $SERVICE_NAME.service failed; fix systemd and re-run to retry"
            fi
            if ! systemctl --user restart "$SERVICE_NAME.service" >/dev/null 2>&1; then
                if ! systemctl --user start "$SERVICE_NAME.service" >/dev/null 2>&1; then
                    state_clear "service_installed"
                    die "service: systemctl --user start $SERVICE_NAME.service failed; fix systemd and re-run to retry"
                fi
            fi
            if ! systemctl --user is-active "$SERVICE_NAME.service" >/dev/null 2>&1; then
                state_clear "service_installed"
                die "service: $SERVICE_NAME.service is not active after start; fix systemd and re-run to retry"
            fi
            state_mark "service_installed"
            say "app: service installed (login service $SERVICE_NAME)"
        else
            if ! command -v systemctl >/dev/null 2>&1; then
                state_clear "service_installed"
                die "service: systemctl is not available; cannot verify the user unit $SYSTEMD_UNIT (re-run with systemd --user)"
            fi
            if systemctl --user is-active "$SERVICE_NAME.service" >/dev/null 2>&1; then
                say "app: service already enabled ($SERVICE_NAME)"
            else
                if ! systemctl --user restart "$SERVICE_NAME.service" >/dev/null 2>&1; then
                    if ! systemctl --user start "$SERVICE_NAME.service" >/dev/null 2>&1; then
                        state_clear "service_installed"
                        die "service: systemctl --user start $SERVICE_NAME.service failed; fix systemd and re-run to retry"
                    fi
                fi
                if ! systemctl --user is-active "$SERVICE_NAME.service" >/dev/null 2>&1; then
                    state_clear "service_installed"
                    die "service: $SERVICE_NAME.service is not active after start; fix systemd and re-run to retry"
                fi
                say "app: service re-enabled (login service $SERVICE_NAME)"
            fi
        fi
    fi
    say "app: http://localhost:$PORT/ (open it to install the PWA)"
    if [ -t 1 ] && [ "${ORBIT_OFFICE_NO_OPEN:-0}" != "1" ]; then
        if [ "$OS" = "macos" ] && command -v open >/dev/null 2>&1; then
            open "http://localhost:$PORT/" >/dev/null 2>&1 || true
        elif command -v xdg-open >/dev/null 2>&1; then
            xdg-open "http://localhost:$PORT/" >/dev/null 2>&1 || true
        fi
    fi
}

# ---------------------------------------------------------------------------
# Uninstall / purge
# ---------------------------------------------------------------------------

do_uninstall() {
    _purge="$1"
    # Prove ownership before any teardown. A refusal must leave the
    # service, plugin, tools, workspace and data folder in place.
    if [ "$_purge" -eq 1 ]; then
        refuse_protected_purge
        if ! is_office_workspace; then
            die "refusing to purge $WORKSPACE: it is not an office workspace this installer owns"
        fi
    fi
    step "uninstall"
    # Service first.
    if [ "$OS" = "macos" ]; then
        if [ -f "$LAUNCHD_PLIST" ]; then
            if command -v launchctl >/dev/null 2>&1; then
                launchctl unload "$LAUNCHD_PLIST" >/dev/null 2>&1 || true
            fi
            rm -f "$LAUNCHD_PLIST"
            say "uninstall: removed the login service"
        else
            say "uninstall: service already removed"
        fi
    else
        if [ -f "$SYSTEMD_UNIT" ]; then
            if command -v systemctl >/dev/null 2>&1; then
                systemctl --user disable --now "$SERVICE_NAME.service" >/dev/null 2>&1 || true
                systemctl --user stop "$SERVICE_NAME.service" >/dev/null 2>&1 || true
            fi
            rm -f "$SYSTEMD_UNIT"
            if command -v systemctl >/dev/null 2>&1; then
                systemctl --user daemon-reload >/dev/null 2>&1 || true
            fi
            say "uninstall: removed the login service"
        else
            if command -v systemctl >/dev/null 2>&1; then
                systemctl --user disable --now "$SERVICE_NAME.service" >/dev/null 2>&1 || true
            fi
            say "uninstall: service already removed"
        fi
    fi
    case "$AGENT" in
        claude) remove_mcp_claude ;;
        codex) remove_mcp_codex ;;
        muse) remove_mcp_muse ;;
    esac
    # Skills this installer copied in, proven by the receipt, never by
    # a same-named directory in the current plugin tree.
    remove_recorded_skills
    # Uninstall also finds optional plugins from an earlier install without
    # requiring --with-cloudcli again.
    for _plugin in office gateway cloudcli; do
        _added=$(plugin_marker "$_plugin" added)
        _owned=0
        if state_has "$_added" || state_has "$(plugin_marker "$_plugin" enabled)"; then
            _owned=1
        elif [ "$_plugin" = office ] && plugin_is_listed office; then
            _owned=1
        fi
        if [ "$_owned" -eq 1 ]; then
            command -v orbit >/dev/null 2>&1 || die "uninstall: Orbit is required to remove $_plugin"
            if plugin_is_listed "$_plugin"; then
                orbit plugin remove "$_plugin" --yes >/dev/null 2>&1 \
                    || die "uninstall: could not remove $_plugin; retry when Orbit is available"
                say "uninstall: removed the $_plugin plugin"
            else
                _list_status=$?
                [ "$_list_status" -eq 1 ] \
                    || die "uninstall: could not list Orbit plugins; retry when Orbit is available"
                say "uninstall: $_plugin plugin already removed"
            fi
        fi
        state_clear "$_added"
        state_clear "$(plugin_marker "$_plugin" enabled)"
        if state_has "plugin.$_plugin.exported"; then
            rm -rf "${DATA_DIR:?}/plugins/$_plugin"
            state_clear "plugin.$_plugin.exported"
        fi
    done
    remove_recorded_launcher "$HOME/.local/bin/orbit-office" uninstall
    remove_recorded_launcher "$LEGACY_LAUNCHER" uninstall
    # tools/ is not the data folder. On Linux the data folder is its
    # parent; on macOS they are different trees. Remove only tools/.
    case "$TOOLS_DIR" in
        "$HOME/.local/share/orbit-office/tools")
            if [ -e "$TOOLS_DIR" ]; then
                rm -rf "$TOOLS_DIR"
                say "uninstall: removed tools ($TOOLS_DIR)"
            else
                say "uninstall: tools already removed"
            fi
            ;;
        *) die "refusing to remove unexpected tools path $TOOLS_DIR" ;;
    esac
    state_clear "plugin_added"
    state_clear "plugin_enabled"
    state_clear "service_installed"
    state_clear "app_built"
    state_clear "agent_registered"
    if [ "$_purge" -eq 1 ]; then
        # The check above already ran. Repeat the protected-path guard
        # immediately before the deletes so HOME, / and an empty path
        # cannot be removed even if that earlier check is bypassed or
        # teardown created a path that now resolves to one of them.
        refuse_protected_purge
        if [ -e "$WORKSPACE" ]; then
            rm -rf "$WORKSPACE"
            say "purge: removed the workspace $WORKSPACE"
        else
            say "purge: workspace already removed"
        fi
        rm -rf "$DATA_DIR"
        say "purge: removed $DATA_DIR; the workspace and Library are gone"
    else
        say "uninstall: kept the workspace and Library at $WORKSPACE"
        say "uninstall: run install.sh --purge to remove them too"
    fi
}

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

if [ "$MODE" = "uninstall" ]; then
    do_uninstall 0
    exit 0
fi
if [ "$MODE" = "purge" ]; then
    do_uninstall 1
    exit 0
fi

INSTALL_PLUGINS="office gateway"
[ "$WITH_CLOUDCLI" -eq 0 ] || INSTALL_PLUGINS="$INSTALL_PLUGINS cloudcli"

check_workspace_adoptable
step_preflight
step_python
step_orbit
step_launcher
step_plugin
step_workspace_init
step_secrets
step_agent
step_app

step "summary"
say "summary: launcher   $LAUNCHER (add $HOME/.local/bin to PATH if needed)"
say "summary: workspace  $WORKSPACE"
say "summary: library    $WORKSPACE/library"
say "summary: data       $DATA_DIR"
say "summary: tools      $TOOLS_DIR"
say "summary: plugins    $INSTALL_PLUGINS"
if [ "$AGENT" = "claude" ]; then
    say "summary: deny rules Bash(curl *), Bash(wget *), Bash(http *), Bash(aws *), Bash(gcloud *)"
elif [ "$AGENT" = "muse" ]; then
    say "summary: deny rules unavailable for Muse Code shell calls; it has no per-command deny settings to write"
else
    say "summary: deny rules unavailable for sandboxed Codex shell calls; its execpolicy rules apply outside the sandbox only"
fi
say "summary: app        http://localhost:$PORT/"
say "summary: uninstall  install.sh --uninstall (keeps $WORKSPACE)"
say "summary: purge      install.sh --purge (also removes $WORKSPACE)"
