/**
 * Draw the To-dos screen. Visible text is plain words: the task id stays
 * in the close and schedule handlers and is never written into the page.
 */
import type { TodoItem, TodoList } from "@orbit-office/contracts";

import { card, emptyState, provenanceLink } from "../../ui/index.js";
import { pickerStyles, scheduledWords, schedulePicker } from "../../ui/when.js";

export interface TodoDrawOptions {
  /** Kept attached so a poll can tell that the screen is still showing. */
  marker?: HTMLElement;
  onClose?: (id: string) => void;
  /** Set a scheduled start (an RFC 3339 instant) or clear it with null. Without it, no schedule actions show. */
  onSchedule?: (id: string, notBefore: string | null) => Promise<void>;
  /** The to-do whose picker is open, or null. */
  editing?: string | null;
  /** Open the picker on a to-do, or close it with null. */
  onEdit?: (id: string | null) => void;
  /** The clock scheduled times are read against. Default now. */
  now?: Date;
}

interface ScheduleActions {
  onSchedule: (id: string, notBefore: string | null) => Promise<void>;
  onEdit: (id: string | null) => void;
  editing: string | null;
  now: Date;
}

const STYLE = `
.todos-columns { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 14px; align-items: start; }
.todos-column { background: var(--bg-rail); border: 1px solid var(--border); border-radius: var(--radius); padding: 12px; }
.todos-column h2 { display: flex; align-items: center; gap: 8px; padding: 4px 4px 0; font-size: 13px; font-weight: 600; color: var(--fg-dim); }
.todos-column h2::before { content: ""; flex: none; width: 8px; height: 8px; border-radius: 4px; background: var(--attention); }
.todos-column--agents h2::before { background: var(--working); }
.todos-lede { margin: 2px 4px 12px; }
.todos-column .card { position: relative; padding: 12px; gap: 10px; border-radius: 8px; }
.todos-column .card > h3 { font-size: 14px; }
.todos-column .card + .card { margin-top: 10px; }
.todos-column .card-content { display: flex; flex-wrap: wrap; gap: 6px; }
.card-content p.todos-kicker { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); margin: 0; }
.card-content p.todos-note { margin: 0; font-size: 12px; color: var(--fg-mute); }
.todos-column .card-content > div { display: contents; }
.todos-column .provenance, .todos-knowledge li { font-size: 12px; padding: 3px 7px; border-radius: 4px; background: var(--bg-hover); }
.todos-column .provenance { color: var(--accent); }
.todos-knowledge { display: contents; list-style: none; }
.todos-knowledge li { color: var(--fg-dim); }
.todos-column .primary-action { min-height: 36px; font-size: 13px; }
.todos-more { margin-top: 20px; }
.todos-marker { display: none; }
.todos-schedule { flex: 1 0 100%; display: flex; flex-wrap: wrap; align-items: center; gap: 6px 8px; min-width: 0; }
.card-content p.todos-when { margin: 0; font-size: 12px; padding: 3px 7px; border-radius: 4px; background: var(--highlight-bg); color: var(--fg); }
.todos-schedule-action { min-height: 32px; padding: 4px 10px; font-size: 12px; background: var(--bg-elev); border: 1px solid var(--border); border-radius: var(--radius-sm); color: var(--fg); cursor: pointer; }
.todos-schedule-action:hover { background: var(--bg-hover); }
@media (max-width: 720px) {
  .todos-columns { grid-template-columns: minmax(0, 1fr); }
}
`;

/** Accept only the to-do list shape. Anything else is a failed load. */
export function modelFromPayload(payload: unknown): TodoList | null {
  if (payload === null || typeof payload !== "object") return null;
  const record = payload as Record<string, unknown>;
  const you = itemsFrom(record.you);
  const agents = itemsFrom(record.agents);
  if (you === null || agents === null) return null;
  return { you, agents, more: record.more === true };
}

/** Draw the two columns into `root`. */
export function drawTodoScreen(root: HTMLElement, model: TodoList, options: TodoDrawOptions = {}): void {
  const style = document.createElement("style");
  style.textContent = STYLE + pickerStyles;
  const schedule: ScheduleActions | undefined = options.onSchedule === undefined || options.onEdit === undefined
    ? undefined
    : { onSchedule: options.onSchedule, onEdit: options.onEdit, editing: options.editing ?? null, now: options.now ?? new Date() };

  const page = document.createElement("section");
  page.className = "todos";
  const heading = document.createElement("h1");
  heading.textContent = "To-dos";
  const intro = document.createElement("p");
  intro.className = "intro";
  intro.textContent = "What you will do, and what agents are carrying.";
  const columns = document.createElement("div");
  columns.className = "todos-columns";
  columns.append(
    column("you", "You", "Close these when you are finished.", model.you, options.onClose, schedule),
    column("agents", "Agents", "These stay with the agents.", model.agents, options.onClose, schedule),
  );
  page.append(heading, intro, columns);
  if (model.more) {
    const more = document.createElement("p");
    more.className = "intro todos-more";
    more.textContent = "Some to-dos are not listed yet.";
    page.append(more);
  }

  const children: Node[] = [];
  if (options.marker !== undefined) children.push(options.marker);
  children.push(style, page);
  root.replaceChildren(...children);
}

