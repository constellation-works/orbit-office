/** A short, human-readable label. */
export function pill(label: string): HTMLElement {
  const element = document.createElement("span");
  element.className = "pill";
  element.textContent = label;
  return element;
}
