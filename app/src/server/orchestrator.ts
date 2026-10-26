/**
 * Orchestrator boundary (scaffold).
 *
 * The orchestrator itself is the user's agent CLI (Muse Code by
 * default) running the `office-orchestrator` skill over Orbit MCP — it is
 * not code in this repo. This module is only the app's typed view of that
 * boundary for screens that surface what the agent is doing. Behaviour
 * lives in `plugin/.orbit-plugin/skills/orchestrator/SKILL.md`.
 */

export interface OrchestratorStatus {
  /** Display name of the connected agent CLI, e.g. "Muse Code". */
  cli: string;
  /** Whether setup has registered Orbit MCP with it. */
  connected: boolean;
}

/** Placeholder until setup detection lands. */
export function orchestratorStatus(): OrchestratorStatus {
  return { cli: "Muse Code", connected: false };
}
