/** A container for one item and its details. */
export function card(title: string, content: HTMLElement, action?: HTMLButtonElement): HTMLElement {
  const article = document.createElement("article");
  article.className = "card";
  const heading = document.createElement("h3");
  heading.textContent = title;
  article.append(heading, content);
  if (action !== undefined) article.append(action);
  return article;
}
