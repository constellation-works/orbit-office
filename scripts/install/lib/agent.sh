# agent.sh — agent CLI registration (step_agent).
# Fragment of the generated install.sh: assembled by scripts/install/build-install.sh,
# never sourced at runtime. Edit this file, then regenerate install.sh.

# ---------------------------------------------------------------------------
# Agent CLI registration
# ---------------------------------------------------------------------------

edit_claude_settings() {
    SETTINGS_FILE="$AGENT_SETTINGS" SETTINGS_RECEIPT="$DATA_DIR/claude-settings.json" \
    MCP_FILE="$CLAUDE_CONFIG_FILE" MCP_RECEIPT="$DATA_DIR/claude-mcp.json" \
    LEGACY_FILE="$CLAUDE_LEGACY_SETTINGS" \
    SETTINGS_MODE="$1" ORBIT_OFFICE_AGENT_PATH="$AGENT_PATH" node <<'NODE'
const fs = require("fs");
const path = require("path");
const crypto = require("crypto");
const settingsFile = process.env.SETTINGS_FILE;
const settingsReceiptFile = process.env.SETTINGS_RECEIPT;
const mcpFile = process.env.MCP_FILE;
const mcpReceiptFile = process.env.MCP_RECEIPT;
const legacyFile = process.env.LEGACY_FILE;
const installing = process.env.SETTINGS_MODE === "install";
const rules = ["curl", "wget", "http", "aws", "gcloud"].map((p) => `Bash(${p} *)`);
const object = (v) => v !== null && typeof v === "object" && !Array.isArray(v);
function regular(p) {
  try {
    const stat = fs.statSync(p);
    if (!stat.isFile() || stat.uid !== process.getuid() || (stat.mode & 0o002) !== 0) {
      throw new Error("expected an owned regular file without world write access");
    }
    return true;
  } catch (e) {
    if (e.code === "ENOENT") return false;
    throw e;
  }
}
function write(p, text) {
  regular(p);
  let dest = p;
  try {
    if (fs.lstatSync(p).isSymbolicLink()) dest = fs.realpathSync(p);
  } catch (e) {
    if (e.code !== "ENOENT") throw e;
  }
  fs.mkdirSync(path.dirname(dest), { recursive: true, mode: 0o700 });
  const tmp = dest + "." + crypto.randomBytes(12).toString("hex");
  try {
    const fd = fs.openSync(tmp, "wx", 0o600);
    try { fs.writeFileSync(fd, text); fs.fsyncSync(fd); }
    finally { fs.closeSync(fd); }
    fs.renameSync(tmp, dest);
  } finally {
    if (fs.existsSync(tmp)) fs.unlinkSync(tmp);
  }
}
// A validated JSON tree with source ranges. Duplicate keys are refused.
function makeDoc(original) {
  const doc = { text: original };
  doc.tree = function () {
    if (!object(JSON.parse(doc.text))) throw new Error("settings must be an object");
    let pos = 0;
    const text = doc.text;
    const ws = () => { while (/\s/.test(text[pos] || "x")) pos++; };
    function string() {
      const start = pos++;
      while (text[pos] !== '"') { if (text[pos] === "\\") pos++; pos++; }
      pos++;
      return JSON.parse(text.slice(start, pos));
    }
    function value() {
      ws();
      const start = pos;
      const kind = text[pos];
      const entries = [];
      if (kind === "{" || kind === "[") {
        const close = kind === "{" ? "}" : "]";
        pos++; ws();
        const keys = new Set();
        while (text[pos] !== close) {
          const entryStart = pos;
          let key;
          if (kind === "{") {
            key = string(); ws(); pos++;
            if (keys.has(key)) throw new Error("duplicate key");
            keys.add(key);
          }
          const node = value();
          entries.push({ key, start: entryStart, end: node.end, node });
          ws();
          if (text[pos] === close) break;
          pos++; ws();
        }
        pos++;
      } else if (kind === '"') {
        string();
      } else {
        while (pos < text.length && !/[\s,}\]]/.test(text[pos])) pos++;
      }
      return { start, end: pos, kind, entries };
    }
    return value();
  };
  doc.locate = function (keys) {
    let node = doc.tree();
    for (const key of keys) {
      if (node.kind !== "{") throw new Error("expected object");
      const entry = node.entries.find((e) => e.key === key);
      if (!entry) return undefined;
      node = entry.node;
    }
    return node;
  };
  doc.splice = function (start, end, added) { doc.text = doc.text.slice(0, start) + added + doc.text.slice(end); };
  doc.append = function (node, encoded) {
    const last = node.entries.at(-1);
    const offset = last ? last.end : node.start + 1;
    doc.splice(offset, offset, (last ? "," : "") + encoded);
  };
  doc.set = function (keys, value) {
    const parent = doc.locate(keys.slice(0, -1));
    const key = keys.at(-1);
    const entry = parent.entries.find((e) => e.key === key);
    const encoded = JSON.stringify(value, null, 2);
    if (entry) doc.splice(entry.node.start, entry.node.end, encoded);
    else doc.append(parent, JSON.stringify(key) + ": " + encoded);
  };
  doc.removeEntry = function (parent, index) {
    const entry = parent.entries[index];
    if (index > 0) doc.splice(parent.entries[index - 1].end, entry.end, "");
    else if (parent.entries.length > 1) doc.splice(entry.start, parent.entries[1].start, "");
    else doc.splice(entry.start, entry.end, "");
  };
  doc.remove = function (keys) {
    const parent = doc.locate(keys.slice(0, -1));
    if (!parent) return;
    const index = parent.entries.findIndex((e) => e.key === keys.at(-1));
    if (index >= 0) doc.removeEntry(parent, index);
  };
  doc.ensureObject = function (keys, containers) {
    const node = doc.locate(keys);
    if (node) {
      if (node.kind !== "{") throw new Error("expected object");
      return;
    }
    doc.set(keys, {});
    containers.push(keys.join("."));
  };
  // Remove a misplaced mcpServers.orbit from a settings document; drop
  // the container only when it becomes empty.
  doc.removeMisplacedOrbit = function () {
    if (!doc.locate(["mcpServers", "orbit"])) return;
    doc.remove(["mcpServers", "orbit"]);
    const servers = doc.locate(["mcpServers"]);
    if (servers && servers.entries.length === 0) doc.remove(["mcpServers"]);
  };
  return doc;
}
try {
  let settingsReceipt = { added: [], containers: [] };
  const settingsReceiptExisted = regular(settingsReceiptFile);
  if (settingsReceiptExisted) settingsReceipt = JSON.parse(fs.readFileSync(settingsReceiptFile, "utf8"));
  if (!object(settingsReceipt) || !Array.isArray(settingsReceipt.added) || !Array.isArray(settingsReceipt.containers) ||
      settingsReceipt.added.some((r) => !rules.includes(r)) ||
      settingsReceipt.containers.some((p) => !["permissions", "permissions.deny", "mcpServers"].includes(p))) {
    throw new Error("invalid receipt");
  }
  let mcpReceipt = { owned: false, containers: [] };
  const mcpReceiptExisted = regular(mcpReceiptFile);
  if (mcpReceiptExisted) mcpReceipt = JSON.parse(fs.readFileSync(mcpReceiptFile, "utf8"));
  if (!object(mcpReceipt) || typeof mcpReceipt.owned !== "boolean" || !Array.isArray(mcpReceipt.containers) ||
      mcpReceipt.containers.some((p) => p !== "mcpServers")) {
    throw new Error("invalid mcp receipt");
  }
  const load = (file) => {
    const existed = regular(file);
    const raw = existed ? fs.readFileSync(file, "utf8") : "{}\n";
    const original = raw.trim() === "" ? "{}\n" : raw;
    return { file, existed, original, doc: makeDoc(original) };
  };
  const mcp = load(mcpFile);
  const settings = load(settingsFile);
  const legacyDistinct = legacyFile && legacyFile !== settingsFile && legacyFile !== mcpFile;
  const legacy = legacyDistinct ? load(legacyFile) : null;
  if (!installing && !mcp.existed && !settings.existed && !legacy?.existed) {
    if (mcpReceiptExisted) fs.unlinkSync(mcpReceiptFile);
    if (settingsReceiptExisted) fs.unlinkSync(settingsReceiptFile);
    process.exit(0);
  }
  // Validate every document before any edit.
  mcp.doc.tree(); settings.doc.tree(); if (legacy) legacy.doc.tree();
  if (installing) {
    // MCP registration in the file the reader checks.
    const desired = { command: "orbit", args: ["mcp", "serve"], env: { PATH: process.env.ORBIT_OFFICE_AGENT_PATH } };
    const priorOrbit = mcp.doc.locate(["mcpServers", "orbit"]);
    const priorMatches = priorOrbit &&
      JSON.stringify(JSON.parse(mcp.doc.text.slice(priorOrbit.start, priorOrbit.end))) === JSON.stringify(desired);
    if (!priorMatches) mcpReceipt.owned = true;
    mcp.doc.ensureObject(["mcpServers"], mcpReceipt.containers);
    const orbit = mcp.doc.locate(["mcpServers", "orbit"]);
    if (!orbit || JSON.stringify(JSON.parse(mcp.doc.text.slice(orbit.start, orbit.end))) !== JSON.stringify(desired)) {
      mcp.doc.set(["mcpServers", "orbit"], desired);
    }
    // Deny rules stay in settings.json.
    settings.doc.ensureObject(["permissions"], settingsReceipt.containers);
    if (!settings.doc.locate(["permissions", "deny"])) {
      settings.doc.set(["permissions", "deny"], []);
      settingsReceipt.containers.push("permissions.deny");
    }
    let deny = settings.doc.locate(["permissions", "deny"]);
    if (deny.kind !== "[") throw new Error("deny must be an array");
    const existing = JSON.parse(settings.doc.text.slice(deny.start, deny.end));
    if (existing.some((r) => typeof r !== "string")) throw new Error("deny rules must be strings");
    for (const rule of rules) {
      if (existing.includes(rule)) continue;
      deny = settings.doc.locate(["permissions", "deny"]);
      settings.doc.append(deny, JSON.stringify(rule));
      if (!settingsReceipt.added.includes(rule)) settingsReceipt.added.push(rule);
    }
    settingsReceipt.containers = settingsReceipt.containers.filter((c) => c !== "mcpServers");
    // Migrate the earlier misplaced registration out of settings files.
    settings.doc.removeMisplacedOrbit();
    if (legacy && legacy.existed) legacy.doc.removeMisplacedOrbit();
  } else {
    if (mcpReceipt.owned) {
      mcp.doc.remove(["mcpServers", "orbit"]);
      for (const key of [...mcpReceipt.containers].reverse()) {
        const keys = key.split(".");
        const node = mcp.doc.locate(keys);
        if (node && node.entries.length === 0) mcp.doc.remove(keys);
      }
    }
    for (const rule of settingsReceipt.added) {
      const deny = settings.doc.locate(["permissions", "deny"]);
      if (!deny) continue;
      if (deny.kind !== "[") throw new Error("deny must be an array");
      const index = deny.entries.findIndex((e) => JSON.parse(settings.doc.text.slice(e.start, e.end)) === rule);
      if (index >= 0) settings.doc.removeEntry(deny, index);
    }
    // Legacy misplaced entry: owned when an installer receipt proves a
    // prior installer edited that settings family.
    if (settingsReceiptExisted || mcpReceiptExisted) settings.doc.removeMisplacedOrbit();
    for (const key of [...settingsReceipt.containers].reverse()) {
      if (key === "mcpServers") continue;
      const keys = key.split(".");
      const node = settings.doc.locate(keys);
      if (node && node.entries.length === 0) settings.doc.remove(keys);
    }
    if (legacy && legacy.existed && (settingsReceiptExisted || mcpReceiptExisted)) {
      legacy.doc.removeMisplacedOrbit();
    }
  }
  mcp.doc.tree(); settings.doc.tree(); if (legacy) legacy.doc.tree();
  const persistReceipt = (file, receipt, existed) => {
    const serialized = JSON.stringify(receipt) + "\n";
    if (!existed || fs.readFileSync(file, "utf8") !== serialized) write(file, serialized);
  };
  if (installing) {
    persistReceipt(mcpReceiptFile, mcpReceipt, mcpReceiptExisted);
    persistReceipt(settingsReceiptFile, settingsReceipt, settingsReceiptExisted);
  }
  const persistDoc = (entry, createWhenMissing) => {
    if (entry.doc.text !== entry.original) {
      if (entry.existed && !regular(entry.file + ".bak")) {
        fs.writeFileSync(entry.file + ".bak", entry.original, { flag: "wx", mode: 0o600 });
      }
      write(entry.file, entry.doc.text);
    } else if (!entry.existed && installing && createWhenMissing) {
      write(entry.file, entry.doc.text);
    }
  };
  persistDoc(mcp, true); persistDoc(settings, true); if (legacy) persistDoc(legacy, false);
  if (!installing) {
    if (regular(mcpReceiptFile)) fs.unlinkSync(mcpReceiptFile);
    if (regular(settingsReceiptFile)) fs.unlinkSync(settingsReceiptFile);
  }
} catch (_) {
  process.stderr.write("agent settings update refused; existing JSON and ownership receipt must be valid regular files\n");
  process.exit(1);
}
NODE
}

