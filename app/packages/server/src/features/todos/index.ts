/**
 * To-dos: You and Agents columns. Each to-do shows its source and attached
 * knowledge. The user can close their own; an agent to-do has no close
 * action (docs/design/office/specs/web-app.md).
 */
import type { Feature } from "../../http/types.js";
import { createOrbitClient, type OrbitToolClient } from "../../orbit/client.js";
import { handleClose, handleList } from "./http.js";

/** Routes bound to one Orbit client. Tests pass a fake client. */
export function createTodoFeature(client: OrbitToolClient): Feature {
  return {
    name: "todos",
    status: "built",
    routes: [
      {
        method: "GET",
        path: "/api/todos",
        handle: (_request, response) => handleList(client, response),
      },
      {
        method: "POST",
        path: "/api/todos/close",
        handle: (request, response) => handleClose(client, request, response),
      },
    ],
  };
}

export const feature: Feature = createTodoFeature(createOrbitClient());
