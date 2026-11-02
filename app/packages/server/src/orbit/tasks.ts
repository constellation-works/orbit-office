/**
 * Small readers shared by the dashboard routes: an `office.*` envelope, a
 * task's pull request from its Orbit delivery record, and the workspace
 * settings `office.status` reports.
 */
import type { TaskPr } from "@orbit-office/contracts";

import type { OrbitToolClient } from "./client.js";

type Row = Record<string, unknown>;

function record(value: unknown): value is Row {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** An `office.*` tool refused: its refusal code, never its message. */
export class OfficeRefusal extends Error {
  constructor(readonly code: string) {
    super(code);
    this.name = "OfficeRefusal";
  }
}

/** Unwrap an `office.*` exec envelope (`{ok:true,output}`), or a bare output. */
export function officeOutput(value: unknown): Row {
  if (!record(value)) throw new OfficeRefusal("invalid_output");
  if (value.ok === false) {
    const error = value.error;
    const code = record(error) && typeof error.code === "string" ? error.code
      : typeof error === "string" ? error : "refused";
    throw new OfficeRefusal(code);
  }
  const data = value.ok === true ? value.output : value;
  if (!record(data)) throw new OfficeRefusal("invalid_output");
  return data;
}

/** The followed selectors and the meeting agent workspace, from `office.status`. */
export interface WorkspaceSettings {
  followed: string[];
  agentWorkspace: string | null;
}

export function workspaceSettings(status: unknown): WorkspaceSettings {
  const data = officeOutput(status);
  const brief = data.brief;
  const followed = record(brief) && Array.isArray(brief.task_workspaces)
    ? brief.task_workspaces.filter((item: unknown): item is string => typeof item === "string") : [];
  const meetings = data.meetings;
  const agentWorkspace = record(meetings) && typeof meetings.agent_workspace === "string" && meetings.agent_workspace !== ""
    ? meetings.agent_workspace : null;
  return { followed, agentWorkspace };
}

/** An `owner/name` GitHub repository, the only shape a link is built from. */
function repository(value: unknown): string | null {
  if (typeof value !== "string") return null;
  return /^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(value) && !value.includes("..") ? value : null;
}

/**
 * The task's PR from its newest delivery run (`orbit.task.show` field
 * `delivery`), or null when it has none or the record can't be read.
 */
export async function taskPr(
  client: Pick<OrbitToolClient, "pollTool">,
  id: string,
  workspace: string | null,
): Promise<TaskPr | null> {
  try {
    const delivery = await client.pollTool("orbit.task.show", {
      id, field: "delivery", ...(workspace === null ? {} : { workspace }),
    });
    if (!record(delivery) || delivery.task_id !== id) return null;
    const landing = delivery.landing;
    if (!record(landing) || typeof landing.pr_number !== "number" || !Number.isInteger(landing.pr_number)) return null;
    const repo = repository(delivery.repository);
    return {
      number: landing.pr_number,
      url: repo === null ? null : `https://github.com/${repo}/pull/${landing.pr_number}`,
      merged: landing.status === "merged",
    };
  } catch {
    return null;
  }
}

/** The source id from a task's `source:<id>` tag, or null. */
export function sourceTag(tags: unknown): string | null {
  if (!Array.isArray(tags)) return null;
  for (const tag of tags) {
    if (typeof tag === "string" && tag.startsWith("source:src-")) return tag.slice("source:".length);
  }
  return null;
}
