/** Typed access to the app server, with one error path. */
import type {
  AcceptAllResult, AcceptRequest, ApprovalDetail, CalendarSynced, CalendarView, MeetingOutcome, ApprovalOk, ApproveResult, ConnectionSaved, ConnectionWrite,
  GatewaySendResult, Health, PrivateRouteSaved, PrivateRouteStatus, PrivateRouteWrite, SettingsApiCalls,
  SettingsConnections, SettingsStatus, SignInPoll, SignInStarted, BootstrapPhase, CapturedSource, CapturePaste, SourceCitation, SourcePlan, SourceView, SelectionInput, SelectionCount, TodoScheduled,
  ConflictChoice, ShareDocument, SharedLastSync, SharedLibraryBlock, SharedLibrarySaved, SharedLibraryStatus, SharedSync, ShareRequest, ShareResult,
} from "@orbit-office/contracts";

export class ApiError extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
    readonly routeSaved = false,
    /** A tool's refusal code the server passed on, for a plain-words message. */
    readonly reason?: string,
    /** The settings field a refusal names; never its value. */
    readonly field?: string,
    /** Cited documents to share first (`share_cites_unshared`). */
    readonly documents: ShareDocument[] = [],
  ) {
    super(`app server answered ${status} (${code})`);
    this.name = "ApiError";
  }
}

/** Fetch JSON, translating network, HTTP and malformed response errors alike. */
export async function getJson<T>(path: string): Promise<T> {
  return requestJson<T>(path);
}

async function requestJson<T>(path: string, body?: unknown, upload?: Blob): Promise<T> {
  let response: Response;
  try {
    response = await fetch(path, upload !== undefined
      ? { method: "POST", headers: { accept: "application/json", "content-type": "application/octet-stream" }, body: upload }
      : body === undefined
      ? { headers: { accept: "application/json" } }
      : { method: "POST", headers: { accept: "application/json", "content-type": "application/json" }, body: JSON.stringify(body) });
  } catch {
    throw new ApiError(0, "network_error");
  }
  if (!response.ok) {
    const body: unknown = await response.json().catch(() => undefined);
    const code = typeof body === "object" && body !== null && "error" in body && typeof body.error === "string"
      ? body.error : "unknown";
    const routeSaved = typeof body === "object" && body !== null && "route_saved" in body && body.route_saved === true;
    const reason = typeof body === "object" && body !== null && "reason" in body && typeof body.reason === "string"
      ? body.reason : undefined;
    const field = typeof body === "object" && body !== null && "field" in body && typeof body.field === "string"
      ? body.field : undefined;
    const documents = typeof body === "object" && body !== null && "documents" in body && Array.isArray(body.documents)
      ? (body.documents as unknown[]).filter((item): item is ShareDocument => typeof item === "object" && item !== null &&
        "id" in item && typeof item.id === "string" && "title" in item && typeof item.title === "string")
      : [];
    throw new ApiError(response.status, code, routeSaved, reason, field, documents);
  }
  try {
    return (await response.json()) as T;
  } catch {
    throw new ApiError(response.status, "invalid_json");
  }
}

/** Read the server health contract. */
export function getHealth(): Promise<Health> {
  return getJson<Health>("/healthz");
}

/** Read the settings contract: the agent CLI connection. */
export function getSettings(): Promise<SettingsStatus> {
  return getJson<SettingsStatus>("/api/settings");
}

export function getConnections(): Promise<SettingsConnections> {
  return getJson<SettingsConnections>("/api/settings/connections");
}

export function putConnection(input: ConnectionWrite): Promise<ConnectionSaved> {
  return requestJson<ConnectionSaved>("/api/settings/connections/put", input);
}

/** Start an oauth2_device connection's device-code sign-in: the code and where to enter it. */
export function startSignIn(name: string): Promise<SignInStarted> {
  return requestJson<SignInStarted>("/api/settings/connections/sign-in", { name });
}

/** Check once whether the user approved the sign-in. */
export function pollSignIn(name: string): Promise<SignInPoll> {
  return requestJson<SignInPoll>("/api/settings/connections/sign-in/poll", { name });
}

export function getApiCalls(): Promise<SettingsApiCalls> {
  return getJson<SettingsApiCalls>("/api/settings/api-calls");
}

/** The configured private endpoint, or null. Never includes a token value. */
export function getPrivateRoute(): Promise<PrivateRouteStatus> {
  return getJson<PrivateRouteStatus>("/api/settings/private-route");
}

/** Save the endpoint block, then read the check. Omit `token` to leave it. */
export function savePrivateRoute(input: PrivateRouteWrite): Promise<PrivateRouteSaved> {
  return requestJson<PrivateRouteSaved>("/api/settings/private-route", input);
}

/** The team shared library block, or null, and the latest sync the app ran. */
export function getSharedLibrary(): Promise<SharedLibraryStatus> {
  return getJson<SharedLibraryStatus>("/api/settings/shared-library");
}

/** Save the whole block with `office.config-set`; a refusal names its field only. */
export function saveSharedLibrary(input: SharedLibraryBlock): Promise<SharedLibrarySaved> {
  return requestJson<SharedLibrarySaved>("/api/settings/shared-library", input);
}

/** Turn on the hourly `office-shared-sync` routine. */
export function enableSharedSync(): Promise<{ ok: true }> {
  return requestJson<{ ok: true }>("/api/settings/shared-library/routine", { enabled: true });
}

/** **Sync now**: one `office.library-sync`, in counts of documents. */
export function syncSharedLibrary(): Promise<SharedSync> {
  return requestJson<SharedSync>("/api/settings/shared-library/sync", {});
}

