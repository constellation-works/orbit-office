/** Selection tray access through the Office tools. */
import type { IncomingMessage, ServerResponse } from "node:http";
import type { Feature, Route } from "../../http/types.js";
import { createOrbitClient, type OrbitToolClient } from "../../orbit/client.js";

type Row = Record<string, unknown>;
function row(value: unknown): Row {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("invalid data");
  return value as Row;
}
function reply(res: ServerResponse, status: number, value: unknown): void {
  res.writeHead(status, { "content-type": "application/json", "cache-control": "no-store" }); res.end(JSON.stringify(value));
}
async function body(req: IncomingMessage): Promise<Row> {
  if (req.headers["content-type"]?.split(";")[0] !== "application/json") throw new Error("invalid input");
  const chunks: Buffer[] = []; let size = 0;
  for await (const chunk of req) { const part = Buffer.from(chunk as Uint8Array); size += part.length; if (size > 65536) throw new Error("invalid input"); chunks.push(part); }
  return row(JSON.parse(Buffer.concat(chunks).toString("utf8")));
}
export function createSelectionFeature(client: Pick<OrbitToolClient, "callTool">): Feature {
  async function call(name: string, input: Row): Promise<Row> {
    const result = row(await client.callTool(name, input));
    if (result.ok === false) throw new Error("tool refused");
    return result.ok === true ? row(result.output) : result;
  }
  function route(method: "GET" | "POST", path: string, handle: Route["handle"]): Route {
    return { method, path, async handle(req, res) { try { await handle(req, res); } catch { if (!res.headersSent) reply(res, 502, { error: "selection_unavailable" }); else res.end(); } } };
  }
  return { name: "selection", status: "built", routes: [
    route("GET", "/api/selection", async (_req, res) => {
      const data = await call("office.selection-get", { peek: true });
      if (!Array.isArray(data.selections) || data.selections.length > 5) throw new Error("invalid output");
      reply(res, 200, { count: data.selections.length });
    }),
    route("POST", "/api/selection", async (req, res) => {
      let input: Row;
      try {
        input = await body(req);
        const keys = Object.keys(input);
        const clear = keys.length === 1 && input.clear === true;
        const selection = keys.every(key => ["object", "anchor", "quote", "view", "sensitivity"].includes(key)) &&
          typeof input.object === "string" && /^src-[a-z0-9]+(?:-[a-z0-9]+)*$/.test(input.object) &&
          typeof input.anchor === "string" && /^#L\d+(?:-\d+)?$/.test(input.anchor) &&
          typeof input.quote === "string" && input.quote.length > 0 && input.quote.length <= 32000 &&
          input.view === "source" && (input.sensitivity === "normal" || input.sensitivity === "private");
        if (!clear && !selection) throw new Error("invalid input");
        if (input.sensitivity === "private") delete input.quote;
      } catch { reply(res, 400, { error: "invalid_input" }); return; }
      await call("office.selection-put", input); reply(res, 200, { ok: true });
    }),
  ] };
}
export const feature = createSelectionFeature(createOrbitClient());
