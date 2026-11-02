/**
 * `GET /api/today/digest`: the morning digest since the latest meeting.
 *
 * The latest meeting is the newest source that filed a to-do (its
 * `source:<id>` tag on an office task). The digest itself is
 * `office.source-digest` (ORB-21337, sessions.md "Digest since a
 * meeting"): read-only and cited. Not `office.digest` with `source`: that
 * verb is mutating and refuses the app's `local-cli` `[agent]` caller
 * (ORB-21376). Any other refusal answers `unavailable` and the screen
 * shows why.
 */
import type { SourceDigest, TodayDigest } from "@orbit-office/contracts";
import type { IncomingMessage, ServerResponse } from "node:http";

import type { OrbitToolClient } from "../../orbit/client.js";
import { logRouteFailure } from "../../orbit/diagnostics.js";
import { OfficeRefusal, officeOutput, sourceTag } from "../../orbit/tasks.js";

/** Sources tried, newest first, before giving up on finding a meeting. */
const CANDIDATES = 3;

function send(response: ServerResponse, body: TodayDigest): void {
  response.writeHead(200, { "content-type": "application/json", "cache-control": "no-store" });
  response.end(JSON.stringify(body));
}

/** Source ids from task tags, newest task first, each once. */
export function recentSources(listing: unknown): string[] {
  const tasks = typeof listing === "object" && listing !== null && "tasks" in listing && Array.isArray(listing.tasks)
    ? listing.tasks as unknown[] : [];
  const dated = tasks.flatMap((row) => {
    if (typeof row !== "object" || row === null) return [];
    const task = row as Record<string, unknown>;
    const source = sourceTag(task.tags);
    const at = typeof task.created_at === "string" ? Date.parse(task.created_at) : NaN;
    return source === null ? [] : [{ source, at: Number.isFinite(at) ? at : 0 }];
  });
  dated.sort((a, b) => b.at - a.at || b.source.localeCompare(a.source));
  return [...new Set(dated.map(({ source }) => source))];
}

function digestOf(data: Record<string, unknown>): SourceDigest | null {
  if (typeof data.source !== "string" || !Array.isArray(data.decided) || !Array.isArray(data.shipped) ||
      !Array.isArray(data.needs_you)) return null;
  return {
    source: data.source,
    title: typeof data.title === "string" ? data.title : data.source,
    decided: data.decided as SourceDigest["decided"],
    shipped: data.shipped as SourceDigest["shipped"],
    needs_you: data.needs_you as SourceDigest["needs_you"],
    report: typeof data.report === "string" ? data.report : "",
    unavailable: Array.isArray(data.unavailable) ? data.unavailable as string[] : [],
    truncated: data.truncated === true,
  };
}

export async function handleDigest(
  client: Pick<OrbitToolClient, "callTool" | "pollTool">,
  _request: IncomingMessage,
  response: ServerResponse,
): Promise<void> {
  let sources: string[];
  try {
    sources = recentSources(await client.pollTool("orbit.task.list", {
      tag: "owner:you", fields: ["id", "tags", "created_at"], limit: 200,
      status: ["proposed", "backlog", "in-progress", "review", "blocked", "someday", "done"],
    }));
  } catch (error) {
    logRouteFailure("/api/today/digest", "orbit.task.list", error);
    send(response, { state: "unavailable", source: null, digest: null });
    return;
  }
  for (const id of sources.slice(0, CANDIDATES)) {
    try {
      const digest = digestOf(officeOutput(await client.callTool("office.source-digest", { source: id })));
      if (digest === null) {
        send(response, { state: "unavailable", source: { id, title: id }, digest: null });
        return;
      }
      send(response, { state: "ready", source: { id, title: digest.title }, digest });
      return;
    } catch (error) {
      const code = error instanceof OfficeRefusal ? error.code : undefined;
      if (code === "invalid_source_kind") continue;
      if (code === "meeting_not_extracted") {
        send(response, { state: "not_extracted", source: { id, title: id }, digest: null });
        return;
      }
      logRouteFailure("/api/today/digest", "office.source-digest", error);
      send(response, { state: "unavailable", source: { id, title: id }, digest: null });
      return;
    }
  }
  send(response, { state: "no_meeting", source: null, digest: null });
}
