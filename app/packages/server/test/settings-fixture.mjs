/** Actual gateway payload shapes, deliberately oldest first to test both sort boundaries. */
export const connection = {
  name: "acme-crm", base_url: "https://api.acme.example/v2/",
  auth: { scheme: "bearer" }, methods: ["GET", "HEAD"], drafts: ["PATCH"],
  timeout_seconds: 20, max_response_bytes: 1048576, disabled: false,
  slot: "conn_03", slot_state: "set",
};
export const audit = {
  records: [
    { time: "2026-10-26T09:00:00.000Z", caller: { agent: "Codex", model: "Office assistant", task_id: "private-task-id", session_id: "private-session" },
      tool: "gateway.http-get", connection: "acme-crm", method: "GET", path: "contacts", query: {},
      status: 200, bytes: 628, duration_ms: 142, body_sha256: "a".repeat(64), redirects: 0, outcome: "ok" },
    { time: "2026-10-26T09:10:00.000Z", caller: { agent: "Codex", model: "Office assistant" },
      tool: "gateway.http-get", connection: "acme-crm", method: "GET", path: "accounts", query: {},
      status: 200, bytes: 1840, duration_ms: 218, body_sha256: "b".repeat(64), redirects: 0, outcome: "ok" },
    { time: "2026-10-26T09:20:00.000Z", caller: { agent: "Codex", model: "Office assistant" },
      tool: "gateway.http-get", connection: "acme-crm", method: "GET", path: "deals", query: {},
      status: null, bytes: 0, duration_ms: 20000, body_sha256: null, redirects: 0, outcome: "timeout" },
  ], skipped: 0,
};
export const gatewayFixtures = {
  "gateway.connection-list": { connections: [connection], free_slots: 15 },
  "gateway.audit-list": audit,
};
