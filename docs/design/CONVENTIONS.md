---
title: Design Doc Conventions
owner: claude
last_updated: 2026-09-26
status: Accepted
---

# Design Doc Conventions

How design docs under `docs/design/<feature>/` are laid out, condensed from
Orbit's `docs/design/CONVENTIONS.md` (the source this template follows).

## 1. Folder layout

```
docs/design/<feature>/
├── 1_overview.md     what and why
├── 2_design.md       how it works today; ends with Concerns & Honest Limitations
├── 3_vision.md       open questions and directions, labelled as speculation
├── 4_decisions.md    titled decisions with Context · Decision · Consequences (Cost:)
├── specs/            one prescriptive mechanism contract per file
└── references/       glossary and other lookup docs
```

Start a feature by copying the scaffold:

```sh
cp -r docs/design/_templates docs/design/<feature>
mv docs/design/<feature>/specs/_mechanism.md docs/design/<feature>/specs/<mechanism>.md
```

Folder names are lowercase, hyphenated and singular. No `README.md`,
roadmap, changelog or tutorial inside a feature folder.

## 2. Frontmatter and sections

Every numbered doc keeps the frontmatter from `_templates/` (`title` equals
the H1, `status` is `Draft` until reviewed, then `Accepted`; `doc_role`
matches the `1_`–`4_` prefix) and ends with a **Task References** section
listing only the task ids cited in that doc, each with a verb phrase.

## 3. What earns a decision entry

A decision enters `4_decisions.md` through one of two doors:

1. **It explains surprising code** — name the file and symbol in
   `**Code anchors:**`.
2. **It governs future decisions** — a standing rule a reader can apply to a
   case nobody has seen yet.

Either way it names a real alternative and a non-trivial `Cost:`. Everything
else is design prose in `2_design.md`. A departure from an adopted
constellation standard is always a decision entry, citing `STD-nn@<version> §Rn`.

## 4. Links and ids

Relative links only (`./`, `../`). Task ids are plain bracketed text
(`[ORB-NNNNN]`), never links, and always come with what the task did.
Design docs may cite tracker ids; user-facing text (help, output, errors)
never does.
