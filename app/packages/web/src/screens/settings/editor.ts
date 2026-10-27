import type { ConnectionInput, ConnectionWrite, SettingsConnection } from "@orbit-office/contracts";

function text<K extends keyof HTMLElementTagNameMap>(tag: K, content: string): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag); node.textContent = content; return node;
}

/** Microsoft's endpoints, offered for a new device-code connection; every field stays editable. */
const MICROSOFT = {
  token_url: "https://login.microsoftonline.com/common/oauth2/v2.0/token",
  device_url: "https://login.microsoftonline.com/common/oauth2/v2.0/devicecode",
  scopes: "Calendars.Read offline_access",
};

/**
 * A pasted full feed address splits into its origin (the base URL) and its
 * path and query (the secret), so the secret part never lands in the base URL.
 */
export function splitFeedAddress(baseUrl: string, secret: string): { base_url: string; secret: string } {
  if (!/^https?:\/\//i.test(secret)) return { base_url: baseUrl, secret };
  try {
    const url = new URL(secret);
    return { base_url: `${url.origin}/`, secret: `${url.pathname}${url.search}` };
  } catch { return { base_url: baseUrl, secret }; }
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
  function relabel(input: HTMLInputElement, label: string): void {
    const node = input.parentElement?.firstChild; if (node) node.textContent = label;
  }
  const name = field("Connection name", "name", existing?.name ?? "");
  name.pattern = "[a-z][a-z0-9-]{0,47}"; name.maxLength = 48; name.readOnly = existing !== undefined;
  name.title = "Start with a lowercase letter; use lowercase letters, digits and hyphens.";
  const url = field("Base URL", "base_url", existing?.base_url ?? "", "url"); url.placeholder = "https://api.example.com/v2/";
  const authWrap = text("label", "Authentication"); authWrap.className = "settings-field";
  const auth = document.createElement("select"); auth.name = "scheme";
  auth.setAttribute("aria-label", "Authentication");
  for (const [value, label] of [
    ["bearer", "Bearer token"], ["basic", "Username and password"], ["header", "API key header"],
    ["secret_path", "Secret address (calendar feed)"], ["oauth2_device", "Sign in with a code (OAuth device code)"],
    ["none", "No authentication"],
  ]) {
    const option = text("option", label ?? ""); option.value = value ?? ""; auth.append(option);
  }
  auth.value = existing?.auth.scheme ?? "bearer"; authWrap.append(auth); fields.append(authWrap);
  const username = field("Username", "username", existing?.auth.username ?? "");
  const header = field("Header name", "header", existing?.auth.header ?? ""); header.placeholder = "X-Api-Key";
  const oauthFields = [
    field("Token URL", "token_url", existing?.auth.token_url ?? "", "url"),
    field("Device code URL", "device_url", existing?.auth.device_url ?? "", "url"),
    field("Client ID", "client_id", existing?.auth.client_id ?? ""),
    field("Scopes", "scopes", existing?.auth.scopes?.join(" ") ?? ""),
  ] as const;
  const [tokenUrl, deviceUrl, clientId, scopes] = oauthFields;
  scopes.title = "Separate scopes with spaces.";
  const secret = field("Secret", "secret", "", "password"); secret.required = false; secret.autocomplete = "new-password";
  secret.setAttribute("aria-describedby", "settings-secret-hint");
  const hint = text("span", ""); hint.id = "settings-secret-hint";
  hint.className = "settings-hint"; secret.parentElement?.append(hint);
  const isSet = existing?.slot_state === "set";
  function authFields(): void {
    const scheme = auth.value;
    username.disabled = scheme !== "basic"; header.disabled = scheme !== "header";
    secret.disabled = scheme === "none" || scheme === "oauth2_device";
    for (const input of oauthFields) input.disabled = scheme !== "oauth2_device";
    for (const input of [username, header, secret, ...oauthFields]) if (input.parentElement) input.parentElement.hidden = input.disabled;
    if (secret.disabled) secret.value = "";
    const feed = scheme === "secret_path";
    relabel(url, feed ? "Origin" : "Base URL");
    // A pasted full address fills the origin on save.
    url.required = !feed;
    url.placeholder = feed ? "https://outlook.office365.com/" : "https://api.example.com/v2/";
    relabel(secret, feed ? "Secret address" : "Secret");
    secret.setAttribute("aria-label", feed ? "Secret address" : "Secret");
    secret.placeholder = isSet ? "Leave empty to keep the saved secret" : feed ? "Paste the feed's full address" : "Enter a secret";
    hint.textContent = feed
      ? `Address ${isSet ? "set" : "unset"}. The path and query are stored as the secret and never shown again.`
      : `Secret ${isSet ? "set" : "unset"}. Stored securely and never shown again.`;
    if (scheme === "oauth2_device" && existing === undefined) {
      if (!tokenUrl.value) tokenUrl.value = MICROSOFT.token_url;
      if (!deviceUrl.value) deviceUrl.value = MICROSOFT.device_url;
      if (!scopes.value) scopes.value = MICROSOFT.scopes;
    }
    writes.hidden = feed;
  }
  function methods(label: string, options: string[], selected: string[]): [HTMLFieldSetElement, () => string[]] {
    const group = document.createElement("fieldset"); group.append(text("legend", label));
    const inputs = options.map((method) => {
      const wrap = text("label", method); const input = document.createElement("input"); input.type = "checkbox";
      input.value = method; input.checked = selected.includes(method); wrap.prepend(input); group.append(wrap); return input;
    });
    fields.append(group); return [group, () => inputs.filter((input) => input.checked).map((input) => input.value)];
  }
  const [, direct] = methods("Allowed reads", ["GET", "HEAD"], existing?.methods ?? ["GET"]);
  // A secret address names one resource to read, so it has no writes.
  const [writes, drafts] = methods("Writes requiring approval", ["POST", "PUT", "PATCH", "DELETE"], existing?.drafts ?? []);
  auth.addEventListener("change", authFields); authFields();
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
    const feed = scheme === "secret_path" ? splitFeedAddress(url.value, secret.value) : { base_url: url.value, secret: secret.value };
    const value: ConnectionWrite = {
      name: name.value, base_url: feed.base_url, auth: { scheme,
        ...(scheme === "header" ? { header: header.value } : {}), ...(scheme === "basic" ? { username: username.value } : {}),
        ...(scheme === "oauth2_device" ? { token_url: tokenUrl.value, device_url: deviceUrl.value, client_id: clientId.value,
          scopes: scopes.value.split(/\s+/).filter(Boolean) } : {}) },
      methods: direct(), drafts: scheme === "secret_path" ? [] : drafts(), timeout_seconds: Number(timeout.value), max_response_bytes: Number(bytes.value),
      disabled: existing?.disabled ?? false,
      ...(existing?.idempotency_header === undefined ? {} : { idempotency_header: existing.idempotency_header }),
      ...(feed.secret ? { secret: feed.secret } : {}),
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
