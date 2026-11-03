/**
 * Today's digest since the latest meeting: what was Decided and Shipped
 * (`GET /api/today/digest`, office.source-digest), and the open
 * contradictions, each resolved in one click from its Needs-you row
 * (`/api/scribe/contradictions`). Every line keeps its citation. When a piece
 * is not available yet the section says why instead of showing sample content.
 */
import type { Contradiction, ContradictionSide, Contradictions, SourceDigest, TodayDigest } from "@orbit-office/contracts";

import { ApiError, getContradictions, getTodayDigest, resolveContradiction } from "../../api/client.js";
import { sourceHref } from "../../api/source-location.js";
import { monoMeta } from "../../ui/index.js";
import { cite, cites, glyph, listHead, meta, quiet, rowList, text, type Glyph } from "./rows.js";

export interface DigestApi {
  digest(): Promise<TodayDigest>;
  contradictions(): Promise<Contradictions>;
  resolve(key: string, stands: "newer" | "prior"): Promise<unknown>;
}

export const serverDigest: DigestApi = {
  digest: getTodayDigest,
  contradictions: getContradictions,
  resolve: (key, stands) => resolveContradiction({ key, stands }),
};

/** What the digest and contradiction reads returned; either may have failed. */
export interface DigestState {
  digest: TodayDigest | null;
  contradictions: Contradictions | null;
}

export async function readDigest(api: DigestApi): Promise<DigestState> {
  const [digest, contradictions] = await Promise.all([
    api.digest().catch(() => null),
    api.contradictions().catch(() => null),
  ]);
  // Anything but the expected shapes reads as unavailable.
  const states = ["ready", "no_meeting", "not_extracted", "unavailable"];
  return {
    digest: digest !== null && typeof digest === "object" && states.includes(digest.state) &&
      (digest.state !== "ready" || (digest.digest !== null && typeof digest.digest === "object" && Array.isArray(digest.digest.decided) &&
        Array.isArray(digest.digest.shipped) && Array.isArray(digest.digest.needs_you))) ? digest : null,
    contradictions: contradictions !== null && typeof contradictions === "object" && Array.isArray(contradictions.contradictions)
      ? contradictions : null,
  };
}

function line(icon: Glyph, main: HTMLElement, ...trail: HTMLElement[]): HTMLLIElement {
  const item = document.createElement("li");
  item.className = "today-row";
  const row = document.createElement("div");
  row.className = "today-row-line";
  row.append(glyph(icon), main, ...trail);
  item.append(row);
  return item;
}

function external(label: string, url: string | null): HTMLElement {
  const chip = url === null || !url.startsWith("https://github.com/") ? monoMeta(label) : monoMeta(label, url) as HTMLAnchorElement;
  chip.classList.add("today-cite");
  if (chip instanceof HTMLAnchorElement) {
    chip.target = "_blank";
    chip.rel = "noopener noreferrer";
  }
  return chip;
}

function shippedLine(item: SourceDigest["shipped"][number]): HTMLLIElement {
  const refs = document.createElement("div");
  refs.className = "today-cites";
  if (item.pr !== null) refs.append(external(`PR #${item.pr.number}`, item.pr.url));
  if (item.merge !== null) refs.append(external(`merged ${item.merge.commit.slice(0, 7)}`, item.merge.url));
  const where = [item.task, item.workspace].filter((part): part is string => part !== null && part !== "").join(" · ");
  if (where !== "") refs.append(cite(where));
  return line("shipped", text("span", item.title, "today-row-text"), meta(item.merge !== null ? "Merged" : "Done"), refs);
}

const NEED_LABELS = { to_do: "Yours", contradiction: "Contradiction", decision: "Decision" } as const;
const NEED_GLYPHS = { to_do: "todo", contradiction: "contradiction", decision: "approval" } as const;

function needLine(item: SourceDigest["needs_you"][number]): HTMLLIElement {
  return line(NEED_GLYPHS[item.kind], text("span", item.text, "today-row-text"), meta(NEED_LABELS[item.kind], true), cites(item.cites));
}

function day(at: string | null): string {
  if (at === null || !Number.isFinite(Date.parse(at))) return "";
  return new Date(at).toLocaleDateString(undefined, { day: "numeric", month: "short" });
}

/** The matter as the question the user answers: `release-with-open-defects` reads "Which decision on release with open defects stands?". */
function question(matter: string): string {
  return `Which decision on ${matter.replace(/[-_]+/g, " ").trim()} stands?`;
}

function side(choice: ContradictionSide): HTMLElement[] {
  const where = [choice.context ?? "", day(choice.decided_at)].filter(Boolean).join(" · ");
  return [text("span", choice.text), ...(where === "" ? [] : [monoMeta(where)])];
}