register_mcp_claude() {
    edit_claude_settings install || die "agent: could not update $CLAUDE_CONFIG_FILE and $AGENT_SETTINGS (check JSON, types and file permissions)"
    state_mark "agent_registered"
    say "agent: already configured (Orbit MCP entry in $CLAUDE_CONFIG_FILE and gateway deny rules in $AGENT_SETTINGS)"
}

# True when the Codex orbit table already carries exactly the current
# generated PATH. The env map is inline on that table so uninstall,
# which stops at the next [header], removes it with the section.
codex_orbit_path_current() {
    [ -f "$AGENT_SETTINGS" ] || return 1
    awk -v want="env = { PATH = \"$AGENT_PATH\" }" '
        /^\[mcp_servers\.orbit\]$/ { in_s = 1; next }
        /^\[/ { in_s = 0 }
        in_s && $0 == want { found = 1 }
        END { exit !found }
    ' "$AGENT_SETTINGS"
}

strip_codex_orbit_section() {
    _file="$1"
    _tmp="$_file.tmp.$$"
    awk '/^\[mcp_servers\.orbit\]$/{skip=1; next} /^\[/{skip=0} !skip' "$_file" > "$_tmp"
    mv "$_tmp" "$_file"
}

register_mcp_codex() {
    _file="$AGENT_SETTINGS"
    mkdir -p "$(dirname -- "$_file")"
    if codex_orbit_path_current; then
        state_mark "agent_registered"
        say "agent: already done (Orbit MCP entry in $_file)"
        return 0
    fi
    if [ -f "$_file" ] && [ ! -f "$_file.bak" ]; then
        cp "$_file" "$_file.bak"
        say "agent: backed up $_file to $_file.bak"
    fi
    if [ -f "$_file" ] && grep -q '^\[mcp_servers\.orbit\]$' "$_file" 2>/dev/null; then
        strip_codex_orbit_section "$_file"
    fi
    if [ -s "$_file" ]; then
        printf '\n' >> "$_file"
    fi
    {
        printf '[mcp_servers.orbit]\n'
        printf 'command = "orbit"\n'
        printf 'args = ["mcp", "serve"]\n'
        printf 'env = { PATH = "%s" }\n' "$AGENT_PATH"
    } >> "$_file"
    state_mark "agent_registered"
    say "agent: added the Orbit MCP entry to $_file"
}

remove_mcp_claude() {
    if edit_claude_settings uninstall; then
        say "agent: removed the owned Orbit MCP entry from $CLAUDE_CONFIG_FILE and recorded deny rules from $AGENT_SETTINGS"
    else
        warn "agent: could not remove owned entries from $CLAUDE_CONFIG_FILE and $AGENT_SETTINGS; leaving them in place and continuing uninstall"
    fi
}

remove_mcp_codex() {
    _file="$AGENT_SETTINGS"
    [ -f "$_file" ] || { say "agent: already removed (no $_file)"; return 0; }
    if ! grep -q 'mcp_servers.orbit' "$_file" 2>/dev/null; then
        say "agent: already removed (no Orbit MCP entry)"
        return 0
    fi
    strip_codex_orbit_section "$_file"
    say "agent: removed the Orbit MCP entry from $_file"
}

# Muse Code settings: one JSON file holding mcpServers beside the user's
# other settings. Same splice discipline as the Claude editor: validate
# the whole document, edit only the mcpServers.orbit bytes, keep a .bak,
# and record ownership in muse-mcp.json so uninstall removes only an
# entry this installer wrote (a pre-existing identical entry is kept).
edit_muse_settings() {
    SETTINGS_FILE="$AGENT_SETTINGS" SETTINGS_RECEIPT="$DATA_DIR/muse-mcp.json" \
    SETTINGS_MODE="$1" ORBIT_OFFICE_AGENT_PATH="$AGENT_PATH" node <<'NODE'
const fs = require("fs");
const path = require("path");
const crypto = require("crypto");
const settingsFile = process.env.SETTINGS_FILE;
const receiptFile = process.env.SETTINGS_RECEIPT;
const installing = process.env.SETTINGS_MODE === "install";
const object = (v) => v !== null && typeof v === "object" && !Array.isArray(v);
function regular(p) {
  try {
    const stat = fs.statSync(p);
    if (!stat.isFile() || stat.uid !== process.getuid() || (stat.mode & 0o002) !== 0) {
      throw new Error("expected an owned regular file without world write access");
    }
    return true;
  } catch (e) {
    if (e.code === "ENOENT") return false;
    throw e;
  }
}
function write(p, text) {
  regular(p);
  let dest = p;
  try {
    if (fs.lstatSync(p).isSymbolicLink()) dest = fs.realpathSync(p);
  } catch (e) {
    if (e.code !== "ENOENT") throw e;
  }
  fs.mkdirSync(path.dirname(dest), { recursive: true, mode: 0o700 });
  const tmp = dest + "." + crypto.randomBytes(12).toString("hex");
  try {
    const fd = fs.openSync(tmp, "wx", 0o600);
    try { fs.writeFileSync(fd, text); fs.fsyncSync(fd); }
    finally { fs.closeSync(fd); }
    fs.renameSync(tmp, dest);
  } finally {
    if (fs.existsSync(tmp)) fs.unlinkSync(tmp);
  }
}
function makeDoc(original) {
  const doc = { text: original };
  doc.tree = function () {
    if (!object(JSON.parse(doc.text))) throw new Error("settings must be an object");
    let pos = 0;
    const text = doc.text;
    const ws = () => { while (/\s/.test(text[pos] || "x")) pos++; };
    function string() {
      const start = pos++;
      while (text[pos] !== '"') { if (text[pos] === "\\") pos++; pos++; }
      pos++;
      return JSON.parse(text.slice(start, pos));
    }
    function value() {
      ws();
      const start = pos;
      const kind = text[pos];
      const entries = [];
      if (kind === "{" || kind === "[") {
        const close = kind === "{" ? "}" : "]";
        pos++; ws();
        const keys = new Set();
        while (text[pos] !== close) {
          const entryStart = pos;
          let key;
          if (kind === "{") {
            key = string(); ws(); pos++;
            if (keys.has(key)) throw new Error("duplicate key");
            keys.add(key);
          }
          const node = value();
          entries.push({ key, start: entryStart, end: node.end, node });
          ws();
          if (text[pos] === close) break;
          pos++; ws();
        }
        pos++;
      } else if (kind === '"') {
        string();
      } else {
        while (pos < text.length && !/[\s,}\]]/.test(text[pos])) pos++;
      }
      return { start, end: pos, kind, entries };
    }
    return value();
  };
  doc.locate = function (keys) {
    let node = doc.tree();
    for (const key of keys) {
      if (node.kind !== "{") throw new Error("expected object");
      const entry = node.entries.find((e) => e.key === key);
      if (!entry) return undefined;
      node = entry.node;
    }
    return node;
  };
  doc.splice = function (start, end, added) { doc.text = doc.text.slice(0, start) + added + doc.text.slice(end); };
  doc.append = function (node, encoded) {
    const last = node.entries.at(-1);
    const offset = last ? last.end : node.start + 1;
    doc.splice(offset, offset, (last ? "," : "") + encoded);
  };
  doc.set = function (keys, value) {
    const parent = doc.locate(keys.slice(0, -1));
    const key = keys.at(-1);
    const entry = parent.entries.find((e) => e.key === key);
    const encoded = JSON.stringify(value, null, 2);
    if (entry) doc.splice(entry.node.start, entry.node.end, encoded);
    else doc.append(parent, JSON.stringify(key) + ": " + encoded);
  };
  doc.removeEntry = function (parent, index) {
    const entry = parent.entries[index];
    if (index > 0) doc.splice(parent.entries[index - 1].end, entry.end, "");
    else if (parent.entries.length > 1) doc.splice(entry.start, parent.entries[1].start, "");
    else doc.splice(entry.start, entry.end, "");
  };
  doc.remove = function (keys) {
    const parent = doc.locate(keys.slice(0, -1));
    if (!parent) return;
    const index = parent.entries.findIndex((e) => e.key === keys.at(-1));
    if (index >= 0) doc.removeEntry(parent, index);
  };
  doc.ensureObject = function (keys, containers) {
    const node = doc.locate(keys);
    if (node) {
      if (node.kind !== "{") throw new Error("expected object");
      return;
    }
    doc.set(keys, {});
    containers.push(keys.join("."));
  };
  return doc;
}
try {
  let receipt = { owned: false, containers: [] };
  const receiptExisted = regular(receiptFile);
  if (receiptExisted) receipt = JSON.parse(fs.readFileSync(receiptFile, "utf8"));
  if (!object(receipt) || typeof receipt.owned !== "boolean" || !Array.isArray(receipt.containers) ||
      receipt.containers.some((p) => p !== "mcpServers")) {
    throw new Error("invalid muse receipt");
  }
  const existed = regular(settingsFile);
  const raw = existed ? fs.readFileSync(settingsFile, "utf8") : "{}\n";
  const original = raw.trim() === "" ? "{}\n" : raw;
  const doc = makeDoc(original);
  if (!installing && !existed) {
    if (receiptExisted) fs.unlinkSync(receiptFile);
    process.exit(0);
  }
  doc.tree();
  if (installing) {
    const desired = { command: "orbit", args: ["mcp", "serve"], env: { PATH: process.env.ORBIT_OFFICE_AGENT_PATH } };
    const prior = doc.locate(["mcpServers", "orbit"]);
    const priorMatches = prior &&
      JSON.stringify(JSON.parse(doc.text.slice(prior.start, prior.end))) === JSON.stringify(desired);
    if (!priorMatches) receipt.owned = true;
    doc.ensureObject(["mcpServers"], receipt.containers);
    const orbit = doc.locate(["mcpServers", "orbit"]);
    if (!orbit || JSON.stringify(JSON.parse(doc.text.slice(orbit.start, orbit.end))) !== JSON.stringify(desired)) {
      doc.set(["mcpServers", "orbit"], desired);
    }
  } else if (receipt.owned) {
    doc.remove(["mcpServers", "orbit"]);
    for (const key of [...receipt.containers].reverse()) {
      const keys = key.split(".");
      const node = doc.locate(keys);
      if (node && node.entries.length === 0) doc.remove(keys);
    }
  }
  doc.tree();
  if (installing) {
    const serialized = JSON.stringify(receipt) + "\n";
    if (!receiptExisted || fs.readFileSync(receiptFile, "utf8") !== serialized) write(receiptFile, serialized);
  }
  if (doc.text !== original) {
    if (existed && !regular(settingsFile + ".bak")) {
      fs.writeFileSync(settingsFile + ".bak", original, { flag: "wx", mode: 0o600 });
    }
    write(settingsFile, doc.text);
  } else if (!existed && installing) {
    write(settingsFile, doc.text);
  }
  if (!installing && regular(receiptFile)) fs.unlinkSync(receiptFile);
} catch (_) {
  process.stderr.write("agent settings update refused; existing JSON and ownership receipt must be valid regular files\n");
  process.exit(1);
}
NODE
}

