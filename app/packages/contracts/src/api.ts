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
  | "agents"
  | "scribe";

/** Web screens, one folder each under `web/src/screens/`. */
export type ScreenId =
  | "today"
  | "agents"
  | "calendar"
  | "scribe"
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

/**
 * Where the saved private-route token may be sent. A token is bound to the
 * endpoint origin (scheme, host and port) it was saved for and is sent
 * nowhere else. `this_endpoint`: saved for the configured endpoint's origin.
 * `other_endpoint`: saved for another origin, so the configured endpoint is
 * sent no token. `unbound`: stored outside Settings without an origin, so it
 * is never sent. `unset`: no token.
 */
export type PrivateTokenBinding = "unset" | "this_endpoint" | "other_endpoint" | "unbound";

/** `GET /api/settings/private-route`. */
export interface PrivateRouteStatus {
  route: PrivateRouteBlock | null;
  token: "set" | "unset";
  token_binding: PrivateTokenBinding;
}

/**
 * `POST /api/settings/private-route`. `token` is saved for this endpoint's
 * origin only. Omit it to keep the saved token, which is still sent only to
 * the origin it was saved for: a different origin gets no token. Set
 * `clear_token` (never with `token`) to remove the saved token.
 */
export interface PrivateRouteWrite {
  endpoint: string;
  model: string;
  kind: EndpointKind;
  label: string;
  token?: string;
  clear_token?: true;
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
  /** A `todo`'s due date: its `not_before`, RFC 3339. Absent when it has none. */
  due?: string;
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
  /**
   * The scheduled start (`not_before`, RFC 3339), or null when it has none.
   * The user's own to-dos are never dispatched, so the screen reads it as their due date.
   */
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
  /** The task's pull request from its delivery record, read for in-review and done rows only. */
  pr?: TaskPr | null;
  /** The meeting or other source the task was filed from (`source:<id>` tag), or null. */
  source?: string | null;
}

