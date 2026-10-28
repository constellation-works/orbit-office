/** Library reads use the public Office tools, never the Library filesystem. */
import type { IncomingMessage, ServerResponse } from "node:http";

import type { ShareDocument, ShareResult } from "@orbit-office/contracts";

import type { Feature } from "../../http/types.js";
import { createOrbitClient, OrbitExitError, type OrbitToolClient } from "../../orbit/client.js";

type Client = Pick<OrbitToolClient, "callTool">;

class Refusal extends Error {
  constructor(readonly code: string, readonly detail = "") { super(code); }
}

/** A request the route answers with its own status and short code. */
class LibraryError extends Error {
  constructor(readonly status: number, readonly body: Record<string, unknown>) { super(String(body.error)); }
}

const CODE = /^[a-z][a-z0-9_]{0,63}$/;
const KEBAB = "[a-z0-9]+(?:-[a-z0-9]+)*";
/** A member-qualified shared id (`alice/src-…`) is accepted wherever an own id is. */
const MEMBER = `(?:${KEBAB}/)?`;
const PAGE = new RegExp(`^${MEMBER}(?:dom|rb)-${KEBAB}$`);
const ARTIFACT = new RegExp(`^${MEMBER}art-[0-9]{8}-${KEBAB}$`);
const SOURCE = new RegExp(`^${MEMBER}src-[a-z0-9-]+$`);
/** `office.share-set` takes only the user's own documents. */
const OWN = /^(?:(?:dom|rb)-[a-z0-9]+(?:-[a-z0-9]+)*|art-[0-9]{8}-[a-z0-9]+(?:-[a-z0-9]+)*|src-[a-z0-9-]+)$/;
const MAX_ALSO = 20;

function output(value: unknown): Record<string, unknown> {
  if (value === null || typeof value !== "object" || Array.isArray(value)) throw new Error("invalid output");
  const envelope = value as Record<string, unknown>;
  if (envelope.ok === false) {
    const error = envelope.error !== null && typeof envelope.error === "object" ? envelope.error as Record<string, unknown> : {};
    throw new Refusal(typeof error.code === "string" && CODE.test(error.code) ? error.code : "refused",
      typeof error.message === "string" ? error.message : "");
  }
  const result = envelope.ok === true ? envelope.output : envelope;
  if (result === null || typeof result !== "object" || Array.isArray(result)) throw new Error("invalid output");
  return result as Record<string, unknown>;
}

/** A tool refusal from an exec envelope or Orbit's stderr line, with its message. */
function refusal(error: unknown): Refusal | undefined {
  if (error instanceof Refusal) return error;
  if (!(error instanceof OrbitExitError)) return undefined;
  for (const line of error.stderr.split("\n").reverse()) {
    try {
      const parsed: unknown = JSON.parse(line);
      if (parsed === null || typeof parsed !== "object" || Array.isArray(parsed)) continue;
      const record = parsed as Record<string, unknown>;
      const inner = record.error !== null && typeof record.error === "object" ? record.error as Record<string, unknown> : record;
      if (typeof inner.code === "string") {
        return new Refusal(CODE.test(inner.code) ? inner.code : "refused", typeof inner.message === "string" ? inner.message : "");
      }
    } catch {
      // Not a JSON line.
    }
  }
  return undefined;
}

function showTool(id: string): string | null {
  return PAGE.test(id) ? "office.knowledge-show" : ARTIFACT.test(id) ? "office.artifact-show" : SOURCE.test(id) ? "office.source-show" : null;
}

/** Sources first, then outputs, then pages: each kind may cite the ones before it. */
function rank(id: string): number {
  return id.startsWith("src-") ? 0 : id.startsWith("art-") ? 1 : 2;
}
export function sourcesFirst(ids: readonly string[]): string[] {
  return [...new Set(ids)].sort((a, b) => rank(a) - rank(b) || a.localeCompare(b));
}

/**
 * The ids a `share_cites_unshared` refusal lists. The message is the only
 * carrier ("cites unshared own documents: a, b"); anything that is not an own
 * document id is dropped rather than echoed.
 */
export function unsharedIds(message: string): string[] {
  const list = message.slice(message.lastIndexOf(":") + 1);
  return list.split(",").map((part) => part.trim()).filter((id) => OWN.test(id));
}

async function title(client: Client, id: string): Promise<string> {
  const tool = showTool(id);
  if (tool === null) return "A document in your Library";
  try {
    const data = output(await client.callTool(tool, { id }));
    const item = data.page ?? data.artifact ?? data;
    const value = item !== null && typeof item === "object" ? (item as Record<string, unknown>).title : undefined;
    return typeof value === "string" && value.trim() !== "" ? value : "A document in your Library";
  } catch {
    return "A document in your Library";
  }
}

