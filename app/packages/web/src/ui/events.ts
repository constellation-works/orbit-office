/** A screen changed something the rail counts (a routine switched on or off), without importing the shell. */
export const RAIL_REFRESH_EVENT = "office-rail-refresh";

export function requestRailRefresh(): void {
  if (typeof window !== "undefined") window.dispatchEvent(new Event(RAIL_REFRESH_EVENT));
}
