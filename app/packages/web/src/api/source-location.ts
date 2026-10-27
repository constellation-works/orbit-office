/** Ordinary localhost links: source identity in the query, citation in the fragment. */
export function sourceHref(id: string, anchor = "#source"): string {
  return `?source=${encodeURIComponent(id)}${anchor}`;
}
export function screenHash(search: string, hash: string): string {
  if (new URLSearchParams(search).has("source") && (hash === "" || /^#(?:t=|L\d|frame=|chapter=)/.test(hash))) return "#source";
  return hash || "#today";
}
