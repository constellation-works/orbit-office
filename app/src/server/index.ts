/**
 * App server (scaffold).
 *
 * To build: serve the single-page UI, call Orbit as the user through
 * `orbit tool run --json` (see orbit-client.ts), and run the 30-second
 * dispatch pass for `owner:agent` to-dos. Conversation is not here: it
 * happens in the user's agent CLI. Nothing here talks to Orbit yet.
 */
import { createServer } from "node:http";

const port = Number(process.env.PORT ?? 4317);
const host = process.env.HOST ?? "127.0.0.1";

const server = createServer((request, response) => {
  if (request.url === "/healthz") {
    response.writeHead(200, { "content-type": "application/json" });
    response.end(JSON.stringify({ ok: true, app: "orbit-office", scaffold: true }));
    return;
  }
  response.writeHead(501, { "content-type": "application/json" });
  response.end(JSON.stringify({ error: "scaffold: UI and Orbit wiring not built yet" }));
});

server.listen(port, host, () => {
  console.log(`orbit-office app (scaffold) listening on http://${host}:${port}`);
});
