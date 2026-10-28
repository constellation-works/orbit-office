/**
 * The meeting panel: when and where, attendees with their person pages, the
 * agenda as quoted text, the cited prep brief, and after the meeting its
 * notes, summary and to-dos, with **Add notes or transcript**.
 */
import type { CalendarMeeting } from "@orbit-office/contracts";

import { ApiError, captureMeetingFile, captureMeetingNotes, getMeetingOutcome, getSourceCitation } from "../../api/client.js";
import { sourceHref } from "../../api/source-location.js";
import { pill } from "../../ui/index.js";
import { clock, meetingState, prepTime, stateWords } from "./model.js";

function element<K extends keyof HTMLElementTagNameMap>(tag: K, text = "", className?: string): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  node.textContent = text;
  if (className !== undefined) node.className = className;
  return node;
}
function button(label: string, onClick: () => void, className = "calendar-link"): HTMLButtonElement {
  const node = element("button", label, className);
  node.type = "button";
  node.addEventListener("click", onClick);
  return node;
}

const RESPONSES: Record<string, string> = {
  accepted: "Accepted", tentative: "Maybe", tentativelyaccepted: "Maybe", declined: "Declined",
  needsaction: "No reply yet", none: "No reply yet", notresponded: "No reply yet", organizer: "Organizer",
};

function when(meeting: CalendarMeeting): string {
  const start = new Date(meeting.start);
  const day = start.toLocaleDateString(undefined, { weekday: "long", day: "numeric", month: "long", ...(meeting.all_day ? { timeZone: "UTC" } : {}) });
  return meeting.all_day ? `${day}, all day` : `${day}, ${clock(start)}–${clock(new Date(meeting.end))}`;
}

/** A person page's opening text, shown under the attendee who matched it. */
function personLink(page: { id: string; title: string }, label: string): HTMLElement {
  const wrap = element("span", "", "calendar-person");
  const detail = element("p", "", "calendar-person-page");
  detail.hidden = true;
  const open = button(label, () => {
    detail.hidden = !detail.hidden;
    open.setAttribute("aria-expanded", String(!detail.hidden));
    if (detail.hidden || detail.dataset.loaded === "true") return;
    detail.textContent = "Opening their page…";
    void getSourceCitation(page.id, "domain").then(found => {
      detail.textContent = found.text; detail.dataset.loaded = "true";
    }).catch(() => { detail.textContent = "Their page could not be opened. Find it in the Library."; });
  });
  open.setAttribute("aria-expanded", "false");
  wrap.append(open, detail);
  return wrap;
}

/** The brief's Markdown, drawn as headings and items, each citation a link. */
function brief(text: string, meeting: CalendarMeeting): HTMLElement {
  const body = element("div", "", "calendar-brief");
  let list: HTMLUListElement | null = null;
  const titles = new Map([...meeting.attendees.flatMap(a => a.person === null ? [] : [a.person]), ...meeting.people].map(page => [page.id, page.title]));
  for (const line of text.split("\n")) {
    if (!line.trim()) { list = null; continue; }
    const heading = /^#{1,6}\s+(.*)$/.exec(line);
    if (heading) {
      const words = heading[1] ?? "";
      body.append(element("h4", words.charAt(0).toUpperCase() + words.slice(1)));
      list = null; continue;
    }
    const item = /^[-*]\s+(.*?)(?:\s*\[([^\]]+)\])?\s*$/.exec(line);
    if (!item) { body.append(element("p", line)); list = null; continue; }
    if (list === null) { list = element("ul"); body.append(list); }
    const entry = element("li", item[1] ?? "");
    for (const cite of (item[2] ?? "").split(/\s*[,;]\s*/).filter(Boolean)) {
      const [id = "", anchor] = cite.split("#", 2);
      if (/^src-[a-z0-9-]+$/.test(id)) {
        const link = element("a", "Source", "calendar-cite");
        link.href = sourceHref(id, anchor ? `#${anchor}` : "#source");
        entry.append(" ", link);
      } else {
        entry.append(" ", element("span", titles.get(id) ?? "Library page", "calendar-cite"));
      }
    }
    list.append(entry);
  }
  return body;
}

