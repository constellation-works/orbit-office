# Example Library layout

A real Library is created by the `office` plugin in the user's Orbit
workspace (`~/office/library/` by default) and is a git repository. This
example only shows the shape:

```
library/
  sources/YYYY/MM/<source-id>/     # evidence: source.md + original.<ext>
  knowledge/
    domain/<slug>.md               # About: facts, pointers, preferences
    runbooks/<slug>.md             # How-to: repeatable work
  artifacts/YYYY/MM/<artifact-id>/ # approved agent outputs, versioned
  proposals/<proposal-id>.md       # rendered evidence per proposal
  .office/config.yaml              # retention, sensitivity, enabled packs
```

Markdown is the source of truth; the plugin's SQLite index is rebuildable
with `office.reindex`. Binaries (`original.*`) are git-ignored in a real
Library and pinned by the `sha256` in their source's frontmatter.
