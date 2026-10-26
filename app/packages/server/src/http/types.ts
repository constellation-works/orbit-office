/**
 * The feature contract every `features/<f>/index.ts` exports. Features may
 * import this file, but not the router (ARCHITECTURE.md, "Web app").
 */
import type { IncomingMessage, ServerResponse } from "node:http";

import type { FeatureName, Status } from "@orbit-office/contracts";

/** One HTTP route a feature serves. */
export interface Route {
  method: "GET" | "POST";
  /** Exact path, for example `/api/today`. */
  path: string;
  handle(request: IncomingMessage, response: ServerResponse): void | Promise<void>;
}

/** One server feature. A planned feature mounts no routes. */
export interface Feature {
  name: FeatureName;
  status: Status;
  routes: readonly Route[];
}
