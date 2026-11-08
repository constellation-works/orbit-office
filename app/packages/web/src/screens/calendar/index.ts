/**
 * Calendar: the user's week as the agents see it (docs/design/office/specs/
 * calendar.md, "App"). A week from Monday at desktop widths and one day at a
 * time on a phone; meetings and scheduled to-dos as blocks with their state
 * in words; the routines' runs as marks in each day's margin. External calendars
 * remain read only; local event edits stay in the workspace. A scheduled to-do's
 * time is Orbit's, and its panel can move or clear it; a to-do of the user's
 * shows on the day and time it is due, and an overdue one due before the week
 * shown sits at the top of today.
 */
import type { CalendarMeeting, CalendarRoutine, CalendarTask, CalendarView } from "@orbit-office/contracts";

import { ApiError, getCalendar, scheduleTodo, syncCalendar, changeLocalCalendar } from "../../api/client.js";
import type { Screen } from "../../screen.js";
import { button, element, errorState, requestRailRefresh } from "../../ui/index.js";
import { pickerStyles } from "../../ui/when.js";
import {
  addDays, clock, dayRuns, hourRange, meetingState, onDay, placeDay, routineTitle, sameDay, stateWords,
  isDue, syncedWords, taskSlot, taskState, taskWords, weekLabel, weekStart, type TaskSlot,
} from "./model.js";
import { eventEditor } from "./editor.js";
import { meetingPanel } from "./panel.js";
import { styles } from "./style.js";
import { taskPanel } from "./task.js";

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
  if (meeting.local) words.unshift("Local");
  if (words.length > 0) inner.append(element("span", words.join(" · "), "calendar-meeting-state"));
  inner.append(element("span", time, "calendar-meeting-meta"));
  node.append(inner);
  node.addEventListener("click", () => open(meeting));
  return node;
}

/**
 * One scheduled to-do's block: title, its state and whose it is in words, and
 * its time; "Due 09:00" for the user's own, with the date too when it falls
 * before the week shown.
 */
function taskBlock(task: CalendarTask, now: Date, open: (task: CalendarTask) => void, earlier = false): HTMLButtonElement {
  const node = element("button", "", "calendar-meeting calendar-task");
  node.type = "button";
  node.dataset.tone = taskState(task, now).tone;
  node.dataset.task = "true";
  if (isDue(task)) node.dataset.due = "true";
  const words = taskWords(task, now);
  node.title = [task.title, ...words].join(" · ");
  const at = new Date(task.not_before);
  const when = earlier ? `${at.toLocaleDateString(undefined, { weekday: "short", day: "numeric", month: "short" })} ${clock(at)}` : clock(at);
  const inner = element("span", "", "calendar-meeting-inner");
  inner.append(element("span", task.title, "calendar-meeting-title"), element("span", words.join(" · "), "calendar-meeting-state"),
    element("span", isDue(task) ? `Due ${when}` : `To-do · ${when}`, "calendar-meeting-meta"));
  node.append(inner);
  node.addEventListener("click", () => open(task));
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
  /** The meeting (by key) or scheduled to-do (`task:<id>`) whose panel is open. */
  open: string | null;
}

/** The open-panel key of a scheduled to-do; meeting keys never start with it. */
const taskKey = (task: CalendarTask): string => `task:${task.id}`;

