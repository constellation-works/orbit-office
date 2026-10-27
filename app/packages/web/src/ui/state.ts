function state(kind: string, title: string, message: string): HTMLElement {
  const section = document.createElement("section");
  section.className = `state state--${kind}`;
  if (kind === "error") section.setAttribute("role", "alert");
  const heading = document.createElement("h2");
  heading.textContent = title;
  const detail = document.createElement("p");
  detail.textContent = message;
  section.append(heading, detail);
  return section;
}

export function emptyState(title: string, message: string): HTMLElement {
  return state("empty", title, message);
}

export function errorState(title: string, message: string): HTMLElement {
  return state("error", title, message);
}
