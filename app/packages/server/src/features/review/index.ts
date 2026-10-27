/** Generated reviews and the request to create another one. */
import type { ReviewList } from "@orbit-office/contracts";
import type { Feature } from "../../http/types.js";
import { createOrbitClient, OrbitError, type OrbitToolClient } from "../../orbit/client.js";

function object(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}
function output(value: unknown): Record<string, unknown> {
  if (!object(value) || value.ok === false) throw new Error("unavailable");
  const result = value.ok === true ? value.output : value;
  if (!object(result)) throw new Error("invalid output");
  return result;
}

/** Search the two review cadences, then read each matching artifact's body. */
export async function loadReviews(client: Pick<OrbitToolClient, "callTool">): Promise<ReviewList> {
  const series = ["review-quarterly", "review-yearly"];
  const ids = new Set<string>();
  for (const name of series) {
    const result = output(await client.callTool("office.search", { query: name, kind: "artifact", series: name, limit: 100 }));
    if (!Array.isArray(result.hits) || result.hits.length > 100) throw new Error("invalid search");
    for (const hit of result.hits) {
      if (!object(hit) || typeof hit.id !== "string") throw new Error("invalid hit");
      ids.add(hit.id);
    }
  }
  const reviews = [];
  for (const id of ids) {
    const result = output(await client.callTool("office.artifact-show", { id }));
    const artifact = result.artifact;
    if (!object(artifact) || artifact.id !== id || artifact.kind !== "review" ||
        typeof artifact.series !== "string" || !series.includes(artifact.series) ||
        typeof artifact.title !== "string" || !object(artifact.approved) ||
        typeof artifact.approved.at !== "string" || !Number.isFinite(Date.parse(artifact.approved.at)) ||
        typeof result.truncated !== "boolean" || (result.body !== undefined && typeof result.body !== "string")) {
      throw new Error("invalid review");
    }
    reviews.push({ title: artifact.title, date: artifact.approved.at, body: typeof result.body === "string" ? result.body : null, truncated: result.truncated });
  }
  reviews.sort((a, b) => Date.parse(b.date) - Date.parse(a.date) || a.title.localeCompare(b.title));
  return { reviews: reviews.slice(0, 100), latest_100: reviews.length >= 100 };
}

export function createReviewFeature(client: OrbitToolClient): Feature {
  return { name: "review", status: "built", routes: [
    { method: "GET", path: "/api/review", async handle(_request, response) {
      try {
        const data = await loadReviews(client);
        response.writeHead(200, { "content-type": "application/json", "cache-control": "no-store" });
        response.end(JSON.stringify(data));
      } catch {
        response.writeHead(502, { "content-type": "application/json", "cache-control": "no-store" });
        response.end(JSON.stringify({ error: "unavailable" }));
      }
    } },
    { method: "POST", path: "/api/review/regenerate", async handle(_request, response) {
      try {
        await client.mintAutoTask("office-review");
        response.writeHead(200, { "content-type": "application/json", "cache-control": "no-store" });
        response.end(JSON.stringify({ ok: true }));
      } catch (error) {
        const refused = error instanceof OrbitError && (error.code === "not_permitted" || error.code === "unknown_auto_task");
        response.writeHead(refused ? 403 : 503, { "content-type": "application/json", "cache-control": "no-store" });
        response.end(JSON.stringify({ error: refused ? "refused" : "unavailable" }));
      }
    } },
  ] };
}
export const feature = createReviewFeature(createOrbitClient());