/** One open contradiction as the first Needs-you rows: the matter as a question, its two sides as the choices. */
function contradictionRow(item: Contradiction, api: DigestApi, done: () => void): HTMLLIElement {
  const row = line("contradiction", text("span", question(item.matter), "today-row-text"),
    ...(item.held === 0 ? [] : [meta(`holds ${item.held} agent ${item.held === 1 ? "to-do" : "to-dos"}`)]));
  row.classList.add("today-row--contradiction");
  row.dataset.key = item.key;
  row.dataset.kind = "contradiction";
  const choices = document.createElement("div");
  choices.className = "today-choices";
  const status = text("p", "", "today-choice-status");
  status.hidden = true;
  status.setAttribute("aria-live", "polite");
  const buttons = (["newer", "prior"] as const).map((stands) => {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "today-choice";
    button.dataset.stands = stands;
    button.setAttribute("aria-pressed", "false");
    button.append(...side(stands === "newer" ? item.newer : item.prior));
    button.addEventListener("click", () => {
      for (const other of buttons) other.disabled = true;
      button.setAttribute("aria-pressed", "true");
      void api.resolve(item.key, stands).then(() => {
        status.textContent = "Recorded in the ledger. Held agent to-dos are released.";
        delete status.dataset.tone;
        status.hidden = false;
        done();
      }).catch((error: unknown) => {
        button.setAttribute("aria-pressed", "false");
        for (const other of buttons) other.disabled = false;
        status.textContent = error instanceof ApiError && error.code === "not_available"
          ? "Resolving contradictions isn’t available yet." : "Couldn’t record your choice. Try again.";
        status.dataset.tone = "error";
        status.hidden = false;
      });
    });
    return button;
  });
  choices.append(...buttons);
  const sources = cites([item.newer.cite, item.prior.cite]);
  sources.classList.add("today-choice-status");
  row.append(choices, sources, status);
  return row;
}

/** The open contradictions, as Needs-you rows. `resolved` rereads after one is settled. */
export function contradictionRows(state: DigestState, api: DigestApi, resolved: () => void): HTMLLIElement[] {
  return (state.contradictions?.contradictions ?? []).map((item) => contradictionRow(item, api, resolved));
}

const EMPTY: Record<Exclude<TodayDigest["state"], "ready">, string> = {
  no_meeting: "No meeting has filed to-dos yet. Add a recording or notes, and its digest appears here.",
  not_extracted: "This meeting hasn’t been extracted yet. Its decisions and to-dos appear here once it is.",
  unavailable: "The digest since a meeting isn’t available yet.",
};

export const SINCE_HEADING = "today-since-heading";

/**
 * The Since <meeting> section's header bar and rows: decided lines with
 * their citations, shipped lines with their PR and merge, and the meeting's
 * other asks of the user. Contradictions are Needs-you rows instead.
 */
export function sinceContent(state: DigestState): HTMLElement[] {
  const digest = state.digest;
  const ready = digest !== null && digest.state === "ready" && digest.digest !== null ? digest.digest : null;
  const title = digest?.source === null || digest?.source === undefined ? "Since your last meeting" : `Since ${digest.source.title}`;
  const contradictions = state.contradictions?.contradictions ?? [];
  const needs = (ready?.needs_you ?? []).filter((item) => item.kind !== "contradiction" || contradictions.length === 0);
  const items = ready === null ? [] : [
    ...ready.decided.map((item) => line("decided", text("span", item.text, "today-row-text"), cites(item.cites))),
    ...ready.shipped.map(shippedLine),
    ...needs.map(needLine),
  ];
  const head = listHead(title, SINCE_HEADING, ready === null ? null : items.length);
  const source = digest?.source ?? null;
  if (source !== null) {
    const open = document.createElement("a");
    open.className = "today-list-link";
    open.href = sourceHref(source.id);
    open.textContent = "Open meeting";
    head.append(open);
  }
  if (ready === null) {
    return [head, quiet(digest === null ? "Couldn’t read the digest. We’ll try again shortly." : EMPTY[digest.state === "ready" ? "unavailable" : digest.state])];
  }
  const parts: HTMLElement[] = [head];
  if (items.length === 0) parts.push(quiet("Nothing was decided or shipped since this meeting."));
  else {
    const list = rowList();
    list.append(...items);
    parts.push(list);
  }
  if (ready.unavailable.length > 0 || ready.truncated) {
    parts.push(quiet([
      ready.unavailable.length > 0 ? `Tasks in ${ready.unavailable.join(", ")} could not be read.` : "",
      ready.truncated ? "More tasks came from this meeting than could be read." : "",
    ].filter(Boolean).join(" ")));
  }
  return parts;
}
