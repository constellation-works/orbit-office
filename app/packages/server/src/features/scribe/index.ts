/**
 * Scribe: a compact window over scribe v1's SCR files (specs/scribe.md;
 * specs/web-app.md, "Screens"). The feed and the canned views read
 * `office.scribe-query` (model-free); a note goes through
 * `office.scribe-append` as `/scribe note` records it; the ask box calls
 * `office.scribe-ask`, which makes the one low-cost model call through the
 * Models port. Every call is `orbit tool run`; nothing reads the Library.
 * v1 is commanded only: no automatic capture, heartbeat or capture switch.
 */
import type { IncomingMessage, ServerResponse } from "node:http";

import type { ScribeAnswer, ScribeFeed, ScribeNoteResult, ScribeQueryResult, ScribeView } from "@orbit-office/contracts";

import type { Feature } from "../../http/types.js";
import { createOrbitClient, OrbitExitError, type OrbitToolClient } from "../../orbit/client.js";
import {
  answerOf, cursorOf, feedOf, noteEntry, outputOf, ScribeError, VIEWS, viewInput, viewOf, windowStart,
} from "./model.js";

type Client = Pick<OrbitToolClient, "callTool" | "pollTool">;

const MAX_BODY_BYTES = 16_384;
/** Lines the feed asks for at most. */
const FEED_LIMIT = 1000;
/**
 * Feed windows in days, widest first. The general `office.scribe-query`
 * keeps the newest lines up to the limit (oldest first within that bound),
 * so when a window holds more than the limit the feed narrows it to report
 * a tighter `since`; even the final one-day window keeps its newest lines,
 * with `total`/`shown` reporting the truncation.
 */
