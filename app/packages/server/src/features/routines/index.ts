import type { IncomingMessage, ServerResponse } from "node:http";
import type { Feature } from "../../http/types.js";
import { createOrbitClient, type OrbitToolClient } from "../../orbit/client.js";

function object(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}
async function body(request: IncomingMessage): Promise<unknown> {
  const chunks: Buffer[] = []; let size = 0;
  for await (const chunk of request) {
    const data = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk);
    size += data.length;
    if (size > 65536) throw new Error("invalid input");
    chunks.push(data);
  }
  return JSON.parse(Buffer.concat(chunks).toString("utf8")) as unknown;
}
function send(response: ServerResponse, status: number, value: unknown): void {
  response.writeHead(status, { "content-type": "application/json", "cache-control": "no-store" });
  response.end(JSON.stringify(value));
}
const kinds = ["video", "meeting", "chat", "email", "doc", "receipt", "bill", "conversation", "note"];
export function createRoutinesFeature(client: OrbitToolClient): Feature {
  return { name: "routines", status: "built", routes: [
    { method: "GET", path: "/api/routines", async handle(_request, response) {
      try {
        const list = await client.callTool("orbit.auto_task.list", {});
        if (!Array.isArray(list)) throw new Error("invalid listing");
        const routines = list.map((row: unknown) => {
          if (!object(row) || typeof row.name !== "string" || typeof row.description !== "string" ||
              typeof row.effective_enabled !== "boolean" || !object(row.schedule)) throw new Error("invalid listing");
          return { name: row.name, description: row.description, schedule: row.schedule,
            enabled: row.effective_enabled, skipped_reason: typeof row.skipped_reason === "string" ? row.skipped_reason : null };
        });
        send(response, 200, { routines });
      } catch { send(response, 502, { error: "unavailable" }); }
    } },
    { method: "POST", path: "/api/routines/toggle", async handle(request, response) {
      try {
        const input = await body(request);
        if (!object(input) || Object.keys(input).some(key => !["name", "enabled"].includes(key)) ||
            typeof input.name !== "string" || !input.name || typeof input.enabled !== "boolean") {
          send(response, 400, { error: "invalid_input" }); return;
        }
        await client.callTool("orbit.auto_task.update", { name: input.name, enabled: input.enabled });
        send(response, 200, { ok: true });
      } catch { send(response, 502, { error: "unavailable" }); }
    } },
    { method: "POST", path: "/api/routines/retention", async handle(request, response) {
      try {
        const input = await body(request);
        if (!object(input) || Object.keys(input).some(key => !["kind", "value"].includes(key)) ||
            typeof input.kind !== "string" || !kinds.includes(input.kind) ||
            !(input.value === null || typeof input.value === "number" && Number.isFinite(input.value))) {
          send(response, 400, { error: "invalid_input" }); return;
        }
        const result = await client.callTool("office.config-set", { key: `retention.${input.kind}`, value: input.value });
        if (object(result) && result.ok === false) {
          const error = object(result.error) ? result.error : {};
          send(response, 400, { error: "refused", reason: typeof error.message === "string" ? error.message : "This value was refused." }); return;
        }
        send(response, 200, { ok: true });
      } catch { send(response, 502, { error: "unavailable" }); }
    } },
  ] };
}
export const feature: Feature = createRoutinesFeature(createOrbitClient());
