/** One primary action for an item. */
export function primaryAction(kind: "approve" | "edit" | "dismiss", onClick: () => void): HTMLButtonElement {
  const button = document.createElement("button");
  button.type = "button";
  button.className = "primary-action";
  button.textContent = { approve: "Approve", edit: "Edit", dismiss: "Dismiss" }[kind];
  button.addEventListener("click", onClick);
  return button;
}
