/**
 * Today's digest since the latest meeting: what was Decided and Shipped
 * and what the meeting asks of the user (`GET /api/today/digest`,
 * office.source-digest). Every line keeps its citation. When the digest is
 * not available yet the section says why instead of showing sample content.
 */
import type { SourceDigest, TodayDigest } from "@orbit-office/contracts";

import { getTodayDigest } from "../../api/client.js";
import { sourceHref } from "../../api/source-location.js";
import { monoMeta } from "../../ui/index.js";
import { cite, cites, glyph, listHead, meta, quiet, rowList, text, type Glyph } from "./rows.js";

export interface DigestApi {
  digest(): Promise<TodayDigest>;
}

export const serverDigest: DigestApi = {
  digest: getTodayDigest,
};

/** What the digest read returned; null when it failed. */
export interface DigestState {
  digest: TodayDigest | null;
}

export async function readDigest(api: DigestApi): Promise<DigestState> {
  const digest = await api.digest().catch(() => null);
  // Anything but the expected shapes reads as unavailable.
  const states = ["ready", "no_meeting", "not_extracted", "unavailable"];
  return {
    digest: digest !== null && typeof digest === "object" && states.includes(digest.state) &&
      (digest.state !== "ready" || (digest.digest !== null && typeof digest.digest === "object" && Array.isArray(digest.digest.decided) &&
        Array.isArray(digest.digest.shipped) && Array.isArray(digest.digest.needs_you))) ? digest : null,
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

const EMPTY: Record<Exclude<TodayDigest["state"], "ready">, string> = {
  no_meeting: "No meeting has filed to-dos yet. Add a recording or notes, and its digest appears here.",
  not_extracted: "This meeting hasn’t been extracted yet. Its decisions and to-dos appear here once it is.",
  unavailable: "The digest since a meeting isn’t available yet.",
};

export const SINCE_HEADING = "today-since-heading";

/**
 * The Since <meeting> section's header bar and rows: decided lines with
 * their citations, shipped lines with their PR and merge, and the meeting's
 * other asks of the user.
 */
export function sinceContent(state: DigestState): HTMLElement[] {
  const digest = state.digest;
  const ready = digest !== null && digest.state === "ready" && digest.digest !== null ? digest.digest : null;
  const title = digest?.source === null || digest?.source === undefined ? "Since your last meeting" : `Since ${digest.source.title}`;
  const needs = ready?.needs_you ?? [];
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
