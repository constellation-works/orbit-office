/**
 * App server composition: build the router from every feature and listen
 * on loopback. Nothing else lives here (ARCHITECTURE.md, "Web app").
 *
 * The server is the only process the browser talks to. It calls Orbit as
 * the user through `orbit tool run --json` (orbit/client.ts) and will run
 * the 30-second dispatch pass (dispatch/pass.ts). Conversation is not
 * here: it happens in the user's agent CLI.
 */
import { createServer } from "node:http";

import { handle } from "./http/router.js";

const port = Number(process.env.PORT ?? 4317);
const host = process.env.HOST ?? "127.0.0.1";

const server = createServer((request, response) => {
  handle(request, response);
});

server.listen(port, host, () => {
  console.log(`orbit-office app listening on http://${host}:${port}`);
});
