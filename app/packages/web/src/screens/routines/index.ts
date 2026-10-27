/**
 * Routines: the next hour of scheduled work, then the routines that are on
 * and the ones that are paused, each with when it runs, when it runs next
 * and how its last time went. Plain words: no task ids, runs or crews.
 */
import type { Screen } from "../../screen.js";
import { ApiError, getJson, postJson } from "../../api/client.js";
import { requestRailRefresh } from "../../ui/index.js";
import { eventDriven, firesInNextHour, nextFire, scheduleWords } from "./schedule.js";
import { styles } from "./style.js";

export { scheduleWords } from "./schedule.js";

interface LastFire { kind: "fired" | "skipped"; at: string; task_status: string | null; run_state?: string }
interface Routine {
  kind?: "auto_task" | "routine";
  name: string;
  description: string;
  schedule: Record<string, unknown>;
  enabled: boolean;
  skipped_reason: string | null;
  last_fire?: LastFire | null;
}
interface Listing { routines: Routine[]; hidden_count: number; routines_unavailable: boolean }

function element<K extends keyof HTMLElementTagNameMap>(tag: K, text: string, className?: string): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  node.textContent = text;
  if (className !== undefined) node.className = className;
  return node;
}

function skipWords(reason: string): string {
  if (reason.includes("switched off in this workspace")) return "Its plugin is switched off in this workspace.";
  if (reason.includes("not enabled on this host")) return "Its plugin is not enabled on this computer.";
  return "This task is unavailable. Check its settings in Orbit.";
}

/** `daily-digest` reads as "Daily digest". */
export function routineTitle(name: string): string {
  // Short words without a vowel are initialisms: kt, qbr, gc.
  const words = name.split(/[-_\s]+/).filter(Boolean).map(word => /^[^aeiouy]{2,3}$/i.test(word) ? word.toUpperCase() : word);
  const text = words.join(" ");
  return text.charAt(0).toUpperCase() + text.slice(1);
}

const TASK_WORDS: Record<string, string> = {
  done: "Finished",
  "in-progress": "Working on it",
  review: "Waiting for review",
  proposed: "Waiting for you",
  backlog: "Queued",
  blocked: "Stuck",
  rejected: "Turned down",
  someday: "Set aside",
  archived: "Put away",
};

function taskTone(status: string | null): string {
  if (status === "done") return "done";
  if (status === "in-progress" || status === "backlog") return "working";
  if (status === "review" || status === "proposed") return "attention";
  if (status === "blocked" || status === "rejected") return "danger";
  return "idle";
}

function clock(at: Date, now: Date): string {
  const time = at.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
  const days = Math.round((new Date(at.getFullYear(), at.getMonth(), at.getDate()).getTime()
    - new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime()) / 86_400_000);
  if (days === 0) return time;
  if (Math.abs(days) < 7) return `${at.toLocaleDateString(undefined, { weekday: "short" })} ${time}`;
  return `${at.toLocaleDateString(undefined, { day: "numeric", month: "short" })} ${time}`;
}

export function relativeWords(at: Date, now: Date): string {
  const minutes = Math.round((at.getTime() - now.getTime()) / 60_000);
  const size = Math.abs(minutes);
  const amount = size < 1 ? "" : size < 60 ? `${size} min` : size < 48 * 60 ? `${Math.round(size / 60)} h` : `${Math.round(size / 1440)} days`;
  if (amount === "") return "just now";
  return minutes > 0 ? `in ${amount}` : `${amount} ago`;
}

function cell(label: string, main: string, sub?: string, mainClass = "", subClass = "routine-cell-sub routine-cell-sub--mono"): HTMLElement {
  const node = element("div", "", "routine-cell");
  node.setAttribute("data-label", label);
  node.append(element("span", main, `routine-cell-main ${mainClass}`.trim()));
  if (sub !== undefined && sub !== "") node.append(element("span", sub, subClass));
  return node;
}

function whenCell(item: Routine): HTMLElement {
  const cron = typeof item.schedule.cron === "string" ? item.schedule.cron : undefined;
  return cell("When", scheduleWords(item.schedule), cron);
}

function nextCell(item: Routine, now: Date): HTMLElement {
  if (eventDriven(item.schedule)) return cell("Next", item.enabled ? "When work lands" : "Paused", undefined, "muted");
  const lastAt = item.last_fire?.kind === "fired" ? new Date(item.last_fire.at) : null;
  const next = nextFire(item.schedule, now, lastAt);
  if (next === null) return cell("Next", item.enabled ? "Soon after it starts" : "Paused", undefined, "muted");
  if (!item.enabled) return cell("Next", `Would be ${clock(next, now)}`, undefined, "muted");
  return cell("Next", relativeWords(next, now), clock(next, now));
}

