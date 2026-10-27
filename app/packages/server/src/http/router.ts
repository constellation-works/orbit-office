/**
 * Every server feature, in one list, and the request router.
 *
 * Integrator-owned (ARCHITECTURE.md): the scaffold listed every planned
 * feature, so a task that builds one edits only its own folder. Only
 * built features mount routes, so nothing planned is reachable.
 *
 * Every request first passes the loopback Host/Origin guard (STD-05
 * R16/R17, http/guard.ts). Built feature routes answer next, then the
 * web UI is served from `web/static` and `web/dist` (http/static.ts),
 * with `/` serving the UI shell.
 */
import type { IncomingMessage, ServerResponse } from "node:http";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

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
import { checkLoopbackRequest } from "./guard.js";
import { serveStatic } from "./static.js";
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

export interface RouterOptions {
  /** Directory holding the app shell (`index.html`). Default `web/static`. */
  staticDir?: string;
  /** Directory holding the compiled UI. Default `web/dist`. */
  distDir?: string;
}

function defaultWebDirs(): { staticDir: string; distDir: string } {
  // From `packages/server/{src,dist}/http/` up to `packages/`, then `web/`.
  const packages = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..", "..");
  return {
    staticDir: resolve(packages, "web", "static"),
    distDir: resolve(packages, "web", "dist"),
  };
}

const built = FEATURES.filter((feature) => feature.status === "built");
const routes: readonly Route[] = built.flatMap((feature) => feature.routes);

function sendJson(response: ServerResponse, status: number, body: unknown): void {
  response.writeHead(status, { "content-type": "application/json" });
  response.end(JSON.stringify(body));
}

/** Create the request handler, with injectable web directories for tests. */
export function createHandler(options: RouterOptions = {}): (
  request: IncomingMessage,
  response: ServerResponse,
) => void {
  const defaults = defaultWebDirs();
  const roots: readonly string[] = [options.staticDir ?? defaults.staticDir, options.distDir ?? defaults.distDir];
  return (request, response) => {
    void (async () => {
      if (!checkLoopbackRequest(request).ok) {
        sendJson(response, 403, { error: "forbidden" });
        return;
      }
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
      if (route !== undefined) {
        await route.handle(request, response);
        return;
      }
      if (await serveStatic(request, response, roots, path)) return;
      sendJson(response, 404, { error: "not_found" });
    })().catch(() => {
      if (!response.headersSent) {
        sendJson(response, 500, { error: "internal_error" });
      } else {
        response.end();
      }
    });
  };
}

/** Route one request: guard, `/healthz`, built features, static UI, else 404. */
export const handle: (request: IncomingMessage, response: ServerResponse) => void = createHandler();
