/**
 * Scribe (scribe v2; specs/scribe.md, "Feed" and "Configuration";
 * specs/web-app.md, "Screens"). The feed reads `office.session-brief`
 * with `view: list`: one card per project plus the global card, or one SCR
 * with its `previous`. The Preferences and Decisions panels read
 * `office.scribe-query`. The Tags editor reads the effective tags from
 * `office.status` and saves through `office.config-set`; until Orbit
 * grants the app that tool for the `scribe.*` keys (ORB-20363) a save is
 * `not_permitted` with the operator command, never a claimed success. The
 * ask box calls `office.scribe-ask`, the one low-cost model call. The
 * legacy v1 handoff list stays at `/api/scribe/handoffs` (`legacy: true`)
 * for the compatibility window. Open contradictions are read through
 * `contradictions.ts`, which answers "not available" until ORB-21340 lands.
 * Every call is `orbit tool run`; nothing reads the Library.
 */
import type { IncomingMessage, ServerResponse } from "node:http";

import type { ScrDetail, ScrFeed, ScribeAgreementResponse, ScribeAnswer, ScribeDecisions, ScribeHandoffs, ScribePreferences, ScribeTagSaved, ScribeTags } from "@orbit-office/contracts";

import type { Feature } from "../../http/types.js";
import { createOrbitClient, OrbitExitError, type OrbitToolClient } from "../../orbit/client.js";
import { handoffInput, handoffQuery, handoffsOf, type HandoffQuery } from "./handoffs.js";
import { agreementOf, answerOf, decisionsOf, detailOf, feedOf, outputOf, preferencesOf, ScribeError } from "./model.js";
import { readContradictions, resolveContradiction } from "./contradictions.js";
import { configSetInput, operatorCommand, tagSaveOf, tagsOf } from "./tags.js";

type Client = Pick<OrbitToolClient, "callTool" | "pollTool">;

const MAX_BODY_BYTES = 16_384;
const CODE = /^[a-z][a-z0-9_]{0,63}$/;
const SCR_ID = /^[A-Za-z0-9._-]{1,128}$/;
/** Orbit's refusals of the app's authority; a tag save says `not_permitted`. */
const PERMISSION = new Set(["capability_denied", "policy_denied", "not_permitted"]);

/** A tool refusal code from an exec envelope or Orbit's stderr, if any. */
function refusalCode(error: unknown): string | null {
  if (error instanceof ScribeError) return error.code;
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

/** The cards feed: exactly one card per project plus the global card. */
export async function loadFeed(client: Client): Promise<ScrFeed> {
  return feedOf(outputOf(unwrap(await client.pollTool("office.session-brief", { view: "list" }))));
}

/** One SCR and its `previous`; walking those ids is the only way to older SCRs. */
export async function loadScr(client: Client, id: string): Promise<ScrDetail> {
  return detailOf(outputOf(unwrap(await client.pollTool("office.session-brief", { view: "list", id }))));
}

export async function loadPreferences(client: Client): Promise<ScribePreferences> {
  return preferencesOf(outputOf(unwrap(await client.pollTool("office.scribe-query", { mode: "preferences" }))));
}

export async function loadDecisions(client: Client): Promise<ScribeDecisions> {
  return decisionsOf(outputOf(unwrap(await client.pollTool("office.scribe-query", { mode: "decisions" }))));
}

/** The one Orbit read behind the Agreement view; a later live surface renames only this. */
export const AGREEMENT_TOOL = "orbit.decision.readout";
export const AGREEMENT_CONTRACT = "precedent-decision-readout/1";
/** The app server reads the readout at most this often. */
export const AGREEMENT_POLL_MS = 60_000;

const NO_READOUT_CODES = new Set([
  "tool_not_found", "unknown_tool", "not_found", "readout_not_available",
  "no_readout", "consumer_off", "not_implemented", "unknown_tool_name",
]);

function isNoReadoutError(error: unknown): boolean {
  const code = refusalCode(error);
  if (code !== null && NO_READOUT_CODES.has(code)) return true;
  if (error instanceof OrbitExitError) {
    return /unknown tool|tool not found|no such tool|readout.*not (available|found)/i.test(error.stderr);
  }
  return false;
}

/**
 * The Agreement readout, or null when hidden (consumer off, no readout, or
 * Orbit offers no readout surface yet). Any other failure is a plain
 * `agreement_unavailable` read failure.
 */
export async function loadDecisionAgreement(client: Client): Promise<ScribeAgreementResponse> {
  let raw: unknown;
  try {
    raw = await client.pollTool(AGREEMENT_TOOL, { contract: AGREEMENT_CONTRACT });
  } catch (error) {
    if (isNoReadoutError(error)) return { agreement: null };
    throw new ScribeError(502, "agreement_unavailable");
  }
  if (raw === null || raw === undefined) return { agreement: null };
  const unwrapped = unwrap(raw);
  if (unwrapped === null || unwrapped === undefined) return { agreement: null };
  // A nested readout object is accepted; the flat pinned shape is canonical.
  const row = outputOf(unwrapped);
  const inner = row.readout !== null && typeof row.readout === "object" && !Array.isArray(row.readout)
    ? row.readout as Record<string, unknown> : row;
  return { agreement: agreementOf(inner) };
}

export async function loadHandoffs(client: Client, query: HandoffQuery): Promise<ScribeHandoffs> {
  return handoffsOf(outputOf(unwrap(await client.pollTool("office.session-brief", handoffInput(query)))));
}

/** The effective tags the editor shows. */
export async function loadTags(client: Client): Promise<ScribeTags> {
  return tagsOf(outputOf(unwrap(await client.pollTool("office.status", {}))));
}

/**
 * One editor save through `office.config-set`, then the effective tags read
 * back. Orbit refusing the app's authority is `not_permitted` (403) with the
 * operator command; Office refusing the definition keeps its own code.
 */
export async function saveTags(client: Client, body: unknown): Promise<ScribeTagSaved> {
  const save = tagSaveOf(body);
  try {
    outputOf(unwrap(await client.callTool("office.config-set", configSetInput(save))));
  } catch (error) {
    const code = refusalCode(error);
    if (code !== null && PERMISSION.has(code)) {
      throw new ScribeError(403, "not_permitted", { command: operatorCommand(save) });
    }
    throw error;
  }
  return { saved: true, tags: await loadTags(client) };
}

export async function ask(client: Client, body: unknown): Promise<ScribeAnswer> {
  const question = questionBody(body);
  return answerOf(outputOf(unwrap(await client.callTool("office.scribe-ask", { question }))));
}

function questionBody(body: unknown): string {
  if (body === null || typeof body !== "object" || Array.isArray(body)) throw new ScribeError(400, "invalid_input");
  const row = body as Record<string, unknown>;
  if (Object.keys(row).some((key) => key !== "question")) throw new ScribeError(400, "invalid_input");
  if (typeof row.question !== "string" || row.question.trim() === "" || row.question.length > 4096) throw new ScribeError(400, "question_required");
  return row.question.trim();
}

/** `?id=<SCR id>` and nothing else: an unknown key is refused, never ignored. */
function feedParams(request: IncomingMessage): string | null {
  const url = new URL(request.url ?? "/", "http://localhost");
  if ([...url.searchParams.keys()].some((key) => key !== "id")) throw new ScribeError(400, "invalid_input");
  const id = url.searchParams.get("id");
  if (id === null || id === "") return null;
  if (!SCR_ID.test(id)) throw new ScribeError(400, "invalid_input");
  return id;
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
    if (error instanceof ScribeError) { send(response, error.status, { error: error.code, ...error.extra }); return; }
    const code = refusalCode(error);
    if (code !== null) { send(response, 422, { error: code }); return; }
    console.error(`scribe: ${label} failed`, error instanceof Error ? error.name : "unknown");
    send(response, 502, { error: "unavailable" });
  }
}

