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
  | "review"
  | "settings";

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

/** Gateway connection fields; credential values never belong to a tool input. */
export interface ConnectionInput {
  name: string;
  base_url: string;
  auth: { scheme: "bearer" | "basic" | "header" | "none"; header?: string; username?: string };
  methods: string[];
  drafts: string[];
  timeout_seconds: number;
  max_response_bytes: number;
  disabled: boolean;
}

/** Only the app's HTTP write accepts a secret, forwarded exclusively on stdin. */
export interface ConnectionWrite extends ConnectionInput { secret?: string }
export interface SettingsConnection extends ConnectionInput { slot_state: "set" | "unset" | "none" }
export interface SettingsConnections { connections: SettingsConnection[]; free_slots: number }
export interface ConnectionSaved { connection: SettingsConnection; created: boolean }

/** Redacted gateway audit metadata, never response bodies. */
export interface ApiCall {
  time: string;
  caller: { agent?: string; model?: string };
  tool: string;
  connection: string | null;
  method: string | null;
  path: string | null;
  status: number | null;
  bytes: number;
  duration_ms: number;
  outcome: string;
}
export interface SettingsApiCalls { records: ApiCall[]; skipped: number }

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

/**
 * One open to-do on `GET /api/todos`.
 * Labels are plain words: no ids, crews, runs, paths, or raw statuses.
 */
export interface TodoItem {
  /** Used only to close the to-do. The screen must not render it. */
  id: string;
  title: string;
  /** Source name, or null when the to-do has no source. */
  source: string | null;
  /** Names of knowledge attached to the to-do. */
  knowledge: string[];
  /** True only for the user's own to-dos, which offer a close action. */
  closable: boolean;
}

/** `GET /api/todos`: the You and Agents columns. */
export interface TodoList {
  you: TodoItem[];
  agents: TodoItem[];
  /** True when Orbit had more matching to-dos than this list includes. */
  more: boolean;
}
