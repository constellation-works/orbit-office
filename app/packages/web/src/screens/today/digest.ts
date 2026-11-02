/**
 * Today's morning digest: Decided, Shipped and Needs you since the latest
 * meeting (`GET /api/today/digest`, office.digest with a source), and the
 * open contradictions resolved here in one click (`/api/scribe/contradictions`).
 * Every line keeps its citation. When a piece is not available yet the panel
 * says why instead of showing sample content.
 */
import type { Contradiction, Contradictions, SourceDigest, TodayDigest } from "@orbit-office/contracts";

import { ApiError, getContradictions, getTodayDigest, resolveContradiction } from "../../api/client.js";
import { sourceHref } from "../../api/source-location.js";
import { dotHeading, eyebrow, monoMeta, panel, panelBody, panelColumns, pill } from "../../ui/index.js";

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

function text(tag: "p" | "span" | "div", value: string, className?: string): HTMLElement {
  const element = document.createElement(tag);
  element.textContent = value;
  if (className !== undefined) element.className = className;
  return element;
}

/** A citation such as `src-…#t=00:02:14` as a link to that span; a task id as plain mono text. */
function cite(value: string): HTMLElement {
  const match = /^(src-[a-z0-9-]+)(#.+)?$/.exec(value);
  if (match?.[1] === undefined) return monoMeta(value);
  const link = monoMeta(`${match[1].replace(/^src-\d{8}-/, "")}${match[2] === undefined ? "" : ` ${match[2]}`}`, sourceHref(match[1], match[2] ?? "#source"));
  link.title = value;
  return link;
}

function cites(values: readonly string[]): HTMLElement {
  const line = document.createElement("div");
  line.className = "today-cites";
  for (const value of values) line.append(cite(value));
  return line;
}

function line(main: HTMLElement, ...details: HTMLElement[]): HTMLElement {
  const item = document.createElement("li");
  item.className = "today-digest-line";
  item.append(main, ...details);
  return item;
}

function list(items: readonly HTMLElement[], empty: string): HTMLElement {
  if (items.length === 0) {
    const none = text("p", empty, "today-digest-empty");
    none.hidden = empty === "";
    return none;
  }
  const element = document.createElement("ul");
  element.className = "today-digest-list";
  element.append(...items);
  return element;
}

function external(label: string, url: string | null): HTMLElement {
  if (url === null || !url.startsWith("https://github.com/")) return monoMeta(label);
  const link = monoMeta(label, url) as HTMLAnchorElement;
  link.target = "_blank";
  link.rel = "noopener noreferrer";
  return link;
}

function shippedLine(item: SourceDigest["shipped"][number]): HTMLElement {
  const head = document.createElement("div");
  head.className = "today-digest-head";
  head.append(text("span", item.title, "row-title"), item.merge !== null ? pill("Merged", "done") : pill("Done", "done"));
  const meta = document.createElement("div");
  meta.className = "today-cites";
  if (item.pr !== null) meta.append(external(`PR #${item.pr.number}`, item.pr.url));
  if (item.merge !== null) meta.append(external(`merged ${item.merge.commit.slice(0, 7)}`, item.merge.url));
  meta.append(monoMeta([item.task, item.workspace].filter((part): part is string => part !== null && part !== "").join(" · ")));
  return line(head, meta);
}

const NEED_LABELS = { to_do: "Yours", contradiction: "Contradiction", decision: "Decision" } as const;

function needLine(item: SourceDigest["needs_you"][number]): HTMLElement {
  return line(text("span", item.text, "row-title"), text("span", NEED_LABELS[item.kind], "today-need-kind"), cites(item.cites));
}

function day(at: string | null): string {
  if (at === null || !Number.isFinite(Date.parse(at))) return "";
  return new Date(at).toLocaleDateString(undefined, { day: "numeric", month: "short" });
}

function short(value: string): string {
  return value.length > 42 ? `${value.slice(0, 40).trimEnd()}…` : value;
}

/** One contradiction, resolved with one click: which decision stands. */
function contradictionCallout(item: Contradiction, api: DigestApi, done: () => void): HTMLElement {
  const box = document.createElement("div");
  box.className = "callout";
  box.dataset.key = item.key;
  const newer = day(item.newer.decided_at);
  const prior = day(item.prior.decided_at);
  const held = item.held === 0 ? "" : ` ${item.held === 1 ? "One agent to-do is" : `${item.held} agent to-dos are`} held until you choose.`;
  box.append(eyebrow(`Contradiction · ${item.matter}`),
    text("p", `“${item.newer.text}”${newer ? ` (${newer})` : ""} conflicts with “${item.prior.text}”${prior ? ` (${prior})` : ""}.${held}`));
  const sources = document.createElement("div");
  sources.className = "today-cites";
  sources.append(cite(item.newer.cite), cite(item.prior.cite));
  const choices = document.createElement("div");
  choices.className = "choice-row";
  const status = text("p", "", "callout-done");
  status.hidden = true;
  status.setAttribute("aria-live", "polite");
  const buttons = (["newer", "prior"] as const).map((stands) => {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "choice";
    button.setAttribute("aria-pressed", "false");
    button.textContent = `Keep: ${short(stands === "newer" ? item.newer.text : item.prior.text)}`;
    button.addEventListener("click", () => {
      for (const other of buttons) other.disabled = true;
      button.setAttribute("aria-pressed", "true");
      void api.resolve(item.key, stands).then(() => {
        status.textContent = "Recorded in the ledger. Held agent to-dos are released.";
        status.hidden = false;
        done();
      }).catch((error: unknown) => {
        button.setAttribute("aria-pressed", "false");
        for (const other of buttons) other.disabled = false;
        status.textContent = error instanceof ApiError && error.code === "not_available"
          ? "Resolving contradictions isn’t available yet." : "Couldn’t record your choice. Try again.";
        status.hidden = false;
      });
    });
    return button;
  });
  choices.append(...buttons);
  box.append(sources, choices, status);
  return box;
}

const EMPTY: Record<Exclude<TodayDigest["state"], "ready">, string> = {
  no_meeting: "No meeting has filed to-dos yet. Drop a recording or notes below, and its digest appears here.",
  not_extracted: "This meeting hasn’t been extracted yet. Its decisions and to-dos appear here once it is.",
  unavailable: "The digest since a meeting isn’t available yet.",
};

/** The Morning digest panel. `resolved` rereads after a contradiction is settled. */
export function digestPanel(state: DigestState, api: DigestApi, resolved: () => void): HTMLElement {
  const digest = state.digest;
  const ready = digest !== null && digest.state === "ready" && digest.digest !== null ? digest.digest : null;
  const title = digest?.source === null || digest?.source === undefined ? "Since your last meeting" : `Since ${digest.source.title}`;
  const element = panel(title, { eyebrow: "Morning digest", id: "today-digest-heading", ...(ready === null ? {} : { meta: "Every line cited" }) });
  element.classList.add("today-digest");
  const contradictions = state.contradictions?.contradictions ?? [];
  if (ready === null && contradictions.length === 0) {
    const reason = digest === null ? "Couldn’t read the digest. We’ll try again shortly." : EMPTY[digest.state === "ready" ? "unavailable" : digest.state];
    element.append(panelBody(text("p", reason)));
    return element;
  }
  const decided = panelBody(dotHeading("Decided", "accent"),
    list((ready?.decided ?? []).map((item) => line(text("span", item.text, "row-title"), cites(item.cites))), ready === null ? "Not available yet." : "No decisions recorded."));
  const shipped = panelBody(dotHeading("Shipped", "done"),
    list((ready?.shipped ?? []).map(shippedLine), ready === null ? "Not available yet." : "Nothing has shipped yet."));
  const needs = (ready?.needs_you ?? []).filter((item) => item.kind !== "contradiction" || contradictions.length === 0);
  const needsBody = panelBody(dotHeading("Needs you", "attention"),
    ...contradictions.map((item) => contradictionCallout(item, api, resolved)),
    list(needs.map(needLine), contradictions.length > 0 ? "" : "Nothing from this meeting needs you."));
  needsBody.dataset.tone = "attention";
  element.append(panelColumns(decided, shipped, needsBody));
  if (ready !== null && (ready.unavailable.length > 0 || ready.truncated)) {
    const notes = [
      ready.unavailable.length > 0 ? `Tasks in ${ready.unavailable.join(", ")} could not be read.` : "",
      ready.truncated ? "More tasks came from this meeting than could be read." : "",
    ].filter(Boolean).join(" ");
    element.append(panelBody(text("p", notes)));
  }
  return element;
}

/** One sentence under the greeting, from the digest's counts. */
export function digestSummary(state: DigestState): string | null {
  const ready = state.digest?.state === "ready" ? state.digest.digest : null;
  if (ready === null) return null;
  const needs = ready.needs_you.length;
  return `Since ${ready.title}: ${ready.decided.length} decided, ${ready.shipped.length} shipped, ${needs} ${needs === 1 ? "needs" : "need"} you.`;
}
