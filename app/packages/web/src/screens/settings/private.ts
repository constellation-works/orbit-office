/** Private route: where private material goes, in plain words. */
import type { PrivateRouteCheckLine, PrivateRouteStatus, PrivateRouteWrite, RouteCapability } from "@orbit-office/contracts";
import { ApiError, savePrivateRoute } from "../../api/client.js";
import { MUSE_SPARK_OPEN_ATTRIBUTION } from "../../ui/index.js";

/** The sentence the private-route section always shows. */
export const PRIVATE_ROUTE_WORDING =
  "Private means not sent to Meta's hosted API, sent only to the endpoint you chose";

const REFUSAL: Record<RouteCapability, string> = {
  text: "this server didn't answer a text request, so nothing private is sent to it",
  image: "this server doesn't take images, so private frames and receipts can't be read",
  audio: "this server doesn't take audio, so private videos can't be transcribed",
};

const ACCEPTANCE: Record<RouteCapability, string> = {
  text: "This server answers text, so private writing can be drafted there.",
  image: "This server takes images, so private frames and receipts can be read.",
  audio: "This server takes audio, so private videos can be transcribed.",
};

const KIND_LABELS: Record<PrivateRouteWrite["kind"], string> = {
  company: "Your company's server",
  provider: "A hosted provider",
  own_hardware: "Your own hardware",
};

const DESTINATIONS: Record<PrivateRouteWrite["kind"], string> = {
  company: "Your employer's infrastructure. The strongest option for work material.",
  provider: "That provider, under its terms. Not Meta's hosted API, but not your company either.",
  own_hardware: "Machines you own.",
};

const ORDER: readonly RouteCapability[] = ["text", "image", "audio"];

export interface PrivateRouteMount {
  data: PrivateRouteStatus;
  busy(): boolean;
  setBusy(busy: boolean): void;
  alive(): boolean;
  onConfigured(configured: boolean): void;
}

/** One probe as a sentence. A refused probe prefers the check's own note. */
export function capabilityLine(check: Pick<PrivateRouteCheckLine, "capability" | "accepted" | "note">): string {
  if (check.accepted) return ACCEPTANCE[check.capability];
  if (check.note !== null && check.note !== "") return check.note;
  return REFUSAL[check.capability];
}

export function capabilitySentences(checks: readonly PrivateRouteCheckLine[]): string[] {
  return checks.map((check) => capabilityLine(check));
}

/** What a stored capability list means. Null means the endpoint is not checked yet. */
export function sentencesForCapabilities(capabilities: readonly RouteCapability[] | null): string[] {
  if (capabilities === null) return ["This endpoint has not been checked yet."];
  return ORDER.map((capability) => capabilities.includes(capability) ? ACCEPTANCE[capability] : REFUSAL[capability]);
}

