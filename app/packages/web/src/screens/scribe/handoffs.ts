/**
 * Scribe › Handoffs (specs/web-app.md, "Screens"; specs/sessions.md,
 * "Handoff list"): read-only views over `GET /api/scribe/handoffs`.
 * Current groups each open thread's newest handoff by project; Global lists
 * open global handoffs, with a toggle for expired and resolved ones; History
 * lists every handoff of the filtered threads, newest first, a page at a time.
 */
import type { ScribeHandoff, ScribeHandoffs } from "@orbit-office/contracts";

import { getJson } from "../../api/client.js";
import { sourceHref } from "../../api/source-location.js";
import { ageLabel, byProject, taskHref, threadLabel } from "../../ui/handoff.js";
import { pill } from "../../ui/index.js";
import { el, resumeRow } from "./dom.js";
import { timeLabel } from "./model.js";

type Sub = "current" | "global" | "history";
const SUBS: ReadonlyArray<[Sub, string]> = [["current", "Current"], ["global", "Global"], ["history", "History"]];
type Filter = "project" | "orchestrator" | "thread";
/** The server's history page size. */
const HISTORY_PAGE = 50;
const FILTERS: ReadonlyArray<[Filter, string]> = [["project", "Project"], ["orchestrator", "Orchestrator"], ["thread", "Thread"]];

const sourceLink = (handoff: { id: string }): HTMLAnchorElement => {
  const [source = "", line = ""] = handoff.id.split("#L");
  const link = el("a", handoff.id);
  link.href = sourceHref(source, `#L${line}`);
  link.title = "Conversation source line";
  return link;
};

