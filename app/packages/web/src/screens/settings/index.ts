/** Settings sections. Navigation registration belongs to the integrator. */
import type { ConnectionWrite, SettingsApiCalls, SettingsConnection } from "@orbit-office/contracts";
import { ApiError, getApiCalls, getConnections, getPrivateRoute, getSettings, getSharedLibrary, putConnection } from "../../api/client.js";
import type { Screen } from "../../screen.js";
import { PRIVATE_ROUTE_EVENT, card, emptyState, errorState, pill } from "../../ui/index.js";
import { connectionEditor } from "./editor.js";
import { mountPrivateRoute } from "./private.js";
import { mountSharedLibrary } from "./shared.js";
import { signInPanel } from "./signin.js";
import { style } from "./style.js";

function text<K extends keyof HTMLElementTagNameMap>(tag: K, content: string): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag); node.textContent = content; return node;
}
function button(label: string, action: () => void): HTMLButtonElement {
  const node = text("button", label); node.type = "button"; node.addEventListener("click", action); return node;
}
function config(item: SettingsConnection): ConnectionWrite {
  const { slot_state: _state, auth_state: _signedIn, ...input } = item; return input;
}

/** What the card says about a connection's credential, in plain words. */
function credentialState(item: SettingsConnection): string {
  if (item.auth.scheme === "oauth2_device") {
    const state = item.auth_state ?? (item.slot_state === "set" ? "signed_in" : "signed_out");
    return state === "signed_in" ? "Signed in" : state === "expired" ? "Sign-in expired" : "Not signed in";
  }
  if (item.slot_state === "none") return "No authentication";
  if (item.auth.scheme === "secret_path") return `Secret address ${item.slot_state}`;
  return `Secret ${item.slot_state}`;
}

/** The sign-in action an oauth2_device card offers, if any. */
function signInLabel(item: SettingsConnection): string | undefined {
  if (item.auth.scheme !== "oauth2_device" || item.disabled) return undefined;
  const state = item.auth_state ?? (item.slot_state === "set" ? "signed_in" : "signed_out");
  return state === "expired" ? "Sign in again" : state === "signed_out" ? "Sign in" : undefined;
}

function saveMessage(error: unknown): string {
  if (error instanceof ApiError) {
    if (["secret_not_saved", "secret_timeout"].includes(error.code)) return "The connection was saved, but its secret could not be saved. Edit it to enter the secret again.";
    if (error.code === "secret_cleanup_failed") return "The connection was saved, but the previous secret could not be removed. Please try saving again after checking Orbit.";
    if (error.code === "no_free_slot") return "All secret slots are in use. Remove an unused secret in Orbit before adding this connection.";
    if (error.code === "settings_busy") return "Another connection is being saved. Please try again in a moment.";
    if (["invalid_input", "invalid_connection"].includes(error.code)) return "Check the connection fields. Use HTTPS, or HTTP for a local API, and the authentication fields for that API.";
  }
  return "Could not confirm the save. Refresh connections before trying again.";
}

type Section = "Connections" | "API calls" | "Private route" | "Shared library";

/** `?settings=shared-library#settings` opens that section; the query is then dropped. */
function linkedSection(): Section | undefined {
  const params = new URLSearchParams(window.location.search);
  if (params.get("settings") !== "shared-library") return undefined;
  params.delete("settings");
  const query = params.toString();
  window.history.replaceState(window.history.state, "", `${window.location.pathname}${query === "" ? "" : `?${query}`}${window.location.hash}`);
  return "Shared library";
}

