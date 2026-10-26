//! Record shapes every driver agrees on. These are the crate's contract
//! types: I/O-free, serde-shaped, and stable to rename around (fields are
//! added, never renamed or retyped).

use serde::{Deserialize, Serialize};

/// Which Library layer a document belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentKind {
    /// `sources/…/source.md` — evidence, captured once.
    Source,
    /// `knowledge/domain/<slug>.md` — About pages.
    Domain,
    /// `knowledge/runbooks/<slug>.md` — How-to pages.
    Runbook,
    /// `artifacts/…/artifact.md` — approved agent outputs.
    Artifact,
}

impl DocumentKind {
    /// The canonical lowercase token, as stored in the index.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Source => "source",
            Self::Domain => "domain",
            Self::Runbook => "runbook",
            Self::Artifact => "artifact",
        }
    }

    /// Parse a canonical token; unknown tokens are refused, never guessed.
    #[must_use]
    pub fn parse(token: &str) -> Option<Self> {
        match token {
            "source" => Some(Self::Source),
            "domain" => Some(Self::Domain),
            "runbook" => Some(Self::Runbook),
            "artifact" => Some(Self::Artifact),
            _ => None,
        }
    }
}

/// One indexed Library document, as read from its Markdown file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentRecord {
    /// `src-…`, `dom-…`, `rb-…` or `art-…`.
    pub id: String,
    /// Which Library layer it belongs to.
    pub kind: DocumentKind,
    /// Frontmatter `title` (falls back to the id).
    pub title: String,
    /// Frontmatter `summary` (empty when absent).
    pub summary: String,
    /// Frontmatter `tags`.
    pub tags: Vec<String>,
    /// Frontmatter `series` (artifacts group versions by series).
    pub series: Option<String>,
    /// Frontmatter `sensitivity` (`normal` when absent).
    pub sensitivity: String,
    /// Path relative to the Library root, forward slashes.
    pub path: String,
    /// The document body (after the frontmatter), indexed for search.
    pub body: String,
    /// Frontmatter `cites` entries (`src-…#anchor`, page ids).
    pub cites: Vec<String>,
    /// Frontmatter `related` page ids.
    pub related: Vec<String>,
}

/// Filters for [`crate::LibraryStore::search`]. An absent field filters
/// nothing; filters apply before the limit.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchFilter {
    /// Only this Library layer.
    pub kind: Option<DocumentKind>,
    /// Only documents carrying this tag.
    pub tag: Option<String>,
    /// Only documents in this artifact series.
    pub series: Option<String>,
    /// Maximum hits to return.
    pub limit: Option<usize>,
}

/// One search hit: the document and why it matched.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchHit {
    /// The matching document (without its body).
    pub id: String,
    /// Its Library layer.
    pub kind: DocumentKind,
    /// Its title.
    pub title: String,
    /// Its path relative to the Library root.
    pub path: String,
    /// A short excerpt around the first match.
    pub snippet: String,
}
