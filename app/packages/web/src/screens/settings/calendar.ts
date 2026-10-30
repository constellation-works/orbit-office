/** Independent local/external calendar visibility; disabling never deletes data. */
import { changeLocalCalendar, getCalendarSources } from "../../api/client.js";

export async function mountCalendar(content: HTMLElement, current: () => boolean): Promise<void> {
  const sources = await getCalendarSources(); if (!current()) return;
  const form = document.createElement("form"); form.setAttribute("aria-label", "Calendar sources");
  const title = document.createElement("h2"); title.textContent = "Calendar";
  const note = document.createElement("p"); note.textContent = "Use either calendar or both. Hidden events are kept. Local event changes stay on this machine; connected calendars remain read only.";
  const choice = (label: string, checked: boolean): HTMLInputElement => {
    const wrap = document.createElement("label"); wrap.style.display = "block";
    const input = document.createElement("input"); input.type = "checkbox"; input.checked = checked;
    wrap.append(input, document.createTextNode(label)); form.append(wrap); return input;
  };
  form.append(title, note);
  const local = choice("Local calendar", sources.local);
  const external = choice("Connected calendar", sources.external);
  const save = document.createElement("button"); save.type = "submit"; save.textContent = "Save calendar choices";
  const feedback = document.createElement("p"); feedback.setAttribute("role", "status");
  form.append(save, feedback);
  form.addEventListener("submit", event => {
    event.preventDefault(); local.disabled = true; external.disabled = true; save.disabled = true;
    void changeLocalCalendar({action:"sources",local:local.checked,external:external.checked})
      .then(() => { if (current()) feedback.textContent = "Calendar choices saved."; })
      .catch(() => { if (current()) feedback.textContent = "Could not save calendar choices. Try again."; })
      .finally(() => { local.disabled = false; external.disabled = false; save.disabled = false; });
  });
  content.replaceChildren(form);
}