export function mountPrivateRoute(content: HTMLElement, mount: PrivateRouteMount): void {
  const route = mount.data.route;
  const form = document.createElement("form");
  form.className = "settings-form";
  form.setAttribute("aria-label", "Private route");
  const fields = document.createElement("div");
  fields.className = "settings-fields";

  const kindWrap = text("label", "Endpoint kind");
  kindWrap.className = "settings-field";
  const kind = document.createElement("select");
  kind.name = "kind";
  kind.required = true;
  kind.setAttribute("aria-label", "Endpoint kind");
  const placeholder = text("option", "Choose where it runs");
  placeholder.value = "";
  placeholder.disabled = true;
  kind.append(placeholder);
  for (const value of Object.keys(KIND_LABELS) as PrivateRouteWrite["kind"][]) {
    const option = text("option", KIND_LABELS[value]);
    option.value = value;
    kind.append(option);
  }
  kind.value = route?.kind ?? "";
  kindWrap.append(kind);
  fields.append(kindWrap);

  const endpoint = field(fields, "Endpoint URL", "endpoint", route?.endpoint ?? "", "url");
  endpoint.placeholder = "https://muse.example/v1";
  endpoint.classList.add("settings-url");
  const model = field(fields, "Model", "model", route?.model ?? "");
  model.placeholder = "the model name that server uses";
  const label = field(fields, "Label", "label", route?.label ?? "");
  label.maxLength = 80;
  label.placeholder = "Acme's server";

  const token = field(fields, "Endpoint token", "token", "", "password");
  token.required = false;
  token.autocomplete = "new-password";
  token.setAttribute("aria-label", "Endpoint token");
  const savedTokenHint = "A token is saved. Leave this empty to keep it. Stored with Orbit and never shown again.";
  const hint = text("span", mount.data.token === "set"
    ? savedTokenHint
    : "No token is saved. Stored with Orbit and never shown again.");
  hint.className = "settings-hint";
  token.parentElement?.append(hint);

  const destination = text("p", "");
  destination.className = "settings-destination";
  const preview = text("p", "");
  preview.className = "settings-preview";
  function describe(): void {
    const selected = kind.value;
    destination.textContent = selected === "company" || selected === "provider" || selected === "own_hardware"
      ? DESTINATIONS[selected] : "Choose where private material goes.";
    const name = label.value.trim();
    preview.textContent = name === "" ? "Private: (add a label)" : `Private: ${name}`;
  }
  kind.addEventListener("change", describe);
  label.addEventListener("input", describe);
  describe();

  const notes = document.createElement("ul");
  notes.className = "capability-notes";
  const attribution = text("p", MUSE_SPARK_OPEN_ATTRIBUTION);
  attribution.className = "settings-attribution";
  function showNotes(sentences: readonly string[]): void {
    notes.replaceChildren(...sentences.map((sentence) => text("li", sentence)));
  }
  function showAttribution(configured: boolean): void {
    attribution.hidden = !configured;
  }
  if (route === null) {
    showNotes([]);
    notes.hidden = true;
    showAttribution(false);
  } else {
    showNotes(sentencesForCapabilities(route.capabilities));
    showAttribution(true);
  }

  const feedback = text("p", route === null
    ? "Nothing private is configured yet. Private material is refused until you save an endpoint."
    : "");
  feedback.className = "settings-feedback";
  feedback.setAttribute("role", "status");

  const actions = document.createElement("div");
  actions.className = "settings-actions";
  const submit = text("button", "Save private route");
  submit.type = "submit";
  actions.append(submit);

  const honest = text("p", PRIVATE_ROUTE_WORDING);
  honest.className = "settings-honest";
  form.append(fields, destination, preview, honest, notes, attribution, feedback, actions);

  const heading = document.createElement("div");
  heading.className = "settings-heading";
  heading.append(text("h2", "Private route"));
  content.replaceChildren(
    heading,
    text("p", "Choose the one endpoint private material is sent to."),
    form,
  );

  form.addEventListener("submit", (event) => {
    event.preventDefault();
    if (mount.busy()) return;
    const selected = kind.value;
    if (selected !== "company" && selected !== "provider" && selected !== "own_hardware") return;
    const entered = token.value;
    token.value = "";
    const body: PrivateRouteWrite = {
      endpoint: endpoint.value,
      model: model.value,
      kind: selected,
      label: label.value,
      ...(entered === "" ? {} : { token: entered }),
    };
    mount.setBusy(true);
    submit.disabled = true;
    void (async () => {
      try {
        const saved = await savePrivateRoute(body);
        if (!mount.alive()) return;
        mount.onConfigured(true);
        if (entered !== "") hint.textContent = savedTokenHint;
        notes.hidden = false;
        showNotes(capabilitySentences(saved.check.checks));
        showAttribution(true);
        feedback.setAttribute("role", "status");
        feedback.textContent = saved.check.routable
          ? "Private route saved."
          : "Private route saved. Nothing private is sent until a text request succeeds.";
      } catch (error) {
        if (!mount.alive()) return;
        if (error instanceof ApiError && error.routeSaved) {
          mount.onConfigured(true);
          showAttribution(true);
          if (entered !== "" && error.code !== "secret_not_saved" && error.code !== "secret_timeout") {
            hint.textContent = savedTokenHint;
          }
        }
        feedback.setAttribute("role", "alert");
        feedback.textContent = saveMessage(error);
      } finally {
        mount.setBusy(false);
        submit.disabled = false;
      }
    })();
  });
}

function saveMessage(error: unknown): string {
  if (error instanceof ApiError) {
    if (error.routeSaved && (error.code === "secret_not_saved" || error.code === "secret_timeout")) {
      return "The private route was saved, but its token could not be saved. Enter the token again.";
    }
    if (error.routeSaved && error.code === "private_route_check_failed") {
      return "The private route was saved, but its capabilities could not be checked. Try saving again.";
    }
    if (error.code === "invalid_input") return "Check the endpoint URL, model, kind and label.";
    if (error.code === "settings_busy") return "Another settings change is being saved. Please try again in a moment.";
    if (error.code === "private_route_unavailable") return "Could not reach the private route. Check that Orbit is available, then try again.";
  }
  return "Could not confirm the save. Refresh the private route before trying again.";
}

function text<K extends keyof HTMLElementTagNameMap>(tag: K, content: string): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  node.textContent = content;
  return node;
}

function field(fields: HTMLElement, label: string, name: string, value: string, type = "text"): HTMLInputElement {
  const wrap = text("label", label);
  wrap.className = "settings-field";
  if (name === "endpoint" || name === "model" || name === "label") wrap.classList.add("settings-wide");
  const input = document.createElement("input");
  input.name = name;
  input.type = type;
  input.value = value;
  input.required = type !== "password";
  wrap.append(input);
  fields.append(wrap);
  return input;
}
