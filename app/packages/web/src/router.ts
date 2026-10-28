/**
 * Every screen, in one list, in navigation order.
 *
 * Integrator-owned (ARCHITECTURE.md): the scaffold listed every planned
 * screen, so a task that builds one edits only its own folder.
 */
import type { Screen } from "./screen.js";
import { screen as calendar } from "./screens/calendar/index.js";
import { screen as insights } from "./screens/insights/index.js";
import { screen as library } from "./screens/library/index.js";
import { screen as review } from "./screens/review/index.js";
import { screen as routines } from "./screens/routines/index.js";
import { screen as settings } from "./screens/settings/index.js";
import { screen as source } from "./screens/source/index.js";
import { screen as today } from "./screens/today/index.js";
import { screen as todos } from "./screens/todos/index.js";

/** Every screen, built or planned. */
export const SCREENS: readonly Screen[] = [
  today,
  calendar,
  todos,
  routines,
  library,
  insights,
  review,
  source,
  settings,
];

/** The screens shown in the navigation: built ones only. */
export function navigableScreens(): readonly Screen[] {
  return SCREENS.filter((screen) => screen.status === "built");
}

/** The screen a location hash (`#today`) names, if it is built. */
export function screenForHash(hash: string): Screen | undefined {
  const id = hash.replace(/^#/, "");
  return navigableScreens().find((screen) => screen.id === id);
}