register_mcp_muse() {
    edit_muse_settings install || die "agent: could not update $AGENT_SETTINGS (check JSON, types and file permissions)"
    state_mark "agent_registered"
    say "agent: already configured (Orbit MCP entry in $AGENT_SETTINGS)"
}

remove_mcp_muse() {
    if edit_muse_settings uninstall; then
        say "agent: removed the owned Orbit MCP entry from $AGENT_SETTINGS"
    else
        warn "agent: could not remove owned entries from $AGENT_SETTINGS; leaving them in place and continuing uninstall"
    fi
}

# Skill ownership (STD-03@2 §R29): a matching name is not ownership.
# The receipt at $SKILLS_RECEIPT holds one "<name> <sha256>" line per
# skill this installer itself copied, where the digest covers every
# file's relative path and bytes (and every symlink's target) in the
# copied tree. Install records a skill only when it copies it; a
# pre-existing same-named skill is left alone and never recorded.
# Uninstall removes only a recorded skill whose tree still matches its
# recorded digest; anything else is kept and named in the output.

skill_name_valid() {
    case "$1" in
        ''|.|..|*[!A-Za-z0-9._-]*) return 1 ;;
        *) return 0 ;;
    esac
}

skill_digest_valid() {
    [ "${#1}" -eq 64 ] || return 1
    case "$1" in
        *[!0-9a-f]*) return 1 ;;
        *) return 0 ;;
    esac
}

