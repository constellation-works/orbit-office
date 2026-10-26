/**
 * Orbit access (scaffold).
 *
 * The app server talks to Orbit through one long-lived `orbit mcp` stdio
 * client, for both `orbit.*` and `office.*` tools. It never reads `.orbit/`
 * directly and never uses Orbit's dashboard HTTP API.
 */
export interface OrbitToolClient {
  /** Call one tool by its MCP name, e.g. `orbit_task_list` or `office_status`. */
  callTool(name: string, input: Record<string, unknown>): Promise<unknown>;
}

/** Placeholder until the MCP stdio client lands. */
export function createOrbitClient(): OrbitToolClient {
  return {
    callTool(name: string): Promise<unknown> {
      return Promise.reject(new Error(`scaffold: orbit client not built yet (wanted ${name})`));
    },
  };
}
