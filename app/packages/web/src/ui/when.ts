/**
 * Scheduled starts in this computer's time zone, shared by To-dos and
 * Calendar: the words a scheduled to-do reads ("Scheduled Fri 09:00") and
 * the date-time picker that sets one (docs/design/office/specs/calendar.md,
 * "Scheduled tasks"). The picker hands back an RFC 3339 instant in UTC, which
 * is what Orbit's `not_before` stores.
 */

/** `09:05`, whatever the locale. */
export function clockWords(at: Date): string {
  return `${String(at.getHours()).padStart(2, "0")}:${String(at.getMinutes()).padStart(2, "0")}`;
}

/** Local midnight at the start of `at`'s day. */
function dayOf(at: Date): number {
  return new Date(at.getFullYear(), at.getMonth(), at.getDate()).getTime();
}

/**
 * When a scheduled start falls, in words: `Fri 09:00` within the coming
 * week, `Fri 13 Nov 09:00` further off or already past.
 */
export function whenWords(iso: string, now: Date): string {
  const at = new Date(iso);
  const days = Math.round((dayOf(at) - dayOf(now)) / 86_400_000);
  const weekday = at.toLocaleDateString(undefined, { weekday: "short" });
  if (days >= 0 && days < 7) return `${weekday} ${clockWords(at)}`;
  const date = at.toLocaleDateString(undefined, { day: "numeric", month: "short" });
  return `${weekday} ${date} ${clockWords(at)}`;
}

/** "Scheduled Fri 09:00". */
export function scheduledWords(iso: string, now: Date): string {
  return `Scheduled ${whenWords(iso, now)}`;
}

/** A `datetime-local` value for `at` in this computer's time zone: `2026-10-30T09:00`. */
export function localInputValue(at: Date): string {
  const pad = (value: number): string => String(value).padStart(2, "0");
  return `${at.getFullYear()}-${pad(at.getMonth() + 1)}-${pad(at.getDate())}T${clockWords(at)}`;
}

/** A `datetime-local` value read in this computer's time zone, as a UTC instant; null when it is not one. */
export function instantFromInput(value: string): string | null {
  const parts = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})$/.exec(value);
  if (parts === null) return null;
  const [year, month, day, hour, minute] = parts.slice(1).map(Number) as [number, number, number, number, number];
  const at = new Date(year, month - 1, day, hour, minute);
  if (Number.isNaN(at.getTime()) || at.getMonth() !== month - 1 || at.getDate() !== day) return null;
  return at.toISOString().replace(/\.\d{3}Z$/, "Z");
}

/** The time zone the picker reads in, by name, or null when the browser does not say. */
export function timeZoneWords(): string | null {
  const zone = Intl.DateTimeFormat().resolvedOptions().timeZone;
  return typeof zone === "string" && zone !== "" ? zone : null;
}

/** The first suggestion: the next day at 09:00. */
function suggestion(now: Date): Date {
  return new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1, 9, 0);
}

export interface SchedulePickerOptions {
  /** The current scheduled start, or null to suggest one. */
  initial: string | null;
  now: Date;
  /** Write the instant; a rejection keeps the picker open with `failed`'s words. */
  save: (instant: string) => Promise<void>;
  cancel: () => void;
  failed?: (error: unknown) => string;
}

/** Rules for the picker, appended to each screen's own styles. */
export const pickerStyles = `
.schedule-picker { display:flex; flex-wrap:wrap; align-items:flex-end; gap:8px; width:100%; min-width:0; }
.schedule-picker label { display:flex; flex-direction:column; gap:4px; font-size:12px; color:var(--fg-mute); flex:1 1 180px; min-width:0; }
.schedule-picker input { min-height:36px; width:100%; min-width:0; padding:4px 8px; border:1px solid var(--border); border-radius:var(--radius-sm); background:var(--bg); color:var(--fg); font:inherit; color-scheme:dark; }
.schedule-picker button { align-self:auto; min-height:36px; }
.schedule-picker .schedule-picker-note { flex:1 0 100%; margin:0; font-size:12px; color:var(--fg-mute); }
.schedule-picker .schedule-picker-status { flex:1 0 100%; margin:0; font-size:12px; color:var(--attention-fg); }
.schedule-picker .schedule-picker-status:empty { display:none; }
`;

/** A date-time picker in this computer's time zone, with Save and Cancel. */
export function schedulePicker(options: SchedulePickerOptions): HTMLFormElement {
  const form = document.createElement("form");
  form.className = "schedule-picker";
  form.setAttribute("aria-label", "Schedule");
  const label = document.createElement("label");
  const caption = document.createElement("span");
  caption.textContent = "Date and time";
  const input = document.createElement("input");
  input.type = "datetime-local";
  input.required = true;
  input.min = localInputValue(options.now);
  input.value = localInputValue(options.initial === null ? suggestion(options.now) : new Date(options.initial));
  label.append(caption, input);
  const save = document.createElement("button");
  save.type = "submit";
  save.className = "primary-action";
  save.textContent = "Save";
  const cancel = document.createElement("button");
  cancel.type = "button";
  cancel.className = "secondary-action";
  cancel.textContent = "Cancel";
  cancel.addEventListener("click", () => options.cancel());
  const note = document.createElement("p");
  note.className = "schedule-picker-note";
  const zone = timeZoneWords();
  note.textContent = zone === null ? "In this computer's time zone." : `In this computer's time zone, ${zone}.`;
  const status = document.createElement("p");
  status.className = "schedule-picker-status";
  status.setAttribute("role", "status");
  let busy = false;
  form.addEventListener("submit", (event) => {
    event.preventDefault();
    if (busy) return;
    const instant = instantFromInput(input.value);
    if (instant === null) {
      status.textContent = "Choose a date and a time.";
      return;
    }
    if (Date.parse(instant) <= options.now.getTime()) {
      status.textContent = "Choose a time that is still to come.";
      return;
    }
    busy = true;
    save.disabled = true;
    status.textContent = "";
    void options.save(instant).catch((error: unknown) => {
      status.textContent = options.failed?.(error) ?? "The time could not be saved. Try again.";
    }).finally(() => {
      busy = false;
      save.disabled = false;
    });
  });
  form.append(label, save, cancel, note, status);
  return form;
}