# Print the digest of the tree at $1, or fail.
skill_tree_digest() {
    _tree="$1"
    [ -d "$_tree" ] && [ ! -L "$_tree" ] || return 1
    _list=$(mktemp "${TMPDIR:-/tmp}/orbit-office-skills.XXXXXX") || return 1
    _hashes=$(mktemp "${TMPDIR:-/tmp}/orbit-office-skills.XXXXXX") || { rm -f "$_list"; return 1; }
    if ! (CDPATH='' cd -- "$_tree" && find . \( -type f -o -type l \) -print | LC_ALL=C sort) > "$_list"; then
        rm -f "$_list" "$_hashes"
        return 1
    fi
    _rc=0
    : > "$_hashes"
    while IFS= read -r _rel || [ -n "$_rel" ]; do
        _path="$_tree/$_rel"
        if [ -L "$_path" ]; then
            _target=$(readlink "$_path") || _rc=1
            printf 'link %s %s\n' "$_rel" "${_target:-}" >> "$_hashes"
        elif [ -f "$_path" ]; then
            _h=$(sha256_file "$_path") || _rc=1
            printf 'file %s %s\n' "${_h:-}" "$_rel" >> "$_hashes"
        else
            _rc=1
        fi
    done < "$_list"
    _digest=""
    if [ "$_rc" -eq 0 ]; then
        _digest=$(sha256_file "$_hashes") || _rc=1
    fi
    rm -f "$_list" "$_hashes"
    [ "$_rc" -eq 0 ] && [ -n "$_digest" ] || return 1
    printf '%s\n' "$_digest"
}