export interface PanelContext {
  now: Date;
  leadMinutes: number;
  /** Reload the week after a capture and show this meeting again. */
  captured(key: string): Promise<void>;
  close(): void;
}

function captureForm(meeting: CalendarMeeting, context: PanelContext): HTMLElement {
  const section = element("section", "", "calendar-capture");
  section.setAttribute("aria-label", "Add notes or transcript");
  section.append(element("h3", meeting.capture === null ? "Add notes or transcript" : "Add more notes or a transcript"));
  const status = element("p", "", "calendar-status");
  status.setAttribute("role", "status");
  const label = element("label", "Meeting notes", "calendar-field");
  const notes = element("textarea");
  notes.rows = 4;
  label.append(notes);
  const save = button("Save notes", () => {
    if (!notes.value.trim()) { status.textContent = "Type or paste the notes first."; return; }
    void submit(() => captureMeetingNotes(meeting.key, notes.value));
  }, "primary-action");
  const drop = element("label", "", "calendar-drop");
  drop.append(element("span", "Drop a .vtt, .txt or .md file, or audio, here, or "));
  const picker = element("input");
  picker.type = "file";
  picker.accept = ".vtt,.txt,.md,.wav,.mp3,.m4a,.webm";
  picker.setAttribute("aria-label", "Choose a notes or transcript file");
  drop.append(element("span", "choose a file", "calendar-drop-choose"), picker);
  picker.addEventListener("change", () => { const file = picker.files?.[0]; if (file) void submit(() => captureMeetingFile(meeting.key, file)); });
  drop.addEventListener("dragover", event => { event.preventDefault(); drop.dataset.over = "true"; });
  drop.addEventListener("dragleave", () => { delete drop.dataset.over; });
  drop.addEventListener("drop", event => {
    event.preventDefault(); delete drop.dataset.over;
    const file = event.dataTransfer?.files[0];
    if (file) void submit(() => captureMeetingFile(meeting.key, file));
  });
  let busy = false;
  async function submit(send: () => Promise<unknown>): Promise<void> {
    if (busy) return;
    busy = true; save.disabled = true; picker.disabled = true;
    status.textContent = "Saving the notes…";
    try {
      await send();
      status.textContent = "Saved. The summary and to-dos follow once the meeting inbox has read it.";
      await context.captured(meeting.key);
    } catch (error) {
      const code = error instanceof ApiError ? error.code : "";
      status.textContent = code === "unknown_event" ? "This meeting is no longer in your calendar. Sync, then try again."
        : code === "unsupported_file" ? "Choose a .vtt, .txt or .md file, or an audio file."
        : code === "upload_too_large" ? "This file is too large to add."
        : "The notes could not be saved. Try again.";
    } finally { busy = false; save.disabled = false; picker.disabled = false; }
  }
  section.append(label, save, drop, status);
  return section;
}

/** Notes, summary and to-dos of a captured meeting. */
function outcome(meeting: CalendarMeeting): HTMLElement {
  const section = element("section", "", "calendar-outcome");
  section.setAttribute("aria-label", "Notes and summary");
  section.append(element("h3", "Notes and summary"));
  if (meeting.capture === null) return section;
  const link = element("a", "Open the notes", "calendar-open-notes");
  link.href = sourceHref(meeting.capture.source);
  section.append(link);
  if (!meeting.capture.summarized) {
    section.append(element("p", "Summary on its way: the meeting inbox writes it, with to-dos, after the notes are saved.", "calendar-muted"));
    return section;
  }
  const body = element("div", "", "calendar-summary");
  body.append(element("p", "Opening the summary…", "calendar-muted"));
  section.append(body);
  void getMeetingOutcome(meeting.capture.source).then(found => {
    body.replaceChildren(element("p", found.summary ?? "The summary is not available yet.", "calendar-summary-text"));
    body.append(element("h4", "To-dos"));
    if (found.todos.length === 0) body.append(element("p", "No to-dos came from this meeting.", "calendar-muted"));
    else {
      const list = element("ul");
      for (const todo of found.todos) {
        const owner = todo.owner === "you" ? "For you" : todo.owner === "agent" ? "For an agent" : "";
        list.append(element("li", [todo.title, owner, todo.done ? "Done" : ""].filter(Boolean).join(" · ")));
      }
      body.append(list);
      if (found.more) body.append(element("p", "More to-dos are in To-dos.", "calendar-muted"));
    }
  }).catch(() => { body.replaceChildren(element("p", "The summary could not be opened. Try again in a moment.", "calendar-muted")); });
  return section;
}

