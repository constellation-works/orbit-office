/**
 * Draw the To-dos screen. Your to-dos are plain words: their task id stays
 * in the close and schedule handlers and is never written into the page.
 * Agent to-dos filed from meetings show their Orbit task id, crew, state
 * and PR, as the approved mock does (docs/design/office/mocks/Todos.dc.html).
 */
import type { AgentTodo, AgentTodos, AgentTodoState, TodoItem, TodoList } from "@orbit-office/contracts";

import { sourceHref } from "../../api/source-location.js";
import { card, emptyState, monoMeta, pill, provenanceLink, row, rows, type Tone } from "../../ui/index.js";
import { dueWords, isOverdue, pickerStyles, scheduledWords, schedulePicker } from "../../ui/when.js";

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
  /** Create a to-do with this title. Without it, no create form shows. */
  onCreate?: (title: string) => Promise<void>;
  /** Draft title kept across poll redraws while the user is typing. */
  createDraft?: string;
  /** Record the draft title as the user types, without redrawing. */
  onCreateDraft?: (title: string) => void;
  /** Agent to-dos from meetings and the target workspace; null when they could not be read. */
  agentTodos?: AgentTodos | null;
  /** Set the target workspace, or clear it with null. Without it, no selector shows. */
  onTarget?: (workspace: string | null) => Promise<void>;
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
.todos-create { display: flex; flex-wrap: wrap; align-items: flex-end; gap: 8px; margin: 0 0 14px; padding: 12px; background: var(--bg-rail); border: 1px solid var(--border); border-radius: var(--radius); }
.todos-create label { display: flex; flex-direction: column; gap: 4px; font-size: 12px; color: var(--fg-mute); flex: 1 1 220px; min-width: 0; }
.todos-create input { min-height: 40px; width: 100%; min-width: 0; padding: 4px 8px; border: 1px solid var(--border); border-radius: var(--radius-sm); background: var(--bg); color: var(--fg); font: inherit; color-scheme: dark; box-sizing: border-box; }
.todos-create .primary-action { align-self: flex-end; }
.todos-create .todos-create-status { flex: 1 0 100%; margin: 0; font-size: 12px; color: var(--attention-fg); }
.todos-create .todos-create-status:empty { display: none; }
.todos-head { display: flex; flex-wrap: wrap; align-items: flex-end; justify-content: space-between; gap: 12px 16px; margin: 0 0 20px; }
.todos-head > div:first-child { flex: 1 1 320px; min-width: 0; }
.todos-head .intro { margin: 0; font-size: 15px; color: var(--fg-dim); }
.todos-target { display: flex; flex-direction: column; gap: 6px; font-size: 12px; color: var(--fg-mute); }
.todos-target select { min-width: 200px; max-width: 100%; }
.todos-target-status { margin: 0; font-size: 12px; color: var(--fg-mute); }
.todos-column .rows { margin-top: 10px; padding: 0 4px; }
.todos-column .row { align-items: flex-start; }
.todos-agent-meta { display: flex; flex-wrap: wrap; gap: 4px 12px; }
.todos-held-note { margin: 0; font-size: 12px; color: var(--attention-soft); }
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

const AGENT_STATES = new Set(["queued", "working", "review", "merged", "done", "held"]);

