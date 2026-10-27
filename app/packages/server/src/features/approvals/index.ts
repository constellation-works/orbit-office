/**
 * Approvals: approve or send back a reviewed to-do (promoting its output),
 * keep or dismiss a script's output, accept or dismiss a proposal (one at a
 * time or a whole card), and approve or dismiss a gateway draft. Error
 * bodies are short codes; the Today screen says them in plain words.
 */
import type { IncomingMessage, ServerResponse } from "node:http";
import { userInfo } from "node:os";

import type { Feature, Route } from "../../http/types.js";
import { createOrbitClient } from "../../orbit/client.js";
import {
  accept,
  acceptAll,
  approve,
  ApprovalError,
  type ApprovalClient,
  dismiss,
  isDigest,
  isTaskKey,
  loadDetail,
  send,
  sendBack,
} from "./service.js";

const MAX_BODY_BYTES = 65_536;
const MAX_COMMENT_CHARS = 4_000;
const MAX_ACCEPT_ALL = 50;

export interface ApprovalsOptions {
  client: ApprovalClient;
  /** Who approved, recorded on the task. Defaults to the signed-in user's name. */
  approver?: () => string;
}

function defaultApprover(): string {
  try {
    const name = userInfo().username.trim();
    return name === "" ? "the user" : name;
  } catch {
    return "the user";
  }
}

function reply(response: ServerResponse, status: number, body: unknown): void {
  const payload = JSON.stringify(body);
  response.writeHead(status, {
    "content-type": "application/json; charset=utf-8",
    "cache-control": "no-store",
    "content-length": Buffer.byteLength(payload),
  });
  response.end(payload);
}

async function readBody(request: IncomingMessage): Promise<Record<string, unknown>> {
  const chunks: Buffer[] = [];
  let size = 0;
  for await (const chunk of request) {
    const buffer = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk as string);
    size += buffer.length;
    if (size > MAX_BODY_BYTES) throw new ApprovalError(400, "invalid_input");
    chunks.push(buffer);
  }
  try {
    const parsed: unknown = JSON.parse(Buffer.concat(chunks).toString("utf8"));
    if (typeof parsed === "object" && parsed !== null && !Array.isArray(parsed)) return parsed as Record<string, unknown>;
  } catch {
    // Reported below.
  }
  throw new ApprovalError(400, "invalid_input");
}

/** Unknown keys are refused, never ignored. */
function closed(body: Record<string, unknown>, allowed: readonly string[]): void {
  if (Object.keys(body).some((key) => !allowed.includes(key))) throw new ApprovalError(400, "invalid_input");
}

function key(body: Record<string, unknown>): string {
  if (!isTaskKey(body.key)) throw new ApprovalError(400, "invalid_input");
  return body.key;
}

function digest(body: Record<string, unknown>): string {
  if (!isDigest(body.sha256)) throw new ApprovalError(400, "invalid_input");
  return body.sha256;
}

function text(value: unknown, required: boolean): string {
  if (value === undefined && !required) return "";
  if (typeof value !== "string") throw new ApprovalError(400, "invalid_input");
  const trimmed = value.trim();
  if ((required && trimmed === "") || trimmed.length > MAX_COMMENT_CHARS) throw new ApprovalError(400, "invalid_input");
  return trimmed;
}

function route(method: Route["method"], path: string, run: (request: IncomingMessage) => Promise<unknown>): Route {
  return {
    method,
    path,
    async handle(request, response) {
      try {
        reply(response, 200, await run(request));
      } catch (error) {
        if (error instanceof ApprovalError) {
          reply(response, error.status, error.reason === undefined ? { error: error.code } : { error: error.code, reason: error.reason });
          return;
        }
        console.error("approvals: request failed");
        reply(response, 502, { error: "unavailable" });
      }
    },
  };
}

/** Routes bound to one Orbit client. Tests pass a fake client. */
export function createApprovalsFeature({ client, approver = defaultApprover }: ApprovalsOptions): Feature {
  return {
    name: "approvals",
    status: "built",
    routes: [
      route("GET", "/api/approvals/detail", async (request) => {
        const params = new URL(request.url ?? "/", "http://localhost").searchParams;
        const value = params.get("key");
        if (!isTaskKey(value) || [...params.keys()].some((name) => name !== "key")) throw new ApprovalError(400, "invalid_input");
        return loadDetail(client, value);
      }),
      route("POST", "/api/approvals/approve", async (request) => {
        const body = await readBody(request);
        closed(body, ["key"]);
        return approve(client, key(body), approver());
      }),
      route("POST", "/api/approvals/send-back", async (request) => {
        const body = await readBody(request);
        closed(body, ["key", "comment"]);
        await sendBack(client, key(body), text(body.comment, true), approver());
        return { ok: true };
      }),
      route("POST", "/api/approvals/dismiss", async (request) => {
        const body = await readBody(request);
        closed(body, ["key", "reason"]);
        await dismiss(client, key(body), text(body.reason, false), approver());
        return { ok: true };
      }),
      route("POST", "/api/approvals/accept", async (request) => {
        const body = await readBody(request);
        closed(body, ["key", "sha256"]);
        await accept(client, key(body), digest(body));
        return { ok: true };
      }),
      route("POST", "/api/approvals/accept-all", async (request) => {
        const body = await readBody(request);
        closed(body, ["proposals"]);
        const list = body.proposals;
        if (!Array.isArray(list) || list.length === 0 || list.length > MAX_ACCEPT_ALL) throw new ApprovalError(400, "invalid_input");
        const proposals = list.map((item: unknown) => {
          if (typeof item !== "object" || item === null || Array.isArray(item)) throw new ApprovalError(400, "invalid_input");
          const entry = item as Record<string, unknown>;
          closed(entry, ["key", "sha256"]);
          return { key: key(entry), sha256: digest(entry) };
        });
        if (new Set(proposals.map((item) => item.key)).size !== proposals.length) throw new ApprovalError(400, "invalid_input");
        return acceptAll(client, proposals);
      }),
      route("POST", "/api/approvals/send", async (request) => {
        const body = await readBody(request);
        closed(body, ["key", "sha256"]);
        return send(client, key(body), digest(body));
      }),
    ],
  };
}

export const feature: Feature = createApprovalsFeature({ client: createOrbitClient() });
