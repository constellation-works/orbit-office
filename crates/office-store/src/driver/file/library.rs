//! Reading and writing Library documents on disk, and scanning the indexed
//! layers for a reindex.
//!
//! The pure frontmatter reader lives with `DocumentRecord` in office-core;
//! files remain the source of truth for fields the index does not consume.

use std::path::PathBuf;

mod base;
mod collection;
mod io;
mod layout;
mod publish;
mod scan;
mod templates;

pub(crate) use layout::LAYOUT_DIRS;

/// A template file under `.office/templates/`, read without following a symlink.
pub(crate) struct TemplateOnDisk {
    /// Library-relative path, forward slashes.
    pub(crate) relative: String,
    /// Markdown, or `None` when the path is a symlink or could not be read.
    pub(crate) markdown: Option<String>,
}

/// The file driver over one Library root.
pub(crate) struct LibraryFs {
    root: PathBuf,
}