function grid(view: CalendarView, state: State, now: Date, open: (meeting: CalendarMeeting) => void, openTask: (task: CalendarTask) => void): HTMLElement {
  const days = Array.from({ length: 7 }, (_, index) => addDays(state.week, index));
  const lead = view.prep_lead_minutes;
  // The user's to-dos already overdue before this week sit at the top of today, so they stay in view.
  const earlier = view.tasks.filter(task => Date.parse(task.not_before) < state.week.getTime());
  const slots = view.tasks.filter(task => !earlier.includes(task)).map(taskSlot);
  const { first, last } = hourRange([...view.meetings, ...slots], days);
  const allDayRows = Math.max(1, ...days.map(day => view.meetings.filter(meeting => meeting.all_day && onDay(meeting, day)).length +
    (sameDay(day, now) ? earlier.length : 0)));
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
    if (sameDay(day, now)) for (const task of earlier) allDay.append(taskBlock(task, now, openTask, true));
    const body = element("div", "", "calendar-body");
    const margin = element("div", "", "calendar-margin");
    margin.setAttribute("aria-label", "Routine runs");
    for (const routine of view.routines) margin.append(...marks(routine, day, first, last));
    const events = element("div", "", "calendar-events");
    for (const placed of placeDay<CalendarMeeting | TaskSlot>([...view.meetings, ...slots], day)) {
      const entry = placed.item;
      const node = "task" in entry ? taskBlock(entry.task, now, openTask) : block(entry, now, lead, open);
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
  /** Set a to-do's scheduled start, or clear it with null. */
  schedule?: (id: string, notBefore: string | null) => Promise<unknown>;
  now?: () => Date;
}

export function createCalendarScreen({ load = getCalendar, sync = syncCalendar, schedule = scheduleTodo, now = () => new Date() }: CalendarScreenOptions = {}): Screen {
  async function render(root: HTMLElement): Promise<void> {
    const page = element("section", "", "calendar");
    const style = element("style", styles + pickerStyles);
    const header = element("header", "", "screen-head calendar-header");
    const titles = element("div");
    const intro = element("p", "Your meetings with their prep and notes, the to-dos scheduled for a time or due then, and the routines that run. Local events stay on this machine. Connected calendars are read only. ", "intro");
    const connectLink = element("a", "Connect a calendar here");
    connectLink.href = "?settings=calendar#settings";
    intro.append(connectLink);
    titles.append(element("h1", "Calendar"), intro);
    const actions = element("div", "", "screen-head-actions calendar-actions");
    const syncLine = element("div", "", "calendar-sync");
    const syncState = element("span", "", "mono-meta");
    syncState.setAttribute("role", "status");
    // The week controls sit in the header, as in the approved mock; they stay when a read fails.
    const nav = element("div", "", "calendar-nav");
    nav.setAttribute("role", "group");
    nav.setAttribute("aria-label", "Week");
    const navLabel = element("span", "", "calendar-nav-label");
    actions.append(syncLine, nav);
    header.append(titles, actions);
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
    function editLocal(meeting: CalendarMeeting | null): void {
      panel?.remove(); state.open = null;
      panel = eventEditor(meeting, addDays(state.week, state.day), changeLocalCalendar, async () => { closePanel(); requestRailRefresh(); await refresh(); }, closePanel);
      page.append(panel); panel.querySelector<HTMLInputElement>("input")?.focus();
    }
    function openPanel(meeting: CalendarMeeting): void {
      panel?.remove();
      state.open = meeting.key;
      panel = meetingPanel(meeting, {
        now: now(), leadMinutes: shown?.prep_lead_minutes ?? 120, close: closePanel,
        captured: async key => { state.open = key; requestRailRefresh(); await refresh(); },
      });
      if (meeting.local) {
        const actions = element("div", "", "calendar-task-buttons");
        const feedback = element("p"); feedback.setAttribute("role", "status");
        const remove = button("Delete local event", () => {
          actions.replaceChildren(element("span", "Delete this local event? Notes and summaries stay in the Library."), button("Cancel", () => openPanel(meeting)), button("Delete event", () => {
            for (const control of actions.querySelectorAll("button")) control.disabled = true;
            void changeLocalCalendar({action:"delete",key:meeting.key}).then(async () => { closePanel(); requestRailRefresh(); await refresh(); })
              .catch(() => { feedback.textContent = "The event could not be deleted. Open it again to retry."; });
          }));
        });
        actions.append(button("Edit local event", () => editLocal(meeting)), remove); panel.append(actions, feedback);
      }
      page.append(panel);
      panel.querySelector<HTMLButtonElement>(".calendar-close")?.focus();
    }
    function openTaskPanel(task: CalendarTask): void {
      panel?.remove();
      state.open = taskKey(task);
      panel = taskPanel(task, {
        now: now(), close: closePanel,
        schedule: async (id, notBefore) => { await schedule(id, notBefore); await refresh(); },
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
    const newEvent = button("New local event", () => editLocal(null));
    syncLine.append(syncState, syncButton, newEvent);
    const move = (weeks: number): void => {
      state.week = weeks === 0 ? weekStart(now()) : addDays(state.week, weeks * 7);
      state.day = weeks === 0 ? (now().getDay() + 6) % 7 : 0;
      navLabel.textContent = weekLabel(state.week);
      closePanel();
      void refresh();
    };
    const arrow = (glyph: string, label: string, weeks: number): HTMLButtonElement => {
      const control = button(glyph, () => move(weeks));
      control.setAttribute("aria-label", label);
      control.className = "calendar-arrow";
      return control;
    };
    nav.append(navLabel, arrow("‹", "Previous week", -1), button("This week", () => move(0)), arrow("›", "Next week", 1));
    navLabel.textContent = weekLabel(state.week);

    function draw(view: CalendarView): void {
      shown = view;
      const time = now();
      const parts: HTMLElement[] = [];
      syncState.textContent = view.synced_at === null ? "" : view.sync_failed
        ? `Last synced ${syncedWords(view.synced_at, time)}, the latest sync failed`
        : `Last synced ${syncedWords(view.synced_at, time)}`;
      newEvent.hidden = view.sources?.local !== true;
      syncButton.hidden = view.sources?.external === false;
      if (view.sync_failed && view.synced_at !== null) {
        const notice = element("p", `Last synced ${syncedWords(view.synced_at, time)}, the latest sync failed. These are the meetings from the last good sync.`, "calendar-notice");
        notice.dataset.tone = "attention";
        notice.setAttribute("role", "alert");
        parts.push(notice);
      }
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
      parts.push(grid(view, state, time, openPanel, openTaskPanel));
      const legend = element("p", "", "calendar-legend");
      const routines = element("a", "Open Routines");
      routines.href = "#routines";
      legend.append(element("span", "Meeting", "calendar-swatch"), element("span", "Needs you", "calendar-swatch calendar-swatch--attention"),
        element("span", "Scheduled or due to-do", "calendar-swatch calendar-swatch--task"),
        element("span", "Marks in each day’s margin are routine runs:"), element("span", "will run", "calendar-key"),
        element("span", "paused, skipped", "calendar-key calendar-key--paused"), routines);
      if (view.routines_unavailable) legend.append(element("span", "Some routines could not be read, so their runs are not shown."));
      if (view.scheduled_unavailable) legend.append(element("span", "Some scheduled to-dos could not be read, so not all of them are shown."));
      parts.push(legend);
      body.replaceChildren(...parts);
      if (state.open !== null) {
        const meeting = view.meetings.find(item => item.key === state.open);
        const task = view.tasks.find(item => taskKey(item) === state.open);
        if (meeting !== undefined) openPanel(meeting);
        else if (task !== undefined) openTaskPanel(task);
        else closePanel();
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
