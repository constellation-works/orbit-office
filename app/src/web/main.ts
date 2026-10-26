/**
 * Single-page UI entry (scaffold).
 *
 * Screens (plain words only): Today, Source view, To-dos, Routines,
 * Library, Insights, Review. There is no Chat screen: conversation
 * happens in the user's agent CLI. Each item has one primary action and
 * a provenance link on every agent output. Live status polls the app
 * server.
 */
export const SCREENS = [
  "today",
  "source",
  "todos",
  "routines",
  "library",
  "insights",
  "review",
] as const;

export type Screen = (typeof SCREENS)[number];

console.log("orbit-office app (scaffold):", SCREENS.join(", "));
