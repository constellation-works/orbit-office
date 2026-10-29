/**
 * Settings: agent CLI, the private route, the team shared library, gateway
 * connections (with the device-code sign-in for oauth2_device ones) and
 * redacted API call history.
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
import { auditRow, connection, connectionInput, count, object, SettingsError, signInInput, signInPoll, signInStarted, toolData } from "./gateway.js";
import { logRouteFailure, safeOrbitCode } from "../../orbit/diagnostics.js";
import { privateRouteInput, readPrivateRoute, savePrivateRoute } from "./private-route.js";
import { isSlot, SecretError, writeSecret, type SecretOptions } from "./secret.js";
import { enableSharedSync, readSharedLibrary, saveSharedLibrary, sharedLibraryInput, SharedSyncMemory } from "./shared-library.js";

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

export interface SettingsOptions {
  /**
   * Runs `office.library-sync`, whose git fetch and push can take minutes:
   * the routine allows six. Defaults to `client`.
   */
  syncClient?: Pick<OrbitToolClient, "callTool">;
}

/** A sync runs each git command under a two-minute deadline; allow the routine's six. */
const SYNC_TIMEOUT_MS = 360_000;

/** Bound to one shared Orbit client; writes are never cached or automatically retried. */
export function createSettingsFeature(client: OrbitToolClient, secretOptions: SecretOptions = {}, options: SettingsOptions = {}): Feature {
  const syncClient = options.syncClient ?? client;
  const memory = new SharedSyncMemory();
  // A put and its slot writes must not interleave with another browser tab.
  // Refuse concurrent writes instead of retaining a queue of credentials.
  let writing = false;
  function route(method: "GET" | "POST", path: string,
    action: (request: IncomingMessage) => Promise<unknown>, tool?: string): Feature["routes"][number] {
    return { method, path, async handle(request, response) {
      try { send(response, 200, await action(request)); }
      catch (error) {
        // One redacted diagnostic line per failed gateway call: route, tool
        // and typed kind / safe orbit code only. No error messages, child
        // streams, request bodies or tool output are logged.
        if (error instanceof SettingsError) {
          const orbitCode = error.orbitCode ?? safeOrbitCode(error);
          const logTool = error.tool ?? tool;
          const toolFailure = orbitCode !== undefined ||
            ["gateway_unavailable", "invalid_gateway_data", "sign_in_failed", "invalid_connection", "no_free_slot", "unknown_connection", "sign_in_not_started"].includes(error.code);
          if (logTool !== undefined && toolFailure) logRouteFailure(path, logTool, error);
          send(response, error.status, {
            error: error.code, ...error.details,
            ...(orbitCode !== undefined && error.code === "gateway_unavailable" && orbitCode !== error.code ? { code: orbitCode } : {}),
          });
        }
        else if (error instanceof SecretError) send(response, 502, { error: error.code, connection_saved: true });
        else {
          const orbitCode = safeOrbitCode(error);
          if (tool !== undefined) logRouteFailure(path, tool, error);
          send(response, 502, { error: "gateway_unavailable", ...(orbitCode !== undefined ? { code: orbitCode } : {}) });
        }
      }
    } };
  }
  return { name: "settings", status: "built", routes: [
    { method: "GET", path: "/api/settings", handle: sendSettings },
    route("GET", "/api/settings/connections", async () => {
      const data = toolData(await client.callTool("gateway.connection-list", {}), "gateway.connection-list");
      if (!Array.isArray(data.connections)) throw new SettingsError(502, "invalid_gateway_data");
      return { connections: data.connections.map(connection), free_slots: count(data.free_slots) };
    }, "gateway.connection-list"),
    route("POST", "/api/settings/connections/put", async (request) => {
      if (writing) throw new SettingsError(503, "settings_busy");
      writing = true;
      try {
        const { secret, ...input } = connectionInput(await readBody(request));
        const data = toolData(await client.callTool("gateway.connection-put", input), "gateway.connection-put");
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
    }, "gateway.connection-put"),
    // The device-code sign-in. The refresh token reaches the slot through
    // Orbit inside the gateway call; no response here ever carries it.
    route("POST", "/api/settings/connections/sign-in", async (request) => {
      const name = signInInput(await readBody(request));
      return signInStarted(toolData(await client.callTool("gateway.oauth-device-start", { connection: name }), "gateway.oauth-device-start"), name);
    }, "gateway.oauth-device-start"),
    route("POST", "/api/settings/connections/sign-in/poll", async (request) => {
      const name = signInInput(await readBody(request));
      return signInPoll(toolData(await client.callTool("gateway.oauth-device-poll", { connection: name }), "gateway.oauth-device-poll"), name);
    }, "gateway.oauth-device-poll"),
    route("GET", "/api/settings/private-route", async () => readPrivateRoute(client)),
    route("POST", "/api/settings/private-route", async (request) => {
      const input = privateRouteInput(await readBody(request));
      if (writing) throw new SettingsError(503, "settings_busy");
      writing = true;
      try { return await savePrivateRoute(client, input, secretOptions); }
      finally { writing = false; }
    }),
    route("GET", "/api/settings/shared-library", async () => readSharedLibrary(client, memory)),
    route("POST", "/api/settings/shared-library", async (request) => {
      const input = sharedLibraryInput(await readBody(request));
      if (writing) throw new SettingsError(503, "settings_busy");
      writing = true;
      try { return await saveSharedLibrary(client, input); }
      finally { writing = false; }
    }),
    route("POST", "/api/settings/shared-library/routine", async (request) => {
      const body = await readBody(request);
      if (!object(body) || Object.keys(body).some((key) => key !== "enabled") || body.enabled !== true) throw new SettingsError(400, "invalid_input");
      await enableSharedSync(client);
      return { ok: true };
    }),
    route("POST", "/api/settings/shared-library/sync", async (request) => {
      const body = await readBody(request);
      if (!object(body) || Object.keys(body).length > 0) throw new SettingsError(400, "invalid_input");
      return memory.sync(syncClient);
    }),
    // Memory only, no Orbit call: Today reads it on every poll.
    route("GET", "/api/settings/shared-library/last-sync", async () => ({ last_sync: memory.last })),
    route("POST", "/api/settings/shared-library/dismiss-skipped", async (request) => {
      const body = await readBody(request);
      if (!object(body) || Object.keys(body).length > 0) throw new SettingsError(400, "invalid_input");
      memory.dismissSkipped();
      return { ok: true };
    }),
    route("GET", "/api/settings/api-calls", async () => {
      const data = toolData(await client.callTool("gateway.audit-list", { limit: 100 }), "gateway.audit-list");
      if (!Array.isArray(data.records)) throw new SettingsError(502, "invalid_gateway_data");
      return { records: data.records.map(auditRow).sort((a, b) => Date.parse(b.time) - Date.parse(a.time)), skipped: count(data.skipped) };
    }, "gateway.audit-list"),
  ] };
}

export const feature: Feature = createSettingsFeature(createOrbitClient(), {}, {
  syncClient: createOrbitClient({ maxConcurrent: 1, timeoutMs: SYNC_TIMEOUT_MS }),
});
