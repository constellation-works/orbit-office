/**
 * To-dos: You and Agents columns. Each to-do shows its source and attached
 * knowledge, and the user can close their own. Any open to-do can be
 * scheduled for a date and time, rescheduled or cleared; the poll waits
 * while the picker is open (docs/design/office/specs/web-app.md).
 */
import type { TodoList } from "@orbit-office/contracts";

import type { Screen } from "../../screen.js";
import { ApiError, getJson, scheduleTodo } from "../../api/client.js";
import { errorState } from "../../ui/index.js";
import { drawTodoScreen, modelFromPayload } from "./view.js";

const POLL_MS = 4_000;

async function renderTodos(root: HTMLElement): Promise<void> {
  const marker = document.createElement("span");
  marker.className = "todos-marker";
  let timer: ReturnType<typeof setInterval> | undefined;
  let stopped = false;
  let busy = false;
  /** The to-do whose picker is open; polling leaves the page alone meanwhile. */
  let editing: string | null = null;
  let shown: TodoList | null = null;

  const stop = (): void => {
    stopped = true;
    if (timer !== undefined) clearInterval(timer);
  };
  const alive = (): boolean => !stopped && marker.isConnected;

  const showError = (title: string, message: string): void => {
    if (!marker.isConnected) {
      stop();
      return;
    }
    root.replaceChildren(marker, errorState(title, message));
  };

  async function refresh(): Promise<void> {
    const payload: unknown = await getJson<unknown>("/api/todos");
    if (!alive()) return;
    const model = modelFromPayload(payload);
    if (model === null) throw new ApiError(200, "invalid_json");
    shown = model;
    if (editing !== null && ![...model.you, ...model.agents].some((item) => item.id === editing && item.schedulable)) editing = null;
    draw(model);
  }

  function draw(model: TodoList): void {
    drawTodoScreen(root, model, { marker, onClose, onSchedule, onEdit, editing, now: new Date() });
  }

  async function tick(): Promise<void> {
    if (!alive()) {
      stop();
      return;
    }
    if (busy || editing !== null) return;
    try {
      await refresh();
    } catch {
      if (!alive()) return;
      showError("To-dos are unavailable", "Try again in a moment.");
    }
  }

  function onClose(id: string): void {
    if (!alive()) return;
    busy = true;
    void (async () => {
      try {
        await postJson("/api/todos/close", { id });
        await refresh();
      } catch (error) {
        const message = error instanceof ApiError ? closeMessage(error) : "Try again in a moment.";
        showError("Could not close that to-do", message);
      } finally {
        busy = false;
      }
    })();
  }

  function onEdit(id: string | null): void {
    if (!alive()) return;
    editing = id;
    if (shown !== null) draw(shown);
  }

  /**
   * Write the time (or clear it with null), then reread the list. A failed
   * save rejects, so the open picker says so and stays open; a failed clear
   * shows the error state, as a failed close does.
   */
  async function onSchedule(id: string, notBefore: string | null): Promise<void> {
    if (!alive()) return;
    busy = true;
    try {
      await scheduleTodo(id, notBefore);
      editing = null;
      await refresh();
    } catch (error) {
      if (notBefore !== null && editing === id) throw error;
      const message = error instanceof ApiError ? scheduleMessage(error) : "Try again in a moment.";
      showError("Could not clear the schedule", message);
    } finally {
      busy = false;
    }
  }

  const loading = document.createElement("p");
  loading.className = "intro";
  loading.textContent = "Loading to-dos.";
  root.replaceChildren(marker, loading);
  await tick();
  if (!alive()) return;
  timer = setInterval(() => {
    void tick();
  }, POLL_MS);
  // Node tests would hang on an active interval. Browsers return a number,
  // which has no unref, so the call is skipped there.
  const handle = timer as unknown as { unref?: () => void };
  handle.unref?.();
}

function closeMessage(error: ApiError): string {
  if (error.code === "not_yours") return "That to-do is not yours to close.";
  if (error.code === "not_found") return "That to-do is no longer here.";
  if (error.code === "not_closable") return "That to-do cannot be closed.";
  return "Try again in a moment.";
}

function scheduleMessage(error: ApiError): string {
  if (error.code === "not_found") return "That to-do is no longer here.";
  if (error.code === "not_schedulable") return "That to-do has started, so its time can no longer change.";
  return "Try again in a moment.";
}

async function postJson(path: string, body: unknown): Promise<void> {
  let response: Response;
  try {
    response = await fetch(path, {
      method: "POST",
      headers: { accept: "application/json", "content-type": "application/json" },
      body: JSON.stringify(body),
    });
  } catch {
    throw new ApiError(0, "network_error");
  }
  if (!response.ok) {
    const payload: unknown = await response.json().catch(() => undefined);
    const code = payload !== null && typeof payload === "object" && "error" in payload
      && typeof payload.error === "string"
      ? payload.error
      : "unknown";
    throw new ApiError(response.status, code);
  }
}

export const screen: Screen = {
  id: "todos",
  title: "To-dos",
  status: "built",
  render: renderTodos,
};
