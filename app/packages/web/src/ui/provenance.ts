/** A link to the source of an item. */
export function provenanceLink(label: string, href: string): HTMLAnchorElement {
  const link = document.createElement("a");
  link.className = "provenance";
  link.textContent = label;
  link.href = href;
  return link;
}
