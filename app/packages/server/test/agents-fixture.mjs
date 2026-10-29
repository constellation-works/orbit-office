/** Deterministic, in-memory Orbit data for the agents feature. No real workspace or child processes. */
export const now = new Date(2026, 9, 26, 14);
const at = (day, hour = 10) => new Date(2026, 9, day, hour).toISOString();
const task = (id, title, status, extra = {}) => ({
  id, title, status, type: "feature", priority: "medium", crew: "muse-wonder",
  updated_at: at(26, 9), history: [], ...extra,
});

/** Tasks per workspace selector; `office` is the office workspace (no selector). */
export function agentsWorkspaces() {
  return {
    office: [
      task("ORB-1", "Office working task", "in-progress", { updated_at: at(26, 13) }),
      task("ORB-2", "Office review task", "review", { updated_at: at(26, 12), priority: "high" }),
      task("ORB-3", "Office queued task", "backlog", { updated_at: at(26, 11) }),
      task("ORB-4", "Office done task", "done", {
        updated_at: at(26, 13, ), history: [{ at: at(26, 10), to_status: "done" }],
      }),
      task("ORB-5", "Office proposed task", "proposed", { updated_at: at(26, 13) }),
      task("ORB-6", "Office done yesterday", "done", {
        updated_at: at(26, 8), history: [{ at: at(25, 10), to_status: "done" }],
      }),
    ],
    ws_alpha: [
      task("ORB-10", "Alpha working task", "in-progress", { updated_at: at(26, 13, ), crew: "codex" }),
      task("ORB-11", "Alpha older working task", "in-progress", { updated_at: at(26, 10) }),
      task("ORB-12", "Alpha queued task", "backlog", { updated_at: at(26, 9) }),
    ],
    ws_beta: [
      task("ORB-20", "Beta review task", "review", { updated_at: at(26, 12) }),
      task("ORB-21", "Beta done task", "done", {
        updated_at: at(26, 11), history: [{ at: at(26, 9), to_status: "done" }],
      }),
    ],
  };
}

/**
 * A fake Orbit client: `office.status` reports the followed selectors and
 * `orbit.task.list` answers per workspace. `failures` names workspaces
 * (`office`, or a selector) whose task lists throw.
 */
export function fakeAgentsClient(workspaces = agentsWorkspaces(), failures = []) {
  const state = { calls: [] };
  const byWorkspace = new Map(Object.entries(workspaces));
  return {
    state,
    client: {
      async callTool(name, input) {
        state.calls.push({ name, input });
        if (name === "office.status") {
          const selectors = [...byWorkspace.keys()].filter((key) => key !== "office");
          return { ok: true, output: { brief: { task_workspaces: selectors } } };
        }
        throw new Error(`unexpected callTool ${name}`);
      },
      async pollTool(name, input) {
        state.calls.push({ name, input });
        if (name !== "orbit.task.list") throw new Error(`unexpected pollTool ${name}`);
        const workspace = typeof input.workspace === "string" ? input.workspace : "office";
        if (failures.includes(workspace)) throw new Error("workspace unavailable");
        const rows = byWorkspace.get(workspace) ?? [];
        const statuses = Array.isArray(input.status) ? input.status : [input.status];
        const tasks = rows.filter((row) => statuses.includes(row.status));
        return structuredClone({ tasks, total: tasks.length, truncated: false });
      },
    },
  };
}
