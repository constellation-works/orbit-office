/**
 * The agent CLI connection (planned).
 *
 * The orchestrator itself is the user's agent CLI (Muse Code by default)
 * running the `office-orchestrator` skill over Orbit MCP — it is not code
 * in this repo. This module is the app's typed view of that connection, for
 * the settings feature. Behaviour lives in
 * `plugin/.orbit-plugin/skills/orchestrator/SKILL.md`.
 */

export interface AgentCliStatus {
  /** Display name of the connected agent CLI, e.g. "Muse Code". */
  cli: string;
  /** Whether the install script has registered Orbit MCP with it. */
  connected: boolean;
}

/** Placeholder until connection detection lands. */
export function agentCliStatus(): AgentCliStatus {
  return { cli: "Muse Code", connected: false };
}
