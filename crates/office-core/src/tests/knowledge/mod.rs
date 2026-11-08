//! Knowledge page parsing, base rules, anchors and the model view through
//! the public domain API.

mod citation;
mod contacts;
mod index;
mod parse;
mod rules;
mod view;

use crate::domain::knowledge::Page;

// The example in specs/library.md, "Knowledge pages", plus `as_of`.
const SPEC_PAGE: &str = r#"---
id: dom-project-acme-renewal   # dom- | rb-
type: domain                   # domain | runbook
template: project              # optional; see knowledge-templates.md
title: Acme renewal
aliases: [Acme renewal, the renewal]
summary: Acme's contract renews in March; Jane Park decides; price sensitivity high.
tags: [project:acme-renewal, client:acme]
cites:
  - src-20261028-acme-qbr#t=00:12:31
  - src-20261024-slack-pricing#L14-20
refs:                          # pointers, domain pages only
  - { label: Master services agreement, path: ~/Documents/Acme/MSA-2025.pdf }
  - { label: Renewal tickets, account: "jira:ACME" }
  - { label: QBR deck template, artifact: art-20261021-qbr-template }
  - { label: Jane Park, contact: "jane.park@acme.example" }
related: [rb-client-qbr-prep, dom-client-acme, dom-person-jane-park]
updated_at: 2026-10-28T15:10:00-04:00
last_verified: 2026-10-28
status: current                # current | stale | retired
---
## Status
Renews in March [src-20261028-acme-qbr#t=00:12:31].
"#;

fn page(markdown: &str) -> Page {
    Page::from_markdown(markdown).expect("valid page")
}

fn code(result: crate::Result<impl std::fmt::Debug>) -> &'static str {
    result.expect_err("refused").code()
}