# Print the recorded digest for $1, or nothing when it is not recorded.
skill_receipt_digest() {
    [ -f "$SKILLS_RECEIPT" ] && [ ! -L "$SKILLS_RECEIPT" ] || return 0
    awk -v name="$1" '$1 == name { print $2; exit }' "$SKILLS_RECEIPT" 2>/dev/null || true
}

# Drop the ownership line for $1; remove the receipt when it becomes empty.
skill_receipt_forget() {
    [ -f "$SKILLS_RECEIPT" ] && [ ! -L "$SKILLS_RECEIPT" ] || return 0
    _tmp="$SKILLS_RECEIPT.tmp.$$"
    grep -v "^$1 " "$SKILLS_RECEIPT" > "$_tmp" || true
    if [ -s "$_tmp" ]; then
        mv "$_tmp" "$SKILLS_RECEIPT"
    else
        rm -f "$_tmp" "$SKILLS_RECEIPT"
    fi
}

# True when Orbit already provides (or is about to provide) the office
# skills for this machine: the plugin was added or enabled by an earlier
# step of this installer, a bundle/source is in use, or Orbit lists it.
plugin_provides_skills() {
    if state_has "plugin_added" || state_has "plugin_enabled"; then
        return 0
    fi
    if [ -n "${ORBIT_OFFICE_PLUGIN_BUNDLE:-}" ] \
        || [ -n "${ORBIT_OFFICE_PLUGIN_SOURCE:-}" ]; then
        return 0
    fi
    if command -v orbit >/dev/null 2>&1 && plugin_is_listed office 2>/dev/null; then
        return 0
    fi
    return 1
}

