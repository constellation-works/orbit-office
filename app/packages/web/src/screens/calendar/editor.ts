/** The local event editor; identity and provenance are assigned by Office. */
import type { CalendarMeeting, LocalCalendarChange } from "@orbit-office/contracts";

function node<K extends keyof HTMLElementTagNameMap>(tag: K, content = ""): HTMLElementTagNameMap[K] {
  const result = document.createElement(tag); result.textContent = content; return result;
}
function localTime(value: string): string {
  const at = new Date(value);
  return `${at.getFullYear()}-${String(at.getMonth()+1).padStart(2,"0")}-${String(at.getDate()).padStart(2,"0")}T${String(at.getHours()).padStart(2,"0")}:${String(at.getMinutes()).padStart(2,"0")}`;
}
export function eventEditor(meeting: CalendarMeeting | null, day: Date, write: (change: LocalCalendarChange) => Promise<unknown>, done: () => Promise<void>, close: () => void): HTMLElement {
  const panel = node("section"); panel.className = "calendar-panel";
  const heading = meeting === null ? "New local event" : "Edit local event";
  panel.setAttribute("role", "dialog"); panel.setAttribute("aria-label", heading);
  const form = node("form"); form.className = "calendar-capture";
  const field = (label: string, value: string, type = "text"): HTMLInputElement => {
    const wrapper = node("label", label); wrapper.className = "calendar-field";
    const input = node("input"); input.type = type; input.value = value;
    wrapper.append(input); form.append(wrapper); return input;
  };
  const defaultStart = new Date(day); defaultStart.setHours(9,0,0,0);
  const title = field("Title", meeting?.title ?? ""); title.required = true; title.maxLength = 512;
  const displayTime = (value: string): string => meeting?.all_day === true ? `${value.slice(0,10)}T00:00` : localTime(value);
  const start = field("Starts", displayTime(meeting?.start ?? defaultStart.toISOString()), "datetime-local"); start.required = true;
  const end = field("Ends", displayTime(meeting?.end ?? new Date(defaultStart.getTime()+3600000).toISOString()), "datetime-local"); end.required = true;
  const allDay = field("All day", "", "checkbox"); allDay.checked = meeting?.all_day ?? false;
  const location = field("Location", meeting?.location ?? ""); location.maxLength = 1024;
  const attendees = field("Attendees (one name per line)", "");
  const names = node("textarea"); names.value = meeting?.attendees.map(a => a.name).join("\n") ?? "";
  attendees.replaceWith(names);
  const agendaLabel = node("label", "Agenda"); agendaLabel.className = "calendar-field";
  const agenda = node("textarea"); agenda.value = meeting?.agenda ?? ""; agenda.maxLength = 8192;
  agendaLabel.append(agenda); form.append(agendaLabel);
  const privateEvent = field("Private", "", "checkbox"); privateEvent.checked = meeting?.private ?? false;
  const feedback = node("p"); feedback.setAttribute("role", "status");
  const save = node("button", "Save event"); save.type = "submit";
  const cancel = node("button", "Cancel"); cancel.type = "button"; cancel.addEventListener("click", close);
  form.append(node("p", "Times use this computer’s time zone. All-day dates use an exclusive end date."), save, cancel, feedback);
  form.addEventListener("submit", event => {
    event.preventDefault();
    const controls = [...form.querySelectorAll<HTMLInputElement | HTMLTextAreaElement | HTMLButtonElement>("input, textarea, button")];
    const at = (value: string): string => allDay.checked ? `${value.slice(0,10)}T00:00:00Z` : new Date(value).toISOString();
    const draft = { title: title.value.trim(), start: at(start.value), end: at(end.value), all_day: allDay.checked,
      location: location.value, agenda: agenda.value, attendees: names.value.split("\n").map(s=>s.trim()).filter(Boolean), private: privateEvent.checked };
    if (Date.parse(draft.end) <= Date.parse(draft.start)) { feedback.textContent = "The end must be after the start."; return; }
    for (const control of controls) control.disabled = true;
    void write(meeting === null ? { action: "create", event: draft } : { action: "update", key: meeting.key, event: draft })
      .then(done).catch(() => { feedback.textContent = "The event could not be saved. Check the fields and try again."; })
      .finally(() => { for (const control of controls) control.disabled = false; });
  });
  panel.append(node("h2", heading), form); return panel;
}
