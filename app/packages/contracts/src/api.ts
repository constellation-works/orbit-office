/** Whether a feature or screen is built yet. Planned ones are not exposed. */
export type Status = "built" | "planned";

/** Server features, one folder each under `server/src/features/`. */
export type FeatureName =
  | "today"
  | "calendar"
  | "sources"
  | "todos"
  | "approvals"
  | "routines"
  | "library"
  | "insights"
  | "review"
  | "selection"
  | "settings"
  | "agents";

/** Web screens, one folder each under `web/src/screens/`. */
export type ScreenId =
  | "today"
  | "agents"
  | "calendar"
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

/** How a gateway connection authenticates. `secret_path`: the slot holds the path and query. `oauth2_device`: the slot holds a refresh token from a device-code sign-in. */
export type ConnectionScheme = "bearer" | "basic" | "header" | "none" | "secret_path" | "oauth2_device";

/** Gateway connection fields; credential values never belong to a tool input. */
export interface ConnectionInput {
  name: string;
  base_url: string;
  auth: {
    scheme: ConnectionScheme; header?: string; username?: string;
    /** oauth2_device only. */
    token_url?: string; device_url?: string; client_id?: string; scopes?: string[];
  };
  methods: string[];
  drafts: string[];
  timeout_seconds: number;
  max_response_bytes: number;
  disabled: boolean;
  idempotency_header?: string;
}

/** Only the app's HTTP write accepts a secret, forwarded exclusively on stdin. */
export interface ConnectionWrite extends ConnectionInput { secret?: string }
export interface SettingsConnection extends ConnectionInput {
  slot_state: "set" | "unset" | "none";
  /** oauth2_device only: `expired` once the refresh token was refused, so the user signs in again. */
  auth_state?: "signed_in" | "signed_out" | "expired";
}
/** `POST /api/settings/connections/sign-in`: what the user needs to approve a device-code sign-in. */
export interface SignInStarted { connection: string; user_code: string; verification_uri: string; expires_in: number; interval: number }
/** `POST /api/settings/connections/sign-in/poll`: one check; the refresh token itself never reaches HTTP. */
export interface SignInPoll { connection: string; state: "pending" | "connected" | "denied" | "expired"; interval?: number }
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

/** The team shared library block of the Library settings (`shared`). */
export interface SharedLibraryBlock {
  /** `ssh://`, scp-style, HTTPS without user information, or an absolute repository path. */
  remote: string;
  branch: string;
  /** This member's lowercase kebab-case name, recorded on what they share. */
  member: string;
}

/** Why a sync left a document out, as a refusal code the screen says in words. */
export interface SharedSkipped {
  phase: "pull" | "push";
  /** The document's name in words, derived from its id or path; never the id itself. */
  name: string;
  reason: string;
}

/** One finished `office.library-sync`, in counts of documents. */
export interface SharedSync {
  synced_at: string;
  /** Documents this sync pushed to the shared library. */
  shared: number;
  /** Documents this sync stopped sharing. */
  removed: number;
  /** New or changed documents this sync received from colleagues. */
  received: number;
  /** Incoming updates filed for review. */
  incoming: number;
  /** Pages changed here and in the shared library. */
  conflicts: number;
  skipped: SharedSkipped[];
}

/** The latest sync the app ran: its result, or when and why it failed. */
export type SharedSyncOutcome =
  | { ok: true; sync: SharedSync }
  | { ok: false; at: string; reason: string };

/** `GET /api/settings/shared-library`. */
export interface SharedLibraryStatus {
  shared: SharedLibraryBlock | null;
  last_sync: SharedSyncOutcome | null;
}

/** Whether the hourly `office-shared-sync` routine is on, off, or can't be read. */
export type SharedRoutineState = "on" | "off" | "unavailable";

/** `POST /api/settings/shared-library`: saved with `office.config-set`. */
export interface SharedLibrarySaved {
  shared: SharedLibraryBlock;
  routine: SharedRoutineState;
}

/** `GET /api/settings/shared-library/last-sync`: the app's memory only, no Orbit call. */
export interface SharedLastSync {
  last_sync: SharedSyncOutcome | null;
}

