//! Shared Library document and search records, independent of storage.

use serde::{Deserialize, Serialize};
use std::path::Path;

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

/// Filters for Library search. An absent field filters
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

/// One commit in a document's Library history.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Revision {
    /// Full git object id.
    pub commit: String,
    /// Author name.
    pub author: String,
    /// Author email.
    pub email: String,
    /// Author timestamp, RFC 3339 with an explicit offset.
    pub authored_at: String,
    /// Commit subject.
    pub message: String,
}

impl DocumentRecord {
    /// Parse the Library's supported frontmatter fields and body without I/O.
    /// Unknown frontmatter remains in the source Markdown, which is the truth.
    #[must_use]
    pub fn from_markdown(relative: &Path, text: &str) -> Self {
        parse_document(text, relative)
    }
}

/// Parse one document's frontmatter and body into an index record.
fn parse_document(text: &str, relative: &Path) -> DocumentRecord {
    let (frontmatter, body) = split_frontmatter(text);
    let kind = kind_for_path(relative);

    let mut record = DocumentRecord {
        id: String::new(),
        kind,
        title: String::new(),
        summary: String::new(),
        tags: Vec::new(),
        series: None,
        sensitivity: "normal".to_string(),
        path: relative.to_string_lossy().replace('\\', "/"),
        body: body.trim().to_string(),
        cites: Vec::new(),
        related: Vec::new(),
    };

    let mut list_key: Option<&str> = None;
    for line in frontmatter.lines() {
        let trimmed = line.trim();
        if let Some(item) = trimmed.strip_prefix("- ") {
            match list_key {
                Some("cites") => record.cites.push(unquote(item.trim())),
                Some("related") => record.related.push(unquote(item.trim())),
                Some("tags") => record.tags.push(unquote(item.trim())),
                _ => {}
            }
            continue;
        }
        let Some((key, value)) = trimmed.split_once(':') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        list_key = None;
        match key {
            "id" => record.id = unquote(value),
            "title" => record.title = unquote(value),
            "summary" => record.summary = unquote(value),
            "tags" => {
                record.tags = inline_list(value);
                list_key = Some("tags");
            }
            "series" if !value.is_empty() && value != "null" => {
                record.series = Some(unquote(value));
            }
            "sensitivity" if !value.is_empty() => record.sensitivity = unquote(value),
            "cites" => {
                record.cites = inline_list(value);
                list_key = Some("cites");
            }
            "related" => {
                record.related = inline_list(value);
                list_key = Some("related");
            }
            _ => {}
        }
    }

    if record.id.is_empty() {
        record.id = relative
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or_default()
            .to_string();
    }
    if record.title.is_empty() {
        record.title = record.id.clone();
    }
    record
}

/// Library layer from a document's relative path.
fn kind_for_path(relative: &Path) -> DocumentKind {
    let text = relative.to_string_lossy();
    if text.starts_with("knowledge/domain") {
        DocumentKind::Domain
    } else if text.starts_with("knowledge/runbooks") {
        DocumentKind::Runbook
    } else if text.starts_with("artifacts") {
        DocumentKind::Artifact
    } else {
        DocumentKind::Source
    }
}

/// Split leading `---` frontmatter from the body. A document without
/// frontmatter parses as all body.
fn split_frontmatter(text: &str) -> (&str, &str) {
    let Some(rest) = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
    else {
        return ("", text);
    };
    match rest.find("\n---") {
        Some(end) => {
            let body_start = end + "\n---".len();
            let body = rest[body_start..]
                .strip_prefix('\n')
                .unwrap_or(&rest[body_start..]);
            (&rest[..end], body)
        }
        None => ("", text),
    }
}

/// `[a, b]` or a bare scalar, as a list of unquoted items.
fn inline_list(value: &str) -> Vec<String> {
    let inner = value
        .strip_prefix('[')
        .and_then(|v| v.strip_suffix(']'))
        .unwrap_or(value);
    if inner.trim().is_empty() {
        return Vec::new();
    }
    inner.split(',').map(|item| unquote(item.trim())).collect()
}

fn unquote(value: &str) -> String {
    value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
        .unwrap_or(value)
        .to_string()
}
