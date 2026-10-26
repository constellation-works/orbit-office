/**
 * Orchestrator agent (scaffold).
 *
 * A Muse Spark tool-calling loop over the `office.*` tools plus a curated
 * subset of `orbit.*` tools (task add/update/list/show, task artifacts,
 * frictions, search, auto-task list, routine control, workflow ship).
 * Behaviour is versioned in `plugin/.orbit-plugin/skills/orchestrator/SKILL.md`.
 *
 * Guardrails: capture pasted external content as a source first; never take
 * an outward-facing action, create standing automation, or write knowledge
 * without the user's approval (proposals carry those changes).
 */

/**
 * Create the orchestrator.
 *
 * The returned object will expose `respond(turn)` returning plain-words
 * text with citations. Placeholder until the Muse Spark loop lands.
 */
export function createOrchestrator() {
  return {
    respond() {
      return Promise.reject(new Error("scaffold: orchestrator not built yet"));
    },
  };
}
