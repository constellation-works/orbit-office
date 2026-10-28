/**
 * The Calendar screen's data: the read-only meeting snapshot beside the
 * routines that run, "Sync now", and notes or a transcript captured from one
 * meeting (docs/design/office/specs/calendar.md, "App"). Every read and write
 * goes through Office and Orbit tools; uploads are staged the way Source
 * stages them, inside the workspace, owner-only, and removed after capture.
 */
import { chmod, mkdtemp, open, realpath, rmdir, unlink } from "node:fs/promises";
import type { IncomingMessage, ServerResponse } from "node:http";
import { join } from "node:path";

import type {
  CalendarAttendee, CalendarMeeting, CalendarRoutine, CalendarSynced, CalendarView, CapturedSource, MeetingOutcome,
} from "@orbit-office/contracts";

import type { Feature, Route } from "../../http/types.js";
import { createOrbitClient, OrbitExitError, type OrbitToolClient } from "../../orbit/client.js";
import { defaultPluginRoots, readDefinitionNames, routineListing, type DefinitionNames } from "../../orbit/definitions.js";

type Row = Record<string, unknown>;

class Refusal extends Error {
  constructor(readonly status: number, readonly code: string) { super(code); }
}

function isRow(value: unknown): value is Row {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}
function row(value: unknown): Row {
  if (!isRow(value)) throw new Refusal(502, "invalid_output");
  return value;
}
function text(value: unknown): string {
  if (typeof value !== "string") throw new Refusal(502, "invalid_output");
  return value;
}
function flag(value: unknown): boolean {
  if (typeof value !== "boolean") throw new Refusal(502, "invalid_output");
  return value;
}
function closed(input: Row, keys: readonly string[]): void {
  if (Object.keys(input).some(key => !keys.includes(key))) throw new Refusal(400, "invalid_input");
}

/** A tool's refusal code, from its envelope or from the failed run's stderr. */
function refusalCode(value: unknown): string | null {
  if (isRow(value) && value.ok === false) return isRow(value.error) && typeof value.error.code === "string" ? value.error.code : "refused";
  return null;
}
function exitCode(error: unknown): string | null {
  if (!(error instanceof OrbitExitError)) return null;
  try {
    const parsed: unknown = JSON.parse(error.stderr.trim());
    if (isRow(parsed) && typeof parsed.code === "string") return parsed.code;
    if (isRow(parsed) && isRow(parsed.error) && typeof parsed.error.code === "string") return parsed.error.code;
  } catch { /* Not a JSON refusal. */ }
  return null;
}

function reply(response: ServerResponse, status: number, value: unknown): void {
  response.writeHead(status, { "content-type": "application/json", "cache-control": "no-store" });
  response.end(JSON.stringify(value));
}
async function jsonBody(request: IncomingMessage, limit: number): Promise<Row> {
  const chunks: Buffer[] = []; let bytes = 0;
  for await (const chunk of request) {
    const part = Buffer.from(chunk as Uint8Array); bytes += part.length;
    if (bytes > limit) throw new Refusal(413, "upload_too_large");
    chunks.push(part);
  }
  try {
    const parsed: unknown = JSON.parse(Buffer.concat(chunks).toString("utf8"));
    if (!isRow(parsed)) throw new Refusal(400, "invalid_input");
    return parsed;
  } catch { throw new Refusal(400, "invalid_input"); }
}

const RFC3339 = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{1,9})?(?:Z|[+-]\d{2}:\d{2})$/;
/** `office.calendar-list` reads at most 42 days at once. */
const MAX_RANGE_MS = 42 * 86_400_000;
const SOURCE_ID = /^src-[a-z0-9]+(?:-[a-z0-9]+)*$/;
/** Notes, transcripts and the audio the meeting inbox transcribes. */
const UPLOADS = /\.(vtt|txt|md|wav|mp3|m4a|webm)$/i;

function instant(value: string | null): string {
  if (value === null || !RFC3339.test(value) || Number.isNaN(Date.parse(value))) throw new Refusal(400, "invalid_input");
  return value;
}
function eventKey(value: unknown): string {
  if (typeof value !== "string" || value.length === 0 || value.length > 1024 || /[\x00-\x1f]/.test(value)) throw new Refusal(400, "invalid_input");
  return value;
}