async function shareOne(client: Client, id: string, shareable: boolean): Promise<void> {
  let data: Record<string, unknown>;
  try {
    data = output(await client.callTool("office.share-set", { id, shareable }));
  } catch (error) {
    const refused = refusal(error);
    if (refused === undefined) throw new LibraryError(502, { error: "library_unavailable" });
    if (refused.code === "share_cites_unshared") {
      const ids = sourcesFirst(unsharedIds(refused.detail)).slice(0, MAX_ALSO);
      const documents: ShareDocument[] = [];
      for (const cited of ids) documents.push({ id: cited, title: await title(client, cited) });
      throw new LibraryError(409, { error: "share_refused", reason: refused.code, documents });
    }
    throw new LibraryError(refused.code === "document_not_found" ? 404 : 409, { error: "share_refused", reason: refused.code });
  }
  if (data.id !== id || data.shareable !== shareable) throw new LibraryError(502, { error: "library_unavailable" });
}

/** `office.share-set` for the listed documents (sources first), then the one asked for. */
export async function share(client: Client, body: unknown): Promise<ShareResult> {
  if (body === null || typeof body !== "object" || Array.isArray(body)) throw new LibraryError(400, { error: "invalid_input" });
  const input = body as Record<string, unknown>;
  if (Object.keys(input).some((key) => !["id", "shareable", "also"].includes(key)) ||
      typeof input.id !== "string" || !OWN.test(input.id) || typeof input.shareable !== "boolean") {
    throw new LibraryError(400, { error: "invalid_input" });
  }
  const also = input.also;
  if (also !== undefined && (!input.shareable || !Array.isArray(also) || also.length > MAX_ALSO ||
      also.some((id: unknown) => typeof id !== "string" || !OWN.test(id) || id === input.id))) {
    throw new LibraryError(400, { error: "invalid_input" });
  }
  for (const id of sourcesFirst((also ?? []) as string[])) await shareOne(client, id, true);
  await shareOne(client, input.id, input.shareable);
  return { shareable: input.shareable };
}

async function readJson(request: IncomingMessage): Promise<unknown> {
  const chunks: Buffer[] = [];
  let size = 0;
  for await (const chunk of request) {
    const data = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk as string);
    size += data.length;
    if (size > 16_384) throw new LibraryError(413, { error: "invalid_input" });
    chunks.push(data);
  }
  try { return JSON.parse(Buffer.concat(chunks).toString("utf8")) as unknown; }
  catch { throw new LibraryError(400, { error: "invalid_input" }); }
}

function send(response: ServerResponse, status: number, body: unknown): void {
  response.writeHead(status, { "content-type": "application/json", "cache-control": "no-store" });
  response.end(JSON.stringify(body));
}

/** Whether a shared library is connected, for the setup step. Null when it can't be read. */
async function sharedSetup(client: Client): Promise<{ configured: boolean } | null> {
  try {
    const status = output(await client.callTool("office.status", {}));
    if (status.shared === null) return { configured: false };
    return status.shared !== undefined && typeof status.shared === "object" ? { configured: true } : null;
  } catch {
    return null;
  }
}

export function createLibraryFeature(client: Client): Feature {
  return { name: "library", status: "built", routes: [{ method: "GET", path: "/api/library", async handle(request, response) {
    try {
      const params = new URL(request.url ?? "/", "http://localhost").searchParams;
      const view = params.get("view") ?? "overview";
      let result: unknown;
      if (view === "overview") {
        const [pages, artifacts, setup, shared] = await Promise.all([
          client.callTool("office.knowledge-list", {}).then(output),
          client.callTool("office.artifact-list", { limit: 100 }).then(output),
          client.callTool("office.bootstrap-status", {}).then(output),
          sharedSetup(client),
        ]);
        if (!Array.isArray(pages.pages) || !Array.isArray(pages.missing_templates) || !Array.isArray(artifacts.artifacts) || typeof artifacts.truncated !== "boolean" || !Array.isArray(setup.phases)) throw new Error("invalid listing");
        result = { pages, artifacts, setup, shared };
      } else if (view === "search") {
        const query = params.get("query")?.trim();
        if (!query) throw new Error("missing query");
        result = output(await client.callTool("office.search", { query, limit: 100 }));
      } else if (view === "outputs") {
        const input: Record<string, unknown> = { limit: 100 };
        const kind = params.get("kind"); const series = params.get("series")?.trim();
        if (kind && ["report", "doc", "draft", "script", "data", "review", "other"].includes(kind)) input.kind = kind;
        if (series) input.series = series;
        result = output(await client.callTool("office.artifact-list", input));
      } else if (view === "document") {
        const id = params.get("id") ?? "";
        const tool = showTool(id);
        if (tool === null) throw new Error("invalid document");
        const anchor = params.get("anchor");
        const input: Record<string, unknown> = { id, history: true };
        if (tool === "office.source-show" && anchor) input.anchor = `#${anchor}`;
        result = output(await client.callTool(tool, input));
      } else throw new Error("invalid view");
      send(response, 200, result);
    } catch {
      send(response, 502, { error: "library_unavailable" });
    }
  } }, { method: "POST", path: "/api/library/share", async handle(request, response) {
    // The user's approval for a document to leave the machine: never retried.
    try { send(response, 200, await share(client, await readJson(request))); }
    catch (error) {
      if (error instanceof LibraryError) send(response, error.status, error.body);
      else send(response, 502, { error: "library_unavailable" });
    }
  } }] };
}
export const feature = createLibraryFeature(createOrbitClient());
