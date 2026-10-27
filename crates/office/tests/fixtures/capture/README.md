# Document capture fixtures

Generated locally for these tests; no third-party document content.

- `confluence.html`: headings, paragraph, list, table and omitted navigation/script/style content.
- `guide.docx`: minimal Word document XML with a heading and a numbered-list paragraph.
- `entities.docx` and `entities.pptx`: one paragraph whose text is an ampersand entity, a less-than entity and a numeric character reference (`Tom &amp; Jerry &lt; Spike &#38; Tyke`).
- `unknown-entity.docx`: the same package shape with an undeclared entity, which conversion must refuse.
- `brief.pptx`: slide relationship order differs from numeric filenames; the opening slide links to notesSlide7.
- `guide.pdf`: two pages with Helvetica text operators.
- `scanned.pdf`: one image-only page without a text layer.
- `missing*`, `malformed.docx` and `escape.docx`: invalid package/part or traversal cases.
- `empty.pptx`: valid presentation structure with no slide text or notes.

The binary tests use a deterministic pdftotext protocol fixture, checking flags and the pinned input before emitting form-feed-separated pages. This isolates the adapter from a host Poppler installation. The PDF files can also be passed directly to Poppler for a separate converter smoke check.
