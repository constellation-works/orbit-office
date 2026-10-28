---
type: design
summary: "Spec: About templates — the bundled page templates, the template field, attach policies, contact privacy and the onboarding interview"
last_validated: 2026-10-26
---

# Spec: About templates and onboarding

A new Library starts with **templates, not pages**. Templates say what a
good About page looks like (the user, a person, a client, a project). Pages
are written from evidence: first from a short onboarding interview in the
user's first session, then from KT videos, meetings and chats as they arrive. Every
claim cites a source from day one
([decision](../4_decisions.md#about-pages-start-from-templates-and-an-onboarding-interview-never-from-stubs)).

## Templates

A template is a Markdown file in `library/.office/templates/<name>.md`.
Its frontmatter is the template's rules, and its body is the skeleton a
new page starts from:

```markdown
---
template: person
type: domain                 # domain (About) | runbook (How-to)
id: dom-person-<slug>        # a fixed id for a one-per-Library template
title: "<Full name>"
one_per_library: false
attach: participants         # always | participants | tagged | matched | never
sensitivity: normal
sections: [Role, Works with, What they care about, History]
required: [Role]
---
## Role
What they do, for whom, and what they decide.

## Works with
...
```

- **Bundled.** The plugin's templates are compiled into the binary from
  `crates/office-core/templates/`. `office.init` copies each one that is
  missing into `.office/templates/`, and never overwrites a template the
  user changed. A later plugin version's new templates arrive the same way,
  on the next `office.init`, which the install script runs on every
  upgrade.
- **Editable.** The user may change a template, or add one, as a file. A
  template is validated whenever it is read. An invalid one is refused as
  `template_invalid`, naming the file and the field. It is never skipped
  silently.
- **Not pages.** Templates are not indexed, searched, attached or counted
  by any metric.

The pure Rust API is `office_core::domain::template`: parse with
`Template::from_markdown(filename, markdown)` and check resolved page fields
with `validate_page(id, DocumentKind, body)`. `BUNDLED` exposes each template's
`filename` and complete `markdown`, ready for init to copy. The parser does
no I/O. All nine rule fields shown above are required; unknown and duplicate
keys are refused. Frontmatter supports a flat YAML mapping with plain or
quoted scalar values, comments, and inline or indented block lists of heading
names. Other YAML constructs are refused with `template_invalid`; diagnostics
name the file and field without echoing rule values.

Ids use lowercase letters, digits and single hyphens between nonempty
components. A pattern may contain one `<slug>` placeholder, matching one or
more such components. A one-per-Library template must use a fixed id.
`type` is `domain` or `runbook`; `sensitivity` is `normal` or `private`.
Required headings must also occur in `sections`. Page headings match the
declared names exactly; headings inside fenced or indented code do not
create sections. A section ends at the next level-one or level-two heading.

## The template field on pages

Knowledge page frontmatter ([library.md](./library.md#knowledge-pages))
gains two optional fields:

```yaml
template: person
aliases: [Jane, J. Park]   # names the subject goes by in sources
```

`office.knowledge-write` checks a page with a `template` against the
current version of that template:

| Check | Refusal |
|---|---|
| The template exists | `unknown_template` |
| The id matches the template's `id` pattern, or equals it for a one-per-Library template | `page_id_mismatch` |
| No other page uses a one-per-Library template | `template_page_exists` |
| Every `required` section is present as a `##` heading with content | `missing_section` |
| The `type` matches the template's `type` | `template_type_mismatch` |

A page without a `template` is a free-form About page, checked against the
base rules only. When a user changes a template, existing pages stay
readable and are not migrated. Pages that no longer meet it are counted
by `office.status` as `pages_off_template`, and their next write must
meet the current template. A template file that does not parse is listed
in `template_invalid` by its Library-relative path; status still returns
the rest of its report.

## Bundled templates

| Template | Page id | Holds | Attach |
|---|---|---|---|
| `user` | `dom-user` (one) | Name, role, team, duties, how the user likes work done. Accounts such as a Jira project key go in `refs` | `always` |
| `standing-directions` | `dom-standing-directions` (one) | The rules the user has given the agent ("always draft, never send"), one bullet each, each citing its `direction` entry | `always` |
| `systems` | `dom-systems` (one) | Tools and where things live: issue tracker projects, document folders, chat channels. Pointers in `refs` only | `always` |
| `person` | `dom-person-<slug>` | One person: role, organization, who they work with, what they care about, history | `participants` |
| `client` | `dom-client-<slug>` | One external account: overview, people (links to person pages), contract, history, open issues | `tagged` (`client:<slug>`) |
| `project` | `dom-project-<slug>` | A project or initiative: status, background, stakeholders, decisions, timeline, the artifact series it produced | `tagged` (`project:<slug>`) |
| `topic` | `dom-topic-<slug>` | Business context for an area (pricing, hiring): overview, how it works here, key numbers, open questions | `tagged` (`topic:<slug>`) |
| `glossary` | `dom-glossary` (one) | Terms and acronyms, one `### Term` each, citing where it was first seen | `matched` |
| `goals` | `dom-goals` (one) | This period's priorities or objectives | `never`; read by signals and reviews |
| `rhythm` | `dom-rhythm` (one) | Recurring meetings and deadlines ("status update every Friday") | `never`; read by signals |
| `runbook` | `rb-<slug>` | Inputs, steps, done when (all required); optional `performable` and `submission` for runbooks the agent carries out as drafts ([library-bootstrap.md](./library-bootstrap.md#runbooks-the-agent-performs-as-drafts)) | named by the task, as today |

The bundled files use these concrete heading names and minimum requirements.
All default to `sensitivity: normal`; all except `runbook` have `type: domain`.
Runbooks use `attach: never` for automatic policy selection and can still be
explicitly named by a task.

| Template | Sections | Required |
|---|---|---|
| `user` | Name, Role, Team, Duties, Working preferences | Name, Role |
| `standing-directions` | Directions | Directions |
| `systems` | Tools, Locations | Tools |
| `person` | Role, Works with, What they care about, History | Role |
| `client` | Overview, People, Contract, History, Open issues | Overview |
| `project` | Status, Background, Stakeholders, Decisions, Timeline, Artifacts | Status, Background |
| `topic` | Overview, How it works here, Key numbers, Open questions | Overview |
| `glossary` | Terms (one `###` heading per term) | Terms |
| `goals` | Priorities, Objectives | Priorities |
| `rhythm` | Recurring meetings, Deadlines | Recurring meetings |
| `runbook` | Inputs, Steps, Done when | Inputs, Steps, Done when |

Skeleton prose prompts for evidence; it is not a populated Library page.
The onboarding and knowledge-write consumers replace it with supported claims
and citations before proposing a page.

- **One page per person.** A meeting links its participants to their own
  pages, so a person's back-links show indexed meeting sources and other
  Library documents that link to them. The Library screen builds the contacts
  list from those pages. Task links are deferred until a supported page-citation
  read exists.
- **Locations live with their subject.** Paths to a project's files are in
  that project page's `refs`, so attaching the page brings the paths with
  it. The `systems` page holds only what no single subject owns.
- **How-to starters ship with packs,** not with `office.init`. The meetings
  pack's follow-up runbook is part of that pack.

## Attach policies

`office.attach` ([plugin-surface.md](./plugin-surface.md#tools)) picks
pages by template, not by name, before it adds runbooks and example
artifacts. All of these count toward its limit of 8 artifacts, in this
order:

1. **`always`:** `dom-user`, `dom-standing-directions` and `dom-systems`,
   when they exist and are `current`.
2. **`participants`:** person pages whose title or `aliases` match a
   participant of the task's source.
3. **`tagged`:** client, project and topic pages whose tag the task
   carries.
4. **`matched`:** the glossary, when the source's text contains one of
   its terms.

Pages attached by policy do not count toward knowledge coverage, which
counts runbooks only ([analytics-and-reviews.md](./analytics-and-reviews.md)).

## Contact details

Person and client pages stay `sensitivity: normal`, so they reach the
models that do the work. The contact details on them never do:

- Email addresses and phone numbers live only in `contact` refs, never in
  the page body. `office.knowledge-write` refuses a body that contains an
  email address or phone number, with `contact_in_body`.
- Every model input and every task attachment carries the page's
  **model view**: each `contact` ref keeps its label and loses its value.
  This covers `office.attach`, `office.extract`, `office.digest`,
  `office.knowledge-show` and any later model call. The one function is
  `office_core::domain::knowledge::Page::model_view`; it also withholds any
  email address or phone number a hand edit left in the body.
- A draft names the people it addresses. When the user sends it, the app
  fills in the addresses from the person pages, on the user's machine.

A page the user marks `private` is routed like any private material
([models-and-privacy.md](./models-and-privacy.md#routing-rules)).

## Refs

`refs` gains an `account` target: a name or key in another system, written
`system:key` (`jira:OFF`, `slack:#pricing`, `drive:Acme/Renewal`). It is a
pointer, never a credential. The full set of targets is now `path`, `url`,
`contact`, `account` and `artifact`.

## Onboarding interview

**When.** `office.session-brief` returns an `onboarding` section listing
each one-per-Library template that has no page yet. While `dom-user` is
missing, the orchestrator offers the interview once per session, in one
line. If the user declines, it carries on and offers again next session.

**How.** The skill asks one question at a time, and the user may skip any
of them:

1. Name, role, team, and what you're responsible for → `user`
2. How you like drafts written, and anything the agent must never do → `standing-directions`
3. The projects or initiatives you're working on now → `project` pages
4. The people you work with most: name, role, organization → `person` and `client` pages
5. Recurring meetings and deadlines → `rhythm`
6. Tools, and where things live: issue tracker projects, folders, channels → `systems`, and `refs` on the other pages
7. Your priorities this period (optional) → `goals`

Jargon that comes up goes to the glossary.

**Record.** Each answer is appended to the session's decision log as one
or more `fact` entries, in the user's words
([sessions.md](./sessions.md#decision-log)). A document the user pastes
during the interview, such as an org chart or a project brief, is captured
as its own source first, as always.

**Pages.** The agent then files one `knowledge_add` proposal per page
([proposals.md](./proposals.md)). Each starts from its template's
skeleton, cites the `fact` entries it draws on, and carries the tag
`onboarding`. The app shows the set as one card, "Set up your Library",
with *Accept all* and per-page review. The orchestrator mentions the set
once, as a group, which does not count against its limit of two proposals
per turn. Nothing is written until the user accepts.

## Growing after onboarding

Templates also shape the proposals that follow. The signals are in
[proposals.md](./proposals.md#signals):

- **New subject:** a participant, or a `client:`/`project:`/`topic:` tag,
  that no page covers, seen in two or more sources → a `knowledge_add`
  proposal from the matching template.
- **Unrecorded direction:** a `direction` entry that `dom-standing-directions`
  does not cite → a `knowledge_update` adding it.
- **Declared cadence:** a `rhythm` entry with a deliverable that no routine
  or auto-task covers → an `auto_task` proposal, before the repetition
  signal would have fired.

A page's sharing approval is independent of its template. `Page::model_view` and its Markdown attachments retain `shareable: true` so an approved page does not lose that field when used as a proposal draft. Absent or false means unshared. Sharing still requires the [citation and sensitivity checks](./shared-library.md#the-shareable-field).
