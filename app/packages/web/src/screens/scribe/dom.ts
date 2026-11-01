/** DOM helpers shared by the Scribe screen's views. */

export const el = <K extends keyof HTMLElementTagNameMap>(tag: K, text = "", className = ""): HTMLElementTagNameMap[K] => {
  const node = document.createElement(tag);
  node.textContent = text;
  if (className) node.className = className;
  return node;
};

/** Copy `text`, saying so on the button; a refused clipboard asks the user to select it. */
export async function copy(text: string, button: HTMLButtonElement): Promise<void> {
  try {
    await navigator.clipboard.writeText(text);
    button.textContent = "Copied";
  } catch {
    button.textContent = "Select to copy";
  }
}

/** A copyable `claude --resume <session-id>`. */
export function resumeRow(resume: string): HTMLElement[] {
  const command = el("code", resume);
  const button = el("button", "Copy resume");
  button.type = "button";
  button.setAttribute("aria-label", `Copy ${resume}`);
  button.addEventListener("click", () => { void copy(resume, button); });
  return [command, button];
}