/** Draw the panel for one meeting. */
export function meetingPanel(meeting: CalendarMeeting, context: PanelContext): HTMLElement {
  const panel = element("aside", "", "calendar-panel");
  panel.setAttribute("role", "dialog");
  panel.setAttribute("aria-label", meeting.title);
  const head = element("div", "", "calendar-panel-head");
  const states = element("p", "", "calendar-panel-states");
  for (const words of stateWords(meeting, context.now, context.leadMinutes)) {
    states.append(pill(words, words === "Private" ? "private" : words === "Add notes" ? "attention" : "neutral"));
  }
  const title = element("h2", meeting.title, meeting.status === "cancelled" ? "calendar-struck" : undefined);
  head.append(states, title, button("Close", context.close, "calendar-close"));
  panel.append(head);

  const facts = element("dl", "", "calendar-facts");
  const fact = (term: string, value: string): void => { facts.append(element("dt", term), element("dd", value)); };
  fact("When", when(meeting));
  fact("Where", meeting.location || "No place given");
  fact("Online", meeting.online ? "Has an online link" : "No online link");
  if (meeting.organizer) fact("Organizer", meeting.organizer);
  panel.append(facts);

  panel.append(element("h3", "Attendees"));
  if (meeting.attendees.length === 0) panel.append(element("p", "No attendees are listed.", "calendar-muted"));
  else {
    const list = element("ul", "", "calendar-attendees");
    for (const attendee of meeting.attendees) {
      const item = element("li");
      item.append(attendee.person === null ? element("span", attendee.name) : personLink(attendee.person, attendee.name));
      const response = RESPONSES[attendee.response.toLowerCase()];
      if (response !== undefined) item.append(element("span", response, "calendar-muted"));
      list.append(item);
    }
    panel.append(list);
  }
  if (meeting.people.length > 0) {
    const related = element("p", "Also matched: ", "calendar-related");
    meeting.people.forEach((page, index) => { if (index > 0) related.append(", "); related.append(personLink(page, page.title)); });
    panel.append(related);
  }

  panel.append(element("h3", "Agenda"));
  if (meeting.agenda.trim()) {
    const quote = element("blockquote", meeting.agenda, "calendar-agenda");
    panel.append(element("p", "From the invite, as written:", "calendar-muted"), quote);
  } else panel.append(element("p", "The invite has no agenda.", "calendar-muted"));

  const past = Date.parse(meeting.end) <= context.now.getTime();
  const prep = element("section", "", "calendar-prep");
  prep.setAttribute("aria-label", "Prep brief");
  prep.append(element("h3", "Prep brief"));
  if (meeting.prep !== null) {
    const body = element("p", "Opening the brief…", "calendar-muted");
    prep.append(body);
    void getSourceCitation(meeting.prep, "artifact").then(found => { body.replaceWith(brief(found.text, meeting)); })
      .catch(() => { body.textContent = "The brief could not be opened. Find it in the Library."; });
  } else if (meeting.status === "cancelled" || meeting.all_day) {
    prep.append(element("p", "No brief is written for this meeting.", "calendar-muted"));
  } else if (past) {
    prep.append(element("p", "No brief was written for this meeting.", "calendar-muted"));
  } else {
    const state = meetingState(meeting, context.now, context.leadMinutes);
    const at = prepTime(meeting, context.leadMinutes);
    prep.append(element("p", state?.words.startsWith("Prep at") === true
      ? `The brief is written at ${clock(at)}, before the meeting.`
      : "The brief is on its way.", "calendar-muted"));
  }
  panel.append(prep);

  if (past && meeting.status !== "cancelled") {
    if (meeting.capture !== null) panel.append(outcome(meeting));
    panel.append(captureForm(meeting, context));
  }
  return panel;
}
