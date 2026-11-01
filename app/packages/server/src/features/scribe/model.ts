/**
 * Pure shaping for the Scribe screen (specs/scribe.md; specs/web-app.md).
 *
 * Every line comes from `office.scribe-query` or `office.scribe-ask`
 * output; nothing here reads the Library. The feed groups lines by
 * session, newest first. A note is parsed exactly as `/scribe note`
 * parses it (plugin/claude-code/scripts/scribe_v1.py `note`). An answer's
 * claims are checked once more against the shortlisted lines the same
 * response carries: a claim whose quote is not on a cited line is dropped.
 */
import type {
  ScribeAnswer, ScribeClaim, ScribeEntry, ScribeFeed, ScribeQueryResult, ScribeSession, ScribeView,
} from "@orbit-office/contracts";

export const VIEWS: readonly ScribeView[] = ["pending", "pref", "landed", "why"];

const TAG = /^[a-z0-9][a-z0-9._-]{0,63}$/;
const NOTE_KEYS = ["resolves", "matter", "due", "refs", "review_by", "supersedes", "topics"] as const;
type NoteKey = typeof NOTE_KEYS[number];
const LEADING_TAG = /^#([a-z0-9][a-z0-9._-]{0,63})(?:\s+|$)([\s\S]*)$/;
const NOTE_KEY = new RegExp(`^(${NOTE_KEYS.join("|")}):\\s*(\\S+)\\s*([\\s\\S]*)$`);
const QUOTED_NOTE = new RegExp(`^"([\\s\\S]*)"((?:\\s+(?:${NOTE_KEYS.join("|")}):\\s*\\S+)*)\\s*$`);
const TRAILING_KEY = new RegExp(`(${NOTE_KEYS.join("|")}):\\s*(\\S+)`, "g");

/** A request the route refuses with a status and a short code. */
export class ScribeError extends Error {
  constructor(readonly status: number, readonly code: string) {
    super(code);
    this.name = "ScribeError";
  }
}

type Row = Record<string, unknown>;

function record(value: unknown): Row | null {
  return value !== null && typeof value === "object" && !Array.isArray(value) ? value as Row : null;
}
const text = (value: unknown): string | null => typeof value === "string" ? value : null;
const strings = (value: unknown): string[] => Array.isArray(value) ? value.filter((item): item is string => typeof item === "string") : [];

/** The tool's output object, or a refusal for a malformed one. */
export function outputOf(value: unknown): Row {
  const row = record(value);
  if (row === null) throw new ScribeError(502, "unavailable");
  return row;
}

/** One `office.scribe-query` result as a screen entry; null when malformed. */
export function entryOf(value: unknown): ScribeEntry | null {
  const row = record(value);
  if (row === null) return null;
  const citation = text(row.citation); const fileLine = text(row.file_line); const time = text(row.time);
  const tag = text(row.tag); const quote = text(row.quote); const session = text(row.session_id);
  if (citation === null || fileLine === null || time === null || tag === null || quote === null || session === null) return null;
  return {
    citation, file_line: fileLine, msg: text(row.msg) ?? "note", time, tag, topics: strings(row.topics), quote,
    why: text(row.why), no_why: row.no_why === true, matter: text(row.matter), review_by: text(row.review_by),
    stale: row.stale === true, refs: strings(row.refs), by: text(row.by), due: text(row.due), session_id: session,
    resume: text(row.resume),
  };
}

function results(output: Row): Row[] {
  if (!Array.isArray(output.results)) throw new ScribeError(502, "unavailable");
  return output.results.map(record).filter((row): row is Row => row !== null);
}

/** Milliseconds for ordering; a line time without seconds parses in every engine. */
function instant(time: string): number {
  const parsed = Date.parse(time);
  return Number.isNaN(parsed) ? 0 : parsed;
}
function line(entry: ScribeEntry): number {
  return Number(entry.file_line.slice(entry.file_line.lastIndexOf(":") + 1)) || 0;
}
const newestFirst = (a: ScribeEntry, b: ScribeEntry): number =>
  instant(b.time) - instant(a.time) || b.time.localeCompare(a.time) || line(b) - line(a);

/** Group a general `office.scribe-query` output into the feed, the most recent session first. */
export function feedOf(output: Row, since: string): ScribeFeed {
  const sessions = new Map<string, ScribeSession>();
  let shown = 0;
  for (const row of results(output)) {
    const entry = entryOf(row);
    if (entry === null) continue;
    shown += 1;
    let session = sessions.get(entry.session_id);
    if (session === undefined) {
      session = {
        session_id: entry.session_id, title: text(row.title), project: text(row.project),
        counterpart_name: text(row.counterpart_name), scribed_through: text(row.scribed_through), latest: entry.time, entries: [],
      };
      sessions.set(entry.session_id, session);
    }
    session.entries.push(entry);
  }
  const list = [...sessions.values()];
  for (const session of list) {
    session.entries.sort(newestFirst);
    session.latest = session.entries[0]?.time ?? session.latest;
  }
  list.sort((a, b) => instant(b.latest) - instant(a.latest) || b.latest.localeCompare(a.latest));
  return {
    sessions: list, shown, total: typeof output.count === "number" ? output.count : shown, since,
    time_zone: text(output.time_zone) ?? "UTC", today: text(output.today) ?? since,
  };
}

/** The `office.scribe-query` input for a canned view. `why` tries `about` as a matter, then as a topic. */
export function viewInput(view: ScribeView, about: string | null, asTopic = false): Row {
  const mode = view;
  if (about === null || view === "pending" || view === "landed") return { mode };
  if (view === "pref") return { mode, matter: about };
  return asTopic ? { mode, topic: about } : { mode, matter: about };
}

