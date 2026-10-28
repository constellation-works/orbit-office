/**
 * The Calendar screen's reading of the snapshot, with no DOM: the week, each
 * meeting's state in words, where blocks and routine marks sit, and the
 * rail's count of today's meetings still without notes.
 * Times are this computer's local time, like the Routines screen's.
 */
import type { CalendarMeeting, CalendarRoutine } from "@orbit-office/contracts";

import { cronFires, scheduleWords } from "../../ui/schedule.js";

/** Local midnight at the start of `date`'s day. */
export function startOfDay(date: Date): Date {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate());
}

export function addDays(date: Date, days: number): Date {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate() + days, date.getHours(), date.getMinutes());
}

/** Monday 00:00 of the week holding `date`: weeks start on Monday. */
export function weekStart(date: Date): Date {
  const day = startOfDay(date);
  return addDays(day, -((day.getDay() + 6) % 7));
}

export function sameDay(a: Date, b: Date): boolean {
  return a.getFullYear() === b.getFullYear() && a.getMonth() === b.getMonth() && a.getDate() === b.getDate();
}

/** `09:05`, whatever the locale, so "Prep at 12:00" reads the same everywhere. */
export function clock(at: Date): string {
  return `${String(at.getHours()).padStart(2, "0")}:${String(at.getMinutes()).padStart(2, "0")}`;
}

/** The day an all-day event falls on: its date, which the snapshot keeps at UTC midnight. */
export function allDayDate(iso: string): Date {
  const at = new Date(iso);
  return new Date(at.getUTCFullYear(), at.getUTCMonth(), at.getUTCDate());
}

/** Whether a meeting shows on `day`: overlapping it for timed ones, by date for all-day ones. */
export function onDay(meeting: CalendarMeeting, day: Date): boolean {
  if (meeting.all_day) {
    const first = allDayDate(meeting.start).getTime();
    const last = allDayDate(meeting.end).getTime();
    const at = startOfDay(day).getTime();
    return at >= first && (at < last || first === last);
  }
  const from = startOfDay(day).getTime();
  return Date.parse(meeting.start) < addDays(startOfDay(day), 1).getTime() && Date.parse(meeting.end) > from;
}

/** When the prep routine, every 15 minutes, first finds the meeting inside its lead time. */
export function prepTime(meeting: CalendarMeeting, leadMinutes: number): Date {
  const at = Date.parse(meeting.start) - leadMinutes * 60_000;
  const quarter = 15 * 60_000;
  return new Date(Math.ceil(at / quarter) * quarter);
}

/** How a state reads at a glance; the words carry the meaning, the tone only repeats it. */
export type StateTone = "attention" | "working" | "done" | "neutral";
export interface MeetingState { words: string; tone: StateTone }

/**
 * A meeting's state in words: upcoming ones by prep, past ones by notes,
 * cancelled ones by that alone. All-day events have no prep or notes.
 */
export function meetingState(meeting: CalendarMeeting, now: Date, leadMinutes: number): MeetingState | null {
  if (meeting.status === "cancelled") return { words: "Cancelled", tone: "neutral" };
  if (meeting.all_day) return null;
  if (Date.parse(meeting.end) <= now.getTime()) {
    if (meeting.capture === null) return { words: "Add notes", tone: "attention" };
    return meeting.capture.summarized ? { words: "Summary saved", tone: "done" } : { words: "Notes added", tone: "working" };
  }
  if (meeting.prep !== null) return { words: "Prep ready", tone: "done" };
  if (Date.parse(meeting.start) <= now.getTime()) return { words: "Happening now", tone: "working" };
  const at = prepTime(meeting, leadMinutes);
  return at.getTime() > now.getTime() ? { words: `Prep at ${clock(at)}`, tone: "working" } : { words: "Prep on its way", tone: "working" };
}

/** Every word a block shows besides its title, in reading order. */
export function stateWords(meeting: CalendarMeeting, now: Date, leadMinutes: number): string[] {
  const state = meetingState(meeting, now, leadMinutes);
  return [
    ...(state === null ? [] : [state.words]),
    ...(meeting.status === "tentative" ? ["Tentative"] : []),
    ...(meeting.private ? ["Private"] : []),
  ];
}

/** The rail's count: today's past meetings that have no notes yet. */
export function meetingsWithoutNotes(meetings: readonly CalendarMeeting[], now: Date): number {
  return meetings.filter(meeting => meeting.status !== "cancelled" && !meeting.all_day && meeting.capture === null &&
    Date.parse(meeting.end) <= now.getTime() && sameDay(new Date(meeting.start), now)).length;
}