async function render(root: HTMLElement): Promise<void> {
  const view = document.createElement("div"); view.className = "settings";
  const css = text("style", style);
  const tabs = document.createElement("div"); tabs.className = "settings-tabs"; tabs.setAttribute("aria-label", "Settings sections");
  const content = document.createElement("section");
  const cli = text("p", "Checking agent CLI connection…"); cli.className = "settings-cli settings-meta";
  let generation = 0;
  let busy = false;
  const alive = (): boolean => root.contains(view) && view.isConnected;
  const connectionsTab = button("Connections", () => { void showConnections(); });
  const privateTab = button("Private route", () => { void showPrivate(); });
  const sharedTab = button("Shared library", () => { void showShared(); });
  const callsTab = button("API calls", () => { void showCalls(); });
  function setTabsDisabled(disabled: boolean): void {
    connectionsTab.disabled = disabled;
    privateTab.disabled = disabled;
    sharedTab.disabled = disabled;
    callsTab.disabled = disabled;
  }
  tabs.append(connectionsTab, privateTab, sharedTab, callsTab);
  view.append(text("h1", "Settings"), text("p", "Manage your API access, the private route, the team's shared library, and what was called."), tabs, content, cli);
  root.replaceChildren(css, view);

  function select(section: Section): number {
    generation += 1;
    connectionsTab.setAttribute("aria-pressed", String(section === "Connections"));
    privateTab.setAttribute("aria-pressed", String(section === "Private route"));
    sharedTab.setAttribute("aria-pressed", String(section === "Shared library"));
    callsTab.setAttribute("aria-pressed", String(section === "API calls"));
    content.setAttribute("aria-label", section); content.replaceChildren(text("p", "Loading…")); return generation;
  }
  function current(token: number): boolean { return alive() && token === generation; }
  function failed(title: string, retry: () => void): void {
    content.replaceChildren(errorState(title, "Check that Orbit and the gateway plugin are available, then try again."), button("Try again", retry));
  }

  async function showConnections(message = ""): Promise<void> {
    const token = select("Connections");
    try {
      const data = await getConnections(); if (!current(token)) return;
      const header = document.createElement("div"); header.className = "settings-heading";
      const editor = document.createElement("div");
      const feedback = text("p", message); feedback.className = "settings-feedback"; feedback.setAttribute("role", "status");
      function signIn(item: SettingsConnection): void {
        if (busy) return;
        feedback.textContent = "";
        editor.replaceChildren(signInPanel(item.name, {
          alive: () => current(token),
          done: (message) => { void showConnections(message); },
          close: () => editor.replaceChildren(),
        }));
      }
      function edit(item?: SettingsConnection): void {
        if (busy) return;
        feedback.textContent = "";
        editor.replaceChildren(connectionEditor(item, async (value) => {
          const ok = await save(value); if (ok && current(token)) editor.replaceChildren(); return ok;
        }, () => editor.replaceChildren()));
        editor.querySelector<HTMLInputElement>("input")?.focus();
      }
      async function save(value: ConnectionWrite): Promise<boolean> {
        if (busy) return false;
        busy = true;
        setTabsDisabled(true);
        for (const control of list.querySelectorAll<HTMLButtonElement>("button")) control.disabled = true;
        try {
          await putConnection(value);
          if (current(token)) await showConnections("Connection saved.");
          return true;
        } catch (error) {
          if (current(token)) { feedback.setAttribute("role", "alert"); feedback.textContent = saveMessage(error); }
          return false;
        } finally {
          delete value.secret;
          busy = false; setTabsDisabled(false);
          for (const control of list.querySelectorAll<HTMLButtonElement>("button")) control.disabled = false;
        }
      }
      header.append(text("h2", "Connections"), button("Add connection", () => edit()));
      const list = document.createElement("div"); list.className = "card-grid";
      for (const item of data.connections) {
        const detail = document.createElement("div"); detail.className = "card-content";
        const url = text("p", item.base_url); url.className = "settings-url";
        const secretState = text("p", credentialState(item));
        if (item.auth_state === "expired") secretState.className = "settings-attention";
        detail.append(url, text("p", `${item.methods.join(", ") || "No reads"} · ${item.drafts.length ? `${item.drafts.join(", ")} with approval` : "No writes"}`), secretState);
        const actions = document.createElement("div"); actions.className = "settings-actions";
        const sign = signInLabel(item);
        if (sign !== undefined) actions.append(button(sign, () => signIn(item)));
        actions.append(button("Edit", () => edit(item)), button(item.disabled ? "Enable" : "Disable", () => { void save({ ...config(item), disabled: !item.disabled }); }));
        detail.append(actions);
        const node = card(item.name, detail); node.prepend(pill(item.disabled ? "Disabled" : "Enabled")); list.append(node);
      }
      if (!data.connections.length) list.append(emptyState("No connections yet", "Add an API connection to give your agents access."));
      content.replaceChildren(header, text("p", "Choose which APIs agents can read and which writes need your approval."), feedback, editor, list);
      const slots = text("p", `${data.free_slots} secret slots available.`); slots.className = "settings-meta"; content.append(slots);
    } catch { if (current(token)) failed("Connections are unavailable", () => { void showConnections(); }); }
  }

  async function showPrivate(): Promise<void> {
    const token = select("Private route");
    try {
      const data = await getPrivateRoute();
      if (!current(token)) return;
      mountPrivateRoute(content, {
        data,
        busy: () => busy,
        setBusy(value: boolean) {
          busy = value;
          setTabsDisabled(value);
          for (const control of content.querySelectorAll<HTMLButtonElement>("button")) control.disabled = value;
        },
        alive: () => current(token),
        onConfigured(configured: boolean) {
          window.dispatchEvent(new CustomEvent(PRIVATE_ROUTE_EVENT, { detail: { configured } }));
        },
      });
    } catch { if (current(token)) failed("The private route is unavailable", () => { void showPrivate(); }); }
  }

  async function showShared(): Promise<void> {
    const token = select("Shared library");
    try {
      const data = await getSharedLibrary();
      if (!current(token)) return;
      mountSharedLibrary(content, {
        data,
        busy: () => busy,
        setBusy(value: boolean) { busy = value; setTabsDisabled(value); },
        alive: () => current(token),
      });
    } catch {
      if (current(token)) {
        content.replaceChildren(errorState("The shared library settings are unavailable", "Check that Orbit and Office are available, then try again."), button("Try again", () => { void showShared(); }));
      }
    }
  }

  async function showCalls(): Promise<void> {
    const token = select("API calls");
    try {
      const data = await getApiCalls(); if (!current(token)) return;
      const header = document.createElement("div"); header.className = "settings-heading";
      header.append(text("h2", "API calls"), button("Refresh", () => { void showCalls(); }));
      content.replaceChildren(header, text("p", "Recent gateway requests, newest first. Response bodies are not recorded."), auditList(data));
    } catch { if (current(token)) failed("API calls are unavailable", () => { void showCalls(); }); }
  }
  const status = getSettings().then((data) => { if (alive()) cli.textContent = `${data.agentCli.cli}: ${data.agentCli.connected ? "connected" : "not connected"}`; })
    .catch(() => { if (alive()) cli.textContent = "Agent CLI connection unavailable."; });
  if (linkedSection() === "Shared library") await showShared();
  else await showConnections();
  await status;
}