/** The latest sync the app ran, from the server's memory. */
export function getLastSharedSync(): Promise<SharedLastSync> {
  return getJson<SharedLastSync>("/api/settings/shared-library/last-sync");
}

/** The skipped documents were read: Needs you stops listing them. */
export function dismissSharedSkipped(): Promise<{ ok: true }> {
  return requestJson<{ ok: true }>("/api/settings/shared-library/dismiss-skipped", {});
}

/** Share a document with the team or stop sharing it (`office.share-set`). */
export function shareDocument(input: ShareRequest): Promise<ShareResult> {
  return requestJson<ShareResult>("/api/library/share", input);
}

/** POST JSON and read the JSON answer, with the same error path as {@link getJson}. */
export function postJson<T>(path: string, body: unknown): Promise<T> {
  return requestJson<T>(path, body);
}

/** Set an open to-do's scheduled start (an RFC 3339 instant), or clear it with null. */
export function scheduleTodo(id: string, notBefore: string | null): Promise<TodoScheduled> {
  return postJson<TodoScheduled>("/api/todos/schedule", { id, not_before: notBefore });
}

/** What a Needs-you card shows before the user acts on it. */
export function getApprovalDetail(key: string): Promise<ApprovalDetail> {
  return getJson<ApprovalDetail>(`/api/approvals/detail?key=${encodeURIComponent(key)}`);
}

/** Add a reviewed output to the Library, then close its task. */
export function approveOutput(key: string): Promise<ApproveResult> {
  return postJson<ApproveResult>("/api/approvals/approve", { key });
}

export function sendBack(key: string, comment: string): Promise<ApprovalOk> {
  return postJson<ApprovalOk>("/api/approvals/send-back", { key, comment });
}

export function dismissNeed(key: string, reason?: string): Promise<ApprovalOk> {
  return postJson<ApprovalOk>("/api/approvals/dismiss", reason === undefined ? { key } : { key, reason });
}

/** Accept a proposal; a shared-library conflict also names whose version wins. */
export function acceptProposal(key: string, sha256: string, choice?: ConflictChoice): Promise<ApprovalOk> {
  return postJson<ApprovalOk>("/api/approvals/accept", choice === undefined ? { key, sha256 } : { key, sha256, choice });
}

export function acceptProposals(proposals: AcceptRequest[]): Promise<AcceptAllResult> {
  return postJson<AcceptAllResult>("/api/approvals/accept-all", { proposals });
}

/** Approve a gateway draft: the server sends it once, bound to `sha256`. */
export function sendDraft(key: string, sha256: string): Promise<GatewaySendResult> {
  return postJson<GatewaySendResult>("/api/approvals/send", { key, sha256 });
}


export function getSource(id: string): Promise<SourceView> {
  return getJson<SourceView>(`/api/sources?id=${encodeURIComponent(id)}`);
}
export function capturePaste(input: CapturePaste): Promise<CapturedSource> {
  return postJson<CapturedSource>("/api/sources/paste", input);
}
export function uploadSource(file: File, phase?: BootstrapPhase, privateSource = false): Promise<CapturedSource> {
  const params = new URLSearchParams({ name: file.name });
  if (phase) params.set("phase", phase);
  if (privateSource) params.set("private", "true");
  return requestJson<CapturedSource>(`/api/sources/upload?${params}`, undefined, file);
}
export function sourcePlan(input: { id?: string; phase?: BootstrapPhase; private: boolean; confirm?: true; digest?: string }): Promise<SourcePlan> {
  return postJson<SourcePlan>("/api/sources/plan", input);
}
export function markSourcePrivate(id: string): Promise<{ ok: true }> {
  return postJson<{ ok: true }>("/api/sources/private", { id });
}
export function getSourceCitation(id: string, kind: string): Promise<SourceCitation> {
  return getJson<SourceCitation>(`/api/sources/citation?${new URLSearchParams({ id, kind })}`);
}

export function getSelectionCount(): Promise<SelectionCount> {
  return getJson("/api/selection");
}
export function putSelection(input: SelectionInput): Promise<{ ok: true }> {
  return postJson("/api/selection", input);
}
export function clearSelections(): Promise<{ ok: true }> {
  return postJson("/api/selection", { clear: true });
}

/** Meetings overlapping `[from, until)` and the routines that run; at most 42 days. */
export function getCalendar(from: Date, until: Date): Promise<CalendarView> {
  return getJson<CalendarView>(`/api/calendar?${new URLSearchParams({ from: from.toISOString(), until: until.toISOString() })}`);
}
/** "Sync now": read the calendar once and replace the snapshot. */
export function syncCalendar(): Promise<CalendarSynced> {
  return postJson<CalendarSynced>("/api/calendar/sync", {});
}
/** Pasted notes become a meeting source linked to the event. */
export function captureMeetingNotes(eventKey: string, text: string): Promise<CapturedSource> {
  return postJson<CapturedSource>("/api/calendar/capture", { event_key: eventKey, text });
}
/** A dropped `.vtt`, text or audio file, through the same staging as Source uploads. */
export function captureMeetingFile(eventKey: string, file: File): Promise<CapturedSource> {
  return requestJson<CapturedSource>(`/api/calendar/capture?${new URLSearchParams({ event_key: eventKey, name: file.name })}`, undefined, file);
}
export function getMeetingOutcome(source: string): Promise<MeetingOutcome> {
  return getJson<MeetingOutcome>(`/api/calendar/meeting?${new URLSearchParams({ source })}`);
}
