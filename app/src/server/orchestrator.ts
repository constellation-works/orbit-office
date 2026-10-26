/**
 * Orchestrator agent (scaffold).
 *
 * A Muse Spark tool-calling loop over the `office.*` tools plus a curated
 * subset of `orbit.*` tools (task add/update/list/show, task artifacts,
 * frictions, search, auto-task list, routine control, workflow ship).
 * Behaviour is versioned in `plugin/skills/orchestrator/SKILL.md`.
 *
 * Guardrails: capture pasted external content as a source first; never take
 * an outward-facing action, create standing automation, or write knowledge
 * without the user's approval (proposals carry those changes).
 */
export interface Orchestrator {
  /** Answer one user turn, returning plain-words text with citations. */
  respond(turn: string): Promise<string>;
}

/** Placeholder until the Muse Spark loop lands. */
export function createOrchestrator(): Orchestrator {
  return {
    respond(): Promise<string> {
      return Promise.reject(new Error("scaffold: orchestrator not built yet"));
    },
  };
}
