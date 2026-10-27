/**
 * HTTP for the To-dos screen. Error bodies are short codes; the screen
 * turns them into plain sentences and never shows the code.
 */
import type { IncomingMessage, ServerResponse } from "node:http";

import { OrbitError, type OrbitToolClient } from "../../orbit/client.js";
import { closeTodo, loadTodos, TodoCloseError } from "./service.js";

const MAX_BODY_BYTES = 65_536;

export async function handleList(client: OrbitToolClient, response: ServerResponse): Promise<void> {
  try {
    send(response, 200, await loadTodos(client));
  } catch (error) {
    if (error instanceof TodoCloseError) {
      send(response, error.status, { error: error.code });
      return;
    }
    console.error("todos: list failed", error instanceof OrbitError ? error.code : "unknown");
    send(response, 502, { error: "unavailable" });
  }
}

export async function handleClose(
  client: OrbitToolClient,
  request: IncomingMessage,
  response: ServerResponse,
): Promise<void> {
  try {
    const id = idFromBody(await readJsonBody(request));
    if (id === null) throw new TodoCloseError(400, "invalid_input");
    await closeTodo(client, id);
    send(response, 200, { ok: true });
  } catch (error) {
    if (error instanceof TodoCloseError) {
      send(response, error.status, { error: error.code });
      return;
    }
    console.error("todos: close failed", error instanceof OrbitError ? error.code : "unknown");
    send(response, 502, { error: "unavailable" });
  }
}

function idFromBody(body: unknown): string | null {
  if (body === null || typeof body !== "object" || !("id" in body)) return null;
  return typeof body.id === "string" ? body.id : null;
}

async function readJsonBody(request: IncomingMessage): Promise<unknown> {
  const chunks: Buffer[] = [];
  let size = 0;
  for await (const chunk of request) {
    const buffer = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk);
    size += buffer.length;
    if (size > MAX_BODY_BYTES) throw new TodoCloseError(400, "invalid_input");
    chunks.push(buffer);
  }
  const text = Buffer.concat(chunks).toString("utf8").trim();
  if (text === "") throw new TodoCloseError(400, "invalid_input");
  try {
    return JSON.parse(text) as unknown;
  } catch {
    throw new TodoCloseError(400, "invalid_input");
  }
}

function send(response: ServerResponse, status: number, body: unknown): void {
  const payload = JSON.stringify(body);
  response.writeHead(status, {
    "content-type": "application/json; charset=utf-8",
    "cache-control": "no-store",
    "content-length": Buffer.byteLength(payload),
  });
  response.end(payload);
}
