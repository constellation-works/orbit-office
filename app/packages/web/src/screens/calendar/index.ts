/**
 * Calendar: the user's week as the agents see it (docs/design/office/specs/
 * calendar.md, "App"). A week from Monday at desktop widths and one day at a
 * time on a phone; meetings as blocks with their state in words; the
 * routines' runs as marks in each day's margin. The calendar is read only:
 * nothing here writes back to it.
 */
import type { CalendarMeeting, CalendarRoutine, CalendarView } from "@orbit-office/contracts";

import { ApiError, getCalendar, syncCalendar } from "../../api/client.js";
import type { Screen } from "../../screen.js";
import { card, errorState, requestRailRefresh } from "../../ui/index.js";
import {
  addDays, clock, dayRuns, hourRange, meetingState, onDay, placeDay, routineTitle, sameDay, stateWords,
  syncedWords, weekLabel, weekStart,
} from "./model.js";
import { meetingPanel } from "./panel.js";
import { styles } from "./style.js";

function element<K extends keyof HTMLElementTagNameMap>(tag: K, text = "", className?: string): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  node.textContent = text;
  if (className !== undefined) node.className = className;
  return node;
}
function button(label: string, onClick: () => void): HTMLButtonElement {
  const node = element("button", label);
  node.type = "button";
  node.addEventListener("click", onClick);
  return node;
}

/** One meeting block: title, time and every state in words. */
function block(meeting: CalendarMeeting, now: Date, lead: number, open: (meeting: CalendarMeeting) => void): HTMLButtonElement {
  const node = element("button", "", "calendar-meeting");
  node.type = "button";
  const state = meetingState(meeting, now, lead);
  if (state !== null) node.dataset.tone = state.tone;
  if (meeting.private) node.dataset.private = "true";
  if (meeting.status === "cancelled") node.dataset.cancelled = "true";
  node.dataset.key = meeting.key;
  node.title = [meeting.title, ...stateWords(meeting, now, lead)].join(" · ");
  const time = meeting.all_day ? "All day" : `${clock(new Date(meeting.start))}–${clock(new Date(meeting.end))}`;
  // The state comes straight after the title, so even a short block shows it in words.
  const words = stateWords(meeting, now, lead);
  // A button centres overflowing content; the inner box keeps the title on top.
  const inner = element("span", "", "calendar-meeting-inner");
  inner.append(element("span", meeting.title, "calendar-meeting-title"));
  if (words.length > 0) inner.append(element("span", words.join(" · "), "calendar-meeting-state"));
  inner.append(element("span", time, "calendar-meeting-meta"));
  node.append(inner);
  node.addEventListener("click", () => open(meeting));
  return node;
}

/** Marks for one routine on one day, in that day's margin; each opens Routines. */
function marks(routine: CalendarRoutine, day: Date, first: number, last: number): HTMLElement[] {
  const runs = dayRuns(routine, day);
  if (runs === null) return [];
  const name = routineTitle(routine.name);
  const paused = routine.enabled ? "" : ", paused";
  if (runs.kind === "band") {
    const band = element("a", "", `calendar-band${routine.enabled ? "" : " calendar-band--paused"}`);
    band.href = "#routines";
    band.setAttribute("aria-label", `${name}, ${runs.words.toLowerCase()}${paused}`);
    band.title = `${name} · ${runs.words}${paused}`;
    return [band];
  }
  const span = (last - first) * 60;
  return runs.times.map(at => {
    const mark = element("a", "", `calendar-mark${routine.enabled ? "" : " calendar-mark--paused"}`);
    mark.href = "#routines";
    const minutes = at.getHours() * 60 + at.getMinutes() - first * 60;
    mark.style.top = `${Math.min(100, Math.max(0, (minutes / span) * 100)).toFixed(3)}%`;
    mark.setAttribute("aria-label", `${name} at ${clock(at)}${paused}`);
    mark.title = `${name} · ${clock(at)}${paused}`;
    return mark;
  });
}

interface State {
  /** Monday of the week shown. */
  week: Date;
  /** The day a phone shows, 0 (Monday) to 6. */
  day: number;
  /** The meeting whose panel is open. */
  open: string | null;
}