/** Accept only the agent to-dos shape; anything else reads as unavailable (null). */
export function agentTodosFromPayload(payload: unknown): AgentTodos | null {
  if (payload === null || typeof payload !== "object") return null;
  const record = payload as Record<string, unknown>;
  const target = record.target_workspace;
  if (!(target === null || typeof target === "string") || !Array.isArray(record.todos) || !Array.isArray(record.followed)) return null;
  const todos: AgentTodo[] = [];
  for (const value of record.todos as unknown[]) {
    if (value === null || typeof value !== "object") return null;
    const item = value as Record<string, unknown>;
    if (typeof item.id !== "string" || typeof item.title !== "string" || typeof item.workspace !== "string" ||
        typeof item.state !== "string" || !AGENT_STATES.has(item.state)) return null;
    const pr = item.pr;
    todos.push({
      id: item.id, title: item.title, workspace: item.workspace, state: item.state as AgentTodoState,
      crew: typeof item.crew === "string" ? item.crew : null,
      pr: pr !== null && typeof pr === "object" && typeof (pr as Record<string, unknown>).number === "number"
        ? { number: (pr as { number: number }).number, url: typeof (pr as Record<string, unknown>).url === "string" ? (pr as { url: string }).url : null, merged: (pr as Record<string, unknown>).merged === true }
        : null,
      source: typeof item.source === "string" ? item.source : null,
    });
  }
  return {
    target_workspace: target, todos, target_unavailable: record.target_unavailable === true,
    followed: (record.followed as unknown[]).filter((item): item is string => typeof item === "string"),
  };
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
  const head = document.createElement("header");
  head.className = "todos-head";
  const titles = document.createElement("div");
  const heading = document.createElement("h1");
  heading.textContent = "To-dos";
  const intro = document.createElement("p");
  intro.className = "intro";
  intro.textContent = "Every to-do names where it came from. Agent to-dos from meetings become Orbit tasks in the workspace you choose.";
  titles.append(heading, intro);
  head.append(titles);
  if (options.agentTodos !== undefined && options.agentTodos !== null && options.onTarget !== undefined) {
    head.append(targetSelect(options.agentTodos, options.onTarget));
  }
  const columns = document.createElement("div");
  columns.className = "todos-columns";
  const meeting = options.agentTodos?.todos ?? [];
  columns.append(
    column("you", "You", "Close these when you are finished.", model.you, options.onClose, schedule),
    column("agents", "Agents", "Orbit tasks agents carry. Merged work shows its PR.", model.agents, options.onClose, schedule, meeting),
  );
  if (options.agentTodos === null) {
    const note = document.createElement("p");
    note.className = "intro todos-more";
    note.textContent = "Agent to-dos from meetings could not be read.";
    columns.append(note);
  } else if (options.agentTodos?.target_unavailable === true) {
    const note = document.createElement("p");
    note.className = "intro todos-more";
    note.textContent = `Tasks in ${options.agentTodos.target_workspace ?? "the target workspace"} could not be read.`;
    columns.append(note);
  }
  page.append(head);
  if (options.onCreate !== undefined) page.append(createForm(options.onCreate, options.createDraft ?? "", options.onCreateDraft));
  page.append(columns);
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

/** A title field and **Add to-do**, above the columns. An empty title is refused here and on the server. */
function createForm(
  onCreate: (title: string) => Promise<void>,
  draft: string,
  onCreateDraft: ((title: string) => void) | undefined,
): HTMLFormElement {
  const form = document.createElement("form");
  form.className = "todos-create";
  form.setAttribute("aria-label", "New to-do");
  const label = document.createElement("label");
  const caption = document.createElement("span");
  caption.textContent = "New to-do";
  const input = document.createElement("input");
  input.type = "text";
  input.placeholder = "What needs doing?";
  input.setAttribute("aria-label", "New to-do title");
  (input as HTMLInputElement).value = draft;
  input.addEventListener("input", () => onCreateDraft?.((input as HTMLInputElement).value));
  label.append(caption, input);
  const add = document.createElement("button");
  add.type = "submit";
  add.className = "primary-action";
  add.textContent = "Add to-do";
  const status = document.createElement("p");
  status.className = "todos-create-status";
  status.setAttribute("role", "status");
  let busy = false;
  form.addEventListener("submit", (event) => {
    event.preventDefault();
    if (busy) return;
    const title = (input as HTMLInputElement).value.trim();
    if (title === "") {
      status.textContent = "Give the to-do a title.";
      return;
    }
    busy = true;
    add.disabled = true;
    status.textContent = "";
    void onCreate(title).then(() => {
      (input as HTMLInputElement).value = "";
      onCreateDraft?.("");
    }).catch(() => {
      status.textContent = "That to-do could not be added. Try again in a moment.";
    }).finally(() => {
      busy = false;
      add.disabled = false;
    });
  });
  form.append(label, add, status);
  return form;
}

/** The "Agent to-dos go to" selector: the followed workspaces, the current target, or none. */
function targetSelect(agentTodos: AgentTodos, onTarget: (workspace: string | null) => Promise<void>): HTMLElement {
  const label = document.createElement("label");
  label.className = "todos-target";
  const caption = document.createElement("span");
  caption.textContent = "Agent to-dos go to";
  const select = document.createElement("select");
  const choices = [...new Set([...(agentTodos.target_workspace === null ? [] : [agentTodos.target_workspace]), ...agentTodos.followed])];
  for (const workspace of choices) {
    const option = document.createElement("option");
    option.value = workspace;
    option.textContent = workspace;
    select.append(option);
  }
  const none = document.createElement("option");
  none.value = "";
  none.textContent = "None: keep every to-do as mine";
  select.append(none);
  select.value = agentTodos.target_workspace ?? "";
  const status = document.createElement("p");
  status.className = "todos-target-status";
  status.setAttribute("role", "status");
  if (choices.length === 0) status.textContent = "Follow a workspace in your Library settings to choose it here.";
  select.addEventListener("change", () => {
    select.disabled = true;
    status.textContent = "Saving…";
    void onTarget(select.value === "" ? null : select.value).then(() => {
      status.textContent = "Saved. New meetings file agent to-dos there.";
    }).catch(() => {
      select.value = agentTodos.target_workspace ?? "";
      status.textContent = "Couldn’t save. Try again in a moment.";
    }).finally(() => { select.disabled = false; });
  });
  label.append(caption, select, status);
  return label;
}

const STATE_PILL: Record<AgentTodoState, [string, Tone]> = {
  queued: ["Queued", "neutral"],
  working: ["Working", "working"],
  review: ["In review", "working"],
  merged: ["Merged", "done"],
  done: ["Done", "done"],
  held: ["Held", "attention"],
};

/** One agent to-do from a meeting: title and state, then task id, crew, PR and source. */
function agentRow(todo: AgentTodo): HTMLElement {
  const [label, tone] = STATE_PILL[todo.state];
  const meta = document.createElement("div");
  meta.className = "todos-agent-meta";
  meta.append(monoMeta(todo.state === "held" ? "not filed for agents" : `${todo.id} · ${todo.workspace}`));
  if (todo.crew !== null) meta.append(monoMeta(todo.crew));
  if (todo.pr !== null) {
    const pr = monoMeta(`PR #${todo.pr.number}`, todo.pr.url ?? undefined);
    if (pr instanceof HTMLAnchorElement) { pr.target = "_blank"; pr.rel = "noopener noreferrer"; }
    meta.append(pr);
  }
  if (todo.source !== null) meta.append(monoMeta(todo.source, sourceHref(todo.source)));
  const details: HTMLElement[] = [meta];
  if (todo.state === "held") {
    const note = document.createElement("p");
    note.className = "todos-held-note";
    note.textContent = "Held: its matter is contested. Choose which decision stands on Today to release it.";
    details.push(note);
  }
  return row(todo.title, { details, trail: [pill(label, tone)] });
}

function column(
  kind: "you" | "agents",
  title: string,
  lede: string,
  items: readonly TodoItem[],
  onClose: ((id: string) => void) | undefined,
  schedule: ScheduleActions | undefined,
  meeting: readonly AgentTodo[] = [],
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
  if (meeting.length > 0) section.append(rows(meeting.map(agentRow), "Agent to-dos from meetings"));
  if (items.length === 0 && meeting.length > 0) return section;
  if (items.length === 0) {
    const empty = kind === "you"
      ? emptyState("All clear", "Nothing is waiting on you.")
      : emptyState("Nothing in progress", "No agent to-dos right now.");
    section.append(empty);
    return section;
  }
  for (const item of items) section.append(todoCard(item, onClose, schedule, kind === "you"));
  return section;
}

function todoCard(
  item: TodoItem,
  onClose: ((id: string) => void) | undefined,
  schedule: ScheduleActions | undefined,
  due: boolean,
): HTMLElement {
  const content = document.createElement("div");
  content.className = "card-content";
  content.append(sourceBlock(item.source), knowledgeBlock(item.knowledge));
  const when = due ? dueBlock(item, schedule) : scheduleBlock(item, schedule);
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

/**
 * A to-do of yours: its scheduled time is its due date, since nothing starts
 * it for you. "Due Fri 09:00" (with **Overdue** once it has passed), and
 * **Set due date…** (or **Change due date…** and **Clear due date**), all
 * writing `not_before`. The open picker takes the actions' place.
 */
function dueBlock(item: TodoItem, schedule: ScheduleActions | undefined): HTMLElement | null {
  const now = schedule?.now ?? new Date();
  const canSchedule = schedule !== undefined && item.schedulable;
  if (item.scheduled === null && !canSchedule) return null;
  const block = document.createElement("section");
  block.className = "todos-schedule";
  block.setAttribute("aria-label", "Due date");
  if (item.scheduled !== null) {
    const words = document.createElement("p");
    words.className = "todos-when";
    words.textContent = dueWords(item.scheduled, now);
    block.append(words);
    if (isOverdue(item.scheduled, now)) block.append(pill("Overdue", "attention"));
  }
  if (!canSchedule) return block;
  if (schedule.editing === item.id) {
    block.append(schedulePicker({
      initial: item.scheduled,
      now,
      label: "Due date",
      save: (instant) => schedule.onSchedule(item.id, instant),
      cancel: () => schedule.onEdit(null),
      failed: () => "The due date could not be saved. Try again in a moment.",
    }));
    return block;
  }
  block.append(scheduleButton(item.scheduled === null ? "Set due date…" : "Change due date…", () => schedule.onEdit(item.id)));
  if (item.scheduled !== null) {
    const clear = scheduleButton("Clear due date", () => {
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
