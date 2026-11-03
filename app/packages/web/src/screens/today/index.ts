/**
 * Today: what needs the user, what was decided and shipped since the
 * latest meeting, what finished today, and beside them the next meeting and
 * live agent work.
 * Lists are hairline rows (rows.ts); a row's title opens its background and
 * its details, and its actions sit inline.
 */
import type {
  AgentWork, AgentWorkTask, CalendarMeeting, SharedLastSync, SharedSkipped, Today, TodayItem, TodayNeed, TodayResult,
} from "@orbit-office/contracts";

import { dismissSharedSkipped, getJson, getLastSharedSync, uploadSource } from "../../api/client.js";
import { sourceHref } from "../../api/source-location.js";
import type { Screen } from "../../screen.js";
import { errorState, monoMeta, renderMarkdown } from "../../ui/index.js";
import { dueWords, isOverdue } from "../../ui/when.js";
import { ApprovalControls, serverApprovals, type ApprovalApi } from "./approvals.js";
import { readDigest, serverDigest, SINCE_HEADING, sinceContent, type DigestApi, type DigestState } from "./digest.js";
import { nextMeetingCard, todaysMeetings, upcoming } from "./next-meeting.js";
import { dot, glyph, listHead, listSection, meta, quiet, rowList, text, type Glyph } from "./rows.js";

function paragraph(value: string): HTMLParagraphElement {
  const element = document.createElement("p");
  element.textContent = value;
  return element;
}

function background(item: TodayItem): HTMLElement {
  const detail = item.background ? renderMarkdown(item.background, { className: "today-background", headingLevel: 4 }) : paragraph("No background has been added yet.");
  detail.classList.add("today-background");
  return detail;
}

/** Row keys whose detail is open; they stay open across redraws. */
type Expanded = Set<string>;

interface RowParts {
  /** The row's key: `data-key`, and the `approval-actions` that go inline. */
  key: string;
  /** The kind icon, or a state dot. */
  glyph: Glyph | HTMLElement;
  title: string;
  meta?: HTMLElement | null;
  /** The controls ApprovalControls drew for the row. */
  controls?: readonly HTMLElement[];
  /** What the title opens before the controls' own details. */
  detail?: readonly Node[];
  /** The `data-focus` of the title, kept across redraws. */
  focus: string;
}

/**
 * One row: the title toggles its detail; the row's own `approval-actions`
 * sit inline; forms and messages stay visible under it; files, previews and
 * pages open with the title.
 */
function expandableRow(parts: RowParts, expanded: Expanded): HTMLLIElement {
  const item = document.createElement("li");
  item.className = "today-row";
  item.dataset.key = parts.key;
  const line = document.createElement("div");
  line.className = "today-row-line";
  const title = document.createElement("button");
  title.type = "button";
  title.className = "today-row-title";
  title.dataset.focus = parts.focus;
  title.append(typeof parts.glyph === "string" ? glyph(parts.glyph) : parts.glyph, text("span", parts.title));
  line.append(title);
  if (parts.meta) line.append(parts.meta);
  const inline: HTMLElement[] = [];
  const live: HTMLElement[] = [];
  const opened: Node[] = [...(parts.detail ?? [])];
  for (const element of parts.controls ?? []) {
    if (element.dataset.slot === "approval-actions" && element.dataset.key === parts.key) inline.push(element);
    else if (element instanceof HTMLFormElement || element.classList.contains("approval-message")) live.push(element);
    else opened.push(element);
  }
  if (inline.length > 0) {
    const actions = document.createElement("div");
    actions.className = "today-row-actions";
    actions.append(...inline);
    line.append(actions);
  }
  item.append(line);
  if (live.length > 0) {
    const under = document.createElement("div");
    under.className = "today-row-live";
    under.append(...live);
    item.append(under);
  }
  const detail = document.createElement("div");
  detail.className = "today-row-detail";
  detail.id = `today-detail-${parts.focus}`;
  detail.append(...opened);
  const open = expanded.has(parts.focus);
  detail.hidden = !open;
  title.setAttribute("aria-expanded", String(open));
  title.setAttribute("aria-controls", detail.id);
  title.addEventListener("click", () => {
    const next = detail.hidden;
    detail.hidden = !next;
    title.setAttribute("aria-expanded", String(next));
    if (next) expanded.add(parts.focus);
    else expanded.delete(parts.focus);
  });
  item.append(detail);
  return item;
}

