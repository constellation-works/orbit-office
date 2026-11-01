/**
 * Scribe › Handoffs and Today's handoff summary (specs/web-app.md,
 * "Screens"; specs/sessions.md, "Handoff list"): one read-only
 * `office.session-brief` call with `view: list`. Filters and paging narrow
 * the history only; private sources arrive withheld and stay that way.
 */
import type { IncomingMessage } from "node:http";

import type { HandoffClosing, HandoffFields, ScribeHandoff, ScribeHandoffs } from "@orbit-office/contracts";

import { ScribeError } from "./model.js";

/** History page size the screen asks for. */
export const HISTORY_PAGE = 50;
/** A nonempty single-line filter of at most 256 characters. */
const filterOk = (value: string): boolean =>
  value.trim() !== "" && value.length <= 256 && ![...value].some((char) => char.charCodeAt(0) < 32 || char.charCodeAt(0) === 127);
const ID = /^(.+)#L(\d+)$/;

type Row = Record<string, unknown>;

const record = (value: unknown): Row | null =>
  value !== null && typeof value === "object" && !Array.isArray(value) ? value as Row : null;
const text = (value: unknown): string | null => typeof value === "string" ? value : null;
const strings = (value: unknown): string[] => Array.isArray(value) ? value.filter((item): item is string => typeof item === "string") : [];

export interface HandoffQuery {
  project?: string;
  orchestrator?: string;
  thread?: string;
  offset: number;
}

/** `?project&orchestrator&thread&offset` and nothing else: an unknown key is refused, never ignored. */
export function handoffQuery(request: IncomingMessage): HandoffQuery {
  const url = new URL(request.url ?? "/", "http://localhost");
  const allowed = ["project", "orchestrator", "thread", "offset"];
  if ([...url.searchParams.keys()].some((key) => !allowed.includes(key))) throw new ScribeError(400, "invalid_input");
  const query: HandoffQuery = { offset: 0 };
  for (const key of ["project", "orchestrator", "thread"] as const) {
    const value = url.searchParams.get(key);
    if (value === null || value === "") continue;
    if (!filterOk(value)) throw new ScribeError(400, "invalid_input");
    query[key] = value;
  }
  const offset = url.searchParams.get("offset");
  if (offset !== null && offset !== "") {
    if (!/^\d{1,6}$/.test(offset) || Number(offset) > 100_000) throw new ScribeError(400, "invalid_input");
    query.offset = Number(offset);
  }
  return query;
}

/** The `office.session-brief` input for a list request. */
export function handoffInput(query: HandoffQuery): Row {
  const { offset, ...filters } = query;
  return { view: "list", include_closed: true, limit: HISTORY_PAGE, ...filters, ...(offset > 0 ? { offset } : {}) };
}

function fieldsOf(value: unknown): HandoffFields | null {
  const row = record(value);
  if (row === null) return null;
  const objective = text(row.objective); const next = text(row.next);
  if (objective === null || next === null) return null;
  const observations = (Array.isArray(row.observations) ? row.observations : []).flatMap((item: unknown) => {
    const obs = record(item);
    const [t, at, recheck] = [text(obs?.text), text(obs?.observed_at), text(obs?.recheck)];
    return t !== null && at !== null && recheck !== null ? [{ text: t, observed_at: at, recheck }] : [];
  });
  return { objective, next, authorization: text(row.authorization) ?? "", observations, evidence: strings(row.evidence) };
}

function closingOf(value: unknown): HandoffClosing | null {
  const row = record(value);
  const id = text(row?.id); const time = text(row?.time);
  return row === null || id === null || time === null ? null : { id, text: text(row.text), time };
}

const STATES = ["open", "resolved", "expired"] as const;

/** One listed item; null when malformed. Withheld items never carry text or fields. */
export function handoffOf(value: unknown): ScribeHandoff | null {
  const row = record(value);
  const listing = record(row?.listing);
  if (row === null || listing === null) return null;
  const id = text(row.id); const time = text(row.time);
  const anchor = id === null ? null : ID.exec(id);
  const title = text(listing.source_title);
  const state = STATES.find((name) => name === listing.state);
  if (id === null || time === null || anchor === null || title === null || state === undefined) return null;
  const withheld = listing.withheld === true;
  const session = text(listing.session_id);
  const orchestrator = text(listing.orchestrator);
  // Only Claude Code sessions resume with `claude --resume`.
  const resumable = session !== null && (orchestrator === null || orchestrator === "claude");
  return {
    id, source_id: anchor[1] ?? "", line: Number(anchor[2]), time,
    text: withheld ? "" : text(row.text) ?? "",
    fields: withheld ? null : fieldsOf(row.structured_handoff),
    legacy: !withheld && row.legacy === true, withheld,
    scope: listing.scope === "global" ? "global" : "project",
    thread: text(listing.thread), project: text(listing.project), orchestrator,
    assignment: text(listing.assignment), session_id: session, resume: resumable ? `claude --resume ${session}` : null,
    source_title: title, superseded: listing.superseded === true, state, until: text(listing.until),
    resolved_by: closingOf(listing.resolved_by),
  };
}

/** The three list sections as the screen reads them. */
export function handoffsOf(output: Row): ScribeHandoffs {
  if (!Array.isArray(output.sections)) throw new ScribeError(502, "unavailable");
  const sections = new Map<string, Row>();
  for (const item of output.sections) {
    const section = record(item);
    const kind = text(section?.kind);
    if (section !== null && kind !== null) sections.set(kind, section);
  }
  const items = (kind: string): ScribeHandoff[] => {
    const section = sections.get(kind);
    if (section === undefined || !Array.isArray(section.items)) throw new ScribeError(502, "unavailable");
    return section.items.map(handoffOf).filter((handoff): handoff is ScribeHandoff => handoff !== null);
  };
  const count = (kind: string, key: string): number => {
    const value = sections.get(kind)?.[key];
    return typeof value === "number" ? value : 0;
  };
  return {
    threads: items("threads"), threads_total: count("threads", "total"),
    global: items("global"), global_total: count("global", "total"),
    history: items("history"), history_total: count("history", "total"),
    history_offset: count("history", "offset"), history_more: sections.get("history")?.truncated === true,
  };
}
