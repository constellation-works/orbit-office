/**
 * Orbit access (scaffold).
 *
 * The app server talks to Orbit as the user, through `orbit tool run
 * --json`, for both `orbit.*` and `office.*` tools — including the tools
 * hidden from MCP (`mcp_scope: none`) that only the app may call. It never
 * reads `.orbit/` directly and never uses Orbit's dashboard HTTP API.
 */

export interface OrbitToolClient {
  /** Call one tool by name, e.g. `orbit.task.list` or `office.status`. */
  callTool(name: string, input: Record<string, unknown>): Promise<unknown>;
}

/** Placeholder until the `orbit tool run` client lands. */
export function createOrbitClient(): OrbitToolClient {
  return {
    callTool(name: string): Promise<unknown> {
      return Promise.reject(
        new Error(`scaffold: orbit client not built yet (wanted ${name})`),
      );
    },
  };
}