function person(value: unknown): { id: string; title: string } {
  const item = row(value);
  return { id: text(item.id), title: text(item.title) };
}

/**
 * One snapshot event as the screen shows it. Attendees link to the person
 * page whose title is their name; the rest of the matched pages stay listed.
 */
function meeting(value: unknown): CalendarMeeting {
  const event = row(value);
  const status = event.status;
  if (status !== "confirmed" && status !== "tentative" && status !== "cancelled") throw new Refusal(502, "invalid_output");
  if (event.sensitivity !== "normal" && event.sensitivity !== "private") throw new Refusal(502, "invalid_output");
  if (!Array.isArray(event.attendees) || !Array.isArray(event.people)) throw new Refusal(502, "invalid_output");
  const people = event.people.map(person);
  const linked = new Set<string>();
  const attendees = event.attendees.map((entry: unknown): CalendarAttendee => {
    const attendee = row(entry);
    const name = text(attendee.name);
    const match = people.find(page => !linked.has(page.id) && page.title.trim().toLowerCase() === name.trim().toLowerCase()) ?? null;
    if (match !== null) linked.add(match.id);
    return { name, response: text(attendee.response), person: match };
  });
  const prep = event.prep === null ? null : text(event.prep);
  let capture: CalendarMeeting["capture"] = null;
  if (event.capture !== null) {
    const captured = row(event.capture);
    capture = { source: text(captured.source), summarized: flag(captured.summarized) };
  }
  const start = text(event.start); const end = text(event.end);
  if (Number.isNaN(Date.parse(start)) || Number.isNaN(Date.parse(end))) throw new Refusal(502, "invalid_output");
  return {
    key: text(event.key), title: text(event.title), start, end, all_day: flag(event.all_day), status,
    location: text(event.location), online: flag(event.online), organizer: text(row(event.organizer).name),
    attendees, people: people.filter(page => !linked.has(page.id)), agenda: text(event.agenda),
    private: event.sensitivity === "private", prep, capture,
  };
}

function firedAt(value: unknown): string | null {
  if (!isRow(value) || typeof value.at !== "string") return null;
  if (value.kind !== undefined && value.kind !== "fired") return null;
  return value.at;
}

export interface CalendarOptions {
  client: Pick<OrbitToolClient, "callTool">;
  /** Must be within the workspace granted to the plugin, never a browser path. */
  workspaceRoot?: string;
  maxUploadBytes?: number;
  pluginRoots?: readonly string[];
  /**
   * Minutes before a meeting that `office-meeting-prep` writes its brief. No
   * tool reads the configured `calendar.prep_lead_minutes` back yet, so this
   * is the configuration's default unless the composer passes another.
   */
  prepLeadMinutes?: number;
}

