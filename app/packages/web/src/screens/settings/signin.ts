/** The device-code sign-in for an oauth2_device connection: the code, where to enter it, then the result. */
import type { SignInStarted } from "@orbit-office/contracts";
import { ApiError, pollSignIn, startSignIn } from "../../api/client.js";

function text<K extends keyof HTMLElementTagNameMap>(tag: K, content: string): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag); node.textContent = content; return node;
}
function button(label: string, action: () => void): HTMLButtonElement {
  const node = text("button", label); node.type = "button"; node.addEventListener("click", action); return node;
}

export interface SignInHooks {
  /** False once the Connections section is gone or re-rendered. */
  alive(): boolean;
  /** The connection is signed in: refresh the list with this message. */
  done(message: string): void;
  close(): void;
}

function failure(error: unknown): string {
  if (error instanceof ApiError) {
    if (error.code === "sign_in_not_started") return "The sign-in was interrupted. Start it again.";
    if (error.code === "sign_in_failed") return "The sign-in service refused the request. Check the connection's client ID, scopes and URLs.";
    if (error.code === "unknown_connection") return "This connection is disabled or no longer exists. Refresh connections.";
    if (["invalid_input", "invalid_connection"].includes(error.code)) return "This connection does not sign in with a code.";
  }
  return "Could not reach the sign-in service. Check that Orbit and the gateway plugin are available, then try again.";
}

/** Starts the flow at once and polls at the interval the service asks for until it ends or the panel goes away. */
export function signInPanel(name: string, hooks: SignInHooks): HTMLElement {
  const panel = document.createElement("section"); panel.className = "settings-form settings-signin";
  panel.setAttribute("aria-label", `Sign in to ${name}`);
  const body = document.createElement("div");
  const status = text("p", "Starting the sign-in…"); status.className = "settings-feedback"; status.setAttribute("role", "status");
  const actions = document.createElement("div"); actions.className = "settings-actions";
  panel.append(text("h3", `Sign in to ${name}`), body, status, actions);
  let stopped = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let interval = 5;
  const live = (): boolean => !stopped && hooks.alive() && panel.isConnected;
  function stop(): void { stopped = true; if (timer !== undefined) clearTimeout(timer); }
  function cancelButton(): HTMLButtonElement { return button("Cancel", () => { stop(); hooks.close(); }); }

  function show(started: SignInStarted): void {
    const where = text("p", "Open ");
    const link = text("a", started.verification_uri); link.href = started.verification_uri;
    link.target = "_blank"; link.rel = "noopener noreferrer"; link.className = "settings-url";
    where.append(link, " and enter this code:");
    const code = text("p", started.user_code); code.className = "settings-code"; code.setAttribute("aria-label", "Sign-in code");
    const minutes = Math.max(1, Math.round(started.expires_in / 60));
    const expiry = text("p", `The code works for ${minutes} minute${minutes === 1 ? "" : "s"}. This page updates once you approve.`);
    expiry.className = "settings-meta";
    body.replaceChildren(where, code, expiry);
    status.setAttribute("role", "status"); status.textContent = "Waiting for you to approve the sign-in…";
    actions.replaceChildren(cancelButton());
  }
  function fail(message: string): void {
    stop();
    status.setAttribute("role", "alert"); status.textContent = message;
    actions.replaceChildren(button("Try again", () => { stopped = false; void start(); }), cancelButton());
  }
  function schedule(seconds: number): void {
    timer = setTimeout(() => { void poll(); }, Math.max(1, seconds) * 1000);
  }
  async function start(): Promise<void> {
    body.replaceChildren(); status.setAttribute("role", "status"); status.textContent = "Starting the sign-in…";
    actions.replaceChildren(cancelButton());
    try {
      const started = await startSignIn(name); if (!live()) return;
      interval = started.interval; show(started); schedule(interval);
    } catch (error) { if (live()) fail(failure(error)); }
  }
  async function poll(): Promise<void> {
    if (!live()) return;
    try {
      const result = await pollSignIn(name); if (!live()) return;
      if (result.state === "pending") { interval = result.interval ?? interval; schedule(interval); return; }
      if (result.state === "connected") { stop(); hooks.done(`Signed in to ${name}.`); return; }
      body.replaceChildren();
      fail(result.state === "denied" ? "The sign-in was declined." : "The code expired before the sign-in was approved.");
    } catch (error) { if (live()) fail(failure(error)); }
  }
  void start();
  return panel;
}
