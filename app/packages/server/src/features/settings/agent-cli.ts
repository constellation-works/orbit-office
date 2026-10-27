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

/** True when any string inside `value` mentions Orbit (case-insensitive). */
function mentionsOrbit(value: unknown): boolean {
  if (typeof value === "string") return value.toLowerCase().includes("orbit");
  if (Array.isArray(value)) return value.some(mentionsOrbit);
  if (value !== null && typeof value === "object") {
    return Object.entries(value).some(
      ([key, entry]) => key.toLowerCase().includes("orbit") || mentionsOrbit(entry),
    );
  }
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
  if (servers === null || typeof servers !== "object") return false;
  return mentionsOrbit(servers);
}

/**
 * True when a Codex `config.toml` registers an Orbit MCP server: a
 * `[mcp_servers.<name>]` section whose name mentions Orbit, or whose body
 * does. Dotted sub-tables (`[mcp_servers.<name>.env]`) belong to their
 * server's section. Exported for tests.
 */
export function tomlHasOrbitMcp(text: string): boolean {
  interface Section {
    name: string;
    orbit: boolean;
  }
  let current: Section | null = null;
  let found = false;
  const close = (): void => {
    if (current !== null && (current.name.toLowerCase().includes("orbit") || current.orbit)) {
      found = true;
    }
    current = null;
  };
  for (const rawLine of text.split("\n")) {
    const line = rawLine.trim();
    if (line.startsWith("[") && line.endsWith("]")) {
      const inner = line.slice(1, -1).trim();
      const server = /^mcp_servers\s*\.(.+)$/i.exec(inner);
      if (server?.[1] !== undefined) {
        // A dotted sub-table stays inside the current server's section.
        if (server[1].includes(".") && current !== null) continue;
        if (current !== null) close();
        current = { name: server[1].trim(), orbit: false };
        continue;
      }
      if (/^mcp_servers\s*$/i.test(inner)) continue;
      if (current !== null) close();
      continue;
    }
    if (current !== null && !line.startsWith("#") && line.toLowerCase().includes("orbit")) {
      current.orbit = true;
    }
  }
  if (current !== null) close();
  return found;
}

/** Home directory the lookup runs against. Empty when unknown. */
function homeOf(env: NodeJS.ProcessEnv): string {
  return env["HOME"] ?? "";
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
    const museConfig = env["XDG_CONFIG_HOME"] ?? join(home, ".config");
    const museSettings = readText(join(museConfig, "muse", "settings.json"));
    if (museSettings !== undefined && mcpServersHasOrbit(museSettings)) {
      return { cli: "Muse Code", connected: true };
    }
    const claudeConfig = env["CLAUDE_CONFIG_DIR"] ?? home;
    const claudeSettings = readText(join(claudeConfig, ".claude.json"));
    if (claudeSettings !== undefined && mcpServersHasOrbit(claudeSettings)) {
      return { cli: "Claude Code", connected: true };
    }
    const codexHome = env["CODEX_HOME"] ?? join(home, ".codex");
    const codexConfig = readText(join(codexHome, "config.toml"));
    if (codexConfig !== undefined && tomlHasOrbitMcp(codexConfig)) {
      return { cli: "Codex", connected: true };
    }
  }
  return { cli: DEFAULT_CLI, connected: false };
}