/** Minutes since local midnight of `day`, clamped to that day. */
function minutesInto(day: Date, iso: string): number {
  return Math.min(1440, Math.max(0, (Date.parse(iso) - startOfDay(day).getTime()) / 60_000));
}

/** The hours drawn: always 08:00–18:00, widened to the earliest and latest meeting. */
export function hourRange(meetings: readonly CalendarMeeting[], days: readonly Date[]): { first: number; last: number } {
  let first = 8; let last = 18;
  for (const day of days) {
    for (const meeting of meetings) {
      if (meeting.all_day || !onDay(meeting, day)) continue;
      first = Math.min(first, Math.floor(minutesInto(day, meeting.start) / 60));
      last = Math.max(last, Math.ceil(minutesInto(day, meeting.end) / 60));
    }
  }
  return { first, last: Math.max(last, first + 1) };
}

export interface Placed { meeting: CalendarMeeting; from: number; to: number; lane: number; lanes: number }

/** Timed meetings on one day, side by side where they overlap. */
export function placeDay(meetings: readonly CalendarMeeting[], day: Date): Placed[] {
  const items = meetings.filter(meeting => !meeting.all_day && onDay(meeting, day))
    .map(meeting => ({ meeting, from: minutesInto(day, meeting.start), to: Math.max(minutesInto(day, meeting.end), minutesInto(day, meeting.start) + 15), lane: 0, lanes: 1 }))
    .sort((a, b) => a.from - b.from || b.to - a.to);
  let cluster: Placed[] = []; let clusterEnd = -1;
  const close = (): void => { const lanes = Math.max(...cluster.map(item => item.lane)) + 1; for (const item of cluster) item.lanes = lanes; };
  for (const item of items) {
    if (item.from >= clusterEnd && cluster.length > 0) { close(); cluster = []; }
    const busy = new Set(cluster.filter(other => other.to > item.from).map(other => other.lane));
    while (busy.has(item.lane)) item.lane += 1;
    cluster.push(item); clusterEnd = Math.max(clusterEnd, item.to);
  }
  if (cluster.length > 0) close();
  return items;
}

/** `office-daily-digest` reads as "Office daily digest", as on the Routines screen. */
export function routineTitle(name: string): string {
  const words = name.split(/[-_\s]+/).filter(Boolean).map(word => /^[^aeiouy]{2,3}$/i.test(word) ? word.toUpperCase() : word);
  const text = words.join(" ");
  return text.charAt(0).toUpperCase() + text.slice(1);
}

/** More runs than this in a day draw as one band instead of single marks. */
const MAX_MARKS = 24;

/** One day's runs of a routine: single times, a band for frequent ones, or nothing for event schedules. */
export type DayRuns = { kind: "marks"; times: Date[] } | { kind: "band"; words: string };

export function dayRuns(routine: CalendarRoutine, day: Date): DayRuns | null {
  const from = startOfDay(day); const until = addDays(from, 1);
  const { schedule } = routine;
  if (typeof schedule.cron === "string") {
    const fires = cronFires(schedule.cron, new Date(from.getTime() - 1), new Date(until.getTime() - 1), MAX_MARKS + 1);
    if (fires === null) return null;
    return fires.length > MAX_MARKS ? { kind: "band", words: scheduleWords(schedule) } : { kind: "marks", times: fires };
  }
  const minutes = schedule.every_minutes;
  if (typeof minutes !== "number" || minutes <= 0) return null;
  if (1440 / minutes > MAX_MARKS) return { kind: "band", words: scheduleWords(schedule) };
  if (routine.last_fired_at === null) return null;
  const step = minutes * 60_000;
  const last = Date.parse(routine.last_fired_at);
  if (Number.isNaN(last)) return null;
  const times: Date[] = [];
  let at = last + Math.ceil((from.getTime() - last) / step) * step;
  for (; at < until.getTime() && times.length <= MAX_MARKS; at += step) times.push(new Date(at));
  return { kind: "marks", times };
}

/** `26 Oct – 1 Nov 2026`. */
export function weekLabel(start: Date): string {
  const end = addDays(start, 6);
  const first = start.toLocaleDateString(undefined, { day: "numeric", month: "short" });
  const last = end.toLocaleDateString(undefined, { day: "numeric", month: "short", year: "numeric" });
  return `${first} – ${last}`;
}

/** Synced times: `10:15` today, otherwise with the date. */
export function syncedWords(iso: string, now: Date): string {
  const at = new Date(iso);
  return sameDay(at, now) ? clock(at) : `${at.toLocaleDateString(undefined, { day: "numeric", month: "short" })} ${clock(at)}`;
}
