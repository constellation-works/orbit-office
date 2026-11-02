/**
 * Pure shaping for the Scribe screen (specs/scribe.md; specs/web-app.md).
 *
 * The feed reads `office.session-brief` with `view: list` (SCR cards: one
 * per project plus the global card, or one SCR with its `previous`); the
 * Preferences and Decisions panels read `office.scribe-query`; an answer's
 * claims are checked once more against the shortlisted lines the same
 * response carries: a claim whose quote is not on a cited line is dropped.
 * Nothing here reads the Library.
 */
import type {
  ScrCard, ScrDetail, ScrFeed, ScrTagEntry, ScribeAgreementCategory, ScribeAnswer, ScribeClaim, ScribeDecisionAgreement, ScribeDecisions, ScribePreferences,
} from "@orbit-office/contracts";

/** A request the route refuses with a status and a short code. */
export class ScribeError extends Error {
  constructor(readonly status: number, readonly code: string, readonly extra: Record<string, unknown> = {}) {
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
const count = (value: unknown): number => typeof value === "number" && Number.isFinite(value) && value >= 0 ? value : 0;

/** The tool's output object, or a refusal for a malformed one. */
export function outputOf(value: unknown): Row {
  const row = record(value);
  if (row === null) throw new ScribeError(502, "unavailable");
  return row;
}

function tagEntryOf(value: unknown): ScrTagEntry | null {
  const row = record(value);
  if (row === null) return null;
  const id = text(row.id); const citation = text(row.citation); const group = text(row.group);
  const msg = text(row.msg); const time = text(row.time); const by = text(row.by);
  const scope = text(row.scope); const quote = text(row.quote);
  if (id === null || citation === null || group === null || msg === null || time === null || by === null || scope === null || quote === null) return null;
  const fields: Record<string, string> = {};
  const raw = record(row.fields) ?? {};
  for (const [key, item] of Object.entries(raw)) if (typeof item === "string") fields[key] = item;
  return { id, citation, group, msg, time, by, scope, quote, fields };
}

/** One SCR card from a `view: list` answer; null when malformed. A withheld card never carries content. */
export function cardOf(value: unknown): ScrCard | null {
  const row = record(value);
  if (row === null) return null;
  const id = text(row.id); const path = text(row.path); const scope = text(row.scope);
  const session = text(row.session); const started = text(row.started); const updated = text(row.updated);
  if (id === null || path === null || scope === null || session === null || started === null || updated === null) return null;
  const withheld = row.content_withheld === true;
  const tagCounts: Record<string, number> = {};
  if (!withheld) {
    for (const [key, item] of Object.entries(record(row.tag_counts) ?? {})) {
      if (typeof item === "number" && Number.isFinite(item) && item >= 0) tagCounts[key] = item;
    }
  }
  return {
    id, path, title: text(row.title), title_by: text(row.title_by), scope, project: text(row.project),
    session, started, updated, previous: text(row.previous), scribed_through: text(row.scribed_through),
    summary: withheld ? null : text(row.summary),
    goals: withheld ? [] : strings(row.goals), outcome: withheld ? [] : strings(row.outcome),
    next: withheld ? [] : strings(row.next), tag_entries: count(row.tag_entries), tag_counts: tagCounts,
    tags: withheld ? [] : (Array.isArray(row.tags) ? row.tags : []).map(tagEntryOf).filter((entry): entry is ScrTagEntry => entry !== null),
    withheld,
  };
}

/** The cards feed: exactly what the tool returned, in its order (global first, then projects). */
export function feedOf(output: Row): ScrFeed {
  if (!Array.isArray(output.cards)) throw new ScribeError(502, "unavailable");
  const cards = output.cards.map(cardOf).filter((card): card is ScrCard => card !== null);
  if (cards.length !== output.cards.length) throw new ScribeError(502, "unavailable");
  return { cards, count: count(output.count), files_searched: count(output.files_searched) };
}

/** One SCR and its `previous`, for walking the chain. */
export function detailOf(output: Row): ScrDetail {
  const scr = cardOf(output.scr);
  if (scr === null) throw new ScribeError(502, "unavailable");
  const previous = output.previous === null || output.previous === undefined ? null : cardOf(output.previous);
  if (output.previous !== null && output.previous !== undefined && previous === null) throw new ScribeError(502, "unavailable");
  return { scr, previous };
}

/** Preference page sections from `office.scribe-query` mode `preferences`, in its order. */
export function preferencesOf(output: Row): ScribePreferences {
  if (!Array.isArray(output.preferences)) throw new ScribeError(502, "unavailable");
  const preferences = output.preferences.flatMap((item: unknown) => {
    const row = record(item);
    if (row === null) return [];
    const page = text(row.page); const scope = text(row.scope); const matter = text(row.matter);
    const citation = text(row.citation); const statement = text(row.statement);
    const statedAt = text(row.stated_at); const reviewBy = text(row.review_by);
    if (page === null || scope === null || matter === null || citation === null || statement === null || statedAt === null || reviewBy === null) return [];
    const proposed = (Array.isArray(row.proposed) ? row.proposed : []).flatMap((entry: unknown) => {
      const proposalRow = record(entry);
      const entryId = text(proposalRow?.entry); const proposal = text(proposalRow?.proposal);
      return entryId !== null && proposal !== null ? [{ entry: entryId, proposal }] : [];
    });
    return [{ page, scope, matter, citation, statement, cites: strings(row.cites), stated_at: statedAt, review_by: reviewBy, stale: row.stale === true, proposed }];
  });
  return { preferences, count: count(output.count), today: text(output.today) ?? "" };
}

/** Ledger entries from `office.scribe-query` mode `decisions`, newest decision first for reading. */
export function decisionsOf(output: Row): ScribeDecisions {
  if (!Array.isArray(output.decisions)) throw new ScribeError(502, "unavailable");
  const decisions = output.decisions.flatMap((item: unknown) => {
    const row = record(item);
    if (row === null) return [];
    const id = text(row.id); const ledger = text(row.ledger); const scope = text(row.scope);
    const decidedBy = text(row.decided_by); const decidedAt = text(row.decided_at); const source = text(row.source);
    if (id === null || ledger === null || scope === null || decidedBy === null || decidedAt === null || source === null) return [];
    const because = record(row.because);
    return [{
      id, ledger, line: count(row.line), question: text(row.question), chosen: text(row.chosen),
      quote: text(row.quote), because: because === null ? null : text(because.text),
      scope, decided_by: decidedBy, decided_at: decidedAt, reverses: text(row.reverses),
      reversed_by: text(row.reversed_by), current: row.current === true, source, private: row.private === true,
    }];
  });
  decisions.sort((a, b) => b.decided_at.localeCompare(a.decided_at) || b.id.localeCompare(a.id));
  return { decisions, count: count(output.count) };
}

const fraction = (value: unknown): number | null =>
  typeof value === "number" && Number.isFinite(value) && value >= 0 && value <= 1 ? value : null;
const positiveCount = (value: unknown): number | null =>
  typeof value === "number" && Number.isInteger(value) && value > 0 ? value : null;

/** Hidden marker: the readout tool answered that there is nothing to show. */
export function isNoReadout(output: Row): boolean {
  if (output.available === false || output.readout === null) return true;
  if (output.consumer_mode === "off" || output.mode === "off") return true;
  return output.contract === undefined && output.categories === undefined;
}

/**
 * Shape a `precedent-decision-readout/1` readout. Only the pinned aggregate
 * fields are forwarded; any per-request field is dropped here and never
 * reaches the browser. Returns null when hidden (consumer off / no readout);
 * a malformed readout is a read failure (`agreement_unavailable`).
 */
export function agreementOf(output: Row): ScribeDecisionAgreement | null {
  if (isNoReadout(output)) return null;
  if (output.contract !== "precedent-decision-readout/1") throw new ScribeError(502, "agreement_unavailable");
  const consumerMode = output.consumer_mode;
  if (consumerMode !== "shadow" && consumerMode !== "enabled") throw new ScribeError(502, "agreement_unavailable");
  if (!Array.isArray(output.categories)) throw new ScribeError(502, "agreement_unavailable");
  const fallbackMin = positiveCount(output.min_samples) ?? 30;
  const window = record(output.window);
  const categories: ScribeAgreementCategory[] = [];
  for (const item of output.categories) {
    const row = record(item);
    if (row === null) throw new ScribeError(502, "agreement_unavailable");
    const category = text(row.category);
    if (category === null || category === "") throw new ScribeError(502, "agreement_unavailable");
    if (row.mode !== "shadow" && row.mode !== "enabled") throw new ScribeError(502, "agreement_unavailable");
    const scored = row.scored_requests;
    if (typeof scored !== "number" || !Number.isInteger(scored) || scored < 0) throw new ScribeError(502, "agreement_unavailable");
    const [workspacePart, ...kindParts] = category.split(":");
    categories.push({
      category,
      workspace: text(row.workspace) ?? (kindParts.length ? workspacePart ?? null : null),
      kind: text(row.kind) ?? (kindParts.length ? kindParts.join(":") : category),
      mode: row.mode,
      scored_requests: scored,
      accuracy: fraction(row.accuracy),
      accuracy_wilson_low: fraction(row.accuracy_wilson_low),
      accuracy_wilson_high: fraction(row.accuracy_wilson_high),
      coverage: fraction(row.coverage),
      calibration_error: fraction(row.calibration_error),
      abstention_rate: fraction(row.abstention_rate),
      conflict_rate: fraction(row.conflict_rate),
      min_samples: positiveCount(row.min_samples) ?? fallbackMin,
    });
  }
  return {
    contract: "precedent-decision-readout/1",
    consumer_mode: consumerMode,
    generated_at: text(output.generated_at),
    window_start: window === null ? null : text(window.start),
    window_end: window === null ? null : text(window.end),
    categories,
  };
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
