---
template: scribe-rules
type: domain
id: dom-scribe-rules
title: Scribe rules
one_per_library: true
attach: never
sensitivity: normal
sections: [Note, Skip, Kinds]
required: [Note]
---
## Note
List what the live scribe should capture from your conversations, one
bullet each: for example standing directions, decisions with their reason,
rejected options and why, changes to crews, pools and configuration, and
facts about hosts and people.

## Skip
List what it should leave out, such as status chatter, restated plans or
anything about a named private topic.

## Kinds
Give guidance for borderline lines, such as when a reply counts as a
decision. Rules can narrow what is captured; they never let an assistant
line become a decision or direction. To let the scribe also write a
handoff, add `scribe:` with `handoff: true` to this page's frontmatter.