/** Routes bound to one Orbit client. Tests pass a fake client. */
export function createScribeFeature(client: Client): Feature {
  // At most one Orbit readout read per AGREEMENT_POLL_MS; failures are not cached.
  let agreementCache: { expiresAt: number; value: ScribeAgreementResponse } | null = null;
  const loadAgreementCached = async (): Promise<ScribeAgreementResponse> => {
    if (agreementCache !== null && Date.now() < agreementCache.expiresAt) return agreementCache.value;
    const value = await loadDecisionAgreement(client);
    agreementCache = { expiresAt: Date.now() + AGREEMENT_POLL_MS, value };
    return value;
  };
  return {
    name: "scribe",
    status: "built",
    routes: [
      {
        method: "GET", path: "/api/scribe/feed",
        handle: (request, response) => answer(response, "feed", async () => {
          const id = feedParams(request);
          return id === null ? loadFeed(client) : loadScr(client, id);
        }),
      },
      { method: "GET", path: "/api/scribe/preferences", handle: (_request, response) => answer(response, "preferences", () => loadPreferences(client)) },
      { method: "GET", path: "/api/scribe/decisions", handle: (_request, response) => answer(response, "decisions", () => loadDecisions(client)) },
      { method: "GET", path: "/api/scribe/decision-agreement", handle: (_request, response) => answer(response, "decision agreement", () => loadAgreementCached()) },
      { method: "GET", path: "/api/scribe/tags", handle: (_request, response) => answer(response, "tags", () => loadTags(client)) },
      { method: "POST", path: "/api/scribe/tags", handle: (request, response) => answer(response, "tag save", async () => saveTags(client, await readJsonBody(request))) },
      {
        method: "GET", path: "/api/scribe/handoffs",
        handle: (request, response) => answer(response, "handoffs", async () => loadHandoffs(client, handoffQuery(request))),
      },
      { method: "GET", path: "/api/scribe/contradictions", handle: (_request, response) => answer(response, "contradictions", () => readContradictions()) },
      {
        method: "POST", path: "/api/scribe/contradictions/resolve",
        handle: (request, response) => answer(response, "contradiction resolve", async () => resolveContradiction(await readJsonBody(request))),
      },
      { method: "POST", path: "/api/scribe/ask", handle: (request, response) => answer(response, "ask", async () => ask(client, await readJsonBody(request))) },
    ],
  };
}

export const feature: Feature = createScribeFeature(createOrbitClient());
