//! Layout constants and indexed-document selection for the Library.

use std::path::Path;

/// Directory layout created under a Library root.
pub(crate) const LAYOUT_DIRS: &[&str] = &[
    "sources",
    "knowledge/domain",
    "knowledge/runbooks",
    "artifacts",
    "proposals",
    ".office",
];

/// Layers `reindex` rebuilds, in scan order. An empty stem list means every
/// `*.md` file; otherwise only those stems (`source.md` beside `original.*`
/// and `v2.md`, `artifact.md` beside its companion files). Document writes
/// accept only paths this table covers, so a successful write survives the
/// next rebuild.
pub(super) const INDEXED_DOCUMENT_LAYERS: &[(&str, &[&str])] = &[
    ("sources", &["source"]),
    ("knowledge", &[]),
    ("artifacts", &["artifact"]),
];

pub(super) const INDEXED_DOCUMENT_KIND: &str = concat!(
    "indexed document (",
    "sources/**/source.md, sources/decisions/<scope>.md, knowledge/**/*.md, ",
    "artifacts/**/artifact.md",
    ")"
);

/// Whether `relative` is a file `scan_documents` would collect.
pub(super) fn is_indexed_document(relative: &Path) -> bool {
    if office_core::domain::source::decisions::is_ledger_path(relative) {
        return true;
    }
    let mut components = relative.components();
    let Some(std::path::Component::Normal(layer)) = components.next() else {
        return false;
    };
    let Some(layer) = layer.to_str() else {
        return false;
    };
    let Some(&(_, stems)) = INDEXED_DOCUMENT_LAYERS
        .iter()
        .find(|(name, _)| *name == layer)
    else {
        return false;
    };
    let mut inside_layer = false;
    for component in components {
        let std::path::Component::Normal(_) = component else {
            return false;
        };
        inside_layer = true;
    }
    inside_layer && markdown_selected(relative, stems)
}

pub(super) fn markdown_selected(path: &Path, stems: &[&str]) -> bool {
    path.extension().is_some_and(|ext| ext == "md")
        && (stems.is_empty()
            || path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .is_some_and(|stem| stems.contains(&stem)))
}