/** Short words beside a need's title: its kind, or a to-do's due date in the To-dos wording, amber once due. */
function needMeta(item: TodayNeed, now: Date): HTMLElement | null {
  switch (item.kind) {
    case "proposal": return meta("Suggestion");
    case "approval": return meta("Review");
    case "script_output": return meta("Script files");
    case "gateway_draft": return meta("Request");
    case "todo": {
      if (item.due === undefined) return null;
      const overdue = isOverdue(item.due, now);
      return meta(overdue ? `Overdue · ${dueWords(item.due, now)}` : dueWords(item.due, now), overdue);
    }
  }
}

function needRow(item: TodayNeed, controls: ApprovalControls, expanded: Expanded, now: Date): HTMLLIElement {
  const drawn = item.kind === "proposal" ? controls.proposal(item)
    : item.kind === "approval" ? controls.reviewed(item)
      : item.kind === "script_output" ? controls.scriptOutput(item)
        : item.kind === "gateway_draft" ? controls.gatewayDraft(item) : [];
  const row = expandableRow({
    key: item.key, glyph: item.kind, title: item.title, meta: needMeta(item, now), controls: drawn,
    detail: [background(item)], focus: `${item.key}-background`,
  }, expanded);
  row.dataset.kind = item.kind;
  return row;
}

/** One row for proposals that belong together: the onboarding set or one bootstrap subject. */
function groupRow(key: string, title: string, items: readonly TodayNeed[], controls: ApprovalControls, expanded: Expanded): HTMLLIElement {
  const row = expandableRow({
    key, glyph: "group", title, meta: meta(`${items.length} ${items.length === 1 ? "page" : "pages"}`),
    controls: controls.proposalGroup(key, items), focus: `${key}-group`,
  }, expanded);
  row.dataset.group = key;
  row.dataset.kind = "group";
  return row;
}

const SHARED_LIBRARY = "shared-library";

/** The shared library's one row: its open proposals and what the last sync left out. */
function sharedRow(items: readonly TodayNeed[], shared: SharedNeeds, controls: ApprovalControls, expanded: Expanded): HTMLLIElement {
  const row = expandableRow({
    key: SHARED_LIBRARY, glyph: "shared", title: "Changes from the shared library", meta: meta("From your team"),
    controls: controls.sharedGroup(items, shared.skipped, shared.dismissSkipped), focus: `${SHARED_LIBRARY}-group`,
  }, expanded);
  row.dataset.group = SHARED_LIBRARY;
  row.dataset.kind = "shared";
  return row;
}

interface SharedNeeds {
  skipped: readonly SharedSkipped[];
  dismissSkipped: () => Promise<void>;
}

/** Needs-you rows in order; a group takes the place of its first proposal. */
function needRows(needs: readonly TodayNeed[], controls: ApprovalControls, shared: SharedNeeds, expanded: Expanded): HTMLLIElement[] {
  const groups = new Map<string, { title: string; items: TodayNeed[] }>();
  const order: Array<TodayNeed | string> = [];
  for (const item of needs) {
    if (item.group === null) {
      order.push(item);
      continue;
    }
    const group = groups.get(item.group.key);
    if (group === undefined) {
      groups.set(item.group.key, { title: item.group.title, items: [item] });
      order.push(item.group.key);
    } else {
      group.items.push(item);
    }
  }
  // Skipped documents are not tasks: they make the row on their own.
  if (shared.skipped.length > 0 && !groups.has(SHARED_LIBRARY)) {
    groups.set(SHARED_LIBRARY, { title: "", items: [] });
    order.push(SHARED_LIBRARY);
  }
  const now = new Date();
  return order.flatMap((entry) => {
    if (typeof entry !== "string") return [needRow(entry, controls, expanded, now)];
    const group = groups.get(entry);
    if (group === undefined) return [];
    return [entry === SHARED_LIBRARY ? sharedRow(group.items, shared, controls, expanded) : groupRow(entry, group.title, group.items, controls, expanded)];
  });
}

