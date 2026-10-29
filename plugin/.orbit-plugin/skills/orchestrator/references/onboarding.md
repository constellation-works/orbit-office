# Onboarding

How to set up a new user or a new role: the onboarding interview first, then the Library bootstrap, then the habits that make the Library grow.

## When to onboard

Read the `onboarding` section of `office.session-brief` (see [sessions.md](sessions.md)).

- The section lists each one-per-Library template that has no page yet:
  `user`, `standing-directions`, `systems`, `glossary`, `goals`, `rhythm`.
  Each item's `id` is the page id (for example `dom-user`), `template` is
  the template name, and `link` is the installed template file.
- The section is absent once all of them exist.
- A page counts only when its frontmatter names the template. A hand-written
  `dom-user` without `template: user` still shows as missing.
- **While `user` is listed, offer the interview once per session, in one
  line.** For example: "Want to spend five minutes telling me about your
  work, so I can set up your Library?"
- If the user declines, carry on with what they asked. Offer again next
  session, not again this session.
- If `user` exists but other templates are still listed, do not run the
  whole interview again. Ask only the missing topic when it comes up
  naturally.

A new role (same user, new job) also starts here. Ask before reusing old
pages: propose updates to `dom-user` and friends rather than new pages.

## The interview

Ask **one question at a time**. The user may skip any question. Keep each
question short and plain. Follow this order:

| # | Ask about | Feeds |
|---|---|---|
| 1 | Name, role, team, and what they are responsible for | `user` |
| 2 | How they like drafts written, and anything the agent must never do | `standing-directions` |
| 3 | The projects or initiatives they are working on now | `project` pages |
| 4 | The people they work with most: name, role, organization | `person` and `client` pages |
| 5 | Recurring meetings and deadlines | `rhythm` |
| 6 | Tools, and where things live: issue tracker projects, folders, channels | `systems`, and `refs` on the other pages |
| 7 | Priorities this period (optional) | `goals` |

Jargon and acronyms that come up go to the glossary (`dom-glossary`, one
`### Term` per term).

### Record every answer

- Append each answer to the session's conversation source as one or more
  `fact` entries, **in the user's words**. Do not paraphrase.
- A rule the user gives ("never send without asking me") is a `direction`
  entry, also verbatim. `dom-standing-directions` cites those.
- A pasted document (an org chart, a project brief) is captured first as its
  own source with `office.capture`. Cite it; do not paste it into the log.
- See [sessions.md](sessions.md) for the entry format and `append_to`.

```json
{"append_to":"<source-id>","entries":[{"kind":"fact","text":"I lead procurement analytics on the EMEA team."},{"kind":"direction","text":"Always draft client emails, never send."}]}
```

After the appends, read `office.source-show` on the conversation source to
get the `#L<n>` anchor of each `fact`. Pages cite those anchors.

## Turn answers into proposals

File **one `knowledge_add` proposal per page** with `office.proposal-create`.
Never write a page yourself; `office.knowledge-write` is not exposed to you.

- Start each page from its template. Use the `page` form with `template`
  and fill `sections` by heading name. Leave `body` empty when you use
  `template`.
- Use the template's id: the fixed id for one-per-Library pages
  (`dom-user`), the pattern for the rest (`dom-person-jane-park`,
  `dom-project-renewal`, `dom-client-acme`).
- Every required section must have real, cited content. A section you leave
  out keeps the template's prompt text. Fill each section you have facts
  for, and tell the user which sections are still empty.
- Cite the `fact` (and `direction`) anchors in `cites` and `evidence`.
- Tag every onboarding proposal `onboarding`. The tool adds the `proposal`
  tags itself; never pass `proposal` or `proposal:*` tags.
- Contact details go only in `contact` refs, never in the body. Page rules
  and refs are in [library.md](library.md) and the `office-library-conventions` skill.

```json
{"title":"About Dana Lee","description":"Your profile page, from the onboarding interview.","evidence":["<source-id>#L2","<source-id>#L3"],"tags":["onboarding"],"payload":{"kind":"knowledge_add","page":{"id":"dom-user","title":"About Dana Lee","template":"user","sections":{"Name":"Dana Lee [<source-id>#L2]","Role":"Leads procurement analytics on the EMEA team [<source-id>#L3]"},"cites":["<source-id>#L2","<source-id>#L3"]}}}
```

Then:

1. **Mention the set once, as a group.** "I've suggested six pages for your
   Library; review them in the app under Needs you." The app shows them on
   one card with *Accept all* and per-page review. The group does not count
   against the two-proposals-per-turn limit.
2. **Wait for acceptance.** Nothing is written until the user accepts. Do
   not accept for them, and do not act as if a page exists before it does.
3. Append an `action` entry naming the filed proposals, and a new `handoff`
   ("Onboarding pages proposed; waiting for the user to review them").
