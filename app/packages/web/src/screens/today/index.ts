/** Today: the next meeting, the user's next steps and the actions on them, live agent work, openable outcomes and the latest handoffs. */
import type {
  AgentWork, AgentWorkTask, CalendarMeeting, ScrCard, ScrFeed, SharedLastSync, SharedSkipped, Today, TodayItem, TodayNeed, TodayResult,
} from "@orbit-office/contracts";

import { dismissSharedSkipped, getJson, getLastSharedSync, uploadSource } from "../../api/client.js";
import { sourceHref } from "../../api/source-location.js";
import type { Screen } from "../../screen.js";
import { ageLabel, cardSummary, cardTitle, scopeLabel, SCRIBE_FEED_HREF, tagCountsLabel } from "../../ui/handoff.js";
import { card, emptyState, errorState, monoMeta, panel, panelBody, panelGrid, pill, provenanceLink, row, rows } from "../../ui/index.js";
import { ApprovalControls, serverApprovals, type ApprovalApi } from "./approvals.js";
import { digestPanel, digestSummary, readDigest, serverDigest, type DigestApi, type DigestState } from "./digest.js";
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

/** Feed cards Today lists at most; the rest are on the Scribe feed. */
const HANDOFFS_SHOWN = 5;

function handoffLine(card: ScrCard, now: Date): HTMLElement {
  const item = document.createElement("li");
  item.className = "today-handoff";
  item.dataset.id = card.id;
  const meta = document.createElement("p");
  meta.className = "today-handoff-meta";
  meta.append(document.createTextNode(`${scopeLabel(card)} · ${cardTitle(card)}`));
  const time = document.createElement("time");
  time.dateTime = card.updated;
  time.textContent = ageLabel(card.updated, now);
  meta.append(document.createTextNode(" · "), time);
  if (!card.withheld) meta.append(document.createTextNode(` · ${tagCountsLabel(card)}`));
  const summary = paragraph(cardSummary(card));
  summary.className = "today-handoff-text";
  item.append(meta, summary);
  return item;
}

/** The compact Handoffs summary: the same cards as the Scribe feed, most recently updated first, linking to Scribe. */
function handoffSection(feed: ScrFeed): HTMLElement | null {
  if (feed.cards.length === 0) return null;
  const now = new Date();
  const shown = [...feed.cards].sort((a, b) => b.updated.localeCompare(a.updated)).slice(0, HANDOFFS_SHOWN);
  const element = document.createElement("section");
  element.className = "today-section today-handoffs";
  element.setAttribute("aria-label", "Handoffs");
  const header = document.createElement("div");
  header.className = "today-section-heading";
  const heading = document.createElement("h2");
  heading.textContent = "Handoffs";
  header.append(heading, pill(String(feed.cards.length)));
  const list = document.createElement("ul");
  list.className = "today-handoff-list";
  for (const card of shown) list.append(handoffLine(card, now));
  const more = document.createElement("a");
  more.href = SCRIBE_FEED_HREF;
  more.className = "today-handoff-more";
  more.textContent = "Open Scribe";
  element.append(header, list, more);
  return element;
}

/**
 * The "Agents working on" rows: in-progress agent tasks in the office
 * workspace, then those in the followed and target workspaces with their crew
 * (`GET /api/agents`, when it can be read).
 */
function workingRows(snapshot: Today, elsewhere: readonly AgentWorkTask[]): HTMLElement {
  const office = snapshot.agents_working_on.map((item) => row(item.title, { dot: "working", details: [background(item)] }));
  const titles = new Set(snapshot.agents_working_on.map((item) => item.key));
  const others = elsewhere.filter((task) => !titles.has(task.id)).map((task) => row(task.title, {
    dot: "working",
    details: [monoMeta(`${task.workspace} · ${task.id}`)],
    trail: task.crew === null ? [] : [monoMeta(task.crew)],
  }));
  if (office.length + others.length === 0) return paragraph("No agent work is underway.");
  return rows([...office, ...others], "Agents working on");
}

