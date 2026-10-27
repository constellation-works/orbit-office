import type { Screen } from "../../screen.js";
import { ApiError, getJson, postJson } from "../../api/client.js";

interface Routine { name: string; description: string; schedule: Record<string, unknown>; enabled: boolean; skipped_reason: string | null }
function element<K extends keyof HTMLElementTagNameMap>(tag: K, text: string): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag); node.textContent = text; return node;
}
export function scheduleWords(schedule: Record<string, unknown>): string {
  const minutes = schedule.every_minutes;
  if (typeof minutes === "number" && minutes > 0) {
    if (minutes % 1440 === 0) return `Every ${minutes / 1440} ${minutes === 1440 ? "day" : "days"}`;
    if (minutes % 60 === 0) return `Every ${minutes / 60} ${minutes === 60 ? "hour" : "hours"}`;
    return `Every ${minutes} ${minutes === 1 ? "minute" : "minutes"}`;
  }
  if (typeof schedule.cron === "string") {
    const [minute, hour, day, month, weekday] = schedule.cron.split(" ");
    if (minute && hour && day && month && weekday && /^\d+$/.test(minute)) {
      const time = /^\d+$/.test(hour) ? ` at ${hour.padStart(2, "0")}:${minute.padStart(2, "0")}` : "";
      if (day === "*" && month === "*" && weekday === "*" && time) return `Every day${time}`;
      if (day === "1" && month === "*" && weekday === "*" && time) return `Every month on day 1${time}`;
      const days = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
      if (day === "*" && month === "*" && /^[0-6]$/.test(weekday) && time) return `Every ${days[Number(weekday)]}${time}`;
      if (hour === "*" && day === "*" && month === "*" && weekday === "*") return `Every hour at ${minute} minutes past`;
      const interval = /^\*\/(\d+)$/.exec(hour);
      if (interval && day === "*" && month === "*" && weekday === "*") return `Every ${interval[1]} hours at ${minute} minutes past`;
    }
    const parts = schedule.cron.split(" ");
    if (parts.length === 5) {
      const units = ["minute", "hour", "day of the month", "month", "day of the week"];
      const describe = (part: string, unit: string): string => {
        if (part === "*") return `every ${unit}`;
        return part.split(",").map(piece => {
          const step = /^(\*|\d+-\d+)\/(\d+)$/.exec(piece);
          if (step) return step[1] === "*" ? `every ${step[2]} ${unit}s` : `every ${step[2]} ${unit}s from ${step[1]?.replace("-", " to ")}`;
          if (/^\d+-\d+$/.test(piece)) return `${unit}s ${piece.replace("-", " through ")}`;
          return /^\d+$/.test(piece) ? `${unit} ${piece}` : `${unit} managed in Orbit`;
        }).join(" or ");
      };
      return `Scheduled for ${parts.map((part, index) => describe(part, units[index] ?? "time")).join(", ")}`;
    }
    return "On a schedule managed in Orbit";
  }
  if (schedule.deliveries_landed && typeof schedule.deliveries_landed === "object") {
    const delivery = schedule.deliveries_landed as Record<string, unknown>;
    const count = delivery.threshold;
    const wait = delivery.max_wait_minutes;
    if (typeof count === "number" && typeof wait === "number") return `After ${count} deliveries or up to ${wait} minutes`;
    return "After deliveries are completed";
  }
  return "On a schedule managed in Orbit";
}
function skipWords(reason: string): string {
  if (reason.includes("switched off in this workspace")) return "Its plugin is switched off in this workspace.";
  if (reason.includes("not enabled on this host")) return "Its plugin is not enabled on this computer.";
  return "This task is unavailable. Check its settings in Orbit.";
}
async function render(root: HTMLElement): Promise<void> {
  root.replaceChildren(element("p", "Loading routines."));
  try {
    const data = await getJson<{ routines: Routine[] }>("/api/routines");
    const section = document.createElement("section"); section.className = "routines";
    section.append(element("p", "Scheduled background jobs are managed in Orbit."));
    const grid = document.createElement("div"); grid.className = "card-grid";
    for (const item of data.routines) {
      const card = document.createElement("article"); card.className = "card";
      card.append(element("h2", `${scheduleWords(item.schedule)}: ${item.description}`));
      const label = document.createElement("label");
      const toggle = document.createElement("input"); toggle.type = "checkbox"; toggle.checked = item.enabled;
      const state = element("span", item.enabled ? " On" : " Off");
      label.append(toggle, state); card.append(label);
      if (item.skipped_reason) card.append(element("p", skipWords(item.skipped_reason)));
      const message = element("p", ""); message.setAttribute("role", "status"); card.append(message);
      toggle.addEventListener("change", () => {
        const desired = toggle.checked; toggle.disabled = true; message.textContent = "";
        void (async () => {
          try {
            await postJson("/api/routines/toggle", { name: item.name, enabled: desired });
            const latest = await getJson<{ routines: Routine[] }>("/api/routines");
            const updated = latest.routines.find(routine => routine.name === item.name);
            if (!updated) throw new Error("routine unavailable");
            toggle.checked = updated.enabled; item.enabled = updated.enabled; state.textContent = updated.enabled ? " On" : " Off";
            if (updated.skipped_reason) message.textContent = skipWords(updated.skipped_reason);
          } catch {
            toggle.checked = item.enabled; message.textContent = "Could not confirm this change. Refresh and try again.";
          } finally { toggle.disabled = false; }
        })();
      });
      grid.append(card);
    }
    if (!data.routines.length) section.append(element("p", "No routines are available."));
    section.append(grid, retentionForm()); root.replaceChildren(section);
  } catch { root.replaceChildren(element("p", "Routines are unavailable. Try again in a moment.")); }
}
function retentionForm(): HTMLElement {
  const section = document.createElement("section"); section.className = "routines-retention";
  section.append(element("h2", "Retention"), element("p", "Choose how long to keep originals. Leave days empty to keep them indefinitely."));
  const form = document.createElement("form");
  const kindLabel = element("label", "Type "); const kind = document.createElement("select");
  for (const [value, label] of [["video", "Video"], ["meeting", "Meeting"], ["chat", "Chat"], ["email", "Email"], ["doc", "Document"], ["receipt", "Receipt"], ["bill", "Bill"], ["conversation", "Conversation"], ["note", "Note"]] as const) {
    const option = element("option", label); option.value = value; kind.append(option);
  }
  kindLabel.append(kind);
  const daysLabel = element("label", " Days "); const days = document.createElement("input"); days.type = "number"; days.step = "any"; daysLabel.append(days);
  const button = element("button", "Save retention"); button.type = "submit";
  const message = element("p", ""); message.setAttribute("role", "status"); form.append(kindLabel, daysLabel, button, message);
  form.addEventListener("submit", event => {
    event.preventDefault(); button.disabled = true; message.textContent = "";
    void postJson("/api/routines/retention", { kind: kind.value, value: days.value === "" ? null : Number(days.value) }).then(() => {
      message.textContent = "Retention saved.";
    }).catch((error: unknown) => {
      message.textContent = error instanceof ApiError && error.reason ? error.reason : "Could not save retention. Try again.";
    }).finally(() => { button.disabled = false; });
  });
  section.append(form); return section;
}
export const screen: Screen = { id: "routines", title: "Routines", status: "built", render };