function lastCell(item: Routine, now: Date): HTMLElement {
  const fire = item.last_fire ?? null;
  if (fire === null) return cell("Last time", "Never", undefined, "muted");
  const at = new Date(fire.at);
  const node = element("div", "", "routine-cell");
  node.setAttribute("data-label", "Last time");
  const main = element("span", "", "routine-cell-main routine-outcome");
  if (fire.kind === "skipped") {
    main.append(element("span", "", "routine-dot"), element("span", `Skipped ${relativeWords(at, now)}`));
    node.append(main, element("span", "It was not due to run then.", "routine-cell-sub"));
    return node;
  }
  const words = fire.run_state !== undefined
    ? ({ completed: "Finished", success: "Finished", failed: "Failed", running: "Working on it", stopped: "Stopped", pending: "Queued" }[fire.run_state] ?? "Ran")
    : fire.task_status === null ? "Its task was removed" : TASK_WORDS[fire.task_status] ?? "Ran";
  main.append(element("span", "", `routine-dot routine-dot--${taskTone(fire.task_status)}`), element("span", `Ran ${relativeWords(at, now)}`));
  node.append(main, element("span", words, "routine-cell-sub"));
  return node;
}

function routineRow(item: Routine, now: Date, onChanged: (listing: Listing) => void): HTMLElement {
  const row = element("article", "", "routine-row");
  const toggle = document.createElement("input");
  toggle.type = "checkbox";
  toggle.checked = item.enabled;
  toggle.className = "routine-switch";
  toggle.setAttribute("role", "switch");
  toggle.setAttribute("aria-label", `${routineTitle(item.name)} on`);
  const control = element("div", "", "routine-cell routine-cell--control");
  control.append(toggle);

  const identity = element("div", "", "routine-cell routine-identity");
  identity.setAttribute("data-label", "Routine");
  identity.append(element("h3", routineTitle(item.name)), element("p", item.description, "routine-description"));
  const message = element("p", item.skipped_reason ? skipWords(item.skipped_reason) : "", "routine-note");
  message.setAttribute("role", "status");
  identity.append(message);

  toggle.addEventListener("change", () => {
    const desired = toggle.checked; toggle.disabled = true; message.textContent = "";
    void (async () => {
      try {
        await postJson("/api/routines/toggle", { kind: item.kind ?? "auto_task", name: item.name, enabled: desired });
        const latest = await getJson<Listing>("/api/routines");
        const updated = latest.routines.find(routine => routine.name === item.name && routine.kind === item.kind);
        if (!updated) throw new Error("routine unavailable");
        toggle.checked = updated.enabled; item.enabled = updated.enabled;
        requestRailRefresh();
        onChanged(latest);
      } catch {
        toggle.checked = item.enabled; message.textContent = "Could not confirm this change. Refresh and try again.";
      } finally { toggle.disabled = false; }
    })();
  });

  row.append(control, identity, whenCell(item), nextCell(item, now), lastCell(item, now));
  return row;
}

function group(tone: "on" | "off", title: string, hint: string, items: readonly Routine[], now: Date, onChanged: (listing: Listing) => void): HTMLElement {
  const section = element("section", "", "routine-group");
  section.setAttribute("aria-label", title);
  const head = element("div", "", "routine-group-head");
  const name = element("h2", "", "routine-group-title");
  name.append(element("span", "", `routine-group-dot routine-group-dot--${tone}`), element("span", title), element("span", String(items.length), "routine-group-count"));
  head.append(name, element("span", hint, "routine-group-hint"));
  const columns = element("div", "", "routine-columns");
  columns.setAttribute("aria-hidden", "true");
  columns.append(element("span", ""), element("span", "Routine"), element("span", "When"), element("span", "Next"), element("span", "Last time"));
  section.append(head);
  if (items.length === 0) {
    section.append(element("p", tone === "on" ? "No routines are on." : "Nothing is paused.", "routine-empty"));
    return section;
  }
  section.append(columns, ...items.map(item => routineRow(item, now, onChanged)));
  return section;
}