function resultRow(item: TodayResult, expanded: Expanded): HTMLLIElement {
  const outcome = item.result ? renderMarkdown(item.result, { className: "today-result", headingLevel: 4 }) : paragraph("No written result was recorded for this work.");
  outcome.classList.add("today-result");
  const detail: Node[] = [outcome];
  if (item.background) detail.push(meta("Background"), background(item));
  return expandableRow({ key: item.key, glyph: dot("done"), title: item.title, detail, focus: `${item.key}-result` }, expanded);
}

/**
 * The Agents working rows: in-progress agent tasks in the office workspace,
 * then those in the followed and target workspaces with their crew
 * (`GET /api/agents`, when it can be read).
 */
function workingRows(snapshot: Today, elsewhere: readonly AgentWorkTask[], expanded: Expanded): { rows: HTMLElement; count: number } {
  const office = snapshot.agents_working_on.map((item) => {
    const row = expandableRow({ key: item.key, glyph: dot("working"), title: item.title, detail: [background(item)], focus: `${item.key}-working` }, expanded);
    row.dataset.kind = "working";
    return row;
  });
  const titles = new Set(snapshot.agents_working_on.map((item) => item.key));
  const others = elsewhere.filter((task) => !titles.has(task.id)).map((task) => {
    const row = document.createElement("li");
    row.className = "today-side-row";
    row.dataset.key = task.id;
    row.title = `${task.workspace} · ${task.id}`;
    row.append(dot("working"), text("span", task.title), monoMeta(task.crew ?? task.workspace));
    return row;
  });
  const count = office.length + others.length;
  if (count === 0) return { rows: paragraph("No agent work is underway."), count };
  const list = document.createElement("ul");
  list.className = "today-rows today-working-rows";
  list.setAttribute("aria-label", "Agents working");
  list.append(...office, ...others);
  return { rows: list, count };
}

export interface TodayScreenOptions {
  load?: () => Promise<Today>;
  /** The approval calls; tests pass their own. */
  approvals?: ApprovalApi;
  /** Poll after the last request settles, so requests never overlap. */
  pollIntervalMs?: number;
  /** Today's remaining meetings for Next meeting; tests pass their own. */
  meetings?: (now: Date) => Promise<CalendarMeeting[]>;
  /** How often Next meeting rereads the calendar. */
  meetingIntervalMs?: number;
  /** The latest shared-library sync the app ran, for its skipped documents; tests pass their own. */
  sharedSync?: { last(): Promise<SharedLastSync>; dismissSkipped(): Promise<unknown> };
  /** Agent work across the followed workspaces, for Agents working; a failed read shows the office's only. */
  agents?: () => Promise<AgentWork>;
  /** The digest call; tests pass its own. */
  digest?: DigestApi;
  /** How often the digest rereads (it changes slowly). */
  digestIntervalMs?: number;
}

const serverSharedSync = { last: getLastSharedSync, dismissSkipped: dismissSharedSkipped };

/** The skipped documents of the last successful sync; none when it can't be read. */
async function lastSkipped(sharedSync: NonNullable<TodayScreenOptions["sharedSync"]>): Promise<SharedSkipped[]> {
  try {
    const { last_sync: last } = await sharedSync.last();
    return last !== null && last.ok ? last.sync.skipped : [];
  } catch {
    return [];
  }
}

/** A quiet section of the right column: a small heading over its body. */
function side(title: string, id: string): { section: HTMLElement; head: HTMLElement; body: HTMLElement } {
  const section = document.createElement("section");
  section.className = "today-side";
  section.setAttribute("aria-labelledby", id);
  const head = document.createElement("div");
  head.className = "today-side-head";
  const heading = document.createElement("h2");
  heading.id = id;
  heading.textContent = title;
  head.append(heading);
  const body = document.createElement("div");
  body.className = "today-side-body";
  section.append(head, body);
  return { section, head, body };
}

