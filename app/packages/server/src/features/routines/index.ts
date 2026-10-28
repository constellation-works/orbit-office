import type { IncomingMessage, ServerResponse } from "node:http";
import type { Feature } from "../../http/types.js";
import { createOrbitClient, type OrbitToolClient } from "../../orbit/client.js";
import { defaultPluginRoots, readDefinitionNames, routineListing, type DefinitionNames } from "../../orbit/definitions.js";

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
/**
 * The latest fire, reduced to when it happened and how its task stands. Task
 * ids and skip diagnostics stay on the server: the screen shows plain words.
 */
function lastFire(value: unknown): { kind: "fired" | "skipped"; at: string; task_status: string | null } | null {
  if (!object(value) || typeof value.at !== "string" || (value.kind !== "fired" && value.kind !== "skipped")) return null;
  return { kind: value.kind, at: value.at, task_status: typeof value.task_status === "string" ? value.task_status : null };
}
const kinds = ["video", "meeting", "chat", "email", "doc", "receipt", "bill", "conversation", "note"];

export function createRoutinesFeature(client: OrbitToolClient, options: { pluginRoots?: readonly string[] } = {}): Feature {
  let names: DefinitionNames | undefined;
  try { names = readDefinitionNames(options.pluginRoots ?? defaultPluginRoots()); }
  catch { /* A bad shipped tree makes this feature unavailable, not the other screens. */ }
  const definitions = (): DefinitionNames => {
    if (!names) throw new Error("shipped definitions unavailable");
    return names;
  };
  return { name: "routines", status: "built", routes: [
    { method: "GET", path: "/api/routines", async handle(_request, response) {
      try {
        const shipped = definitions();
        const list = await client.callTool("orbit.auto_task.list", {});
        if (!Array.isArray(list)) throw new Error("invalid listing");
        const routines = list.map((row: unknown) => {
          if (!object(row) || typeof row.name !== "string" || typeof row.description !== "string" ||
              typeof row.effective_enabled !== "boolean" || !object(row.schedule)) throw new Error("invalid listing");
          return { kind: "auto_task", name: row.name, description: row.description, schedule: row.schedule,
            enabled: row.effective_enabled, skipped_reason: typeof row.skipped_reason === "string" ? row.skipped_reason : null,
            last_fire: lastFire(row.last_fire) };
        });
        let jobs: Record<string, unknown>[] = [];
        let routinesUnavailable = false;
        try { jobs = await routineListing(client); }
        catch (error) {
          if (!object(error) || error.code !== "not_permitted") throw error;
          routinesUnavailable = true;
        }
        const visible = routines.filter(row => shipped.auto_tasks.has(row.name));
        const visibleJobs = jobs.filter(row => shipped.routines.has(String(row.name)));
        send(response, 200, { routines: [...visible, ...visibleJobs.map(row => ({
          kind: "routine", name: row.name, description: row.description, schedule: row.schedule,
          enabled: row.effective, skipped_reason: typeof row.toggle_reason === "string" ? row.toggle_reason : null,
          last_fire: object(row.last_fire) && typeof row.last_fire.at === "string" ? {
            kind: "fired", at: row.last_fire.at, task_status: null,
            run_state: typeof row.last_fire.state === "string" ? row.last_fire.state : "unknown",
          } : null,
        }))], hidden_count: list.length - visible.length + jobs.length - visibleJobs.length,
          routines_unavailable: routinesUnavailable });
      } catch { send(response, 502, { error: "unavailable" }); }
    } },
    { method: "POST", path: "/api/routines/toggle", async handle(request, response) {
      try {
        const input = await body(request);
        if (!object(input) || Object.keys(input).some(key => !["name", "enabled", "kind"].includes(key)) ||
            typeof input.name !== "string" || !input.name || typeof input.enabled !== "boolean" ||
            !(input.kind === undefined || input.kind === "auto_task" || input.kind === "routine")) {
          send(response, 400, { error: "invalid_input" }); return;
        }
        const kind = input.kind === "routine" ? "routine" : "auto_task";
        if (!(kind === "routine" ? definitions().routines : definitions().auto_tasks).has(input.name)) {
          send(response, 400, { error: "invalid_input" }); return;
        }
        if (kind === "routine") {
          const rows = await routineListing(client);
          const row = rows.find(row => row.name === input.name);
          if (!row) throw new Error("routine unavailable");
          await client.callTool("orbit.routine.control", { action: "toggle", name: input.name,
            expected_enabled: row.enabled, enabled: input.enabled, target: row.target });
        } else await client.callTool("orbit.auto_task.update", { name: input.name, enabled: input.enabled });
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
