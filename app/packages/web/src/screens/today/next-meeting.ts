/**
 * Today's Next meeting: the meeting now or next today, its time, and "Prep
 * ready" linking to its brief on the Calendar. A quiet line takes its place
 * when nothing is left today or the calendar cannot be read.
 */
import type { CalendarMeeting } from "@orbit-office/contracts";

import { getCalendar } from "../../api/client.js";

/** Meetings from now to the end of today; null hides the card. */
export async function todaysMeetings(now: Date): Promise<CalendarMeeting[]> {
  const midnight = new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1);
  return (await getCalendar(now, midnight)).meetings;
}

function clock(at: Date): string {
  return `${String(at.getHours()).padStart(2, "0")}:${String(at.getMinutes()).padStart(2, "0")}`;
}

/** The meeting now or next today that is neither cancelled nor all-day. */
export function upcoming(meetings: readonly CalendarMeeting[], now: Date): CalendarMeeting | null {
  const end = new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1).getTime();
  return meetings
    .filter(meeting => meeting.status !== "cancelled" && !meeting.all_day && Date.parse(meeting.end) > now.getTime() && Date.parse(meeting.start) < end)
    .sort((a, b) => Date.parse(a.start) - Date.parse(b.start))[0] ?? null;
}

export function nextMeetingCard(meeting: CalendarMeeting, now: Date): HTMLElement {
  const element = document.createElement("div");
  element.className = "today-next";
  element.dataset.key = meeting.key;
  const title = document.createElement("span");
  title.className = "today-next-title";
  title.textContent = meeting.title;
  const start = new Date(meeting.start);
  const time = document.createElement("span");
  time.className = "today-next-time";
  time.textContent = [
    `${start.getTime() <= now.getTime() ? "Now, " : ""}${clock(start)}–${clock(new Date(meeting.end))}`,
    meeting.location,
    meeting.private ? "Private" : "",
    meeting.prep === null ? "No prep brief yet" : "",
  ].filter(Boolean).join(" · ");
  const link = document.createElement("a");
  link.href = `?${new URLSearchParams({ meeting: meeting.key })}#calendar`;
  link.textContent = meeting.prep === null ? "Open in Calendar" : "Prep ready";
  element.append(title, time, link);
  return element;
}