/** `POST /api/library/share`: `office.share-set`, after the `also` documents, sources first. */
export interface ShareRequest {
  id: string;
  shareable: boolean;
  /** Documents to share first (**Share these too**). Only with `shareable: true`. */
  also?: string[];
}
export interface ShareResult { shareable: boolean }
/** A cited document that must be shared first. The id is for the request only. */
export interface ShareDocument { id: string; title: string }

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
  /** From the shared library: a colleague's update, or a page changed on both sides. */
  shared?: "incoming" | "conflict";
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
  /** The scheduled start (`not_before`, RFC 3339), or null when it has none. */
  scheduled: string | null;
  /** True while nothing has started it, so **Schedule…** can set or clear the time. */
  schedulable: boolean;
}

/** One Orbit task an agent is carrying, on `GET /api/agents`. */
export interface AgentWorkTask {
  id: string;
  title: string;
  /** The followed workspace selector, or `office` for the office workspace. */
  workspace: string;
  type: string;
  priority: string;
  crew: string | null;
  /** RFC 3339 time of the last update. */
  updated_at: string;
}

/** `GET /api/agents`: agent-carried Orbit tasks across the followed workspaces, newest first. */
export interface AgentWork {
  working_now: AgentWorkTask[];
  in_review: AgentWorkTask[];
  queued: AgentWorkTask[];
  done_today: AgentWorkTask[];
  /** Every followed workspace, office first, then the configured selectors in order. */
  workspaces: string[];
  /** Followed workspaces that could not be read; their tasks are absent. */
  unavailable: string[];
  /** RFC 3339 time this snapshot was assembled. */
  updated_at: string;
}

/** `GET /api/todos`: the You and Agents columns. */
export interface TodoList {
  you: TodoItem[];
  agents: TodoItem[];
  /** True when Orbit had more matching to-dos than this list includes. */
  more: boolean;
}

/**
 * `POST /api/todos/schedule`: set an open to-do's scheduled start, or clear
 * it with null. Answers {@link TodoScheduled}.
 */
export interface TodoScheduleRequest {
  id: string;
  /** An RFC 3339 instant with an offset, or null to clear the schedule. */
  not_before: string | null;
}
export interface TodoScheduled { ok: true; scheduled: string | null }

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
    /** Accepting also shares the page with the team. */
    shareable: boolean;
    /** A page changed here and in the shared library: accept takes a `choice`. */
    conflict: boolean;
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

/** How a shared-library conflict is settled: their version, or the user's own. */
export type ConflictChoice = "theirs" | "mine";

/** `POST /api/approvals/accept`: `office.proposal-accept`, called once. */
export interface AcceptRequest { key: string; sha256: string; choice?: ConflictChoice }

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
  /** True for a private source: the tool withheld its text; metadata only. */
  content_withheld: boolean;
  sensitivity: "normal" | "private";
  /** Whether the user shares this source with the team. */
  shareable: boolean;
  /** The member who shared it, or null for the user's own source. */
  shared_by: string | null;
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

/** Library setup status and the result of an explicit setup action. */
export interface SetupStatus { state: "ready" | "missing" | "plugin_disabled" | "workspace_unregistered" | "unavailable"; reason?: string; fix?: string }
export interface SetupResult { created: string[]; already_initialized: boolean }

/** One attendee on a meeting: name and response only, never an address. */
export interface CalendarAttendee {
  name: string;
  /** As the calendar reports it, for example `accepted` or `needsAction`. */
  response: string;
  /** The Library person page this attendee matched, or null. */
  person: { id: string; title: string } | null;
}

/**
 * One meeting instance from the read-only calendar snapshot
 * (`office.calendar-list`). `key` identifies the instance for capture; the
 * screen never renders it.
 */