4. A page the user dismisses stays dismissed. A recently dismissed
   equivalent proposal comes back `suppressed`; do not refile it.

## Library bootstrap (new role)

After the interview pages exist, lead the bootstrap. The interview comes
first because its `user`, `project` and `person` pages give consolidation
names to attach claims to. The full per-phase procedure is in the
**`office-bootstrap` skill**; load it now. This section is the outline.

Run the four phases in order, one batch per phase. Do not start a phase
before the previous one has been walked through. If the user has no
material for a phase, say so and move on; never skip one silently.

| Phase | Ask for | Typical result |
|---|---|---|
| `docs` | Onboarding pages, design docs, project briefs (Confluence exports as HTML or PDF, Word, PowerPoint, PDF, Markdown, text) | `project`, `topic`, `systems` and owner `person` pages, glossary |
| `kt` | KT videos, with a `.vtt` transcript if there is one | Runbooks, `topic` and `systems` updates, gotchas, questions for the presenter |
| `team` | Team docs, presentations, reports | Updates to the pages above, artifact locations as `refs` |
| `process` | Company processes (request an AD group, request PTO) and policies | Runbooks, policy summaries that link to the official policy |

For each phase:

1. **Ask for the material** in plain words. The easiest path is a folder
   dropped into the app's Source screen: the app captures each file tagged
   `bootstrap:<phase>` and shows the phase's batch preview. Pasted text you
   capture yourself with `office.capture` and `tags: ["bootstrap:<phase>"]`.
   Fetching pages through a gateway connection is not part of the bootstrap.
2. **The batch preview is the user's.** It shows the counts, model calls and
   cost estimate (or "Rates unset"). The user presses Process in the app,
   which confirms with the digest they saw. You cannot preview or confirm a
   batch yourself: `office.batch-plan` is not exposed to you. KT video plans
   follow [packs.md](packs.md).
3. **The `office-bootstrap` routine runs the batch** (every 15 minutes). It
   ships switched off. If a confirmed batch is not moving, tell the user to
   switch on *office-bootstrap* on the Routines screen. Never run digest,
   KT or consolidation steps yourself, and never work around a
   `plan_changed`, `plan_unconfirmed` or `plan_exceeded` refusal.
4. **Walk the proposals by subject.** When consolidation has filed them,
   group by subject and go one subject at a time: the page proposed, what
   it claims, and the source anchors behind it. The user accepts or
   dismisses each one in the app.
5. **Report progress and gaps** with `office.bootstrap-status` (input `{}`).

Reading `office.bootstrap-status`:

- `phases`: one entry per phase in order, with `captured`, `digested`,
  `consolidating`, `proposed`, `accepted`, `dismissed`, `overflow_waiting`,
  `shared` and `gaps`.
- `pending_batches`: confirmed batches not consolidated yet.
- Each gap has `kind`, `subject` and `sources`. Turn each into one action:
  - `subject_without_page`: three or more sources, no page. Offer a page.
  - `procedure_without_runbook`: ask for the process doc, or offer a runbook.
  - `owner_without_person`: ask who the owner is, or offer a person page.
  - `policy_without_current_as_of`: ask for the current official policy.
- Gaps are computed. Report them as returned; never guess extra ones.
- `overflow_waiting` above zero means the page budget deferred subjects to
  the next run of that phase. Say so.

The app's Library screen shows the same progress. Its "Ask the agent" button
copies a message like "Please help with Ingest service: …" for the user to
paste to you. Treat it as the user's request: search the Library, then offer
the material request or the proposal that closes the gap.

A phase can be repeated when more material turns up. A repeated phase only
adds and updates pages.

## The first week

Help the user build the habits that make the system compound. Mention one
at a time, when it fits, not as a lecture.

- **Capture often.** Paste chats, emails and notes, or drop files. Everything
  captured is searchable and citable, even with no page about it.
- **Say rules out loud.** Each "always" or "never" becomes a `direction`
  entry, and the signals propose adding it to `dom-standing-directions`.
- **Review Needs you.** Accept or dismiss proposals soon. Pages and runbooks
  only help agents once they are accepted. See
  [proposals-and-insights.md](proposals-and-insights.md).
- **Hand over repeatable work.** Ask "do this for me" when a runbook exists;
  the result comes back as a draft to approve. See [tasks.md](tasks.md).
- **Let the routines run.** The daily digest, the KT inbox and the signals
  routine are on by default; signals turn repeated work into automation
  proposals. Routine switches are on the Routines screen.
- **Name the cadence.** A recurring meeting or deadline in `dom-rhythm` lets
  the signals propose an automation before the repetition shows up.

Never write a page, accept a proposal, or enable automation for the user.
Every one of those is the user's click.
