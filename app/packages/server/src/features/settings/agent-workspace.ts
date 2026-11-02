/**
 * Where meeting agent to-dos go: `meetings.agent_workspace`, written with
 * `office.config-set` (plugin-surface.md). Null clears it, so every
 * extracted to-do stays yours.
 */
import type { AgentWorkspaceSaved, AgentWorkspaceWrite } from "@orbit-office/contracts";

import type { OrbitToolClient } from "../../orbit/client.js";
import { OfficeRefusal, officeOutput } from "../../orbit/tasks.js";
import { SettingsError } from "./gateway.js";

const KEY = "meetings.agent_workspace";

/** One workspace selector: no whitespace or control characters. Unknown keys are refused. */
export function agentWorkspaceInput(value: unknown): AgentWorkspaceWrite {
  if (typeof value !== "object" || value === null || Array.isArray(value) ||
      Object.keys(value).some((key) => key !== "workspace") || !("workspace" in value)) {
    throw new SettingsError(400, "invalid_input");
  }
  const { workspace } = value as { workspace: unknown };
  if (workspace === null) return { workspace: null };
  if (typeof workspace !== "string" || workspace.length === 0 || workspace.length > 200 || /[\s\p{Cc}]/u.test(workspace)) {
    throw new SettingsError(400, "invalid_input", { field: "workspace" });
  }
  return { workspace };
}

export async function saveAgentWorkspace(client: Pick<OrbitToolClient, "callTool">, input: AgentWorkspaceWrite): Promise<AgentWorkspaceSaved> {
  try {
    officeOutput(await client.callTool("office.config-set", { key: KEY, value: input.workspace }));
  } catch (error) {
    if (error instanceof OfficeRefusal && (error.code === "invalid_library_setting" || error.code === "invalid_argument")) {
      throw new SettingsError(400, "invalid_input", { field: "workspace" });
    }
    throw new SettingsError(502, "settings_unavailable");
  }
  return { target_workspace: input.workspace };
}
