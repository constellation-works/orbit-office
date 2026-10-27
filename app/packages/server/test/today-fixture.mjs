/** Deterministic, in-memory Orbit data. No real workspace or child processes. */
export const now = new Date(2026, 9, 26, 14);
const at = (day, hour = 10) => new Date(2026, 9, day, hour).toISOString();
const task = (id, title, status, tags = [], extra = {}) => ({
  id, title, status, tags,
  description: "From the planning meeting notes. Keep the next step clear and close at hand.",
  execution_summary: "", history: [], ...extra,
});

export function todayTasks() {
  return [
    task("suggestion", "Turn meeting notes into a weekly brief", "proposed", ["proposal", "proposal:routine"]),
    task("review", "The project brief is ready to read", "review", ["owner:agent"]),
    task("personal", "Send your feedback on the plan", "backlog", ["owner:you"]),
    task("working", "Preparing the research notes", "in-progress", ["owner:agent"]),
    task("completed", "A clearer plan for the week", "done", ["owner:agent"], {
      history: [{ at: at(26), to_status: "done" }],
      execution_summary: "The plan brings together the priorities, next steps and supporting notes for the week.",
    }),
    task("old", "Yesterday’s finished work", "done", [], {
      history: [{ at: at(25), to_status: "done" }], updated_at: at(26),
    }),
    task("unassigned", "Unassigned work", "in-progress"),
    task("queued", "Agent work waiting to start", "backlog", ["owner:agent"]),
    task("ordinary", "An ordinary proposed task", "proposed"),
    task("rejected", "A dismissed suggestion", "rejected", ["proposal"]),
  ];
}

export function fakeTodayClient(initial = todayTasks()) {
  const state = { tasks: initial, calls: [], failure: null, truncated: false };
  return {
    state,
    client: {
      async pollTool(name, input) {
        state.calls.push({ name, input });
        if (state.failure) throw state.failure;
        const statuses = Array.isArray(input.status) ? input.status : [input.status];
        const tasks = state.tasks.filter((row) => statuses.includes(row.status));
        return structuredClone({ tasks, total: tasks.length, truncated: state.truncated });
      },
    },
  };
}