function grid(view: CalendarView, state: State, now: Date, open: (meeting: CalendarMeeting) => void): HTMLElement {
  const days = Array.from({ length: 7 }, (_, index) => addDays(state.week, index));
  const lead = view.prep_lead_minutes;
  const { first, last } = hourRange(view.meetings, days);
  const allDayRows = Math.max(1, ...days.map(day => view.meetings.filter(meeting => meeting.all_day && onDay(meeting, day)).length));
  const table = element("div", "", "calendar-grid");
  table.style.setProperty("--hours", String(last - first));
  table.style.setProperty("--allday", `${allDayRows * 34 + 6}px`);
  table.setAttribute("aria-label", `Week of ${weekLabel(state.week)}`);

  const gutter = element("div", "", "calendar-col calendar-col--gutter");
  gutter.setAttribute("aria-hidden", "true");
  const hours = element("div", "", "calendar-body");
  for (let hour = first; hour < last; hour += 1) {
    const label = element("span", `${String(hour).padStart(2, "0")}:00`, "calendar-hour");
    label.style.top = `calc(${hour - first} * var(--hour))`;
    hours.append(label);
  }
  gutter.append(element("div", "", "calendar-head"), element("div", "All day", "calendar-allday"), hours);
  table.append(gutter);

  days.forEach((day, index) => {
    const column = element("section", "", "calendar-col");
    column.dataset.selected = String(index === state.day);
    if (sameDay(day, now)) column.dataset.today = "true";
    const name = day.toLocaleDateString(undefined, { weekday: "long", day: "numeric", month: "long" });
    column.setAttribute("aria-label", name);
    const head = element("div", "", "calendar-head");
    head.append(element("span", day.toLocaleDateString(undefined, { weekday: "short" })), element("strong", String(day.getDate())));
    const allDay = element("div", "", "calendar-allday");
    for (const meeting of view.meetings.filter(item => item.all_day && onDay(item, day))) allDay.append(block(meeting, now, lead, open));
    const body = element("div", "", "calendar-body");
    const margin = element("div", "", "calendar-margin");
    margin.setAttribute("aria-label", "Routine runs");
    for (const routine of view.routines) margin.append(...marks(routine, day, first, last));
    const events = element("div", "", "calendar-events");
    for (const placed of placeDay(view.meetings, day)) {
      const node = block(placed.meeting, now, lead, open);
      const top = Math.max(0, placed.from - first * 60);
      const bottom = Math.min((last - first) * 60, placed.to - first * 60);
      node.style.top = `calc(${(top / 60).toFixed(4)} * var(--hour))`;
      node.style.height = `calc(${(Math.max(15, bottom - top) / 60).toFixed(4)} * var(--hour) - 2px)`;
      node.style.left = `${((placed.lane / placed.lanes) * 100).toFixed(3)}%`;
      node.style.width = `calc(${(100 / placed.lanes).toFixed(3)}% - 2px)`;
      if (placed.lanes > 1) node.dataset.narrow = "true";
      events.append(node);
    }
    body.append(margin, events);
    if (sameDay(day, now)) {
      const minutes = now.getHours() * 60 + now.getMinutes();
      if (minutes >= first * 60 && minutes <= last * 60) {
        const line = element("div", "", "calendar-now");
        line.style.top = `calc(${((minutes - first * 60) / 60).toFixed(4)} * var(--hour))`;
        line.setAttribute("role", "img");
        line.setAttribute("aria-label", `Now, ${clock(now)}`);
        body.append(line);
      }
    }
    column.append(head, allDay, body);
    table.append(column);
  });
  return table;
}

export interface CalendarScreenOptions {
  load?: (from: Date, until: Date) => Promise<CalendarView>;
  sync?: () => Promise<unknown>;
  now?: () => Date;
}