# Remove bare-named copies this installer recorded in an earlier release
# (for example $SKILLS_DIR/orchestrator). Remove only a copy whose tree
# still matches its recorded digest; keep and warn about a modified one,
# exactly as uninstall does.
migrate_bare_skill_copies() {
    [ -f "$SKILLS_RECEIPT" ] && [ ! -L "$SKILLS_RECEIPT" ] || return 0
    [ -d "$SKILLS_DIR" ] || return 0
    _snapshot=$(mktemp "${TMPDIR:-/tmp}/orbit-office-skills.XXXXXX") || return 0
    cp "$SKILLS_RECEIPT" "$_snapshot"
    while IFS=' ' read -r _name _digest _extra || [ -n "${_name:-}" ]; do
        case "${_name:-}" in
            ''|office-*) continue ;;
            *) ;;
        esac
        if ! skill_name_valid "${_name:-}" || ! skill_digest_valid "${_digest:-}" \
            || [ -n "${_extra:-}" ]; then
            continue
        fi
        _dest="$SKILLS_DIR/$_name"
        if [ ! -e "$_dest" ] && [ ! -L "$_dest" ]; then
            skill_receipt_forget "$_name"
            continue
        fi
        if [ -L "$_dest" ] || [ ! -d "$_dest" ]; then
            warn "agent: kept skill $_name at $_dest (not a directory this installer copied)"
            continue
        fi
        _current=$(skill_tree_digest "$_dest") || _current=""
        if [ -n "$_current" ] && [ "$_current" = "$_digest" ]; then
            rm -rf "${_dest:?}"
            skill_receipt_forget "$_name"
            say "agent: removed stale bare-named skill copy $_name (superseded by office-$_name from the Orbit plugin)"
        else
            warn "agent: kept modified skill $_name at $_dest (it differs from the copy this installer installed; remove it manually if you no longer want it)"
        fi
    done < "$_snapshot"
    rm -f "$_snapshot"
}