/** Drop a recording or notes: it is captured as a source, then opened there. */
function dropPanel(): HTMLElement {
  const element = panel("Drop a recording or notes", { variant: "dashed", id: "today-drop-heading" });
  element.dataset.slot = "drop-zone";
  const hint = paragraph("Audio, .vtt or pasted text. Muse Voice transcribes it; Muse Spark finds the decisions and to-dos.");
  const label = document.createElement("label");
  label.className = "primary-action today-choose";
  label.textContent = "Choose a file";
  const input = document.createElement("input");
  input.type = "file";
  input.className = "today-file";
  label.append(input);
  const status = paragraph("");
  status.setAttribute("aria-live", "polite");
  status.hidden = true;
  input.addEventListener("change", () => {
    const file = input.files?.[0];
    if (file === undefined) return;
    status.textContent = `Uploading ${file.name}…`;
    status.hidden = false;
    void uploadSource(file).then((source) => {
      window.location.assign(sourceHref(source.id));
    }).catch(() => {
      status.textContent = "Couldn’t add that file. Try it on the Source screen.";
    });
  });
  const more = document.createElement("a");
  more.href = "#source";
  more.textContent = "Paste notes or add a folder";
  element.append(panelBody(hint, label, more, status));
  return element;
}

function drawToday(snapshot: Today, controls: ApprovalControls, shared: SharedNeeds, handoffs: ScrFeed | null): HTMLElement {
  const body = document.createElement("div");
  const needed = needCards(snapshot.needs_you, controls, shared);
  const needs = cardSection("Needs you", needed, "Nothing needs your attention.");
  const done = section("Done today", snapshot.done_today, "No completed work yet today.", resultCard);
  done.classList.add("today-done");
  body.append(needs, done);
  const handoffBlock = handoffs === null ? null : handoffSection(handoffs);
  if (handoffBlock !== null) body.append(handoffBlock);
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
  /** The Scribe feed cards for the Handoffs section; tests pass their own. A failed read hides the section. */
  handoffs?: () => Promise<ScrFeed>;
  /** How often the Handoffs section rereads the list (it changes slowly). */
  handoffIntervalMs?: number;
  /** The latest shared-library sync the app ran, for its skipped documents; tests pass their own. */
  sharedSync?: { last(): Promise<SharedLastSync>; dismissSkipped(): Promise<unknown> };
  /** Agent work across the followed workspaces, for "Agents working on"; a failed read shows the office's only. */
  agents?: () => Promise<AgentWork>;
  /** The morning digest and contradiction calls; tests pass their own. */
  digest?: DigestApi;
  /** How often the morning digest rereads (it changes slowly). */
  digestIntervalMs?: number;
}

