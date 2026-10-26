/**
 * App server (scaffold).
 *
 * To build: serve the single-page UI, hold one long-lived `orbit mcp` stdio
 * client (see orbit-client.ts), run the orchestrator loop
 * (see orchestrator.ts), and run the 30-second dispatch pass for
 * `owner:agent` to-dos. Nothing here talks to Orbit yet.
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