# Copy $1 to $SKILLS_DIR/office-$2, or refresh a stale recorded copy.
# A pre-existing unrecorded skill and a user-modified recorded copy are
# kept untouched; a plugin link (a symlink) never reaches this function.
install_skill_fallback() {
    _src="$1"; _prefixed="office-$2"; _dest="$SKILLS_DIR/$_prefixed"
    if [ -e "$_dest" ] || [ -L "$_dest" ]; then
        _recorded=$(skill_receipt_digest "$_prefixed")
        if [ -n "$_recorded" ] && [ -d "$_dest" ] && [ ! -L "$_dest" ]; then
            _current=$(skill_tree_digest "$_dest") || _current=""
            _wanted=$(skill_tree_digest "$_src") || _wanted=""
            if [ -n "$_current" ] && [ -n "$_wanted" ] && [ "$_current" = "$_wanted" ]; then
                say "agent: skill $_prefixed already done"
                return 0
            fi
            if [ -n "$_current" ] && [ "$_current" != "$_recorded" ]; then
                warn "agent: kept modified skill $_prefixed at $_dest (it differs from the copy this installer installed; remove it manually if you no longer want it)"
                return 0
            fi
            if [ -n "$_current" ] && [ -n "$_wanted" ]; then
                rm -rf "${_dest:?}"
                cp -R "$_src" "$_dest"
                if _digest=$(skill_tree_digest "$_dest") \
                    && skill_receipt_record "$_prefixed" "$_digest"; then
                    say "agent: refreshed skill $_prefixed"
                else
                    warn "agent: refreshed skill $_prefixed without an ownership record; uninstall will keep it"
                fi
                return 0
            fi
        fi
        say "agent: skill $_prefixed already done"
        return 0
    fi
    cp -R "$_src" "$_dest"
    if _digest=$(skill_tree_digest "$_dest") \
        && skill_receipt_record "$_prefixed" "$_digest"; then
        say "agent: installed skill $_prefixed"
    else
        warn "agent: installed skill $_prefixed without an ownership record; uninstall will keep it"
    fi
}

# Record (or replace) the ownership line for a skill this run copied.
skill_receipt_record() {
    _name="$1"; _digest="$2"
    mkdir -p "$DATA_DIR"
    if [ -L "$SKILLS_RECEIPT" ]; then
        warn "skills: ownership receipt $SKILLS_RECEIPT is a symlink; not recording $_name"
        return 1
    fi
    _tmp="$SKILLS_RECEIPT.tmp.$$"
    if [ -f "$SKILLS_RECEIPT" ]; then
        grep -v "^$_name " "$SKILLS_RECEIPT" > "$_tmp" || true
    else
        : > "$_tmp"
    fi
    printf '%s %s\n' "$_name" "$_digest" >> "$_tmp"
    mv "$_tmp" "$SKILLS_RECEIPT"
}

