/**
 * Single-page UI entry (scaffold).
 *
 * Screens (plain words only): Today, Chat, Source view, To-dos, Routines,
 * Library, Insights, Review. Each item has one primary action and a
 * provenance link on every agent output. Live status polls the app server.
 */
export const SCREENS = [
  "today",
  "chat",
  "todos",
  "routines",
  "library",
  "insights",
  "review",
] as const;

export type Screen = (typeof SCREENS)[number];

console.log("orbit-office app (scaffold):", SCREENS.join(", "));
