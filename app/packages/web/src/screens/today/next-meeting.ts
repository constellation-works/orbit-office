/**
 * Today's "Next meeting" card: the meeting now or next today, its time, and
 * "Prep ready" linking to its brief on the Calendar. Hidden when nothing is
 * left today, and when the calendar cannot be read.
 */
import type { CalendarMeeting } from "@orbit-office/contracts";

import { getCalendar } from "../../api/client.js";
import { card, pill } from "../../ui/index.js";

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
  const content = document.createElement("div");
  content.className = "card-content today-next-meeting";
  const start = new Date(meeting.start);
  const time = document.createElement("p");
  time.textContent = `${start.getTime() <= now.getTime() ? "Now, " : ""}${clock(start)}–${clock(new Date(meeting.end))}${meeting.location ? ` · ${meeting.location}` : ""}`;
  content.append(time);
  if (meeting.private) content.append(pill("Private", "private"));
  const link = document.createElement("a");
  link.href = `?${new URLSearchParams({ meeting: meeting.key })}#calendar`;
  link.textContent = meeting.prep === null ? "Open in Calendar" : "Prep ready";
  if (meeting.prep === null) {
    const note = document.createElement("p");
    note.textContent = "No prep brief yet.";
    content.append(note);
  }
  content.append(link);
  const element = card(meeting.title, content, undefined, { label: "Next meeting", tone: meeting.prep === null ? "working" : "done" });
  element.classList.add("today-next");
  return element;
}
