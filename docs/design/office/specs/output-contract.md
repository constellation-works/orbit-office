---
type: design
summary: "Spec: exec envelope contract — request, response, errors and exit codes"
last_validated: 2026-10-26
---

# Spec: Exec envelope contract

## Request

```json
{"schema_version": 1, "tool": "office.status", "input": {}, "context": {}}
```

- `tool` must be `office.<verb>` for a verb in the tool table.
- `input` keys must be exactly the keys the tool accepts; unknown keys are
  refused (`invalid_argument`), never ignored.
- Identity and secrets arrive in `context`, never in `input`.

## Response

- Success: `{"ok": true, "output": {...}}` — one JSON document on stdout.
- Tool failure: `{"ok": false, "error": {"code", "message", "retryable"}}`
  with a stable `snake_case` code. The process still exits 0: it served
  the call.
- Usage errors (bad argv) go to stderr and exit 2; a surface failure exits 1.
- Secrets never appear in any output: presence is `set` or `unset`.
