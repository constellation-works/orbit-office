---
template: scribe-rules
type: domain
id: dom-scribe-rules
title: Scribe rules
one_per_library: true
attach: never
sensitivity: normal
sections: [Tags, Topics, Matters, Capture, Models]
required: []
---
## Tags
List one `- name: meaning` bullet per tag, with one or two nested
example bullets under it. For example:

- decision: Daniel settles a choice.
  - lets go one step at a time
- direction: A standing instruction from Daniel.
- question: An open question for later.
- todo: An action, optionally with due.
- reminder: A time-bound note to self.
- fact: A stated fact about hosts, people or configuration.
- landed: A PR merged or a task done, reported in the chat.
- preference: Daniel's standing answer to a class of question, with matter.
- why: The reason for an earlier decision.
- agent-decision: An orchestrator settles a choice within its delegated scope.
- pending: Work an orchestrator started or is waiting on.
- resolved: Closes an earlier pending line.
- incident: A failure or broken run an orchestrator hit.

## Topics
List the allowed topic slugs, one bullet each, for example project
names and feature slugs. The scribe may only use listed topics; it may
suggest a new one, and adding it is a proposal. For example:

- my-project

## Matters
List the allowed preference matter slugs, one bullet each: a short
slug for a class of question a preference answers. For example:

- release-with-open-defects

## Capture
List the capture rules, one bullet each: whose messages may take which
tags, and what to skip. For example:

- Only Daniel's messages and by: user notes take decision, direction, todo, question, reminder and preference.
- Assistant messages take landed and fact, and the agent tags agent-decision, pending, resolved and incident with by: <crew>.
- Skip status chatter and restated plans.

## Models
List one model and effort per level, as `- <level>: <model>@<effort>`.
Canned queries are model-free, so `query` has no model. For example:

- query: none
- ask: haiku@low
- scribe: haiku@medium
