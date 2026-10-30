# Example Library layout

A real Library is created by `office.init` in the user's Orbit workspace
(`~/orbit/library/` by default). It is a git repository of its own, which
the workspace repository ignores. This example only shows the shape:

```
library/
  sources/YYYY/MM/<source-id>/     # evidence: source.md + original.<ext>
  knowledge/
    domain/<slug>.md               # About: facts, pointers, preferences
    runbooks/<slug>.md             # How-to: repeatable work
  artifacts/YYYY/MM/<artifact-id>/ # approved agent outputs, versioned
  proposals/open/<proposal-id>.md  # rendered evidence per waiting proposal
  proposals/{accepted,dismissed}/YYYY/MM/<proposal-id>.md  # closed, by closing month
  .office/config.yaml              # Library settings: retention, sensitivity, packs, thresholds
```

Markdown is the source of truth; the plugin's SQLite index is rebuildable
with `office.reindex`. Binaries (`original.*`) are git-ignored in a real
Library and pinned by the `sha256` in their source's frontmatter.
