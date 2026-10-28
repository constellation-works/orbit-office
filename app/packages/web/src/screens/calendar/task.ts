/**
 * The panel for one scheduled to-do: its time, its state in words, who runs
 * it, and **Reschedule** and **Clear schedule** while nothing has started it
 * (docs/design/office/specs/calendar.md, "Scheduled tasks").
 */
import type { CalendarTask } from "@orbit-office/contracts";

import { pill, type Tone } from "../../ui/index.js";
import { schedulePicker } from "../../ui/when.js";
import { clock, startedLate, taskState, taskWords } from "./model.js";

function element<K extends keyof HTMLElementTagNameMap>(tag: K, text = "", className?: string): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  node.textContent = text;
  if (className !== undefined) node.className = className;
  return node;
}
function button(label: string, onClick: () => void, className: string): HTMLButtonElement {
  const node = element("button", label, className);
  node.type = "button";
  node.addEventListener("click", onClick);
  return node;
}

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
  if (task.owner === "you") return "It is yours: it only shows here, and nothing starts it for you.";
  if (task.state === "not_approved") return "It waits for your approval before an agent can start it.";
  return "An agent starts it on the first dispatch pass after its time, while this computer is on.";
}

const TONES: Record<string, Tone> = { "Not approved": "attention", "Ready for review": "attention", Running: "working", Done: "done" };

/** Draw the panel for one scheduled to-do. */
export function taskPanel(task: CalendarTask, context: TaskPanelContext): HTMLElement {
  const panel = element("aside", "", "calendar-panel calendar-task-panel");
  panel.setAttribute("role", "dialog");
  panel.setAttribute("aria-label", task.title);
  const head = element("div", "", "calendar-panel-head");
  const states = element("p", "", "calendar-panel-states");
  for (const words of taskWords(task)) states.append(pill(words, TONES[words] ?? (words.startsWith("Started") ? "working" : "neutral")));
  head.append(states, element("h2", task.title), button("Close", context.close, "calendar-close"));
  panel.append(head);

  const facts = element("dl", "", "calendar-facts");
  const fact = (term: string, value: string): void => { facts.append(element("dt", term), element("dd", value)); };
  fact("Scheduled", dayAndTime(task.not_before));
  if (task.started_at !== null && (task.state === "running" || task.state === "review" || task.state === "done")) {
    fact("Started", `${dayAndTime(task.started_at)}${startedLate(task) === null ? ", on time" : ""}`);
  }
  fact("For", task.owner === "you" ? "You" : "An agent");
  fact("Now", taskState(task).words);
  panel.append(facts, element("p", runsWords(task), "calendar-muted"));

  const actions = element("section", "", "calendar-task-actions");
  actions.setAttribute("aria-label", "Schedule");
  const status = element("p", "", "calendar-status");
  status.setAttribute("role", "status");
  if (task.state === "not_approved" || task.state === "scheduled") {
    const row = element("div", "", "calendar-task-buttons");
    const reschedule = button("Reschedule", () => {
      const picker = schedulePicker({
        initial: task.not_before,
        now: context.now,
        save: instant => context.schedule(task.id, instant),
        cancel: () => { picker.replaceWith(row); reschedule.focus(); },
        failed: () => "The new time could not be saved. Try again in a moment.",
      });
      row.replaceWith(picker);
      picker.querySelector("input")?.focus();
    }, "primary-action");
    const clear = button("Clear schedule", () => {
      if (clear.disabled) return;
      clear.disabled = true; reschedule.disabled = true;
      status.textContent = "Clearing the time…";
      void context.schedule(task.id, null).catch(() => {
        status.textContent = "The time could not be cleared. Try again in a moment.";
        clear.disabled = false; reschedule.disabled = false;
      });
    }, "secondary-action");
    row.append(reschedule, clear);
    actions.append(row, status);
  } else {
    actions.append(element("p", "It has started, so its time can no longer change.", "calendar-muted"));
  }
  panel.append(actions);
  const todos = element("a", "Open To-dos", "calendar-open-todos");
  todos.href = "#todos";
  panel.append(todos);
  return panel;
}