function column(
  kind: "you" | "agents",
  title: string,
  lede: string,
  items: readonly TodoItem[],
  onClose: ((id: string) => void) | undefined,
  schedule: ScheduleActions | undefined,
): HTMLElement {
  const section = document.createElement("section");
  section.className = `todos-column todos-column--${kind}`;
  section.setAttribute("aria-label", title);
  const heading = document.createElement("h2");
  heading.textContent = title;
  const note = document.createElement("p");
  note.className = "intro todos-lede";
  note.textContent = lede;
  section.append(heading, note);
  if (items.length === 0) {
    const empty = kind === "you"
      ? emptyState("All clear", "Nothing is waiting on you.")
      : emptyState("Nothing in progress", "No agent to-dos right now.");
    section.append(empty);
    return section;
  }
  for (const item of items) section.append(todoCard(item, onClose, schedule));
  return section;
}

function todoCard(
  item: TodoItem,
  onClose: ((id: string) => void) | undefined,
  schedule: ScheduleActions | undefined,
): HTMLElement {
  const content = document.createElement("div");
  content.className = "card-content";
  content.append(sourceBlock(item.source), knowledgeBlock(item.knowledge));
  const when = scheduleBlock(item, schedule);
  if (when !== null) content.append(when);
  const action = item.closable && onClose !== undefined ? closeButton(item.id, onClose) : undefined;
  return action === undefined ? card(item.title, content) : card(item.title, content, action);
}

function sourceBlock(source: string | null): HTMLElement {
  const block = document.createElement("div");
  if (source === null) {
    const note = document.createElement("p");
    note.className = "todos-note";
    note.textContent = "No source attached.";
    block.append(note);
    return block;
  }
  const kicker = document.createElement("p");
  kicker.className = "todos-kicker";
  kicker.textContent = "Source";
  block.append(kicker, provenanceLink(source, "#source"));
  return block;
}

function knowledgeBlock(knowledge: readonly string[]): HTMLElement {
  const block = document.createElement("div");
  const kicker = document.createElement("p");
  kicker.className = "todos-kicker";
  kicker.textContent = "Knowledge";
  block.append(kicker);
  if (knowledge.length === 0) {
    const note = document.createElement("p");
    note.className = "todos-note";
    note.textContent = "No knowledge attached.";
    block.append(note);
    return block;
  }
  const list = document.createElement("ul");
  list.className = "todos-knowledge";
  for (const name of knowledge) {
    const entry = document.createElement("li");
    entry.textContent = name;
    list.append(entry);
  }
  block.append(list);
  return block;
}

/**
 * The scheduled time in words, and **Schedule…** (or **Reschedule…** and
 * **Clear schedule**) while nothing has started the to-do. The open picker
 * takes the actions' place.
 */
function scheduleBlock(item: TodoItem, schedule: ScheduleActions | undefined): HTMLElement | null {
  const now = schedule?.now ?? new Date();
  const canSchedule = schedule !== undefined && item.schedulable;
  if (item.scheduled === null && !canSchedule) return null;
  const block = document.createElement("section");
  block.className = "todos-schedule";
  block.setAttribute("aria-label", "Schedule");
  if (item.scheduled !== null) {
    const words = document.createElement("p");
    words.className = "todos-when";
    words.textContent = scheduledWords(item.scheduled, now);
    block.append(words);
  }
  if (!canSchedule) return block;
  if (schedule.editing === item.id) {
    block.append(schedulePicker({
      initial: item.scheduled,
      now,
      save: (instant) => schedule.onSchedule(item.id, instant),
      cancel: () => schedule.onEdit(null),
      failed: () => "The time could not be saved. Try again in a moment.",
    }));
    return block;
  }
  block.append(scheduleButton(item.scheduled === null ? "Schedule…" : "Reschedule…", () => schedule.onEdit(item.id)));
  if (item.scheduled !== null) {
    const clear = scheduleButton("Clear schedule", () => {
      if (clear.disabled) return;
      clear.disabled = true;
      clear.textContent = "Clearing";
      void schedule.onSchedule(item.id, null).catch(() => undefined);
    });
    block.append(clear);
  }
  return block;
}

function scheduleButton(label: string, onClick: () => void): HTMLButtonElement {
  const button = document.createElement("button");
  button.type = "button";
  button.className = "todos-schedule-action";
  button.textContent = label;
  button.addEventListener("click", onClick);
  return button;
}

function closeButton(id: string, onClose: (id: string) => void): HTMLButtonElement {
  const button = document.createElement("button");
  button.type = "button";
  button.className = "primary-action";
  button.textContent = "Close";
  button.addEventListener("click", () => {
    if (button.disabled) return;
    button.disabled = true;
    button.textContent = "Closing";
    onClose(id);
  });
  return button;
}

function itemsFrom(value: unknown): TodoItem[] | null {
  if (!Array.isArray(value)) return null;
  const items: TodoItem[] = [];
  for (const row of value) {
    if (row === null || typeof row !== "object") return null;
    const record = row as Record<string, unknown>;
    if (typeof record.id !== "string" || typeof record.title !== "string" || typeof record.closable !== "boolean") {
      return null;
    }
    if (record.source !== null && typeof record.source !== "string") return null;
    if (!Array.isArray(record.knowledge) || record.knowledge.some((entry) => typeof entry !== "string")) {
      return null;
    }
    if (record.scheduled !== null && (typeof record.scheduled !== "string" || Number.isNaN(Date.parse(record.scheduled)))) {
      return null;
    }
    if (typeof record.schedulable !== "boolean") return null;
    const knowledge: string[] = [];
    for (const entry of record.knowledge) {
      if (typeof entry === "string") knowledge.push(entry);
    }
    items.push({
      id: record.id,
      title: record.title,
      source: typeof record.source === "string" ? record.source : null,
      knowledge,
      closable: record.closable,
      scheduled: typeof record.scheduled === "string" ? record.scheduled : null,
      schedulable: record.schedulable,
    });
  }
  return items;
}
