---
template: runbook
type: runbook
id: rb-<slug>
title: "<Task name>"
one_per_library: false
attach: never
sensitivity: normal
sections: [Inputs, Steps, Done when]
required: [Inputs, Steps, Done when]
performable: agent
submission:
  system: "<Target system>"
  via: draft
---
## Inputs
List the evidence, context and permissions needed before starting.

## Steps
Write ordered, actionable steps with source citations and explicit approval
boundaries. Keep reusable instructions separate from a single task's facts.

## Done when
State observable completion checks and the artifact or outcome to deliver.
