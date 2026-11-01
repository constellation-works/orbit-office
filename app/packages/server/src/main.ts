/**
 * App server composition: start the dispatch pass and listen on loopback
 * by default, serving the router's handler (`http/router.ts` builds it from
 * every feature). Nothing else lives here (ARCHITECTURE.md, "Web app").
 *
 * The server is the only process the browser talks to. It calls Orbit as
 * the user through `orbit tool run --json` (orbit/client.ts) and runs
 * the 30-second dispatch pass (dispatch/pass.ts), so dispatch continues
 * while the browser is closed. Conversation is not here: it happens in
 * the user's agent CLI.
 */
import { createServer } from "node:http";

import { createDispatcher } from "./dispatch/pass.js";
import { handle } from "./http/router.js";
import { createOrbitClient } from "./orbit/client.js";

const port = Number(process.env.PORT ?? 4317);
const host = process.env.HOST ?? "127.0.0.1";

const orbit = createOrbitClient();
const dispatcher = createDispatcher({
  client: orbit,
  onError: (error) => {
    console.error("dispatch pass failed:", error instanceof Error ? error.message : String(error));
  },
});
dispatcher.start();

const server = createServer((request, response) => {
  handle(request, response);
});

server.listen(port, host, () => {
  console.log(`orbit-office app listening on http://${host}:${port}`);
});