/** The screen sorts independently so any valid fixture ordering renders newest first. */
export function auditList(data: SettingsApiCalls): HTMLElement {
  const section = document.createElement("div");
  const list = document.createElement("ol"); list.className = "api-calls";
  for (const row of [...data.records].sort((a, b) => Date.parse(b.time) - Date.parse(a.time))) {
    const item = document.createElement("li"); item.className = "api-call";
    const header = document.createElement("header");
    const time = text("time", new Date(row.time).toLocaleString()); time.dateTime = row.time;
    header.append(text("strong", row.connection ?? "Gateway"), time);
    item.append(header, text("h3", `${row.method ?? "Request"} ${row.path ?? ""}`));
    const outcome = row.outcome === "ok" ? "Completed" : row.outcome === "outcome_unknown" ? "Outcome unknown" : "Request failed or refused";
    const result = text("p", `${outcome} · ${row.status === null ? "No HTTP response" : `HTTP ${row.status}`} · ${row.duration_ms} ms · ${row.bytes.toLocaleString()} bytes`);
    result.className = "settings-meta"; item.append(result);
    const caller = [row.caller.agent, row.caller.model].filter(Boolean).join(" · ");
    if (caller) item.append(text("p", `Called by ${caller}`));
    list.append(item);
  }
  section.append(data.records.length ? list : emptyState("No API calls yet", "Requests made through the gateway will appear here."));
  if (data.skipped) section.append(text("p", `${data.skipped} unreadable audit entries were omitted.`));
  section.append(text("p", "Showing up to 100 recent calls.")); return section;
}

export const screen: Screen = { id: "settings", title: "Settings", status: "built", render };
