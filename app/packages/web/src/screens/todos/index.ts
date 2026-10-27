/**
 * To-dos: You and Agents columns. Each to-do shows its source and attached
 * knowledge, and the user can close their own
 * (docs/design/office/specs/web-app.md).
 */
import type { Screen } from "../../screen.js";
import { ApiError, getJson } from "../../api/client.js";
import { errorState } from "../../ui/index.js";
import { drawTodoScreen, modelFromPayload } from "./view.js";

const POLL_MS = 4_000;

async function renderTodos(root: HTMLElement): Promise<void> {
  const marker = document.createElement("span");
  marker.className = "todos-marker";
  let timer: ReturnType<typeof setInterval> | undefined;
  let stopped = false;
  let busy = false;

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
    drawTodoScreen(root, model, { marker, onClose });
  }

  async function tick(): Promise<void> {
    if (!alive() || busy) return;
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
