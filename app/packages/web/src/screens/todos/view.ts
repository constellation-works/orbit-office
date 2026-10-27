/**
 * Draw the To-dos screen. Visible text is plain words: the task id stays
 * in the close handler and is never written into the page.
 */
import type { TodoItem, TodoList } from "@orbit-office/contracts";

import { card, emptyState, provenanceLink } from "../../ui/index.js";

export interface TodoDrawOptions {
  /** Kept attached so a poll can tell that the screen is still showing. */
  marker?: HTMLElement;
  onClose?: (id: string) => void;
}

const STYLE = `
.todos-columns { display: grid; grid-template-columns: 1fr 1fr; gap: 28px; align-items: start; }
.todos-column h2 { font-size: 1.4rem; }
.todos-lede { margin: 6px 0 16px; }
.todos-column .card + .card { margin-top: 16px; }
.card-content p.todos-kicker { margin: 14px 0 4px; font-size: .75rem; font-weight: 700; letter-spacing: .04em; color: #225e55; }
.card-content p.todos-kicker:first-child, .card-content p.todos-note:first-child { margin-top: 0; }
.card-content p.todos-note { margin: 0; }
.todos-knowledge { margin: 0; padding-left: 1.15rem; }
.todos-more { margin-top: 20px; }
.todos-marker { display: none; }
@media (max-width: 720px) {
  .todos-columns { grid-template-columns: 1fr; gap: 32px; }
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
  style.textContent = STYLE;

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
    column("you", "You", "Close these when you are finished.", model.you, options.onClose),
    column("agents", "Agents", "These stay with the agents.", model.agents, options.onClose),
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
  for (const item of items) section.append(todoCard(item, onClose));
  return section;
}

function todoCard(item: TodoItem, onClose: ((id: string) => void) | undefined): HTMLElement {
  const content = document.createElement("div");
  content.className = "card-content";
  content.append(sourceBlock(item.source), knowledgeBlock(item.knowledge));
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
    });
  }
  return items;
}