/** The top bar's Add source: a file picker, and a file dropped anywhere on Today. It is captured as a source, then opened there. */
function addSource(view: HTMLElement): HTMLElement {
  const actions = document.createElement("div");
  actions.className = "today-bar-actions";
  const status = text("span", "", "today-upload");
  status.setAttribute("aria-live", "polite");
  status.hidden = true;
  const input = document.createElement("input");
  input.type = "file";
  input.className = "today-file";
  input.tabIndex = -1;
  input.setAttribute("aria-hidden", "true");
  const button = document.createElement("button");
  button.type = "button";
  button.className = "today-button";
  button.dataset.action = "add-source";
  button.append(glyph("plus", ""), "Add source");
  button.addEventListener("click", () => input.click());
  const upload = (file: File): void => {
    status.textContent = `Uploading ${file.name}…`;
    status.hidden = false;
    void uploadSource(file).then((source) => {
      window.location.assign(sourceHref(source.id));
    }).catch(() => {
      const source = document.createElement("a");
      source.href = "#source";
      source.textContent = "Source screen";
      status.replaceChildren("Couldn’t add that file. Try it on the ", source, ".");
    });
  };
  input.addEventListener("change", () => {
    const file = input.files?.[0];
    if (file !== undefined) upload(file);
    input.value = "";
  });
  const carriesFiles = (event: DragEvent): boolean => event.dataTransfer?.types.includes("Files") ?? false;
  view.addEventListener("dragover", (event) => {
    if (!carriesFiles(event)) return;
    event.preventDefault();
    view.dataset.dropping = "true";
  });
  view.addEventListener("dragleave", (event) => {
    if (event.relatedTarget instanceof Node && view.contains(event.relatedTarget)) return;
    delete view.dataset.dropping;
  });
  view.addEventListener("drop", (event) => {
    if (!carriesFiles(event)) return;
    event.preventDefault();
    delete view.dataset.dropping;
    const file = event.dataTransfer?.files[0];
    if (file !== undefined) upload(file);
  });
  actions.append(status, button, input);
  return actions;
}