export interface CalendarMeeting {
  /** Authored only in this workspace. */
  local?: boolean;
  key: string;
  title: string;
  /** RFC 3339 instants. All-day events start and end at UTC midnight. */
  start: string;
  end: string;
  all_day: boolean;
  status: "confirmed" | "tentative" | "cancelled";
  /** Empty when the invite names no place. */
  location: string;
  /** True when the invite has a join link; the link itself is never stored. */
  online: boolean;
  organizer: string;
  attendees: CalendarAttendee[];
  /** Matched person pages no attendee name accounts for. */
  people: Array<{ id: string; title: string }>;
  /** Untrusted invite text; the screen shows it only as quoted text. */
  agenda: string;
  private: boolean;
  /** The prep brief's artifact id, or null while none is written. */
  prep: string | null;
  /** Notes or a transcript captured from this meeting (the first in Library order), or null. */
  capture: { source: string; summarized: boolean } | null;
}

/** A routine's schedule as the Routines screen reads it, for the calendar's margin marks. */
export interface CalendarRoutine {
  kind: "auto_task" | "routine";
  name: string;
  schedule: Record<string, unknown>;
  enabled: boolean;
  /** When it last fired, for interval schedules; null when it never has. */
  last_fired_at: string | null;
}

/**
 * Where a scheduled task stands, read from its Orbit status: `not_approved`
 * is still proposed (or parked), `scheduled` is waiting for its time.
 */
export type CalendarTaskState = "not_approved" | "scheduled" | "running" | "review" | "done";

/** A to-do with a scheduled start in the range, from `orbit.task.list`'s `not_before` range. */
export interface CalendarTask {
  /** Used only to reschedule or clear it. The screen must not render it. */
  id: string;
  title: string;
  /** The scheduled start, RFC 3339. */
  not_before: string;
  /** `you` is never dispatched; `agent` is shipped by the dispatch pass once the time comes. */
  owner: "you" | "agent";
  state: CalendarTaskState;
  /** When it last started, from its history, or null while it has not. */
  started_at: string | null;
}

/** `GET /api/calendar?from&until`: meetings in the range and the routines that run. */
export interface CalendarView {
  /** Source choices persist without deleting hidden events. */
  sources?: CalendarSources;
  from: string;
  until: string;
  /** Null until the calendar has synced once; a synced, empty week reads a time. */
  synced_at: string | null;
  /** True when the latest sync failed; the meetings are the last good snapshot. */
  sync_failed: boolean;
  /** Minutes before a meeting that its prep brief is written. */
  prep_lead_minutes: number;
  meetings: CalendarMeeting[];
  routines: CalendarRoutine[];
  /** Scheduled routines this connection may not read; auto-tasks are still listed. */
  routines_unavailable: boolean;
  /** To-dos whose scheduled start falls in the range, by start. */
  tasks: CalendarTask[];
  /** True when the scheduled to-dos could not all be read; the meetings still show. */
  scheduled_unavailable: boolean;
}

/** `POST /api/calendar/sync`: how many meetings the snapshot now holds. */
export interface CalendarSynced { events: number; skipped: number; synced_at: string }

/** `POST /api/calendar/capture` with JSON: pasted notes for one meeting. */
export interface MeetingNotes { event_key: string; text: string }

/** `GET /api/calendar/meeting?source=<id>`: what a captured meeting produced. */
export interface MeetingOutcome {
  /** `summary.md`, or null until the meeting inbox has summarized it. */
  summary: string | null;
  /** To-dos filed from this meeting, in plain words. */
  todos: Array<{ title: string; owner: "you" | "agent" | null; done: boolean }>;
  /** True when Orbit had more to-dos than listed. */
  more: boolean;
}

/** Persistent calendar sources, independently selectable. */
export interface CalendarSources { local: boolean; external: boolean }
/** Workspace-authored event, with computer-local times converted to RFC 3339. */
export interface LocalCalendarEvent {
  title: string; start: string; end: string; all_day?: boolean;
  location?: string; agenda?: string; attendees?: string[]; private?: boolean;
}
export type LocalCalendarChange =
  | { action: "create"; event: LocalCalendarEvent }
  | { action: "update"; key: string; event: LocalCalendarEvent }
  | { action: "delete"; key: string }
  | ({ action: "sources" } & CalendarSources);