/** A pull request read from a task's Orbit delivery record. `url` only for a GitHub `owner/name` repository. */
export interface TaskPr {
  number: number;
  url: string | null;
  /** True once the delivery landed (merged). */
  merged: boolean;
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

/**
 * `POST /api/todos/create`: create one of the user's own to-dos.
 * Answers {@link TodoCreated}; the new to-do then appears in `GET /api/todos`.
 */
export interface TodoCreateRequest {
  title: string;
}
export interface TodoCreated { ok: true; id: string; title: string }

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
/** `office.digest` with a source: what a meeting decided, shipped and still needs from you. */
export interface SourceDigest {
  source: string;
  title: string;
  decided: Array<{ text: string; cites: string[] }>;
  shipped: Array<{
    task: string; title: string; workspace: string | null; delivery_status: string | null;
    pr: { number: number; url: string | null } | null;
    merge: { commit: string; url: string | null } | null;
    cites: string[];
  }>;
  needs_you: Array<{ kind: "to_do" | "contradiction" | "decision"; text: string; task: string; workspace: string | null; cites: string[] }>;
  report: string;
  /** Extra workspaces whose tasks could not be read. */
  unavailable: string[];
  truncated: boolean;
}

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
  /** The scheduled start, RFC 3339; for an `owner: "you"` to-do, its due date. */
  not_before: string;
  /** `you` is never dispatched, so its time is a due date; `agent` is shipped by the dispatch pass once the time comes. */
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
  /**
   * To-dos whose scheduled start falls in the range, by start. While the
   * range holds now, the user's open to-dos already due before it lead the
   * list, so an overdue one stays in view.
   */
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

/** One Tags entry of a v2 SCR, cited `<SCR id>#T<n>` (specs/scribe.md, "The SCR file"). */
export interface ScrTagEntry {
  /** `T<n>`. */
  id: string;
  /** `<SCR id>#T<n>`. */
  citation: string;
  /** The Tags subsection: `Preferences`, `Decisions`, a custom tag name, or `Requests`. */
  group: string;
  /** The cited message id. */
  msg: string;
  /** The entry's local time as written on the line. */
  time: string;
  /** `user` or `agent`. */
  by: string;
  /** `global` or `project:<name>`. */
  scope: string;
  /** The marker line, verbatim. */
  quote: string;
  /** Trailing fields (`ledger`, `proposal`, `matter`, `task`, …). */
  fields: Record<string, string>;
}

/**
 * One v2 SCR as a feed card, from `office.session-brief` with `view: list`
 * (specs/scribe.md, "Feed"). A private SCR is listed by reference only:
 * `withheld` is true, the summary is null and the sections, Tags entries
 * and tag counts are empty.
 */
export interface ScrCard {
  /** The SCR id (`SCR-<yyyymmdd>-<hhmm>-<session6>`). */
  id: string;
  /** Library-relative SCR file path. */
  path: string;
  title: string | null;
  title_by: string | null;
  /** `global` or `project:<name>`. */
  scope: string;
  project: string | null;
  /** The session this SCR records. */
  session: string;
  started: string;
  updated: string;
  /** The scope's earlier latest SCR, reached only through this link; null at the chain's end. */
  previous: string | null;
  scribed_through: string | null;
  /** The Summary section, keeping its `(scribe)` marker; null when withheld. */
  summary: string | null;
  goals: string[];
  /** Outcome and Blockers items. */
  outcome: string[];
  next: string[];
  /** Tags entries in this SCR, in all. */
  tag_entries: number;
  /** Tags entries per subsection heading (the counts a card shows). */
  tag_counts: Record<string, number>;
  tags: ScrTagEntry[];
  /** A private session: content withheld. */
  withheld: boolean;
}

/** `GET /api/scribe/feed`: exactly one card per project (its latest SCR) plus one global card. */
export interface ScrFeed {
  cards: ScrCard[];
  count: number;
  /** v2 SCR files read, including the older ones only the `previous` chain reaches. */
  files_searched: number;
}

/** `GET /api/scribe/feed?id=<SCR id>`: that SCR and the one its `previous` names. */
export interface ScrDetail {
  scr: ScrCard;
  previous: ScrCard | null;
}

/** One section of a preference page, from `office.scribe-query` mode `preferences`. */
export interface ScribePreference {
  page: string;
  /** `global` or `project:<name>`. */
  scope: string;
  matter: string;
  /** `<page>#<matter>`. */
  citation: string;
  statement: string;
  cites: string[];
  stated_at: string;
  review_by: string;
  /** Past its `review_by`. */
  stale: boolean;
  /** SCR entries that recorded a proposed change for this matter. */
  proposed: Array<{ entry: string; proposal: string }>;
}

/** `GET /api/scribe/preferences`: the current preference pages, by scope. */
export interface ScribePreferences {
  preferences: ScribePreference[];
  count: number;
  today: string;
}

/** One decision ledger entry, from `office.scribe-query` mode `decisions`. */
export interface ScribeDecision {
  /** `dec-<scope>-<n>`. */
  id: string;
  /** The ledger's id (`decisions-<scope>`). */
  ledger: string;
  /** 1-based line in the ledger body. */
  line: number;
  question: string | null;
  chosen: string | null;
  /** The decision message, verbatim; null for a private reference-only entry. */
  quote: string | null;
  /** The stated rationale, verbatim; null when none was stated. */
  because: string | null;
  scope: string;
  decided_by: string;
  decided_at: string;
  reverses: string | null;
  reversed_by: string | null;
  /** Not reversed by a later entry. */
  current: boolean;
  /** The evidence's source id. */
  source: string;
  /** A reference-only entry from a private session. */
  private: boolean;
}

/** `GET /api/scribe/decisions`: ledger entries, oldest ledger and line first. */
export interface ScribeDecisions {
  decisions: ScribeDecision[];
  count: number;
}

/** One category of Orbit's `precedent-decision-readout/1` readout: aggregates only. */
export interface ScribeAgreementCategory {
  /** `<workspace>:<kind>`, as the readout names it. */
  category: string;
  workspace: string | null;
  kind: string;
  mode: "shadow" | "enabled";
  scored_requests: number;
  accuracy: number | null;
  accuracy_wilson_low: number | null;
  accuracy_wilson_high: number | null;
  coverage: number | null;
  calibration_error: number | null;
  abstention_rate: number | null;
  conflict_rate: number | null;
  /** Below this many scored requests the row shows "Not enough decisions yet". */
  min_samples: number;
}

/** The pinned `precedent-decision-readout/1` readout, aggregate fields only. */
export interface ScribeDecisionAgreement {
  contract: "precedent-decision-readout/1";
  consumer_mode: "shadow" | "enabled";
  generated_at: string | null;
  window_start: string | null;
  window_end: string | null;
  categories: ScribeAgreementCategory[];
}

/**
 * `GET /api/scribe/decision-agreement`: the Agreement view's data.
 * `agreement` is null when the view is hidden (consumer off or no readout).
 */
export interface ScribeAgreementResponse {
  agreement: ScribeDecisionAgreement | null;
}

/** Whose scope a tag's entries belong to. */
export type ScribeTagScope = "global" | "project" | "session";
/** Where a tag's entries go; never a file outside the Library. */
export type ScribeTagSink = "decisions_ledger" | "preference_page" | "scr_only" | "orbit_task";
/** How a tag's entries take effect. */
export type ScribeTagMode = "propose" | "apply" | "append";

/** One tag definition in force (specs/scribe.md, "Configuration"). */
export interface ScribeTagDef {
  /** Lowercase kebab-case, unique. */
  name: string;
  scope: ScribeTagScope;
  instruction: string;
  sink: ScribeTagSink;
  mode: ScribeTagMode;
  capture: "markers" | "markers+inferred";
  /** For `orbit_task` only: the Orbit workspace selector. */
  workspace?: string;
}

/** `GET /api/scribe/tags`: the effective `scribe` settings from `office.status`. */
export interface ScribeTags {
  tags: ScribeTagDef[];
  cadence: { minutes: number; turns: number };
  /** The Library's tag list differs from the shipped defaults. */
  tags_customized: boolean;
}

/**
 * `POST /api/scribe/tags`: one Tags editor save through `office.config-set`.
 * `upsert` writes `scribe.tags.<name>`, `delete` writes it as null, and
 * `restore` writes `scribe.tags` as null (the two shipped defaults).
 */
export type ScribeTagSave =
  | { action: "upsert"; tag: ScribeTagDef }
  | { action: "delete"; name: string }
  | { action: "restore" };

/** A save Orbit accepted; `tags` is the effective block read back after it. */
export interface ScribeTagSaved {
  saved: true;
  tags: ScribeTags;
}

/** One cited claim of an answer. */
export interface ScribeClaim {
  text: string | null;
  /** Verbatim from the cited line; null when a private line is cited by reference only. */
  quote: string | null;
  cites: string[];
  file_line: string | null;
  msg: string | null;
  resume: string | null;
  withheld: boolean;
}

/** `POST /api/scribe/ask` with `{question}`: a cited answer, or "not captured". */
export interface ScribeAnswer {
  status: "answered" | "not_captured" | "reference_only";
  claims: ScribeClaim[];
  /** `file:line` of private lines that matched, cited by reference only (no open-weights route). */
  references: string[];
  /** Claims dropped because a quote did not match its cited line. */
  dropped: number;
  since: string | null;
  until: string | null;
  topics: string[];
  files_searched: number;
  /** `<route> <model>` of the one model call, when one was made. */
  model: string | null;
}

/** A structured handoff's fields, in display order (specs/sessions.md, "Structured handoff contract"). */
export interface HandoffFields {
  objective: string;
  next: string;
  /** Empty when none was recorded. */
  authorization: string;
  observations: Array<{ text: string; observed_at: string; recheck: string }>;
  evidence: string[];
}

/** The `resolved` entry that closed a handoff. */
export interface HandoffClosing {
  /** `<source-id>#L<n>`. */
  id: string;
  /** Null when its source is private. */
  text: string | null;
  time: string;
}

/**
 * One legacy v1 handoff, from `office.session-brief` with `view: list` and
 * `legacy: true` (specs/sessions.md, "Handoff list").
 */
export interface ScribeHandoff {
  /** `<source-id>#L<n>`: the conversation source and the handoff's line. */
  id: string;
  source_id: string;
  line: number;
  time: string;
  /** The rendered text; empty when withheld. */
  text: string;
  /** Structured fields, or null for a legacy or withheld handoff. */
  fields: HandoffFields | null;
  /** A historical one-line handoff, shown as text. */
  legacy: boolean;
  /** A private source: listed by reference only. */
  withheld: boolean;
  scope: "project" | "global";
  /** The thread key, passed back as the history filter; null for a global. */
  thread: string | null;
  project: string | null;
  orchestrator: string | null;
  /** The first Orbit task id the handoff cites. */
  assignment: string | null;
  session_id: string | null;
  /** `claude --resume <session-id>`. */
  resume: string | null;
  source_title: string;
  superseded: boolean;
  state: "open" | "resolved" | "expired";
  until: string | null;
  resolved_by: HandoffClosing | null;
}

/**
 * `GET /api/scribe/handoffs[?project&orchestrator&thread&offset]`: the legacy
 * v1 handoff list (`view: list` with `legacy: true`), kept for the scribe v2
 * compatibility window. Filters and paging apply to `history` only. The
 * Scribe feed and Today read {@link ScrFeed} instead.
 */
export interface ScribeHandoffs {
  /** Each open thread's current handoff, newest first. */
  threads: ScribeHandoff[];
  threads_total: number;
  /** Global handoffs, newest first, open and closed (each with its state). */
  global: ScribeHandoff[];
  global_total: number;
  /** Project handoffs, newest first, one page. */
  history: ScribeHandoff[];
  history_total: number;
  history_offset: number;
  /** More history follows this page. */
  history_more: boolean;
}

/**
 * `GET /api/today/digest`: the morning digest since the latest meeting.
 * `state` says why `digest` is null: no meeting has filed to-dos yet, the
 * meeting is not extracted, or the scoped digest is not available.
 */
export interface TodayDigest {
  state: "ready" | "no_meeting" | "not_extracted" | "unavailable";
  /** The meeting the digest covers, or null when none was found. */
  source: { id: string; title: string } | null;
  digest: SourceDigest | null;
}

/** Where an agent to-do from a meeting stands. `held`: not filed for agents because its matter is contested. */
export type AgentTodoState = "queued" | "working" | "review" | "merged" | "done" | "held";

/** One agent to-do on `GET /api/todos/agents`: filed in the target workspace, or held back as yours. */
export interface AgentTodo {
  /** The Orbit task id. For a held to-do, the office task that holds it. */
  id: string;
  title: string;
  /** The workspace selector the task is in, or `office`. */
  workspace: string;
  state: AgentTodoState;
  crew: string | null;
  pr: TaskPr | null;
  /** The source id from the `source:<id>` tag, or null. */
  source: string | null;
}

/** `GET /api/todos/agents`: agent to-dos filed from meetings, and the target workspace. */
export interface AgentTodos {
  /** `meetings.agent_workspace`, or null when every extracted to-do stays yours. */
  target_workspace: string | null;
  /** The followed workspace selectors (`brief.task_workspaces`), offered as targets. */
  followed: string[];
  todos: AgentTodo[];
  /** True when the target workspace could not be read; held to-dos still show. */
  target_unavailable: boolean;
}

/** `POST /api/settings/agent-workspace`: set or clear `meetings.agent_workspace`. */
export interface AgentWorkspaceWrite { workspace: string | null }
export interface AgentWorkspaceSaved { target_workspace: string | null }

/** One side of a contradiction: a ledger decision with its reason and citation. */
export interface ContradictionSide {
  /** The decision ledger entry id, when the source names it. */
  entry?: string;
  text: string;
  because: string | null;
  /** Source id plus anchor, for example `src-…#t=00:02:14` or `src-…#L42`. */
  cite: string;
  decided_at: string | null;
  /** Where it was decided, in plain words (a meeting or session title). */
  context: string | null;
}

/** One open contradiction between a new decision and a prior ledger decision (ORB-21340). */
export interface Contradiction {
  /** Opaque key for resolving it; never rendered. */
  key: string;
  matter: string;
  newer: ContradictionSide;
  prior: ContradictionSide;
  /** Agent to-dos held until it is resolved. */
  held: number;
}

/**
 * `GET /api/scribe/contradictions`: the open decision-conflict to-dos that
 * `office.extract` files. `available` is false when they can't be read.
 */
export interface Contradictions {
  available: boolean;
  contradictions: Contradiction[];
}

/** `POST /api/scribe/contradictions/resolve`: which decision stands. */
export interface ContradictionResolve { key: string; stands: "newer" | "prior" }

/** One job the office runs, the Meta model it calls and where it runs. */
export interface ModelJob {
  job: string;
  model: string;
  where: string;
  /** What the app could check now, in plain words. */
  check: { state: "passed" | "attention" | "off"; label: string };
}

/** `GET /api/settings/models`: every model the office calls, all Meta's. */
export interface ModelsView {
  jobs: ModelJob[];
  /** RFC 3339 time the checks were read. */
  checked_at: string;
}