export function createCalendarFeature(options: CalendarOptions): Feature {
  const { client } = options;
  let names: DefinitionNames | undefined;
  try { names = readDefinitionNames(options.pluginRoots ?? defaultPluginRoots()); }
  catch { /* Without the shipped tree the calendar still shows meetings, with no routine marks. */ }

  async function call(name: string, input: Row): Promise<Row> {
    let value: unknown;
    try { value = await client.callTool(name, input); }
    catch (error) {
      const code = exitCode(error);
      if (code !== null) throw new Refusal(409, code);
      throw error;
    }
    const code = refusalCode(value);
    if (code !== null) throw new Refusal(409, code);
    return row(isRow(value) && value.ok === true ? value.output : value);
  }

  /** Routines that run, the same set the Routines screen lists. A failed read leaves the meetings shown. */
  async function routines(): Promise<{ routines: CalendarRoutine[]; unavailable: boolean }> {
    if (names === undefined) return { routines: [], unavailable: true };
    const shipped = names;
    const result: CalendarRoutine[] = [];
    let unavailable = false;
    const [autos, jobs] = await Promise.allSettled([client.callTool("orbit.auto_task.list", {}), routineListing(client)]);
    if (autos.status === "fulfilled" && Array.isArray(autos.value)) {
      for (const entry of autos.value) {
        if (!isRow(entry) || typeof entry.name !== "string" || typeof entry.effective_enabled !== "boolean" || !isRow(entry.schedule)) { unavailable = true; continue; }
        if (!shipped.auto_tasks.has(entry.name)) continue;
        result.push({ kind: "auto_task", name: entry.name, schedule: entry.schedule, enabled: entry.effective_enabled, last_fired_at: firedAt(entry.last_fire) });
      }
    } else unavailable = true;
    if (jobs.status === "fulfilled") {
      for (const entry of jobs.value) {
        if (!shipped.routines.has(String(entry.name)) || !isRow(entry.schedule)) continue;
        result.push({ kind: "routine", name: String(entry.name), schedule: entry.schedule, enabled: entry.effective === true, last_fired_at: firedAt(entry.last_fire) });
      }
    } else unavailable = true;
    return { routines: result, unavailable };
  }

  async function capture(input: Row): Promise<CapturedSource> {
    try {
      const data = await call("office.capture", input);
      const id = text(data.id);
      if (!SOURCE_ID.test(id)) throw new Refusal(502, "invalid_output");
      return { id, kind: text(data.kind), title: text(data.title) };
    } catch (error) {
      if (error instanceof Refusal && error.code === "unknown_event") throw new Refusal(404, "unknown_event");
      if (error instanceof Refusal && error.status === 409) throw new Refusal(409, "capture_refused");
      throw error;
    }
  }

  async function upload(request: IncomingMessage, params: URLSearchParams): Promise<CapturedSource> {
    closed(Object.fromEntries(params), ["event_key", "name"]);
    const key = eventKey(params.get("event_key"));
    const name = params.get("name") ?? "";
    if (!name || name.length > 200 || /[\\/\x00-\x1f]/.test(name) || name.startsWith(".") || !UPLOADS.test(name)) throw new Refusal(400, "unsupported_file");
    const root = options.workspaceRoot ?? process.env.ORBIT_OFFICE_WORKSPACE;
    if (!root) throw new Refusal(503, "upload_root_unconfigured");
    const dir = await mkdtemp(join(await realpath(root), ".office-upload-"));
    const path = join(dir, name);
    let file; let captured: CapturedSource | undefined; let failure: unknown; let cleanupFailed = false;
    try {
      await chmod(dir, 0o700);
      file = await open(path, "wx", 0o600); let bytes = 0;
      for await (const chunk of request) {
        const part = Buffer.from(chunk as Uint8Array); bytes += part.length;
        if (bytes > (options.maxUploadBytes ?? 256 * 1024 * 1024)) throw new Refusal(413, "upload_too_large");
        await file.writeFile(part);
      }
      await file.close(); file = undefined;
      if (bytes === 0) throw new Refusal(400, "invalid_input");
      captured = await capture({ event_key: key, upload_path: path, captured_by: "upload" });
    } catch (error) { failure = error; }
    finally {
      if (file) await file.close().catch(() => { cleanupFailed = true; });
      await unlink(path).catch((error: unknown) => { if ((error as NodeJS.ErrnoException).code !== "ENOENT") cleanupFailed = true; });
      await rmdir(dir).catch(() => { cleanupFailed = true; });
    }
    if (cleanupFailed) throw new Refusal(502, "upload_cleanup_failed");
    if (failure !== undefined) throw failure;
    if (captured === undefined) throw new Refusal(502, "calendar_unavailable");
    return captured;
  }

  function route(method: Route["method"], path: string, handle: Route["handle"]): Route {
    return { method, path, async handle(request, response) {
      try { request.setTimeout(30_000, () => { request.destroy(); }); await handle(request, response); }
      catch (error) {
        if (response.headersSent) { response.end(); return; }
        reply(response, error instanceof Refusal ? error.status : 502, { error: error instanceof Refusal ? error.code : "calendar_unavailable" });
      }
    } };
  }

  return { name: "calendar", status: "built", routes: [
    route("GET", "/api/calendar", async (request, response) => {
      const params = new URL(request.url ?? "/", "http://localhost").searchParams;
      closed(Object.fromEntries(params), ["from", "until"]);
      const from = instant(params.get("from")); const until = instant(params.get("until"));
      const span = Date.parse(until) - Date.parse(from);
      if (span <= 0 || span > MAX_RANGE_MS) throw new Refusal(400, "invalid_input");
      const [listing, schedule] = await Promise.all([
        call("office.calendar-list", { from, until, include_cancelled: true }).catch((error: unknown) => {
          throw error instanceof Refusal && error.status === 409 ? new Refusal(502, "calendar_unavailable") : error;
        }),
        routines(),
      ]);
      if (!Array.isArray(listing.events)) throw new Refusal(502, "invalid_output");
      const synced = listing.synced_at === null ? null : text(listing.synced_at);
      if (listing.last_error !== null) text(listing.last_error);
      const view: CalendarView = {
        from, until, synced_at: synced, sync_failed: listing.last_error !== null, prep_lead_minutes: options.prepLeadMinutes ?? 120,
        meetings: listing.events.map(meeting).sort((a, b) => Date.parse(a.start) - Date.parse(b.start)),
        routines: schedule.routines, routines_unavailable: schedule.unavailable,
      };
      reply(response, 200, view);
    }),
    route("POST", "/api/calendar/sync", async (request, response) => {
      if (request.headers["content-type"]?.split(";")[0] !== "application/json") throw new Refusal(415, "invalid_input");
      closed(await jsonBody(request, 1024), []);
      let data: Row;
      try { data = await call("office.calendar-sync", {}); }
      catch (error) {
        if (error instanceof Refusal && error.code === "calendar_unconfigured") throw new Refusal(409, "calendar_unconfigured");
        if (error instanceof Refusal && error.status === 409) throw new Refusal(502, "sync_failed");
        throw error;
      }
      const events = data.events; const skipped = data.skipped;
      if (typeof events !== "number" || typeof skipped !== "number") throw new Refusal(502, "invalid_output");
      const synced: CalendarSynced = { events, skipped, synced_at: text(data.synced_at) };
      reply(response, 200, synced);
    }),
    route("POST", "/api/calendar/capture", async (request, response) => {
      const params = new URL(request.url ?? "/", "http://localhost").searchParams;
      const type = request.headers["content-type"]?.split(";")[0];
      if (type === "application/octet-stream") { reply(response, 200, await upload(request, params)); return; }
      if (type !== "application/json" || [...params.keys()].length > 0) throw new Refusal(415, "invalid_input");
      const input = await jsonBody(request, 1024 * 1024);
      closed(input, ["event_key", "text"]);
      if (typeof input.text !== "string" || !input.text.trim()) throw new Refusal(400, "invalid_input");
      reply(response, 200, await capture({ event_key: eventKey(input.event_key), text: input.text, captured_by: "paste" }));
    }),
    route("GET", "/api/calendar/meeting", async (request, response) => {
      const params = new URL(request.url ?? "/", "http://localhost").searchParams;
      closed(Object.fromEntries(params), ["source"]);
      const id = params.get("source") ?? "";
      if (!SOURCE_ID.test(id)) throw new Refusal(400, "invalid_input");
      const [source, tasks] = await Promise.all([
        call("office.source-show", { id }),
        client.callTool("orbit.task.list", { tag: `source:${id}`, fields: ["title", "status", "tags"], limit: 50 }),
      ]);
      const summary = source.summary === undefined || source.summary === null ? null : text(source.summary);
      const list = Array.isArray(tasks) ? { tasks, truncated: false } : row(tasks);
      if (!Array.isArray(list.tasks)) throw new Refusal(502, "invalid_output");
      const outcome: MeetingOutcome = {
        summary,
        todos: list.tasks.map((entry: unknown) => {
          const task = row(entry);
          const tags = Array.isArray(task.tags) ? task.tags : [];
          const owner = tags.includes("owner:you") ? "you" : tags.includes("owner:agent") ? "agent" : null;
          return { title: text(task.title), owner, done: task.status === "done" };
        }),
        more: list.truncated === true,
      };
      reply(response, 200, outcome);
    }),
  ] };
}

export const feature: Feature = createCalendarFeature({ client: createOrbitClient() });