# Remove recorded skills that are still exactly as installed. Keep and
# name every other skill: unrecorded, malformed, missing-digest tooling,
# a symlink or non-directory, or content changed since the copy.
remove_recorded_skills() {
    if [ -L "$SKILLS_RECEIPT" ]; then
        warn "uninstall: skills ownership receipt $SKILLS_RECEIPT is a symlink; keeping all skills"
        return 0
    fi
    if [ ! -f "$SKILLS_RECEIPT" ]; then
        say "uninstall: no skills recorded as installed by this installer; keeping any skills present"
        return 0
    fi
    _snapshot=$(mktemp "${TMPDIR:-/tmp}/orbit-office-skills.XXXXXX") || {
        warn "uninstall: could not read the skills ownership receipt; keeping all skills"
        return 0
    }
    _kept=$(mktemp "${TMPDIR:-/tmp}/orbit-office-skills.XXXXXX") || {
        rm -f "$_snapshot"
        warn "uninstall: could not read the skills ownership receipt; keeping all skills"
        return 0
    }
    cp "$SKILLS_RECEIPT" "$_snapshot"
    : > "$_kept"
    while IFS=' ' read -r _name _digest _extra || [ -n "${_name:-}" ]; do
        if ! skill_name_valid "${_name:-}" || ! skill_digest_valid "${_digest:-}" \
            || [ -n "${_extra:-}" ]; then
            warn "uninstall: ignoring a malformed skills ownership entry; keeping its skill"
            printf '%s %s%s\n' "${_name:-}" "${_digest:-}" "${_extra:+ $_extra}" >> "$_kept"
            continue
        fi
        _dest="$SKILLS_DIR/$_name"
        if [ ! -e "$_dest" ] && [ ! -L "$_dest" ]; then
            say "uninstall: skill $_name already removed"
            continue
        fi
        if [ -L "$_dest" ] || [ ! -d "$_dest" ]; then
            say "uninstall: kept skill $_name at $_dest (not a directory this installer copied)"
            printf '%s %s\n' "$_name" "$_digest" >> "$_kept"
            continue
        fi
        _current=$(skill_tree_digest "$_dest") || _current=""
        if [ -z "$_current" ]; then
            warn "uninstall: could not verify skill $_name; keeping it at $_dest"
            printf '%s %s\n' "$_name" "$_digest" >> "$_kept"
            continue
        fi
        if [ "$_current" = "$_digest" ]; then
            rm -rf "${_dest:?}"
            say "uninstall: removed skill $_name"
        else
            say "uninstall: kept modified skill $_name at $_dest (it differs from the copy this installer installed; remove it manually if you no longer want it)"
            printf '%s %s\n' "$_name" "$_digest" >> "$_kept"
        fi
    done < "$_snapshot"
    # Name same-named skills that were never recorded as ours. Plugin
    # links (symlinks under the office- name) belong to Orbit, not to
    # this installer, so they are never named or removed here.
    if [ -n "$PLUGIN_SRC" ] && [ -d "$PLUGIN_SRC/.orbit-plugin/skills" ] \
        && [ -d "$SKILLS_DIR" ]; then
        for _skill in "$PLUGIN_SRC/.orbit-plugin/skills"/*; do
            [ -d "$_skill" ] || continue
            _name=$(basename -- "$_skill")
            skill_name_valid "$_name" || continue
            for _candidate in "$_name" "office-$_name"; do
                if [ -e "$SKILLS_DIR/$_candidate" ] && [ ! -L "$SKILLS_DIR/$_candidate" ] \
                    && ! grep -q "^$_candidate " "$_snapshot" 2>/dev/null; then
                    say "uninstall: kept skill $_candidate at $SKILLS_DIR/$_candidate (not recorded as installed by this installer)"
                fi
            done
        done
    fi
    rm -f "$_snapshot"
    if [ -s "$_kept" ]; then
        mv "$_kept" "$SKILLS_RECEIPT"
    else
        rm -f "$_kept" "$SKILLS_RECEIPT"
    fi
}

step_agent() {
    step "8 agent"
    compute_agent_path
    validate_agent_path
    case "$AGENT" in
        claude) register_mcp_claude ;;
        codex) register_mcp_codex ;;
        muse) register_mcp_muse ;;
    esac
    # Office skills have one source of truth: the Orbit plugin's links,
    # named office-<skill> in the agent's skills dir. When the plugin
    # provides them the installer copies nothing. Bare-named copies an
    # earlier installer recorded are migrated away first. A copy-based
    # fallback, used only when no Orbit plugin path provides the skills,
    # copies under the same office- names and refreshes stale copies.
    migrate_bare_skill_copies
    if [ -n "$PLUGIN_SRC" ] && [ -d "$PLUGIN_SRC/.orbit-plugin/skills" ]; then
        mkdir -p "$SKILLS_DIR"
        if plugin_provides_skills; then
            _plugin_provides=1
        else
            _plugin_provides=0
        fi
        for _skill in "$PLUGIN_SRC/.orbit-plugin/skills"/*; do
            [ -d "$_skill" ] || continue
            _name=$(basename -- "$_skill")
            if ! skill_name_valid "$_name"; then
                warn "agent: skipping skill with an unsafe name: $_name"
                continue
            fi
            _prefixed="office-$_name"
            if [ -L "$SKILLS_DIR/$_prefixed" ]; then
                say "agent: skill $_prefixed already done"
            elif [ -e "$SKILLS_DIR/$_prefixed" ]; then
                install_skill_fallback "$_skill" "$_name"
            elif [ "$_plugin_provides" -eq 1 ]; then
                say "agent: skill $_prefixed provided by the Orbit plugin; not copying"
            else
                install_skill_fallback "$_skill" "$_name"
            fi
        done
    else
        say "agent: skills already done (no local plugin tree; orbit plugin enable links them)"
    fi
    state_mark "agent_registered"
}

