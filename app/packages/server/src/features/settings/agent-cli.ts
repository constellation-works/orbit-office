/**
 * The agent CLI connection the settings feature reports.
 *
 * The orchestrator itself is the user's agent CLI (Muse Code by default)
 * running the `office-orchestrator` skill over Orbit MCP — it is not code
 * in this repo. This module is the app's typed view of that connection:
 * whether the install script's Orbit MCP entry is present in the CLI's
 * user settings (docs/design/office/specs/install.md#steps, step 7).
 *
 * Detection is a read-only presence check over three user-level files:
 * Muse Code's `mcpServers` in `${XDG_CONFIG_HOME:-$HOME/.config}/muse/settings.json`,
 * Claude Code's `mcpServers` in `${CLAUDE_CONFIG_DIR:-$HOME}/.claude.json`,
 * and Codex's `[mcp_servers.*]` tables in `${CODEX_HOME:-$HOME/.codex}/config.toml`.
 * A file that is missing, unreadable or unparsable counts as not
 * connected; file contents are never logged. An entry counts when its
 * name, command, arguments or URL mentions Orbit (case-insensitive).
 */

import { readFileSync } from "node:fs";
import { join } from "node:path";

import type { AgentCliStatus } from "@orbit-office/contracts";

export type { AgentCliStatus };

/** Display name reported when no CLI has Orbit MCP registered. */
const DEFAULT_CLI = "Muse Code";

/** Read a text file, or undefined when it is missing or unreadable. */
function readText(path: string): string | undefined {
  try {
    return readFileSync(path, "utf8");
  } catch {
    return undefined;
  }
}

/** True when a string or string array mentions Orbit (case-insensitive). */
function mentionsOrbit(value: unknown): boolean {
  if (typeof value === "string") return value.toLowerCase().includes("orbit");
  if (Array.isArray(value)) return value.some((entry) => typeof entry === "string" && mentionsOrbit(entry));
  return false;
}

/**
 * True when a parsed `mcpServers` object (Muse or Claude settings JSON)
 * holds an Orbit entry. Exported for tests.
 */
export function mcpServersHasOrbit(text: string): boolean {
  let parsed: unknown;
  try {
    parsed = JSON.parse(text) as unknown;
  } catch {
    return false;
  }
  if (parsed === null || typeof parsed !== "object") return false;
  const servers: unknown = (parsed as Record<string, unknown>)["mcpServers"];
  if (servers === null || typeof servers !== "object" || Array.isArray(servers)) return false;
  return Object.entries(servers).some(([name, entry]) => {
    if (name.toLowerCase().includes("orbit")) return true;
    if (entry === null || typeof entry !== "object" || Array.isArray(entry)) return false;
    const config = entry as Record<string, unknown>;
    return ["command", "args", "arguments", "url"].some((field) => mentionsOrbit(config[field]));
  });
}

/**
 * True when a Codex `config.toml` registers an Orbit MCP server: a
 * `[mcp_servers.<name>]` section whose name or direct command, args,
 * arguments, or URL field mentions Orbit. Other fields, dotted sub-tables
 * and comments do not count. Exported for tests.
 */
export function tomlHasOrbitMcp(text: string): boolean {
  const stripComment = (line: string): string => {
    let quote: "'" | '"' | null = null;
    let escaped = false;
    for (let index = 0; index < line.length; index += 1) {
      const character = line[index];
      if (quote === '"') {
        if (escaped) escaped = false;
        else if (character === "\\") escaped = true;
        else if (character === '"') quote = null;
      } else if (quote === "'") {
        if (character === "'") quote = null;
      } else if (character === '"' || character === "'") {
        quote = character;
      } else if (character === "#") {
        return line.slice(0, index);
      }
    }
    return line;
  };
  const containerDelta = (value: string): number => {
    let delta = 0;
    let quote: "'" | '"' | null = null;
    let escaped = false;
    for (let index = 0; index < value.length; index += 1) {
      const character = value[index];
      if (quote === '"') {
        if (escaped) escaped = false;
        else if (character === "\\") escaped = true;
        else if (character === '"') quote = null;
      } else if (quote === "'") {
        if (character === "'") quote = null;
      } else if (character === '"' || character === "'") {
        quote = character;
      } else if (character === "[" || character === "{") {
        delta += 1;
      } else if (character === "]" || character === "}") {
        delta -= 1;
      }
    }
    return delta;
  };

  let inServerTable = false;
  let openIdentityValue = 0;
  for (const rawLine of text.split("\n")) {
    const line = stripComment(rawLine).trim();
    if (line === "") continue;
    if (line.startsWith("[") && line.endsWith("]")) {
      const server = /^\[\s*mcp_servers\s*\.\s*(?:"((?:\\.|[^"])*)"|'([^']*)'|([A-Za-z0-9_-]+))\s*\]$/i.exec(line);
      inServerTable = server !== null;
      const name = server?.[1] ?? server?.[2] ?? server?.[3] ?? "";
      if (name.toLowerCase().includes("orbit")) return true;
      openIdentityValue = 0;
      continue;
    }
    if (!inServerTable) continue;
    if (openIdentityValue > 0) {
      if (line.toLowerCase().includes("orbit")) return true;
      openIdentityValue += containerDelta(line);
      continue;
    }
    const assignment = /^(?:"(command|args|arguments|url)"|'(command|args|arguments|url)'|(command|args|arguments|url))\s*=\s*(.*)$/i.exec(line);
    if (assignment === null) continue;
    const value = assignment[4] ?? "";
    if (value.toLowerCase().includes("orbit")) return true;
    openIdentityValue = containerDelta(value);
  }
  return false;
}

/** Home directory the lookup runs against. Empty when unknown. */
function homeOf(env: NodeJS.ProcessEnv): string {
  return env["HOME"] ?? "";
}

/**
 * `${NAME:-fallback}`: an unset or empty override uses `fallback`.
 * A non-empty value is the directory itself.
 */
function overrideOr(env: NodeJS.ProcessEnv, name: string, fallback: string): string {
  const value = env[name];
  if (value === undefined || value === "") return fallback;
  return value;
}

/**
 * Report the agent CLI connection: the first CLI (Muse Code, Claude Code,
 * Codex) whose user settings register Orbit MCP, or Muse Code disconnected
 * when none does. `env` defaults to the server's environment and is
 * injectable for tests, so fixtures never touch the real home directory.
 */
export function agentCliStatus(env: NodeJS.ProcessEnv = process.env): AgentCliStatus {
  const home = homeOf(env);
  if (home !== "") {
    const museConfig = overrideOr(env, "XDG_CONFIG_HOME", join(home, ".config"));
    const museSettings = readText(join(museConfig, "muse", "settings.json"));
    if (museSettings !== undefined && mcpServersHasOrbit(museSettings)) {
      return { cli: "Muse Code", connected: true };
    }
    const claudeConfig = overrideOr(env, "CLAUDE_CONFIG_DIR", home);
    const claudeSettings = readText(join(claudeConfig, ".claude.json"));
    if (claudeSettings !== undefined && mcpServersHasOrbit(claudeSettings)) {
      return { cli: "Claude Code", connected: true };
    }
    const codexHome = overrideOr(env, "CODEX_HOME", join(home, ".codex"));
    const codexConfig = readText(join(codexHome, "config.toml"));
    if (codexConfig !== undefined && tomlHasOrbitMcp(codexConfig)) {
      return { cli: "Codex", connected: true };
    }
  }
  return { cli: DEFAULT_CLI, connected: false };
}
