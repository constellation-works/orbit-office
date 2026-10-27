/**
 * Settings: agent CLI, the private route, gateway connections and redacted API call history.
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
import { createOrbitClient, type OrbitToolClient } from "../../orbit/client.js";
import { auditRow, connection, connectionInput, count, object, SettingsError, toolData } from "./gateway.js";
import { privateRouteInput, readPrivateRoute, savePrivateRoute } from "./private-route.js";
import { isSlot, SecretError, writeSecret, type SecretOptions } from "./secret.js";

function sendSettings(_request: IncomingMessage, response: ServerResponse): void {
  const body: SettingsStatus = { agentCli: agentCliStatus() };
  response.writeHead(200, { "content-type": "application/json" });
  response.end(JSON.stringify(body));
}

function send(response: ServerResponse, status: number, body: unknown): void {
  response.writeHead(status, { "content-type": "application/json", "cache-control": "no-store" });
  response.end(JSON.stringify(body));
}

async function readBody(request: IncomingMessage): Promise<unknown> {
  const chunks: Buffer[] = [];
  let bytes = 0;
  const timer = setTimeout(() => request.destroy(), 10_000);
  try {
    for await (const chunk of request) {
      const data = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk);
      bytes += data.length;
      if (bytes > 65_536) throw new SettingsError(413, "invalid_input");
      chunks.push(data);
    }
  } finally {
    clearTimeout(timer);
  }
  try { return JSON.parse(Buffer.concat(chunks).toString("utf8")) as unknown; }
  catch { throw new SettingsError(400, "invalid_input"); }
}

/** Bound to one shared Orbit client; writes are never cached or automatically retried. */
export function createSettingsFeature(client: OrbitToolClient, secretOptions: SecretOptions = {}): Feature {
  // A put and its slot writes must not interleave with another browser tab.
  // Refuse concurrent writes instead of retaining a queue of credentials.
  let writing = false;
  function route(method: "GET" | "POST", path: string,
    action: (request: IncomingMessage) => Promise<unknown>): Feature["routes"][number] {
    return { method, path, async handle(request, response) {
      try { send(response, 200, await action(request)); }
      catch (error) {
        // No error messages, child streams, request bodies or tool output are logged.
        if (error instanceof SettingsError) send(response, error.status, { error: error.code, ...error.details });
        else if (error instanceof SecretError) send(response, 502, { error: error.code, connection_saved: true });
        else send(response, 502, { error: "gateway_unavailable" });
      }
    } };
  }
  return { name: "settings", status: "built", routes: [
    { method: "GET", path: "/api/settings", handle: sendSettings },
    route("GET", "/api/settings/connections", async () => {
      const data = toolData(await client.callTool("gateway.connection-list", {}));
      if (!Array.isArray(data.connections)) throw new SettingsError(502, "invalid_gateway_data");
      return { connections: data.connections.map(connection), free_slots: count(data.free_slots) };
    }),
    route("POST", "/api/settings/connections/put", async (request) => {
      if (writing) throw new SettingsError(503, "settings_busy");
      writing = true;
      try {
        const { secret, ...input } = connectionInput(await readBody(request));
        const data = toolData(await client.callTool("gateway.connection-put", input));
        const saved = connection(data.connection);
        if (typeof data.created !== "boolean" || !object(data.connection) || saved.name !== input.name ||
            !(data.released_slot === null || isSlot(data.released_slot)) ||
            data.released_slot !== null && data.released_slot === data.connection.slot ||
            (secret !== undefined && !isSlot(data.connection.slot))) throw new SettingsError(502, "invalid_gateway_data");
        if (data.released_slot !== null) await writeSecret("rm", data.released_slot, undefined, secretOptions);
        if (secret !== undefined && isSlot(data.connection.slot)) {
          await writeSecret("set", data.connection.slot, secret, secretOptions);
          saved.slot_state = "set";
        }
        return { connection: saved, created: data.created };
      } finally { writing = false; }
    }),
    route("GET", "/api/settings/private-route", async () => readPrivateRoute(client)),
    route("POST", "/api/settings/private-route", async (request) => {
      const input = privateRouteInput(await readBody(request));
      if (writing) throw new SettingsError(503, "settings_busy");
      writing = true;
      try { return await savePrivateRoute(client, input, secretOptions); }
      finally { writing = false; }
    }),
    route("GET", "/api/settings/api-calls", async () => {
      const data = toolData(await client.callTool("gateway.audit-list", { limit: 100 }));
      if (!Array.isArray(data.records)) throw new SettingsError(502, "invalid_gateway_data");
      return { records: data.records.map(auditRow).sort((a, b) => Date.parse(b.time) - Date.parse(a.time)), skipped: count(data.skipped) };
    }),
  ] };
}

export const feature: Feature = createSettingsFeature(createOrbitClient());
