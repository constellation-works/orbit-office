# Sessions: scratch

Where one-off scripts and intermediate files go, and what to delete.

- Put one-off scripts and intermediate files in
  `.orbit/tmp/session-<yyyymmdd>/` in the office workspace, dated today.
- Right after the brief, delete **only your own `session-*` folders older
  than 7 days**, and nothing else under `.orbit/tmp/`. Check the name and
  age first. Leave anything you are unsure about, and follow the host's
  cleanup policy.
- Never write scratch or agent output under `library/`. Never use `/tmp`.
- Scratch is not a result. A keeper becomes a task artifact with
  `orbit.task.artifact.put` (`source_path` under `.orbit/tmp/`, at most
  1 MiB) and reaches the Library only when the user approves it. See
  [tasks.md](tasks.md).
- Runtime rules for Python and API calls are in
  [tools-and-guardrails.md](tools-and-guardrails.md).

Approved scripts that run on a schedule go through routines and
`office.script-run`, which you do not call. Proposing one is in
[proposals-and-insights.md](proposals-and-insights.md).