/** A canned view's lines, exactly as the tool returned them and in its order. */
export function viewOf(view: ScribeView, output: Row): ScribeQueryResult {
  return {
    view, entries: results(output).map(entryOf).filter((entry): entry is ScribeEntry => entry !== null),
    count: typeof output.count === "number" ? output.count : 0, since: text(output.since), until: text(output.until),
    time_zone: text(output.time_zone) ?? "UTC", today: text(output.today) ?? "",
    files_searched: typeof output.files_searched === "number" ? output.files_searched : 0,
  };
}

/** The `office.scribe-append` entry for a note, parsed as `/scribe note` parses it. */
export function noteEntry(raw: string, fallbackTag: string | undefined): Row {
  const source = raw.trim();
  if (source === "" || source.length > 4000) throw new ScribeError(400, "note_text_required");
  const tagged = LEADING_TAG.exec(source);
  let tag: string; let body: string;
  if (tagged) { tag = tagged[1] ?? ""; body = tagged[2] ?? ""; }
  else {
    if (fallbackTag === undefined || !TAG.test(fallbackTag)) throw new ScribeError(400, "tag_required");
    tag = fallbackTag; body = source;
  }
  const entry: Row = { tag, by: "user" };
  const put = (key: NoteKey, value: string): void => {
    entry[key] = key === "refs" || key === "topics" ? value.split(",").filter(Boolean) : value;
  };
  if (tagged) {
    for (let keyed = NOTE_KEY.exec(body); keyed; keyed = NOTE_KEY.exec(body)) {
      put(keyed[1] as NoteKey, keyed[2] ?? ""); body = keyed[3] ?? "";
    }
    body = body.trim();
    const quoted = QUOTED_NOTE.exec(body);
    if (quoted) {
      body = quoted[1] ?? "";
      for (const match of (quoted[2] ?? "").matchAll(TRAILING_KEY)) put(match[1] as NoteKey, match[2] ?? "");
    }
  }
  if (body === "") throw new ScribeError(400, "note_text_required");
  entry.text = body;
  return entry;
}

/** The newest session cursor among `office.scribe-query` results for one session. */
export function cursorOf(output: Row): string | null {
  for (const row of results(output)) {
    const through = text(row.scribed_through);
    if (through !== null) return through;
  }
  return null;
}

/**
 * Lines the answer may cite: `<SCR id>#L<n>` to the full line text, from the
 * shortlist and evidence. A private line cited by reference only maps to null.
 */
function citable(output: Row): Map<string, string | null> {
  const known = new Map<string, string | null>();
  for (const list of [output.shortlist, output.evidence]) {
    if (!Array.isArray(list)) continue;
    for (const item of list) {
      const row = record(item);
      if (row === null) continue;
      const cite = text(row.citation) ?? text(row.cite);
      if (cite === null) continue;
      const lineText = text(row.text) ?? text(row.line_text);
      if (lineText !== null) known.set(cite, lineText);
      else if (row.content_withheld === true && !known.has(cite)) known.set(cite, null);
    }
  }
  return known;
}

/**
 * Shape an `office.scribe-ask` answer. The tool already drops a claim whose
 * quote is not on its cited line; this repeats the check against the lines
 * the same response shortlisted, so a mismatched quote is never displayed.
 */
export function answerOf(output: Row): ScribeAnswer {
  const known = citable(output);
  const claims: ScribeClaim[] = [];
  let dropped = typeof output.dropped_claims === "number" ? output.dropped_claims : 0;
  for (const item of Array.isArray(output.claims) ? output.claims : []) {
    const row = record(item);
    if (row === null) { dropped += 1; continue; }
    const cites = strings(row.cites);
    const quote = text(row.quote);
    const withheld = row.content_withheld === true;
    const verified = cites.length > 0 && cites.every((cite) => known.has(cite)) && (withheld ? quote === null
      : quote !== null && quote.trim() !== "" && cites.some((cite) => {
        const lineText = known.get(cite);
        return typeof lineText === "string" && (lineText.includes(quote) || lineText.replaceAll("\\n", "\n").includes(quote));
      }));
    if (!verified) { dropped += 1; continue; }
    const session = text(row.session_id);
    claims.push({
      text: withheld ? null : text(row.text), quote: withheld ? null : quote, cites, file_line: text(row.file_line),
      msg: text(row.msg), resume: session === null ? null : `claude --resume ${session}`, withheld,
    });
  }
  const range = record(output.date_range);
  const references = (Array.isArray(output.private_references) ? output.private_references : [])
    .map((item) => text(record(item)?.file_line)).filter((item): item is string => item !== null);
  const status = claims.length > 0 ? "answered" : references.length > 0 ? "reference_only" : "not_captured";
  const derived = record(output.derived_by);
  const model = derived === null ? null : [text(derived.route), text(derived.model)].filter(Boolean).join(" ") || null;
  return {
    status, claims, references, dropped, since: text(output.since) ?? text(range?.since), until: text(output.until) ?? text(range?.until),
    topics: strings(output.topics), files_searched: typeof output.files_searched === "number" ? output.files_searched : 0, model,
  };
}

/** The local date `days - 1` days before `now`, in the server's time zone (YYYY-MM-DD). */
export function windowStart(now: Date, days: number): string {
  const start = new Date(now.getFullYear(), now.getMonth(), now.getDate() - (days - 1));
  const pad = (value: number): string => String(value).padStart(2, "0");
  return `${start.getFullYear()}-${pad(start.getMonth() + 1)}-${pad(start.getDate())}`;
}