/** The next hour as one track: a mark for each fire, hollow where a paused routine skips its slot. */
function nextHour(routines: readonly Routine[], now: Date): HTMLElement {
  const strip = element("section", "", "routine-hour");
  strip.setAttribute("aria-label", "The next hour");
  const marks: { at: Date; title: string; fires: boolean }[] = [];
  for (const item of routines) {
    if (eventDriven(item.schedule)) continue;
    const lastAt = item.last_fire?.kind === "fired" ? new Date(item.last_fire.at) : null;
    for (const at of firesInNextHour(item.schedule, now, lastAt)) marks.push({ at, title: routineTitle(item.name), fires: item.enabled });
  }
  marks.sort((a, b) => a.at.getTime() - b.at.getTime());
  const due = marks.filter(mark => mark.fires);
  const active = routines.filter(item => item.enabled).length;
  const until = new Date(now.getTime() + 3_600_000);
  const head = element("div", "", "routine-hour-head");
  head.append(
    element("strong", "Next hour"),
    element("span", `${clock(now, now)} → ${clock(until, now)}`, "routine-hour-range"),
    element("span", due.length === 0
      ? "Nothing runs in the next hour."
      : `${due.length} ${due.length === 1 ? "run" : "runs"} from ${active} ${active === 1 ? "routine" : "routines"} that are on`, "routine-hour-summary"),
  );
  const legend = element("span", "", "routine-hour-legend");
  legend.append(element("span", "will run", "routine-hour-key routine-hour-key--fires"), element("span", "paused, skipped", "routine-hour-key"));
  head.append(legend);
  const track = element("div", "", "routine-hour-track");
  track.append(element("div", "", "routine-hour-axis"));
  marks.slice(0, 24).forEach((mark, index) => {
    const offset = Math.min(100, Math.max(0, (mark.at.getTime() - now.getTime()) / 36_000));
    const node = element("div", "", `routine-hour-mark${mark.fires ? " routine-hour-mark--fires" : ""}${index % 2 === 1 ? " routine-hour-mark--low" : ""}`);
    node.setAttribute("style", `left: ${offset.toFixed(2)}%`);
    node.setAttribute("title", `${mark.title} · ${clock(mark.at, now)}${mark.fires ? "" : " · paused"}`);
    node.append(element("span", "", "routine-hour-tick"), element("span", mark.title, "routine-hour-label"));
    track.append(node);
  });
  strip.append(head, track);
  return strip;
}

function draw(root: HTMLElement, listing: Listing): void {
  const { routines } = listing;
  const now = new Date();
  const redraw = (latest: Listing): void => draw(root, latest);
  const page = element("section", "", "routines");
  const style = document.createElement("style");
  style.textContent = styles;
  const header = element("header", "", "routines-header");
  const titles = element("div", "");
  const on = routines.filter(item => item.enabled);
  const off = routines.filter(item => !item.enabled);
  titles.append(element("h1", "Routines"), element("p", "Work that runs for you on a schedule. Results land in Today.", "intro"));
  header.append(titles, element("span", `${on.length} on of ${routines.length}`, "routines-total"));
  page.append(style, header);
  page.append(element("p", `${listing.hidden_count} Orbit developer routines hidden`, "routines-hidden"));
  if (listing.routines_unavailable) page.append(element("p", "Some routines are unavailable with this connection. The hidden count covers only the routines this connection can read.", "intro"));
  if (!routines.length) page.append(element("p", "No routines are available.", "intro"));
  else page.append(
    nextHour(routines, now),
    group("on", "On", "runs on its schedule on this computer", on, now, redraw),
    group("off", "Paused", "switched off — scheduled times are skipped", off, now, redraw),
  );
  page.append(retentionForm());
  root.replaceChildren(page);
}

async function render(root: HTMLElement): Promise<void> {
  root.replaceChildren(element("p", "Loading routines."));
  try {
    const data = await getJson<Listing>("/api/routines");
    draw(root, data);
  } catch { root.replaceChildren(element("p", "Routines are unavailable. Try again in a moment.")); }
}

function retentionForm(): HTMLElement {
  const section = element("section", "", "routines-retention");
  section.append(element("h2", "Retention"), element("p", "Choose how long to keep originals. Leave days empty to keep them indefinitely."));
  const form = document.createElement("form");
  const kindLabel = element("label", "Type "); const kind = document.createElement("select");
  for (const [value, label] of [["video", "Video"], ["meeting", "Meeting"], ["chat", "Chat"], ["email", "Email"], ["doc", "Document"], ["receipt", "Receipt"], ["bill", "Bill"], ["conversation", "Conversation"], ["note", "Note"]] as const) {
    const option = element("option", label); option.value = value; kind.append(option);
  }
  kindLabel.append(kind);
  const daysLabel = element("label", " Days "); const days = document.createElement("input"); days.type = "number"; days.step = "any"; daysLabel.append(days);
  const button = element("button", "Save retention"); button.type = "submit";
  const message = element("p", ""); message.setAttribute("role", "status"); form.append(kindLabel, daysLabel, button, message);
  form.addEventListener("submit", event => {
    event.preventDefault(); button.disabled = true; message.textContent = "";
    void postJson("/api/routines/retention", { kind: kind.value, value: days.value === "" ? null : Number(days.value) }).then(() => {
      message.textContent = "Retention saved.";
    }).catch((error: unknown) => {
      message.textContent = error instanceof ApiError && error.reason ? error.reason : "Could not save retention. Try again.";
    }).finally(() => { button.disabled = false; });
  });
  section.append(form); return section;
}
export const screen: Screen = { id: "routines", title: "Routines", status: "built", render };
