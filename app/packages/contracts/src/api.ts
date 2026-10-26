/** Whether a feature or screen is built yet. Planned ones are not exposed. */
export type Status = "built" | "planned";

/** Server features, one folder each under `server/src/features/`. */
export type FeatureName =
  | "today"
  | "sources"
  | "todos"
  | "approvals"
  | "routines"
  | "library"
  | "insights"
  | "review"
  | "selection"
  | "settings";

/** Web screens, one folder each under `web/src/screens/`. */
export type ScreenId =
  | "today"
  | "source"
  | "todos"
  | "routines"
  | "library"
  | "insights"
  | "review";

/** `GET /healthz`: the app is up, and which features it serves. */
export interface Health {
  ok: true;
  app: "orbit-office";
  features: FeatureName[];
}
