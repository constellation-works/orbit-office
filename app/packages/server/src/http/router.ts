/**
 * Every server feature, in one list, and the request router.
 *
 * Integrator-owned (ARCHITECTURE.md): the scaffold listed every planned
 * feature, so a task that builds one edits only its own folder. Only
 * built features mount routes, so nothing planned is reachable.
 */
import type { IncomingMessage, ServerResponse } from "node:http";

import type { Health } from "@orbit-office/contracts";

import { feature as approvals } from "../features/approvals/index.js";
import { feature as insights } from "../features/insights/index.js";
import { feature as library } from "../features/library/index.js";
import { feature as review } from "../features/review/index.js";
import { feature as routines } from "../features/routines/index.js";
import { feature as selection } from "../features/selection/index.js";
import { feature as settings } from "../features/settings/index.js";
import { feature as sources } from "../features/sources/index.js";
import { feature as today } from "../features/today/index.js";
import { feature as todos } from "../features/todos/index.js";
import type { Feature, Route } from "./types.js";

/** Every feature, built or planned. */
export const FEATURES: readonly Feature[] = [
  today,
  sources,
  todos,
  approvals,
  routines,
  library,
  insights,
  review,
  selection,
  settings,
];

const built = FEATURES.filter((feature) => feature.status === "built");
const routes: readonly Route[] = built.flatMap((feature) => feature.routes);

function sendJson(response: ServerResponse, status: number, body: unknown): void {
  response.writeHead(status, { "content-type": "application/json" });
  response.end(JSON.stringify(body));
}

/** Route one request: `/healthz`, then the built features' routes, else 404. */
export function handle(request: IncomingMessage, response: ServerResponse): void {
  const path = (request.url ?? "/").split("?")[0] ?? "/";
  if (request.method === "GET" && path === "/healthz") {
    const health: Health = {
      ok: true,
      app: "orbit-office",
      features: built.map((feature) => feature.name),
    };
    sendJson(response, 200, health);
    return;
  }
  const route = routes.find((r) => r.method === request.method && r.path === path);
  if (route === undefined) {
    sendJson(response, 404, { error: "not_found" });
    return;
  }
  Promise.resolve(route.handle(request, response)).catch(() => {
    if (!response.headersSent) {
      sendJson(response, 500, { error: "internal_error" });
    }
  });
}
