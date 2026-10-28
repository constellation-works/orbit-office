/** Today: the next meeting, the user's next steps and the actions on them, live agent work and openable outcomes. */
import type { CalendarMeeting, SharedLastSync, SharedSkipped, Today, TodayItem, TodayNeed, TodayResult } from "@orbit-office/contracts";

import { dismissSharedSkipped, getJson, getLastSharedSync } from "../../api/client.js";
import type { Screen } from "../../screen.js";
import { card, emptyState, errorState, pill, provenanceLink, tile, tileRow } from "../../ui/index.js";
import { ApprovalControls, serverApprovals, type ApprovalApi } from "./approvals.js";
import { nextMeetingCard, todaysMeetings, upcoming } from "./next-meeting.js";

function paragraph(text: string): HTMLParagraphElement {
  const element = document.createElement("p");
  element.textContent = text;
  return element;
}

function background(item: TodayItem): HTMLElement {
  const container = document.createElement("div");
  const link = provenanceLink("About this work", "#today");
  link.dataset.focus = `${item.key}-background`;
  link.setAttribute("aria-expanded", "false");
  const detail = paragraph(item.background || "No background has been added yet.");
  detail.hidden = true;
  detail.className = "today-background";
  link.addEventListener("click", (event) => {
    event.preventDefault();
    detail.hidden = !detail.hidden;
    link.setAttribute("aria-expanded", String(!detail.hidden));
  });
  container.append(link, detail);
  return container;
}

const NEED_LABELS: Record<TodayNeed["kind"], string> = {
  proposal: "A suggestion",
  approval: "Ready for your review",
  todo: "For you",
  script_output: "New files from a script",
  gateway_draft: "A request waiting for your approval",
};

function needCard(item: TodayNeed, controls: ApprovalControls): HTMLElement {
  const content = document.createElement("div");
  content.className = "card-content";
  content.append(background(item));
  switch (item.kind) {
    case "proposal":
      content.append(...controls.proposal(item));
      break;
    case "approval":
      content.append(...controls.reviewed(item));
      break;
    case "script_output":
      content.append(...controls.scriptOutput(item));
      break;
    case "gateway_draft":
      content.append(...controls.gatewayDraft(item));
      break;
    case "todo":
      break;
  }
  return card(item.title, content, undefined, { label: NEED_LABELS[item.kind], tone: "attention" });
}

/** One card for proposals that belong together: the onboarding set or one bootstrap subject. */
function groupCard(key: string, title: string, items: readonly TodayNeed[], controls: ApprovalControls): HTMLElement {
  const content = document.createElement("div");
  content.className = "card-content";
  const onboarding = key === "onboarding";
  const count = paragraph(`${items.length} suggested ${items.length === 1 ? "page" : "pages"}. Review each one, or accept them all.`);
  content.append(count, ...controls.proposalGroup(key, items));
  const element = card(title, content, undefined, {
    label: onboarding ? "Suggested pages for your Library" : "New Library pages to review",
    tone: "attention",
  });
  element.classList.add("card--group");
  element.dataset.group = key;
  return element;
}

const SHARED_LIBRARY = "shared-library";

/** The shared library's one card: its open proposals and what the last sync left out. */
function sharedCard(items: readonly TodayNeed[], shared: SharedNeeds, controls: ApprovalControls): HTMLElement {
  const content = document.createElement("div");
  content.className = "card-content";
  content.append(...controls.sharedGroup(items, shared.skipped, shared.dismissSkipped));
  const element = card("Changes from the shared library", content, undefined, { label: "From your team", tone: "attention" });
  element.classList.add("card--group");
  element.dataset.group = SHARED_LIBRARY;
  return element;
}

interface SharedNeeds {
  skipped: readonly SharedSkipped[];
  dismissSkipped: () => Promise<void>;
}

/** Needs-you cards in order; a group takes the place of its first proposal. */
function needCards(needs: readonly TodayNeed[], controls: ApprovalControls, shared: SharedNeeds): HTMLElement[] {
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
  // Skipped documents are not tasks: they make the card on their own.
  if (shared.skipped.length > 0 && !groups.has(SHARED_LIBRARY)) {
    groups.set(SHARED_LIBRARY, { title: "", items: [] });
    order.push(SHARED_LIBRARY);
  }
  return order.map((entry) => {
    if (typeof entry !== "string") return needCard(entry, controls);
    const group = groups.get(entry);
    if (group === undefined) return document.createElement("div");
    return entry === SHARED_LIBRARY ? sharedCard(group.items, shared, controls) : groupCard(entry, group.title, group.items, controls);
  });
}

