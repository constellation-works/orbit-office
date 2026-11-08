/** Shared DOM node and button factories for every screen. */

/** One element with its text and, when given, its class. */
export function element<K extends keyof HTMLElementTagNameMap>(tag: K, text = "", className?: string): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  node.textContent = text;
  if (className !== undefined) node.className = className;
  return node;
}

/** One button with its label, click handler and, when given, its class. */
export function button(label: string, onClick?: () => void, className?: string): HTMLButtonElement {
  const node = element("button", label, className);
  node.type = "button";
  if (onClick !== undefined) node.addEventListener("click", onClick);
  return node;
}
