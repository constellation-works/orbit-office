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

/** The agent CLI connection the settings feature reports. */
export interface AgentCliStatus {
  /** Display name of the agent CLI, e.g. "Muse Code". */
  cli: string;
  /** Whether the install script has registered Orbit MCP with it. */
  connected: boolean;
}

/** `GET /api/settings`: the settings the app reports. */
export interface SettingsStatus {
  agentCli: AgentCliStatus;
}

/** Work shown on Today. Keys identify items for the UI, never display labels. */
export interface TodayItem {
  key: string;
  title: string;
  background: string;
}

export interface TodayNeed extends TodayItem {
  kind: "proposal" | "approval" | "todo";
}

export interface TodayResult extends TodayItem {
  /** The recorded outcome, or null when no outcome was recorded. */
  result: string | null;
}

/** `GET /api/today`: a live snapshot in the app server's local calendar day. */
export interface Today {
  needs_you: TodayNeed[];
  agents_working_on: TodayItem[];
  done_today: TodayResult[];
  /** At least one Orbit listing reached its limit. */
  truncated: boolean;
  /** Completed items whose completion history could not establish a date. */
  completion_dates_unavailable: number;
}
