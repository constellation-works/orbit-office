/**
 * The screen contract every `screens/<s>/index.ts` exports. Screens may
 * import this file, but not the router (ARCHITECTURE.md, "Web app").
 */
import type { ScreenId, Status } from "@orbit-office/contracts";

/** One screen. A planned screen is left out of the navigation. */
export interface Screen {
  id: ScreenId;
  /** Plain-words title shown in the navigation. */
  title: string;
  status: Status;
  /** Draws the screen into `root`. Absent while planned. */
  render?: (root: HTMLElement) => void | Promise<void>;
}