function greeting(now: Date): string {
  const hour = now.getHours();
  return hour < 12 ? "Good morning" : hour < 18 ? "Good afternoon" : "Good evening";
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
  handoffs = () => getJson<ScrFeed>("/api/scribe/feed"), handoffIntervalMs = 10_000,
  digest = serverDigest, digestIntervalMs = 30_000, agents = () => getJson<AgentWork>("/api/agents"),
}: TodayScreenOptions = {}): Screen {
  return {
    id: "today",
    title: "Today",
    status: "built",
    async render(root) {
      const view = document.createElement("div");
      view.className = "today";
      const started = new Date();
      const header = document.createElement("header");
      header.className = "screen-head";
      const titles = document.createElement("div");
      const date = paragraph(started.toLocaleDateString(undefined, { weekday: "long", day: "numeric", month: "long" }));
      date.className = "today-date";
      const heading = document.createElement("h1");
      heading.textContent = greeting(started);
      const intro = paragraph("Your next steps and the work moving forward.");
      intro.className = "intro";
      titles.append(date, heading, intro);
      const openMeeting = document.createElement("a");
      openMeeting.className = "link-button";
      openMeeting.textContent = "Open the meeting";
      openMeeting.hidden = true;
      header.append(titles, openMeeting);
      const digestSlot = document.createElement("div");
      digestSlot.className = "today-digest-slot";
      digestSlot.setAttribute("aria-busy", "true");
      const working = panel("Agents working on", { variant: "plain", id: "today-working-heading" });
      working.classList.add("today-working");
      const workingBody = panelBody(paragraph("Gathering agent work…"));
      working.append(workingBody);
      const next = panel("Next meeting", { variant: "plain", id: "today-next-heading" });
      const nextSlot = panelBody(paragraph("Reading your calendar…"));
      nextSlot.className = "panel-body today-next-slot";
      nextSlot.setAttribute("aria-busy", "true");
      next.append(nextSlot);
      const grid = panelGrid(working, next, dropPanel());
      grid.classList.add("today-grid");
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
      view.append(header, digestSlot, grid, feedback, outcome, content);
      root.replaceChildren(view);
      let previous: string | undefined;
      let shown: Today | undefined;
      let elsewhere: AgentWorkTask[] = [];
      let handoffList: ScrFeed | null = null;
      let handoffsRead = Number.NEGATIVE_INFINITY;
      /** The feed cards, reread at most every `handoffIntervalMs`; null when they can't be read. */
      async function latestHandoffs(): Promise<ScrFeed | null> {
        if (Date.now() - handoffsRead < handoffIntervalMs) return handoffList;
        handoffsRead = Date.now();
        try {
          const feed = await handoffs();
          handoffList = Array.isArray(feed?.cards) ? feed : null;
        } catch { handoffList = null; }
        return handoffList;
      }
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
        content.replaceChildren(drawToday(snapshot, controls, shared, handoffList));
        workingBody.replaceChildren(workingRows(snapshot, elsewhere));
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
          const [snapshot, left, latest] = await Promise.all([load(), lastSkipped(sharedSync), latestHandoffs()]);
          if (!root.contains(view) || !view.isConnected) return;
          skipped = left;
          const serialized = JSON.stringify([snapshot, left, latest]);
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
          if (previous === undefined) {
            content.replaceChildren();
            workingBody.replaceChildren(paragraph("Couldn’t read agent work."));
          }
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
          const meeting = upcoming(await meetings(now), now);
          if (!root.contains(view) || !view.isConnected) return;
          nextSlot.replaceChildren(meeting === null ? paragraph("Nothing else on your calendar today.") : nextMeetingCard(meeting, now));
        } catch {
          // Stale times are worse than none.
          nextSlot.replaceChildren(paragraph("Couldn’t read your calendar."));
        } finally {
          nextSlot.setAttribute("aria-busy", "false");
          if (root.contains(view) && view.isConnected) window.setTimeout(() => { void nextMeeting(); }, meetingIntervalMs);
        }
      }
      let digestShown: string | undefined;
      /** The digest rereads slowly, and at once after a contradiction is resolved. */
      async function morningDigest(again = true): Promise<void> {
        if (!root.contains(view) || !view.isConnected) return;
        let state: DigestState;
        try { state = await readDigest(digest); }
        finally { digestSlot.setAttribute("aria-busy", "false"); }
        if (!root.contains(view) || !view.isConnected) return;
        const serialized = JSON.stringify(state);
        // A redraw would drop a pending choice's status line; only draw on change.
        if (serialized !== digestShown) {
          digestShown = serialized;
          digestSlot.replaceChildren(digestPanel(state, digest, () => { window.setTimeout(() => { void morningDigest(false); }, 1_500); }));
          const summary = digestSummary(state);
          intro.textContent = summary ?? "Your next steps and the work moving forward.";
          const source = state.digest?.source ?? null;
          openMeeting.hidden = source === null;
          if (source !== null) openMeeting.href = sourceHref(source.id);
        }
        if (again && root.contains(view) && view.isConnected) window.setTimeout(() => { void morningDigest(); }, digestIntervalMs);
      }
      /** Agent work in other workspaces: its own slower loop, so a slow read never holds Today. */
      async function agentWork(): Promise<void> {
        if (!root.contains(view) || !view.isConnected) return;
        const work = await agents().catch(() => null);
        if (!root.contains(view) || !view.isConnected) return;
        // A malformed answer reads as no other work, never as a crash.
        elsewhere = work !== null && Array.isArray(work.working_now)
          ? work.working_now.filter((task) => typeof task?.title === "string" && task.workspace !== "office") : [];
        if (shown !== undefined) workingBody.replaceChildren(workingRows(shown, elsewhere));
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
