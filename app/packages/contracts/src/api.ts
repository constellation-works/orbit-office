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
  idempotency_header?: string;
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

/** Where the one private endpoint runs. */
export type EndpointKind = "company" | "provider" | "own_hardware";

/** A modality `office.private-route-check` probes. */
export type RouteCapability = "text" | "image" | "audio";

/**
 * The public private-route block from `office.status`. The token is never
 * a field here; presence is `PrivateRouteStatus.token`.
 */
export interface PrivateRouteBlock {
  endpoint: string;
  model: string;
  kind: EndpointKind;
  label: string;
  /** Null until a check has recorded what the endpoint accepted. */
  capabilities: RouteCapability[] | null;
  /** RFC 3339 time of the last check, or null until then. */
  verified_at: string | null;
}

/** `GET /api/settings/private-route`. */
export interface PrivateRouteStatus {
  route: PrivateRouteBlock | null;
  token: "set" | "unset";
}

/** `POST /api/settings/private-route`. Omit `token` to leave the saved one. */
export interface PrivateRouteWrite {
  endpoint: string;
  model: string;
  kind: EndpointKind;
  label: string;
  token?: string;
}

/** One probe, in plain words. Tool error codes are not part of this contract. */
export interface PrivateRouteCheckLine {
  capability: RouteCapability;
  accepted: boolean;
  elapsed_ms: number | null;
  note: string | null;
}

/** The check the save just ran. Paths and probe error codes are omitted. */
export interface PrivateRouteCheck {
  label: string;
  kind: EndpointKind;
  model: string;
  routable: boolean;
  capabilities: RouteCapability[];
  checks: PrivateRouteCheckLine[];
}

/** `POST /api/settings/private-route` success. No token value. */
export interface PrivateRouteSaved {
  route: Pick<PrivateRouteBlock, "endpoint" | "model" | "kind" | "label">;
  check: PrivateRouteCheck;
}

/** Work shown on Today. Keys identify items for the UI, never display labels. */
export interface TodayItem {
  key: string;
  title: string;
  background: string;
}

/**
 * What the user is asked to do. `proposal`: accept or dismiss a suggested
 * change. `approval`: approve or send back an agent's finished work.
 * `script_output`: keep or dismiss files a script produced. `gateway_draft`:
 * approve sending, or dismiss, a request to another system.
 */
export type TodayNeedKind = "proposal" | "approval" | "todo" | "script_output" | "gateway_draft";

/**
 * Proposals the app shows together on one card: the onboarding set
 * ("Set up your Library") or one subject of a Library bootstrap phase.
 * `key` identifies the group for the UI; `title` is plain words.
 */
export interface TodayNeedGroup {
  key: string;
  title: string;
}

export interface TodayNeed extends TodayItem {
  kind: TodayNeedKind;
  /** The card this proposal belongs to, or null when it stands alone. */
  group: TodayNeedGroup | null;
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

/** Generated review bodies returned by GET /api/review. */
export interface ReviewItem {
  title: string;
  date: string;
  body: string | null;
  truncated: boolean;
}
export interface ReviewList {
  reviews: ReviewItem[];
  latest_100: boolean;
}

/**
 * `GET /api/approvals/detail?key=<key>`: what the user reviews before acting
 * on a Needs-you item. Plain words and file names only; no ids or paths.
 */
export type ApprovalDetail =
  | {
    kind: "proposal";
    key: string;
    /** The page or change the proposal makes, as reviewable text. */
    preview: string;
    /** SHA-256 of the exact proposal the preview shows; accept binds to it. */
    sha256: string;
  }
  | {
    kind: "approval" | "script_output";
    key: string;
    /** Output file names, without folders. */
    files: string[];
  }
  | {
    kind: "gateway_draft";
    key: string;
    /** Name of the saved connection the request goes to. */
    connection: string;
    method: string;
    url: string;
    /** The request body as text, or null when it has none. */
    body: string | null;
    /** The digest Approve sends with; the send refuses a changed draft. */
    sha256: string;
  };

/** `POST /api/approvals/approve`: promote the output, then close the task. */
export interface ApproveRequest { key: string }
export interface ApproveResult { approved: true; closed: boolean }

/** `POST /api/approvals/send-back`: return reviewed work with a comment. */
export interface SendBackRequest { key: string; comment: string }

/** `POST /api/approvals/dismiss`: reject a proposal, script output or draft. */
export interface DismissRequest { key: string; reason?: string }

/** `POST /api/approvals/accept`: `office.proposal-accept`, called once. */
export interface AcceptRequest { key: string; sha256: string }

/** `POST /api/approvals/accept-all`: accept each proposal in turn. */
export interface AcceptAllRequest { proposals: AcceptRequest[] }
export interface AcceptAllResult {
  /** One entry per requested proposal, in request order. */
  results: Array<{ key: string; accepted: true } | { key: string; accepted: false; error: string }>;
}

/** `POST /api/approvals/send`: `gateway.http-send` with the shown digest. */
export interface GatewaySendRequest { key: string; sha256: string }
export interface GatewaySendResult {
  /** `outcome_unknown`: the request went out and no reply came back. Never a failure. */
  outcome: "sent" | "outcome_unknown";
  /** HTTP status the other system answered, or null. */
  status: number | null;
  task_closed: boolean;
}

/** Every approval mutation that has no other result. */
export interface ApprovalOk { ok: true }

/** Source display data. Original bytes are fetched separately through the server. */
export interface SourceView {
  id: string;
  kind: string;
  title: string;
  text: string;
  sensitivity: "normal" | "private";
  media_type: string | null;
  route_labels: string[];
  cited_by: Array<{ id: string; title: string; kind: string }>;
  task_citers_available: boolean;
  cited_by_truncated: boolean;
  chapters: Array<{ number: number; title: string; label: "core" | "tangent"; start: number; anchor: string }>;
  frames: Array<{ time: number; anchor: string; file: string; text: string }>;
  gotchas: Array<{ text: string; cites: string[] }>;
}
export interface CapturedSource { id: string; kind: string; title: string }
export type BootstrapPhase = "docs" | "kt" | "team" | "process";
export interface CapturePaste { title: string; text: string; private: boolean }
export interface SourcePlan {
  digest: string;
  source?: string;
  phase?: BootstrapPhase;
  private: boolean;
  minutes: number;
  frames: number;
  calls: number;
  sources: number;
  estimate: { amount: number; currency: string } | null;
}
export interface SourceCitation { title: string; text: string }

/** A source span shared with the agent. */
export interface SelectionInput {
  object: string;
  anchor: string;
  quote: string;
  view: "source";
  sensitivity: "normal" | "private";
}
export interface SelectionCount { count: number }