function resultCard(item: TodayResult): HTMLElement {
  const content = document.createElement("div");
  content.className = "card-content";
  const result = document.createElement("details");
  result.dataset.key = item.key;
  const summary = document.createElement("summary");
  summary.dataset.focus = `${item.key}-result`;
  summary.textContent = "Open result";
  const outcome = paragraph(item.result ?? "No written result was recorded for this work.");
  outcome.className = "today-result";
  result.append(summary, outcome);
  content.append(result, background(item));
  return card(item.title, content);
}

function cardSection(title: string, cards: readonly HTMLElement[], empty: string): HTMLElement {
  const element = document.createElement("section");
  element.className = "today-section";
  const heading = document.createElement("h2");
  heading.textContent = title;
  element.setAttribute("aria-label", title);
  const header = document.createElement("div");
  header.className = "today-section-heading";
  header.append(heading, pill(String(cards.length)));
  element.append(header);
  if (cards.length === 0) {
    element.append(emptyState(empty, "This space will update as your work moves forward."));
  } else {
    const list = document.createElement("div");
    list.className = "card-grid";
    list.append(...cards);
    element.append(list);
  }
  return element;
}

function section<T extends TodayItem>(title: string, items: readonly T[], empty: string, draw: (item: T) => HTMLElement): HTMLElement {
  return cardSection(title, items.map(draw), empty);
}

function drawToday(snapshot: Today, controls: ApprovalControls, shared: SharedNeeds): HTMLElement {
  const body = document.createElement("div");
  const needed = needCards(snapshot.needs_you, controls, shared);
  const needs = cardSection("Needs you", needed, "Nothing needs your attention.");
  const working = section("Agents working on", snapshot.agents_working_on, "No agent work is underway.", (item) => {
    const line = document.createElement("article");
    line.className = "today-working-line";
    const title = document.createElement("h3");
    title.textContent = item.title;
    line.append(title, background(item));
    return line;
  });
  working.classList.add("today-working");
  const done = section("Done today", snapshot.done_today, "No completed work yet today.", resultCard);
  done.classList.add("today-done");
  const split = document.createElement("div");
  split.className = "today-split";
  split.append(working, done);
  const counts = tileRow([
    tile("Needs you", String(needed.length), "attention"),
    tile("Agents working", String(snapshot.agents_working_on.length), "working"),
    tile("Done today", String(snapshot.done_today.length), "done"),
  ], "Today at a glance");
  body.append(counts, needs, split);
  if (snapshot.truncated || snapshot.completion_dates_unavailable > 0) {
    const notice = paragraph([
      snapshot.truncated ? "Showing a limited selection of your work." : "",
      snapshot.completion_dates_unavailable > 0 ? "Some finished work has no completion date and cannot be placed today." : "",
    ].filter(Boolean).join(" "));
    notice.className = "intro";
    body.append(notice);
  }
  return body;
}