const FEED_WINDOWS = [30, 7, 1] as const;
const CODE = /^[a-z][a-z0-9_]{0,63}$/;
const SESSION = /^[A-Za-z0-9._:-]{1,128}$/;
const ABOUT = /^[A-Za-z0-9#._-]{1,64}$/;

/** A tool refusal code from an exec envelope or Orbit's stderr, if any. */
function refusalCode(error: unknown): string | null {
  if (!(error instanceof OrbitExitError)) return null;
  for (const line of error.stderr.split("\n").reverse()) {
    try {
      const parsed: unknown = JSON.parse(line);
      if (parsed === null || typeof parsed !== "object" || Array.isArray(parsed)) continue;
      const record = parsed as Record<string, unknown>;
      const inner = record.error !== null && typeof record.error === "object" ? record.error as Record<string, unknown> : record;
      if (typeof inner.code === "string" && CODE.test(inner.code)) return inner.code;
    } catch {
      // Not a JSON line.
    }
  }
  return null;
}

/** Unwrap an exec envelope when Orbit passes one through. */
function unwrap(value: unknown): unknown {
  if (value === null || typeof value !== "object" || Array.isArray(value)) return value;
  const envelope = value as Record<string, unknown>;
  if (envelope.ok === false) {
    const error = envelope.error !== null && typeof envelope.error === "object" ? envelope.error as Record<string, unknown> : {};
    throw new ScribeError(422, typeof error.code === "string" && CODE.test(error.code) ? error.code : "refused");
  }
  return envelope.ok === true ? envelope.output : value;
}

export async function loadFeed(client: Client, now: Date): Promise<ScribeFeed> {
  let feed: ScribeFeed | null = null;
  for (const days of FEED_WINDOWS) {
    const since = windowStart(now, days);
    feed = feedOf(outputOf(unwrap(await client.pollTool("office.scribe-query", { since, limit: FEED_LIMIT }))), since);
    if (feed.total <= feed.shown) break;
  }
  if (feed === null) throw new ScribeError(502, "unavailable");
  return feed;
}

export async function loadView(client: Client, view: ScribeView, about: string | null): Promise<ScribeQueryResult> {
  const read = async (asTopic: boolean): Promise<ScribeQueryResult> =>
    viewOf(view, outputOf(unwrap(await client.pollTool("office.scribe-query", viewInput(view, about, asTopic)))));
  const result = await read(false);
  // `/scribe why <matter|topic>`: a matter first, then the same word as a topic.
  if (view === "why" && about !== null && result.entries.length === 0) return read(true);
  return result;
}

export async function addNote(client: Client, body: unknown): Promise<ScribeNoteResult> {
  const input = noteBody(body);
  const entry = noteEntry(input.text, input.tag);
  // The note keeps the session's cursor: it never marks unread messages scribed.
  const cursor = cursorOf(outputOf(unwrap(await client.callTool("office.scribe-query", { session_id: input.session_id, limit: 1 }))));
  if (cursor === null) throw new ScribeError(409, "no_cursor");
  const output = outputOf(unwrap(await client.callTool("office.scribe-append", {
    session_id: input.session_id, through: cursor, entries: [entry],
  })));
  const path = typeof output.path === "string" ? output.path : "";
  const added = (Array.isArray(output.added) ? output.added : []).flatMap((item: unknown) => {
    if (item === null || typeof item !== "object") return [];
    const row = item as Record<string, unknown>;
    return typeof row.line === "number" && typeof row.text === "string" ? [{ file_line: `${path}:${row.line}`, text: row.text }] : [];
  });
  const refused = (Array.isArray(output.refused) ? output.refused : []).flatMap((item: unknown) => {
    if (item === null || typeof item !== "object") return [];
    const reason = (item as Record<string, unknown>).reason;
    return typeof reason === "string" && CODE.test(reason) ? [reason] : ["refused"];
  });
  const duplicate = typeof output.duplicates === "number" && output.duplicates > 0;
  if (added.length === 0) throw new ScribeError(422, refused[0] ?? (duplicate ? "duplicate" : "refused"));
  return { added, refused };
}

export async function ask(client: Client, body: unknown): Promise<ScribeAnswer> {
  const question = questionBody(body);
  return answerOf(outputOf(unwrap(await client.callTool("office.scribe-ask", { question }))));
}

/** `{session_id, text, tag?}` and nothing else: an unknown key is refused, never ignored. */
function noteBody(body: unknown): { session_id: string; text: string; tag?: string } {
  if (body === null || typeof body !== "object" || Array.isArray(body)) throw new ScribeError(400, "invalid_input");
  const row = body as Record<string, unknown>;
  if (Object.keys(row).some((key) => key !== "session_id" && key !== "text" && key !== "tag")) throw new ScribeError(400, "invalid_input");
  if (typeof row.session_id !== "string" || !SESSION.test(row.session_id) || typeof row.text !== "string") throw new ScribeError(400, "invalid_input");
  if (row.tag !== undefined && typeof row.tag !== "string") throw new ScribeError(400, "invalid_input");
  return { session_id: row.session_id, text: row.text, ...(typeof row.tag === "string" && row.tag !== "" ? { tag: row.tag } : {}) };
}

function questionBody(body: unknown): string {
  if (body === null || typeof body !== "object" || Array.isArray(body)) throw new ScribeError(400, "invalid_input");
  const row = body as Record<string, unknown>;
  if (Object.keys(row).some((key) => key !== "question")) throw new ScribeError(400, "invalid_input");
  if (typeof row.question !== "string" || row.question.trim() === "" || row.question.length > 4096) throw new ScribeError(400, "question_required");
  return row.question.trim();
}

function viewParams(request: IncomingMessage): { view: ScribeView; about: string | null } {
  const url = new URL(request.url ?? "/", "http://localhost");
  if ([...url.searchParams.keys()].some((key) => key !== "view" && key !== "about")) throw new ScribeError(400, "invalid_input");
  const view = VIEWS.find((name) => name === url.searchParams.get("view"));
  if (view === undefined) throw new ScribeError(400, "invalid_input");
  const raw = (url.searchParams.get("about") ?? "").trim().replace(/^#/, "");
  if (raw !== "" && !ABOUT.test(raw)) throw new ScribeError(400, "invalid_input");
  return { view, about: raw === "" ? null : raw };
}

async function readJsonBody(request: IncomingMessage): Promise<unknown> {
  const chunks: Buffer[] = [];
  let size = 0;
  for await (const chunk of request) {
    const buffer = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk);
    size += buffer.length;
    if (size > MAX_BODY_BYTES) throw new ScribeError(400, "invalid_input");
    chunks.push(buffer);
  }
  try {
    return JSON.parse(Buffer.concat(chunks).toString("utf8")) as unknown;
  } catch {
    throw new ScribeError(400, "invalid_input");
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

async function answer(response: ServerResponse, label: string, work: () => Promise<unknown>): Promise<void> {
  try {
    send(response, 200, await work());
  } catch (error) {
    if (error instanceof ScribeError) { send(response, error.status, { error: error.code }); return; }
    const code = refusalCode(error);
    if (code !== null) { send(response, 422, { error: code }); return; }
    console.error(`scribe: ${label} failed`, error instanceof Error ? error.name : "unknown");
    send(response, 502, { error: "unavailable" });
  }
}

/** Routes bound to one Orbit client. Tests pass a fake client and clock. */
export function createScribeFeature(client: Client, now: () => Date = () => new Date()): Feature {
  return {
    name: "scribe",
    status: "built",
    routes: [
      { method: "GET", path: "/api/scribe/feed", handle: (_request, response) => answer(response, "feed", () => loadFeed(client, now())) },
      {
        method: "GET", path: "/api/scribe/query",
        handle: (request, response) => answer(response, "query", async () => {
          const { view, about } = viewParams(request);
          return loadView(client, view, about);
        }),
      },
      { method: "POST", path: "/api/scribe/note", handle: (request, response) => answer(response, "note", async () => addNote(client, await readJsonBody(request))) },
      { method: "POST", path: "/api/scribe/ask", handle: (request, response) => answer(response, "ask", async () => ask(client, await readJsonBody(request))) },
    ],
  };
}

export const feature: Feature = createScribeFeature(createOrbitClient());