function fields(handoff: ScribeHandoff): HTMLElement {
  const h = handoff.fields;
  if (h === null) return el("blockquote", handoff.text, "scribe-quote");
  const list = el("dl", "", "scribe-fields handoff-fields");
  const items = (values: HTMLElement[]): HTMLElement => {
    if (values.length === 0) return el("dd", "None recorded.");
    const dd = el("dd"); const ul = el("ul", "", "handoff-items");
    for (const value of values) { const li = el("li"); li.append(value); ul.append(li); }
    dd.append(ul);
    return dd;
  };
  const evidence = h.evidence.map((ref) => {
    if (!/^(https?|orbit):\/\//.test(ref)) return el("span", ref);
    const link = el("a", ref); link.href = ref; link.rel = "noreferrer"; return link;
  });
  const observations = h.observations.map((o) => {
    const span = el("span", `${o.text} `);
    const at = el("time", `observed at ${timeLabel(o.observed_at)}`); at.dateTime = o.observed_at;
    span.append(at, document.createTextNode(`; recheck: ${o.recheck}`));
    return span;
  });
  list.append(
    el("dt", "Objective"), el("dd", h.objective),
    el("dt", "Next"), el("dd", h.next),
    el("dt", "Authorization"), el("dd", h.authorization || "Not recorded."),
    el("dt", "Observations"), items(observations),
    el("dt", "Evidence"), items(evidence),
  );
  return list;
}

/** One handoff card: time and age, orchestrator, linked assignment, state; its fields or text; source line and resume. */
export function handoffItem(handoff: ScribeHandoff, now: Date): HTMLLIElement {
  const item = el("li", "", "scribe-entry handoff");
  item.dataset.id = handoff.id;
  const head = el("div", "", "scribe-entry-head");
  const time = el("time", `${timeLabel(handoff.time)} · ${ageLabel(handoff.time, now)}`);
  time.dateTime = handoff.time;
  head.append(time);
  if (handoff.orchestrator) { const chip = pill(handoff.orchestrator); chip.classList.add("scribe-chip"); head.append(chip); }
  if (handoff.assignment) {
    const link = el("a", handoff.assignment, "handoff-task");
    link.href = taskHref(handoff.assignment); link.title = "Orbit task";
    head.append(link);
  }
  if (handoff.scope === "global") head.append(pill("Global"));
  if (handoff.superseded) head.append(pill("Superseded"));
  if (handoff.state === "resolved") head.append(pill("Resolved", "done"));
  if (handoff.state === "expired") head.append(pill("Expired"));
  if (handoff.until) head.append(pill(`Until ${timeLabel(handoff.until)}`, handoff.state === "open" ? "attention" : "neutral"));
  if (handoff.legacy) head.append(pill("Legacy"));
  if (handoff.withheld) head.append(pill("Private", "private"));
  item.append(head, handoff.withheld ? el("p", "Private session: content withheld.", "scribe-status") : fields(handoff));
  if (handoff.resolved_by) {
    const closed = el("p", "Resolved by ", "handoff-closed");
    const [source = "", line = ""] = handoff.resolved_by.id.split("#L");
    const link = el("a", handoff.resolved_by.id); link.href = sourceHref(source, `#L${line}`);
    closed.append(link, document.createTextNode(` at ${timeLabel(handoff.resolved_by.time)}${handoff.resolved_by.text === null ? " (private)" : `: ${handoff.resolved_by.text}`}`));
    item.append(closed);
  }
  const cite = el("div", "", "scribe-cite");
  cite.append(sourceLink(handoff));
  if (handoff.source_title) cite.append(el("span", handoff.source_title));
  if (handoff.resume) cite.append(...resumeRow(handoff.resume));
  item.append(cite);
  return item;
}

function list(handoffs: readonly ScribeHandoff[], now: Date): HTMLUListElement {
  const ul = el("ul", "", "scribe-list");
  for (const handoff of handoffs) ul.append(handoffItem(handoff, now));
  return ul;
}

/** The Handoffs tab: its sub-view bar (kept across polls) and a loader that draws into the shared panel. */
export function createHandoffsView(pollMs: number): {
  bar: HTMLElement;
  load: () => Promise<(panel: HTMLElement, stated: HTMLElement) => void>;
  onChange: (listener: () => void) => void;
} {
  let sub: Sub = "current";
  let showClosed = false;
  let offset = 0;
  const chosen: Record<Filter, string> = { project: "", orchestrator: "", thread: "" };
  let changed = (): void => {};

  const bar = el("div", "", "handoff-bar");
  const group = el("div", "", "scribe-tabs handoff-subs");
  group.setAttribute("role", "group");
  group.setAttribute("aria-label", "Handoff views");
  const subButtons = new Map<Sub, HTMLButtonElement>();
  for (const [name, title] of SUBS) {
    const button = el("button", title); button.type = "button";
    button.addEventListener("click", () => { sub = name; draw(); changed(); });
    subButtons.set(name, button); group.append(button);
  }
  const toggle = el("input"); toggle.type = "checkbox";
  const toggleLabel = el("label", "", "handoff-toggle");
  toggleLabel.append(toggle, document.createTextNode(" Show expired and resolved"));
  toggle.addEventListener("change", () => { showClosed = toggle.checked; changed(); });
  const filterRow = el("div", "", "scribe-row handoff-filters");
  const selects = new Map<Filter, HTMLSelectElement>();
  for (const [name, title] of FILTERS) {
    const select = el("select");
    const label = el("label", title);
    label.append(select);
    select.addEventListener("change", () => { chosen[name] = select.value; offset = 0; changed(); });
    selects.set(name, select); filterRow.append(label);
  }
  bar.append(group, toggleLabel, filterRow);
  const draw = (): void => {
    for (const [name, button] of subButtons) button.setAttribute("aria-pressed", String(name === sub));
    toggleLabel.hidden = sub !== "global";
    filterRow.hidden = sub !== "history";
  };
  draw();

  /** Options from every value the list shows, keeping the chosen one. */
  const fillFilters = (data: ScribeHandoffs): void => {
    const all = [...data.threads, ...data.history];
    const values: Record<Filter, Set<string>> = {
      project: new Set(all.flatMap((h) => h.project ? [h.project] : [])),
      orchestrator: new Set(all.flatMap((h) => h.orchestrator ? [h.orchestrator] : [])),
      thread: new Set(all.flatMap((h) => h.thread ? [h.thread] : [])),
    };
    for (const [name] of FILTERS) {
      const select = selects.get(name);
      if (!select) continue;
      if (chosen[name]) values[name].add(chosen[name]);
      const any = el("option", "All"); any.value = "";
      const options = [...values[name]].sort().map((value) => {
        const option = el("option", name === "thread" ? threadLabel(value) : value); option.value = value; return option;
      });
      select.replaceChildren(any, ...options);
      select.value = chosen[name];
    }
  };

  const load = async (): Promise<(panel: HTMLElement, stated: HTMLElement) => void> => {
    const query = new URLSearchParams();
    for (const [name] of FILTERS) if (chosen[name]) query.set(name, chosen[name]);
    if (offset > 0) query.set("offset", String(offset));
    const search = query.toString();
    const data = await getJson<ScribeHandoffs>(`/api/scribe/handoffs${search ? `?${search}` : ""}`);
    return (panel, stated) => {
      const now = new Date();
      fillFilters(data);
      const every = `Refreshed every ${pollMs / 1000} seconds.`;
      if (sub === "current") {
        stated.textContent = `${data.threads.length} open ${data.threads.length === 1 ? "thread" : "threads"}, newest handoff each, grouped by project. ${every}`;
        panel.replaceChildren(...(data.threads.length === 0 ? [el("p", "No open thread has a handoff yet.", "scribe-status")]
          : byProject(data.threads).map((group, index) => {
            const block = el("section", "", "scribe-session");
            block.setAttribute("aria-label", group.project);
            const header = el("header");
            const meta = el("p", `${group.handoffs.length} ${group.handoffs.length === 1 ? "thread" : "threads"}`, "scribe-meta");
            if (index === 0) meta.prepend(pill("Most recent"), document.createTextNode(" "));
            header.append(el("h3", group.project), meta);
            block.append(header, list(group.handoffs, now));
            return block;
          })));
        return;
      }
      if (sub === "global") {
        const open = data.global.filter((h) => h.state === "open");
        const shown = showClosed ? data.global : open;
        const closed = data.global.length - open.length;
        stated.textContent = `${open.length} open global ${open.length === 1 ? "handoff" : "handoffs"}${showClosed ? `, ${closed} expired or resolved` : ""}, newest first. ${every}`;
        panel.replaceChildren(shown.length ? list(shown, now) : el("p", showClosed ? "No global handoff yet." : "No open global handoff.", "scribe-status"));
        return;
      }
      const first = data.history_offset + 1; const last = data.history_offset + data.history.length;
      stated.textContent = data.history.length
        ? `Handoffs ${first}–${last} of ${data.history_total}, newest first; superseded ones are marked. ${every}`
        : `No handoff matches these filters. ${every}`;
      const pager = el("div", "", "scribe-row handoff-pager");
      if (data.history_offset > 0) {
        const newer = el("button", "Newer"); newer.type = "button";
        newer.addEventListener("click", () => { offset = Math.max(0, data.history_offset - HISTORY_PAGE); changed(); });
        pager.append(newer);
      }
      if (data.history_more) {
        const older = el("button", "Older"); older.type = "button";
        older.addEventListener("click", () => { offset = last; changed(); });
        pager.append(older);
      }
      panel.replaceChildren(...(data.history.length ? [list(data.history, now)] : [el("p", "No handoff matches these filters.", "scribe-status")]),
        ...(pager.childElementCount ? [pager] : []));
    };
  };
  return { bar, load, onChange: (listener) => { changed = listener; } };
}
