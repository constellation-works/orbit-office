/**
 * The panel for one scheduled to-do: its time, its state in words, who runs
 * it, and **Reschedule** and **Clear schedule** while nothing has started it
 * (docs/design/office/specs/calendar.md, "Scheduled tasks"). A to-do of the
 * user's reads its time as its due date, with **Change due date** and
 * **Clear due date**.
 */
import type { CalendarTask } from "@orbit-office/contracts";

import { button, element, pill, type Tone } from "../../ui/index.js";
import { schedulePicker } from "../../ui/when.js";
import { clock, isDue, startedLate, taskState, taskWords } from "./model.js";

export interface TaskPanelContext {
  now: Date;
  close(): void;
  /** Write a new time, or clear it with null; the week is reread after. */
  schedule(id: string, notBefore: string | null): Promise<void>;
}

function dayAndTime(iso: string): string {
  const at = new Date(iso);
  return `${at.toLocaleDateString(undefined, { weekday: "long", day: "numeric", month: "long" })}, ${clock(at)}`;
}

/** Who starts it, in words: nothing for the user's own, the dispatch pass for an agent's. */
function runsWords(task: CalendarTask): string {
  if (task.owner === "you") return "It is yours: nothing starts it for you, so its time is the date it is due.";
  if (task.state === "not_approved") return "It waits for your approval before an agent can start it.";
  return "An agent starts it on the first dispatch pass after its time, while this computer is on.";
}

const TONES: Record<string, Tone> = {
  "Not approved": "attention", "Ready for review": "attention", Overdue: "attention", Running: "working", Done: "done",
};

/** Draw the panel for one scheduled to-do. */
export function taskPanel(task: CalendarTask, context: TaskPanelContext): HTMLElement {
  const panel = element("aside", "", "calendar-panel calendar-task-panel");
  panel.setAttribute("role", "dialog");
  panel.setAttribute("aria-label", task.title);
  const head = element("div", "", "calendar-panel-head");
  const states = element("p", "", "calendar-panel-states");
  const due = isDue(task);
  for (const words of taskWords(task, context.now)) states.append(pill(words, TONES[words] ?? (words.startsWith("Started") ? "working" : "neutral")));
  head.append(states, element("h2", task.title), button("Close", context.close, "calendar-close"));
  panel.append(head);

  const facts = element("dl", "", "calendar-facts");
  const fact = (term: string, value: string): void => { facts.append(element("dt", term), element("dd", value)); };
  fact(due ? "Due" : "Scheduled", dayAndTime(task.not_before));
  if (task.started_at !== null && (task.state === "running" || task.state === "review" || task.state === "done")) {
    fact("Started", `${dayAndTime(task.started_at)}${startedLate(task) === null ? ", on time" : ""}`);
  }
  fact("For", task.owner === "you" ? "You" : "An agent");
  fact("Now", taskState(task, context.now).words);
  panel.append(facts, element("p", runsWords(task), "calendar-muted"));

  const actions = element("section", "", "calendar-task-actions");
  actions.setAttribute("aria-label", due ? "Due date" : "Schedule");
  const status = element("p", "", "calendar-status");
  status.setAttribute("role", "status");
  if (task.state === "not_approved" || task.state === "scheduled") {
    const row = element("div", "", "calendar-task-buttons");
    const reschedule = button(due ? "Change due date" : "Reschedule", () => {
      const picker = schedulePicker({
        initial: task.not_before,
        now: context.now,
        ...(due ? { label: "Due date" } : {}),
        save: instant => context.schedule(task.id, instant),
        cancel: () => { picker.replaceWith(row); reschedule.focus(); },
        failed: () => due ? "The new due date could not be saved. Try again in a moment." : "The new time could not be saved. Try again in a moment.",
      });
      row.replaceWith(picker);
      picker.querySelector("input")?.focus();
    }, "primary-action");
    const clear = button(due ? "Clear due date" : "Clear schedule", () => {
      if (clear.disabled) return;
      clear.disabled = true; reschedule.disabled = true;
      status.textContent = due ? "Clearing the due date…" : "Clearing the time…";
      void context.schedule(task.id, null).catch(() => {
        status.textContent = due ? "The due date could not be cleared. Try again in a moment." : "The time could not be cleared. Try again in a moment.";
        clear.disabled = false; reschedule.disabled = false;
      });
    }, "secondary-action");
    row.append(reschedule, clear);
    actions.append(row, status);
  } else {
    actions.append(element("p", due ? "It has started, so its due date can no longer change." : "It has started, so its time can no longer change.", "calendar-muted"));
  }
  panel.append(actions);
  const todos = element("a", "Open To-dos", "calendar-open-todos");
  todos.href = "#todos";
  panel.append(todos);
  return panel;
}
