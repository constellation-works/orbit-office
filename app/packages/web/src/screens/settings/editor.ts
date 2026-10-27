import type { ConnectionInput, ConnectionWrite, SettingsConnection } from "@orbit-office/contracts";

function text<K extends keyof HTMLElementTagNameMap>(tag: K, content: string): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag); node.textContent = content; return node;
}

/** The password input is always empty on open and is cleared before awaiting a write. */
export function connectionEditor(existing: SettingsConnection | undefined, save: (value: ConnectionWrite) => Promise<boolean>, cancel: () => void): HTMLFormElement {
  const form = document.createElement("form"); form.className = "settings-form";
  form.setAttribute("aria-label", existing ? "Edit connection" : "Add connection");
  form.append(text("h3", existing ? `Edit ${existing.name}` : "Add a connection"));
  const fields = document.createElement("div"); fields.className = "settings-fields";
  function field(label: string, name: string, value: string, type = "text"): HTMLInputElement {
    const wrap = text("label", label); wrap.className = "settings-field";
    const input = document.createElement("input"); input.name = name; input.type = type; input.value = value;
    input.required = true; wrap.append(input); fields.append(wrap); return input;
  }
  const name = field("Connection name", "name", existing?.name ?? "");
  name.pattern = "[a-z][a-z0-9-]{0,47}"; name.maxLength = 48; name.readOnly = existing !== undefined;
  name.title = "Start with a lowercase letter; use lowercase letters, digits and hyphens.";
  const url = field("Base URL", "base_url", existing?.base_url ?? "", "url"); url.placeholder = "https://api.example.com/v2/";
  const authWrap = text("label", "Authentication"); authWrap.className = "settings-field";
  const auth = document.createElement("select"); auth.name = "scheme";
  auth.setAttribute("aria-label", "Authentication");
  for (const [value, label] of [["bearer", "Bearer token"], ["basic", "Username and password"], ["header", "API key header"], ["none", "No authentication"]]) {
    const option = text("option", label ?? ""); option.value = value ?? ""; auth.append(option);
  }
  auth.value = existing?.auth.scheme ?? "bearer"; authWrap.append(auth); fields.append(authWrap);
  const username = field("Username", "username", existing?.auth.username ?? "");
  const header = field("Header name", "header", existing?.auth.header ?? ""); header.placeholder = "X-Api-Key";
  const secret = field("Secret", "secret", "", "password"); secret.required = false; secret.autocomplete = "new-password";
  secret.setAttribute("aria-label", "Secret"); secret.setAttribute("aria-describedby", "settings-secret-hint");
  secret.placeholder = existing?.slot_state === "set" ? "Leave empty to keep the saved secret" : "Enter a secret";
  const hint = text("span", `Secret ${existing?.slot_state === "set" ? "set" : "unset"}. Stored securely and never shown again.`);
  hint.id = "settings-secret-hint";
  hint.className = "settings-hint"; secret.parentElement?.append(hint);
  function authFields(): void {
    username.disabled = auth.value !== "basic"; header.disabled = auth.value !== "header"; secret.disabled = auth.value === "none";
    if (username.parentElement) username.parentElement.hidden = username.disabled;
    if (header.parentElement) header.parentElement.hidden = header.disabled;
    if (secret.parentElement) secret.parentElement.hidden = secret.disabled;
    if (secret.disabled) secret.value = "";
  }
  auth.addEventListener("change", authFields); authFields();
  function methods(label: string, options: string[], selected: string[]): () => string[] {
    const group = document.createElement("fieldset"); group.append(text("legend", label));
    const inputs = options.map((method) => {
      const wrap = text("label", method); const input = document.createElement("input"); input.type = "checkbox";
      input.value = method; input.checked = selected.includes(method); wrap.prepend(input); group.append(wrap); return input;
    });
    fields.append(group); return () => inputs.filter((input) => input.checked).map((input) => input.value);
  }
  const direct = methods("Allowed reads", ["GET", "HEAD"], existing?.methods ?? ["GET"]);
  const drafts = methods("Writes requiring approval", ["POST", "PUT", "PATCH", "DELETE"], existing?.drafts ?? []);
  const timeout = field("Timeout (seconds)", "timeout_seconds", String(existing?.timeout_seconds ?? 20), "number");
  timeout.min = "1"; timeout.max = "120"; timeout.step = "1";
  const bytes = field("Maximum response (bytes)", "max_response_bytes", String(existing?.max_response_bytes ?? 1048576), "number");
  bytes.min = "1"; bytes.max = "16777216"; bytes.step = "1";
  const actions = document.createElement("div"); actions.className = "settings-actions";
  const submit = text("button", "Save connection"); submit.type = "submit";
  const back = text("button", "Cancel"); back.type = "button"; back.addEventListener("click", () => { secret.value = ""; cancel(); });
  actions.append(submit, back); form.append(fields, actions);
  let busy = false;
  form.addEventListener("submit", (event) => {
    event.preventDefault(); if (busy) return;
    const scheme = auth.value as ConnectionInput["auth"]["scheme"];
    const value: ConnectionWrite = {
      name: name.value, base_url: url.value, auth: { scheme,
        ...(scheme === "header" ? { header: header.value } : {}), ...(scheme === "basic" ? { username: username.value } : {}) },
      methods: direct(), drafts: drafts(), timeout_seconds: Number(timeout.value), max_response_bytes: Number(bytes.value),
      disabled: existing?.disabled ?? false, ...(secret.value ? { secret: secret.value } : {}),
    };
    secret.value = ""; busy = true;
    const controls = Array.from(form.querySelectorAll<HTMLInputElement | HTMLButtonElement | HTMLSelectElement>("input, button, select"));
    for (const control of controls) control.disabled = true;
    submit.textContent = "Saving…";
    void save(value).finally(() => {
      delete value.secret;
      busy = false;
      for (const control of controls) control.disabled = false;
      authFields(); submit.textContent = "Save connection";
    });
  });
  return form;
}
