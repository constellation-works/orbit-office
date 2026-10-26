/**
 * Orbit access (scaffold).
 *
 * The app server talks to Orbit through one long-lived `orbit mcp` stdio
 * client, for both `orbit.*` and `office.*` tools. It never reads `.orbit/`
 * directly and never uses Orbit's dashboard HTTP API.
 */

/**
 * Create the Orbit tool client.
 *
 * The returned object will expose `callTool(name, input)` for MCP tool
 * names such as `orbit_task_list` or `office_status`. Placeholder until
 * the MCP stdio client lands.
 */
export function createOrbitClient() {
  return {
    callTool(name) {
      return Promise.reject(new Error(`scaffold: orbit client not built yet (wanted ${name})`));
    },
  };
}
