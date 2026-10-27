/**
 * Settings: the agent CLI connection (`GET /api/settings`).
 *
 * Built: reports whether the install script has registered Orbit MCP with
 * the user's agent CLI (docs/design/office/specs/web-app.md#delivery).
 * Plain words only in the UI: the endpoint names the CLI ("Muse Code")
 * and whether it is connected, never ids or raw statuses.
 */
import type { SettingsStatus } from "@orbit-office/contracts";
import type { IncomingMessage, ServerResponse } from "node:http";

import type { Feature } from "../../http/types.js";
import { agentCliStatus } from "./agent-cli.js";

function sendSettings(_request: IncomingMessage, response: ServerResponse): void {
  const body: SettingsStatus = { agentCli: agentCliStatus() };
  response.writeHead(200, { "content-type": "application/json" });
  response.end(JSON.stringify(body));
}

export const feature: Feature = {
  name: "settings",
  status: "built",
  routes: [{ method: "GET", path: "/api/settings", handle: sendSettings }],
};