export interface TodayScreenOptions {
  load?: () => Promise<Today>;
  /** The approval calls; tests pass their own. */
  approvals?: ApprovalApi;
  /** Poll after the last request settles, so requests never overlap. */
  pollIntervalMs?: number;
  /** Today's remaining meetings for the Next meeting card; tests pass their own. */
  meetings?: (now: Date) => Promise<CalendarMeeting[]>;
  /** How often the Next meeting card rereads the calendar. */
  meetingIntervalMs?: number;
  /** The latest shared-library sync the app ran, for its skipped documents; tests pass their own. */
  sharedSync?: { last(): Promise<SharedLastSync>; dismissSkipped(): Promise<unknown> };
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

/** The production screen and browser tests use the same renderer and polling lifecycle. */
export function createTodayScreen({
  load = () => getJson<Today>("/api/today"), approvals = serverApprovals, pollIntervalMs = 5_000,
  meetings = todaysMeetings, meetingIntervalMs = 60_000, sharedSync = serverSharedSync,
}: TodayScreenOptions = {}): Screen {
  return {
    id: "today",
    title: "Today",
    status: "built",
    async render(root) {
      const view = document.createElement("div");
      view.className = "today";
      const heading = document.createElement("h1");
      heading.textContent = "Today";
      const intro = paragraph("Your next steps and the work moving forward.");
      intro.className = "intro";
      const nextSlot = document.createElement("div");
      nextSlot.className = "today-next-slot";
      nextSlot.hidden = true;
      nextSlot.setAttribute("aria-busy", "true");
      const uploadSlot = document.createElement("div");
      uploadSlot.dataset.slot = "drop-zone";
      const feedback = document.createElement("div");
      feedback.setAttribute("aria-live", "polite");
      // Outcomes of the user's own actions outlive the card they came from.
      const outcome = document.createElement("p");
      outcome.className = "today-outcome";
      outcome.setAttribute("aria-live", "polite");
      outcome.hidden = true;
      const content = document.createElement("div");
      content.append(paragraph("Gathering your work…"));
      content.setAttribute("aria-busy", "true");
      view.append(heading, intro, nextSlot, uploadSlot, feedback, outcome, content);
      root.replaceChildren(view);
      let previous: string | undefined;
      let shown: Today | undefined;
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
      function say(tone: string, text: string): void {
        outcome.textContent = text;
        outcome.dataset.tone = tone;
        outcome.hidden = false;
      }
      const controls = new ApprovalControls(approvals, say);

      /** Redraw, keeping open panels, expanded backgrounds, typed comments and focus. */
      function paint(snapshot: Today): void {
        const open = new Set(Array.from(content.querySelectorAll<HTMLDetailsElement>("details[open]"), (detail) => detail.dataset.key));
        const expanded = new Set(Array.from(content.querySelectorAll<HTMLElement>('[aria-expanded="true"]'), (link) => link.dataset.focus));
        const typed = new Map(Array.from(content.querySelectorAll<HTMLTextAreaElement>("textarea[data-focus]"), (field) => [field.dataset.focus, field.value]));
        const focused = content.contains(document.activeElement)
          ? (document.activeElement as HTMLElement).dataset.focus : undefined;
        content.replaceChildren(drawToday(snapshot, controls, shared));
        shown = snapshot;
        for (const detail of content.querySelectorAll<HTMLDetailsElement>("details")) {
          detail.open = open.has(detail.dataset.key);
        }
        for (const field of content.querySelectorAll<HTMLTextAreaElement>("textarea[data-focus]")) {
          field.value = typed.get(field.dataset.focus) ?? "";
        }
        for (const link of content.querySelectorAll<HTMLElement>("[data-focus]")) {
          if (expanded.has(link.dataset.focus)) link.click();
          if (focused !== undefined && link.dataset.focus === focused) link.focus();
        }
      }
      controls.bind(() => {
        if (shown !== undefined && root.contains(view) && view.isConnected) paint(shown);
      });

      async function refresh(): Promise<void> {
        if (!root.contains(view) || !view.isConnected) return;
        try {
          const [snapshot, left] = await Promise.all([load(), lastSkipped(sharedSync)]);
          if (!root.contains(view) || !view.isConnected) return;
          skipped = left;
          const serialized = JSON.stringify([snapshot, left]);
          if (serialized !== previous) {
            paint(snapshot);
            previous = serialized;
          }
          feedback.replaceChildren();
        } catch {
          if (!root.contains(view) || !view.isConnected) return;
          feedback.replaceChildren(errorState("Couldn’t refresh your work", previous === undefined
            ? "Please try again. We’ll keep checking for your work."
            : "The work below may have changed. We’ll try again shortly."));
          if (previous === undefined) content.replaceChildren();
        } finally {
          content.setAttribute("aria-busy", "false");
          // Navigation replaces view. A pending read never redraws another screen.
          if (root.contains(view) && view.isConnected) window.setTimeout(() => { void refresh(); }, pollIntervalMs);
        }
      }
      /** The card follows the calendar; a failed read hides it rather than showing stale times. */
      async function nextMeeting(): Promise<void> {
        if (!root.contains(view) || !view.isConnected) return;
        const now = new Date();
        try {
          const next = upcoming(await meetings(now), now);
          if (!root.contains(view) || !view.isConnected) return;
          nextSlot.replaceChildren(...(next === null ? [] : [nextMeetingCard(next, now)]));
          nextSlot.hidden = next === null;
        } catch {
          nextSlot.replaceChildren();
          nextSlot.hidden = true;
        } finally {
          nextSlot.setAttribute("aria-busy", "false");
          if (root.contains(view) && view.isConnected) window.setTimeout(() => { void nextMeeting(); }, meetingIntervalMs);
        }
      }
      void nextMeeting();
      await refresh();
    },
  };
}

export const screen: Screen = createTodayScreen();