/** The production screen and browser tests use the same renderer and polling lifecycle. */
export function createTodayScreen({
  load = () => getJson<Today>("/api/today"), approvals = serverApprovals, pollIntervalMs = 5_000,
  meetings = todaysMeetings, meetingIntervalMs = 60_000, sharedSync = serverSharedSync,
  digest = serverDigest, digestIntervalMs = 30_000, agents = () => getJson<AgentWork>("/api/agents"),
}: TodayScreenOptions = {}): Screen {
  return {
    id: "today",
    title: "Today",
    status: "built",
    async render(root) {
      const view = document.createElement("div");
      view.className = "today today-view";
      // The whole view takes a dropped file.
      view.dataset.slot = "drop-zone";
      const started = new Date();
      const bar = document.createElement("header");
      bar.className = "today-bar";
      const heading = document.createElement("h1");
      heading.textContent = "Today";
      const date = document.createElement("time");
      date.className = "today-date";
      date.dateTime = `${started.getFullYear()}-${String(started.getMonth() + 1).padStart(2, "0")}-${String(started.getDate()).padStart(2, "0")}`;
      date.textContent = started.toLocaleDateString(undefined, { weekday: "short", month: "short", day: "numeric" });
      bar.append(heading, date, addSource(view));

      // Main column: Needs you, Since <meeting>, Done today.
      const columns = document.createElement("div");
      columns.className = "today-body";
      const main = document.createElement("div");
      main.className = "today-main";
      const feedback = document.createElement("div");
      feedback.setAttribute("aria-live", "polite");
      const needsSection = listSection("today-needs-heading");
      needsSection.dataset.section = "needs";
      const needsHead = listHead("Needs you", "today-needs-heading", 0);
      const needsCount = needsHead.querySelector<HTMLElement>(".today-count")!;
      needsCount.hidden = true;
      // Outcomes of the user's own actions outlive the row they came from.
      const outcome = text("p", "", "today-outcome");
      outcome.setAttribute("aria-live", "polite");
      outcome.hidden = true;
      const needsBody = document.createElement("div");
      needsBody.append(quiet("Gathering your work…"));
      needsBody.setAttribute("aria-busy", "true");
      needsSection.append(needsHead, outcome, needsBody);

      const since = listSection(SINCE_HEADING);
      since.dataset.section = "since";
      since.setAttribute("aria-busy", "true");
      since.append(listHead("Since your last meeting", SINCE_HEADING, null), quiet("Reading the digest…"));

      const done = document.createElement("section");
      done.className = "today-list";
      done.dataset.section = "done";
      done.setAttribute("aria-labelledby", "today-done-label");
      const doneHead = document.createElement("h2");
      doneHead.className = "today-list-heading";
      const doneToggle = document.createElement("button");
      doneToggle.type = "button";
      doneToggle.className = "today-list-head";
      doneToggle.dataset.focus = "done-today";
      doneToggle.setAttribute("aria-controls", "today-done-rows");
      const doneCount = text("span", "", "today-count");
      doneToggle.append(glyph("chevron", "today-chevron"), Object.assign(text("span", "Done today"), { id: "today-done-label" }), doneCount);
      doneHead.append(doneToggle);
      const doneBody = document.createElement("div");
      doneBody.id = "today-done-rows";
      let doneOpen = false;
      const showDone = (): void => {
        doneBody.hidden = !doneOpen;
        doneToggle.setAttribute("aria-expanded", String(doneOpen));
      };
      showDone();
      doneToggle.addEventListener("click", () => { doneOpen = !doneOpen; showDone(); });
      done.append(doneHead, doneBody);
      const notice = document.createElement("div");
      main.append(feedback, needsSection, since, done, notice);

      // Right column: Next meeting, Agents working.
      const aside = document.createElement("aside");
      aside.className = "today-aside";
      const next = side("Next meeting", "today-next-heading");
      next.body.classList.add("today-next-slot");
      next.body.setAttribute("aria-busy", "true");
      next.body.append(paragraph("Reading your calendar…"));
      const working = side("Agents working", "today-working-heading");
      working.section.classList.add("today-working");
      const workingCount = text("span", "", "today-count");
      workingCount.hidden = true;
      working.head.append(workingCount);
      working.body.append(paragraph("Gathering agent work…"));
      aside.append(next.section, working.section);
      columns.append(main, aside);
      view.append(bar, columns);
      root.replaceChildren(view);

      let previous: string | undefined;
      let shown: Today | undefined;
      let elsewhere: AgentWorkTask[] = [];
      const expanded: Expanded = new Set();
      let skipped: SharedSkipped[] = [];
      const shared: SharedNeeds = {
        get skipped() { return skipped; },
        async dismissSkipped() {
          await sharedSync.dismissSkipped();
          skipped = [];
          previous = undefined;
          say("done", "Cleared the documents the last sync left out.");
        },
      };
      function say(tone: string, value: string): void {
        outcome.textContent = value;
        outcome.dataset.tone = tone;
        outcome.hidden = false;
      }
      const controls = new ApprovalControls(approvals, say);
      const alive = (): boolean => root.contains(view) && view.isConnected;

      /** The Needs-you count is its rows; nothing at all reads as one quiet line. */
      function countNeeds(rows: number): void {
        needsCount.hidden = false;
        needsCount.textContent = String(rows);
      }

      function drawWorking(snapshot: Today): void {
        const { rows, count } = workingRows(snapshot, elsewhere, expanded);
        working.body.replaceChildren(rows);
        workingCount.textContent = String(count);
        workingCount.hidden = count === 0;
      }

      /** Redraw, keeping open rows and panels, typed comments and focus. */
      function paint(snapshot: Today): void {
        const painted = [needsBody, doneBody, working.body];
        const within = (selector: string): Element[] => painted.flatMap((slot) => Array.from(slot.querySelectorAll(selector)));
        const open = new Set(within("details[open]").map((detail) => (detail as HTMLElement).dataset.key));
        const typed = new Map(within("textarea[data-focus]").map((field) => [(field as HTMLElement).dataset.focus, (field as HTMLTextAreaElement).value]));
        const active = document.activeElement;
        const focused = active instanceof HTMLElement && painted.some((slot) => slot.contains(active)) ? active.dataset.focus : undefined;

        const needs = needRows(snapshot.needs_you, controls, shared, expanded);
        if (needs.length === 0) {
          const empty = quiet("Nothing needs you.");
          empty.dataset.empty = "";
          needsBody.replaceChildren(empty);
        } else {
          const list = rowList();
          list.append(...needs);
          needsBody.replaceChildren(list);
        }
        countNeeds(needs.length);
        doneCount.textContent = String(snapshot.done_today.length);
        if (snapshot.done_today.length === 0) doneBody.replaceChildren(quiet("No completed work yet today."));
        else {
          const list = rowList();
          list.append(...snapshot.done_today.map((item) => resultRow(item, expanded)));
          doneBody.replaceChildren(list);
        }
        notice.replaceChildren();
        if (snapshot.truncated || snapshot.completion_dates_unavailable > 0) {
          notice.append(quiet([
            snapshot.truncated ? "Showing a limited selection of your work." : "",
            snapshot.completion_dates_unavailable > 0 ? "Some finished work has no completion date and cannot be placed today." : "",
          ].filter(Boolean).join(" ")));
        }
        drawWorking(snapshot);
        shown = snapshot;

        for (const detail of within("details") as HTMLDetailsElement[]) detail.open = open.has(detail.dataset.key);
        for (const field of within("textarea[data-focus]") as HTMLTextAreaElement[]) field.value = typed.get(field.dataset.focus) ?? "";
        if (focused !== undefined) {
          const target = within("[data-focus]").find((element) => (element as HTMLElement).dataset.focus === focused);
          if (target instanceof HTMLElement) target.focus();
        }
      }
      controls.bind(() => {
        if (shown !== undefined && alive()) paint(shown);
      });

      async function refresh(): Promise<void> {
        if (!alive()) return;
        try {
          const [snapshot, left] = await Promise.all([load(), lastSkipped(sharedSync)]);
          if (!alive()) return;
          skipped = left;
          const serialized = JSON.stringify([snapshot, left]);
          if (serialized !== previous) {
            paint(snapshot);
            previous = serialized;
          }
          feedback.replaceChildren();
        } catch {
          if (!alive()) return;
          feedback.replaceChildren(errorState("Couldn’t refresh your work", previous === undefined
            ? "Please try again. We’ll keep checking for your work."
            : "The work below may have changed. We’ll try again shortly."));
          if (previous === undefined) {
            needsBody.replaceChildren();
            working.body.replaceChildren(paragraph("Couldn’t read agent work."));
          }
        } finally {
          needsBody.setAttribute("aria-busy", "false");
          // Navigation replaces view. A pending read never redraws another screen.
          if (alive()) window.setTimeout(() => { void refresh(); }, pollIntervalMs);
        }
      }
      /** Next meeting follows the calendar; a failed read says so rather than showing stale times. */
      async function nextMeeting(): Promise<void> {
        if (!alive()) return;
        const now = new Date();
        try {
          const meeting = upcoming(await meetings(now), now);
          if (!alive()) return;
          next.body.replaceChildren(meeting === null ? paragraph("Nothing else on your calendar today.") : nextMeetingCard(meeting, now));
        } catch {
          // Stale times are worse than none.
          next.body.replaceChildren(paragraph("Couldn’t read your calendar."));
        } finally {
          next.body.setAttribute("aria-busy", "false");
          if (alive()) window.setTimeout(() => { void nextMeeting(); }, meetingIntervalMs);
        }
      }
      let digestShown: string | undefined;
      /** The digest rereads slowly. */
      async function morningDigest(): Promise<void> {
        if (!alive()) return;
        let state: DigestState;
        try { state = await readDigest(digest); }
        finally { since.setAttribute("aria-busy", "false"); }
        if (!alive()) return;
        const serialized = JSON.stringify(state);
        // A redraw would drop a pending choice's status line; only draw on change.
        if (serialized !== digestShown) {
          digestShown = serialized;
          since.replaceChildren(...sinceContent(state));
        }
        if (alive()) window.setTimeout(() => { void morningDigest(); }, digestIntervalMs);
      }
      /** Agent work in other workspaces: its own slower loop, so a slow read never holds Today. */
      async function agentWork(): Promise<void> {
        if (!alive()) return;
        const work = await agents().catch(() => null);
        if (!alive()) return;
        // A malformed answer reads as no other work, never as a crash.
        elsewhere = work !== null && Array.isArray(work.working_now)
          ? work.working_now.filter((task) => typeof task?.title === "string" && task.workspace !== "office") : [];
        if (shown !== undefined) drawWorking(shown);
        window.setTimeout(() => { void agentWork(); }, digestIntervalMs);
      }
      void nextMeeting();
      void morningDigest();
      void agentWork();
      await refresh();
    },
  };
}

export const screen: Screen = createTodayScreen();