export function createCalendarScreen({ load = getCalendar, sync = syncCalendar, now = () => new Date() }: CalendarScreenOptions = {}): Screen {
  async function render(root: HTMLElement): Promise<void> {
    const page = element("section", "", "calendar");
    const style = element("style", styles);
    const header = element("header", "", "calendar-header");
    const titles = element("div");
    titles.append(element("h1", "Calendar"), element("p", "Your meetings with their prep and notes, and the routines that run. Read from your calendar; nothing is written back.", "intro"));
    const syncLine = element("div", "", "calendar-sync");
    const syncState = element("span");
    syncState.setAttribute("role", "status");
    header.append(titles, syncLine);
    const body = element("div");
    page.append(style, header, body);
    root.replaceChildren(page);
    const alive = (): boolean => page.isConnected;

    const today = now();
    const state: State = { week: weekStart(today), day: (today.getDay() + 6) % 7, open: null };
    const requested = new URLSearchParams(window.location.search).get("meeting");
    if (requested !== null) {
      state.open = requested;
      window.history.replaceState(null, "", `${window.location.pathname}${window.location.hash}`);
    }
    let generation = 0;
    let shown: CalendarView | null = null;
    let panel: HTMLElement | null = null;

    function closePanel(): void {
      state.open = null;
      panel?.remove(); panel = null;
    }
    function openPanel(meeting: CalendarMeeting): void {
      panel?.remove();
      state.open = meeting.key;
      panel = meetingPanel(meeting, {
        now: now(), leadMinutes: shown?.prep_lead_minutes ?? 120, close: closePanel,
        captured: async key => { state.open = key; requestRailRefresh(); await refresh(); },
      });
      page.append(panel);
      panel.querySelector<HTMLButtonElement>(".calendar-close")?.focus();
    }
    page.addEventListener("keydown", event => { if (event.key === "Escape" && panel !== null) closePanel(); });

    const syncButton = button("Sync now", () => {
      syncButton.disabled = true; syncState.textContent = "Syncing…";
      void sync().then(async () => { requestRailRefresh(); await refresh(); })
        .catch((error: unknown) => {
          syncState.textContent = error instanceof ApiError && error.code === "calendar_unconfigured"
            ? "No calendar is connected yet." : "The sync failed. Try again in a moment.";
        })
        .finally(() => { syncButton.disabled = false; });
    });
    syncLine.append(syncState, syncButton);

    function draw(view: CalendarView): void {
      shown = view;
      const time = now();
      const parts: HTMLElement[] = [];
      syncState.textContent = view.synced_at === null ? "" : view.sync_failed
        ? `Last synced ${syncedWords(view.synced_at, time)}, the latest sync failed`
        : `Last synced ${syncedWords(view.synced_at, time)}`;
      if (view.synced_at === null) {
        const content = element("div", "", "card-content");
        content.append(element("p", view.sync_failed
          ? "The latest sync failed before any meetings were read. Check the calendar connection, then sync again."
          : "Add your Outlook or ICS calendar as a connection, then sync. Office reads it every 15 minutes and never writes back."));
        const link = element("a", "Open Settings › Connections");
        link.href = "#settings";
        content.append(link);
        const connect = card("Connect your calendar", content, undefined, { label: "Not connected yet", tone: "attention" });
        connect.classList.add("calendar-connect");
        body.replaceChildren(connect);
        return;
      }
      if (view.sync_failed) {
        const notice = element("p", `Last synced ${syncedWords(view.synced_at, time)}, the latest sync failed. These are the meetings from the last good sync.`, "calendar-notice");
        notice.dataset.tone = "attention";
        notice.setAttribute("role", "alert");
        parts.push(notice);
      }
      const nav = element("div", "", "calendar-nav");
      nav.setAttribute("role", "group");
      nav.setAttribute("aria-label", "Week");
      const move = (weeks: number): void => {
        state.week = weeks === 0 ? weekStart(now()) : addDays(state.week, weeks * 7);
        state.day = weeks === 0 ? (now().getDay() + 6) % 7 : 0;
        closePanel();
        void refresh();
      };
      nav.append(button("Previous week", () => move(-1)), button("This week", () => move(0)), button("Next week", () => move(1)),
        element("span", weekLabel(state.week), "calendar-nav-label"));
      parts.push(nav);
      const picker = element("div", "", "calendar-days");
      picker.setAttribute("role", "group");
      picker.setAttribute("aria-label", "Day");
      for (let index = 0; index < 7; index += 1) {
        const day = addDays(state.week, index);
        const choose = button(day.toLocaleDateString(undefined, { weekday: "short", day: "numeric" }), () => {
          state.day = index;
          for (const other of picker.querySelectorAll("button")) other.setAttribute("aria-pressed", String(other === choose));
          body.querySelectorAll<HTMLElement>(".calendar-grid > section.calendar-col").forEach((column, at) => {
            column.dataset.selected = String(at === index);
          });
        });
        choose.setAttribute("aria-pressed", String(index === state.day));
        choose.setAttribute("aria-label", day.toLocaleDateString(undefined, { weekday: "long", day: "numeric", month: "long" }));
        if (sameDay(day, time)) choose.dataset.today = "true";
        picker.append(choose);
      }
      parts.push(picker);
      const meetings = view.meetings.filter(meeting => meeting.status !== "cancelled");
      if (meetings.length === 0) {
        const empty = element("p", view.meetings.length === 0 ? "No meetings this week." : "No meetings this week: the only one was cancelled.", "calendar-notice");
        empty.setAttribute("role", "status");
        parts.push(empty);
      }
      parts.push(grid(view, state, time, openPanel));
      const legend = element("p", "", "calendar-legend");
      const routines = element("a", "Open Routines");
      routines.href = "#routines";
      legend.append(element("span", "Marks in each day’s margin are routine runs:"), element("span", "will run", "calendar-key"),
        element("span", "paused, skipped", "calendar-key calendar-key--paused"), routines);
      if (view.routines_unavailable) legend.append(element("span", "Some routines could not be read, so their runs are not shown."));
      parts.push(legend);
      body.replaceChildren(...parts);
      if (state.open !== null) {
        const meeting = view.meetings.find(item => item.key === state.open);
        if (meeting === undefined) closePanel(); else openPanel(meeting);
      }
    }

    async function refresh(): Promise<void> {
      const token = ++generation;
      const from = state.week; const until = addDays(state.week, 7);
      try {
        const view = await load(from, until);
        if (!alive() || token !== generation) return;
        draw(view);
      } catch {
        if (!alive() || token !== generation) return;
        body.replaceChildren(errorState("Your calendar is unavailable", "Office could not read the calendar snapshot. Try again in a moment."));
      }
    }
    body.replaceChildren(element("p", "Loading your week…", "calendar-muted"));
    await refresh();
  }
  return { id: "calendar", title: "Calendar", status: "built", render };
}

export const screen: Screen = createCalendarScreen();
