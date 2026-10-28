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

/** Every Needs-you card the approval actions cover, for classification and the browser. */
export function approvalTasks() {
  const page = (id, title, tags) => task(id, title, "proposed", ["proposal", "proposal:knowledge_add", ...tags]);
  return [
    page("onboard-1", "How your team works", ["onboarding"]),
    page("onboard-2", "Your goals this quarter", ["onboarding"]),
    page("boot-1", "Ingest service: overview", ["bootstrap:docs", "subject:ingest-service"]),
    page("boot-2", "Ingest service: on-call runbook", ["bootstrap:docs", "subject:ingest-service"]),
    page("boot-3", "Expense policy", ["bootstrap:process"]),
    task("suggestion", "Turn meeting notes into a weekly brief", "proposed", ["proposal", "proposal:auto_task"]),
    task("draft", "Add a follow-up note to the Acme account", "proposed", ["gateway-draft", "proposal", "proposal:gateway_draft"]),
    task("script", "Receipt catalog for October", "proposed", ["script-output"]),
    task("review", "The project brief is ready to read", "review", ["owner:agent"]),
    task("personal", "Send your feedback on the plan", "backlog", ["owner:you"]),
  ];
}

/** A shared-library sync's proposals: one colleague's update and one page changed on both sides. */
export function sharedTasks() {
  const cluster = "proposal-cluster:5f0c2a";
  return [
    task("suggestion", "Turn meeting notes into a weekly brief", "proposed", ["proposal", "proposal:auto_task"]),
    task("shared-update", "When the Lantern spool stalls", "proposed", ["proposal", "proposal:knowledge_update", "shared-library", cluster]),
    task("shared-conflict", "Acme", "proposed", ["proposal", "proposal:shared_conflict", "shared-library", cluster]),
    task("onboard-1", "How your team works", "proposed", ["proposal", "proposal:knowledge_add", "onboarding"]),
  ];
}
