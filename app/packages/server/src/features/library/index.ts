/** Library reads use the public Office tools, never the Library filesystem. */
import type { Feature } from "../../http/types.js";
import { createOrbitClient, type OrbitToolClient } from "../../orbit/client.js";

function output(value: unknown): Record<string, unknown> {
  if (value === null || typeof value !== "object" || Array.isArray(value)) throw new Error("invalid output");
  const envelope = value as Record<string, unknown>;
  if (envelope.ok === false) throw new Error("unavailable");
  const result = envelope.ok === true ? envelope.output : envelope;
  if (result === null || typeof result !== "object" || Array.isArray(result)) throw new Error("invalid output");
  return result as Record<string, unknown>;
}

export function createLibraryFeature(client: Pick<OrbitToolClient, "callTool">): Feature {
  return { name: "library", status: "built", routes: [{ method: "GET", path: "/api/library", async handle(request, response) {
    try {
      const params = new URL(request.url ?? "/", "http://localhost").searchParams;
      const view = params.get("view") ?? "overview";
      let result: unknown;
      if (view === "overview") {
        const [pages, artifacts, setup] = await Promise.all([
          client.callTool("office.knowledge-list", {}).then(output),
          client.callTool("office.artifact-list", { limit: 100 }).then(output),
          client.callTool("office.bootstrap-status", {}).then(output),
        ]);
        if (!Array.isArray(pages.pages) || !Array.isArray(pages.missing_templates) || !Array.isArray(artifacts.artifacts) || typeof artifacts.truncated !== "boolean" || !Array.isArray(setup.phases)) throw new Error("invalid listing");
        result = { pages, artifacts, setup };
      } else if (view === "search") {
        const query = params.get("query")?.trim();
        if (!query) throw new Error("missing query");
        result = output(await client.callTool("office.search", { query, limit: 100 }));
      } else if (view === "outputs") {
        const input: Record<string, unknown> = { limit: 100 };
        const kind = params.get("kind"); const series = params.get("series")?.trim();
        if (kind && ["report", "doc", "draft", "script", "data", "review", "other"].includes(kind)) input.kind = kind;
        if (series) input.series = series;
        result = output(await client.callTool("office.artifact-list", input));
      } else if (view === "document") {
        const id = params.get("id") ?? "";
        const tool = /^(dom|rb)-[a-z0-9]+(?:-[a-z0-9]+)*$/.test(id) ? "office.knowledge-show" : /^art-[0-9]{8}-[a-z0-9]+(?:-[a-z0-9]+)*$/.test(id) ? "office.artifact-show" : /^src-[a-z0-9-]+$/.test(id) ? "office.source-show" : null;
        if (tool === null) throw new Error("invalid document");
        const anchor = params.get("anchor");
        const input: Record<string, unknown> = { id, history: true };
        if (tool === "office.source-show" && anchor) input.anchor = `#${anchor}`;
        result = output(await client.callTool(tool, input));
      } else throw new Error("invalid view");
      response.writeHead(200, { "content-type": "application/json", "cache-control": "no-store" });
      response.end(JSON.stringify(result));
    } catch {
      response.writeHead(502, { "content-type": "application/json", "cache-control": "no-store" });
      response.end(JSON.stringify({ error: "library_unavailable" }));
    }
  } }] };
}
export const feature = createLibraryFeature(createOrbitClient());
