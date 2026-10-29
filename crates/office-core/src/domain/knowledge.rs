//! Shared Library document and search records, independent of storage,
//! and the knowledge page model (`page`): frontmatter, base rules,
//! citations, template checks and the model view.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

pub(crate) mod frontmatter;
pub(crate) use page::inline_citations;
pub use page::withhold_contacts;
mod page;

pub use page::{
    Citation, MAX_PAGE_WORDS, POLICY_TAG, Page, PageFile, PageRef, PageStatus, PageView,
    Performable, RefTarget, RefView, Submission, SubmissionVia, page_kind_for_id, parse_citation,
    resolve_anchor,
};

/// Which kind of document the Library holds.
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

/// Whether a document is owned locally or read from the team clone.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LibraryLayer {
    /// The user's writable Library.
    #[default]
    Own,
    /// The read-only shared clone.
    Shared,
}
impl LibraryLayer {
    /// Stable index and response token.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Own => "own",
            Self::Shared => "shared",
        }
    }
}

/// The canonical Library path inside a shared virtual path, or the own path.
/// Shared paths are `shared/knowledge/…` or `shared/members/<member>/…`.
#[must_use]
pub fn local_document_path(path: &str) -> &str {
    let Some(shared) = path.strip_prefix("shared/") else {
        return path;
    };
    shared
        .strip_prefix("members/")
        .and_then(|rest| rest.split_once('/').map(|(_, path)| path))
        .unwrap_or(shared)
}

/// One indexed Library document, as read from its Markdown file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentRecord {
    /// Read/write ownership boundary.
    #[serde(default)]
    pub layer: LibraryLayer,
    /// Sharing member; always absent on own documents.
    #[serde(default)]
    pub shared_by: Option<String>,
    /// `src-…`, `dom-…`, `rb-…` or `art-…`.
    pub id: String,
    /// Its document kind.
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
    /// Sharing approval; absent frontmatter defaults to false.
    #[serde(default)]
    pub shareable: bool,
    /// Path relative to the Library root, forward slashes.
    pub path: String,
    /// The document body (after the frontmatter), indexed for search.
    pub body: String,
    /// Frontmatter `cites` entries (`src-…#anchor`, page ids).
    pub cites: Vec<String>,
    /// Frontmatter `related` page ids.
    pub related: Vec<String>,
    /// Frontmatter `template` (knowledge pages that follow one).
    pub template: Option<String>,
    /// The document's date, `YYYY-MM-DD`: the first of `occurred_at`,
    /// `captured_at`, `updated_at` and `last_verified`, else the date in a
    /// dated id (`src-YYYYMMDD-…`, `art-YYYYMMDD-…`). `None` when it has none.
    pub date: Option<String>,
}

/// Filters for Library search. An absent field filters
/// nothing; filters apply before the limit.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchFilter {
    /// Only own or shared documents; absent searches both.
    pub layer: Option<LibraryLayer>,
    /// Only this document kind.
    pub kind: Option<DocumentKind>,
    /// Only documents carrying this tag.
    pub tag: Option<String>,
    /// Only documents in this artifact series.
    pub series: Option<String>,
    /// Only knowledge pages that name this template.
    pub template: Option<String>,
    /// Only documents dated on or after this `YYYY-MM-DD` (undated ones are excluded).
    pub since: Option<String>,
    /// Only documents dated on or before this `YYYY-MM-DD` (undated ones are excluded).
    pub until: Option<String>,
    /// Maximum hits to return.
    pub limit: Option<usize>,
}

/// One search hit: the document and why it matched.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchHit {
    /// Document ownership.
    pub layer: LibraryLayer,
    /// Sharing member, absent for own hits.
    pub shared_by: Option<String>,
    /// The matching document (without its body).
    pub id: String,
    /// Its document kind.
    pub kind: DocumentKind,
    /// Its title.
    pub title: String,
    /// Its virtual Library path.
    pub path: String,
    /// A short excerpt around the first match.
    pub snippet: String,
    /// Frontmatter `sensitivity` (`normal` when absent). Read tools withhold
    /// the snippet of a `private` hit (see the MCP private-content decision).
    pub sensitivity: String,
}

/// How one document links to another.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkKind {
    /// Through frontmatter `cites` (`<id>` or `<id>#<anchor>`).
    Cites,
    /// Through frontmatter `related`.
    Related,
}

/// A document that links to a given one, and how.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Backlink {
    /// The linking document's id.
    pub id: String,
    /// Its Library layer.
    pub kind: DocumentKind,
    /// Its title.
    pub title: String,
    /// Its path relative to the Library root.
    pub path: String,
    /// Every way it links, sorted.
    pub via: Vec<LinkKind>,
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
        let path = relative.to_string_lossy();
        let local = local_document_path(&path);
        let mut record = parse_document(text, Path::new(local));
        record.path = path.replace('\\', "/");
        if path.starts_with("shared/") {
            record.layer = LibraryLayer::Shared;
            record.shared_by = frontmatter::split(text)
                .ok()
                .and_then(|(header, _)| frontmatter::parse(header).ok())
                .and_then(|fields| fields.into_iter().find(|(key, _)| key == "shared_by"))
                .and_then(|(_, value)| value.as_str().map(str::to_owned));
            if matches!(record.kind, DocumentKind::Source | DocumentKind::Artifact)
                && let Some(member) = &record.shared_by
            {
                record.id = format!("{member}/{}", record.id);
            }
        }
        record
    }
}

/// Parse one document's frontmatter and body into an index record.
///
/// Documents in the supported YAML subset (see `frontmatter`) are parsed
/// with that parser, so indexing agrees with `Page` on comments, quoted
/// scalars (including escapes) and quoted list items. Documents outside
/// the subset — duplicate keys, malformed or unsupported frontmatter, no
/// frontmatter at all — fall back to a lenient line parser with the same
/// comment/quote handling, so index rebuild never rejects a legacy
/// document that used to index.
fn parse_document(text: &str, relative: &Path) -> DocumentRecord {
    if let Ok((frontmatter, body)) = frontmatter::split(text)
        && let Ok(fields) = frontmatter::parse(frontmatter)
    {
        return parse_document_fields(&fields, body, relative);
    }
    parse_document_lenient(text, relative)
}

/// Build a record from strictly parsed frontmatter fields.
fn parse_document_fields(
    fields: &[(String, Value)],
    body: &str,
    relative: &Path,
) -> DocumentRecord {
    let mut record = DocumentRecord {
        layer: LibraryLayer::Own,
        shared_by: None,
        id: String::new(),
        kind: kind_for_path(relative),
        title: String::new(),
        summary: String::new(),
        tags: Vec::new(),
        series: None,
        sensitivity: "normal".to_string(),
        shareable: false,
        path: relative.to_string_lossy().replace('\\', "/"),
        body: body.trim().to_string(),
        cites: Vec::new(),
        related: Vec::new(),
        template: None,
        date: None,
    };
    let mut dates: [Option<String>; 4] = [None, None, None, None];
    for (key, value) in fields {
        match key.as_str() {
            "id" => {
                if let Some(value) = value_string(value) {
                    record.id = value;
                }
            }
            "title" => {
                if let Some(value) = value_string(value) {
                    record.title = value;
                }
            }
            "summary" => {
                if let Some(value) = value_string(value) {
                    record.summary = value;
                }
            }
            "shareable" => record.shareable = value.as_bool() == Some(true),
            "tags" => record.tags = value_list(value),
            "series" => {
                record.series = value_string(value)
                    .filter(|v| !v.is_empty() && v != "null")
                    .or(record.series.take());
            }
            "sensitivity" => {
                if let Some(value) = value_string(value).filter(|v| !v.is_empty()) {
                    record.sensitivity = value;
                }
            }
            "template" => {
                record.template = value_string(value).filter(|v| !v.is_empty() && v != "null");
            }
            "occurred_at" => dates[0] = value_string(value).as_deref().and_then(date_prefix),
            "captured_at" => dates[1] = value_string(value).as_deref().and_then(date_prefix),
            "updated_at" => dates[2] = value_string(value).as_deref().and_then(date_prefix),
            "last_verified" => dates[3] = value_string(value).as_deref().and_then(date_prefix),
            "cites" => record.cites = value_list(value),
            "related" => record.related = value_list(value),
            _ => {}
        }
    }
    finish_record(record, relative, dates)
}

/// A parsed frontmatter value as a plain string, if it is a scalar.
fn value_string(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Bool(flag) => Some(flag.to_string()),
        Value::Number(number) => Some(number.to_string()),
        Value::Null | Value::Array(_) | Value::Object(_) => None,
    }
}

/// A parsed frontmatter value as a list of plain strings.
fn value_list(value: &Value) -> Vec<String> {
    match value {
        Value::Array(items) => items.iter().filter_map(value_string).collect(),
        Value::String(text) if !text.is_empty() => vec![text.clone()],
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_) => {
            Vec::new()
        }
    }
}

/// The lenient line parser for documents outside the supported subset.
fn parse_document_lenient(text: &str, relative: &Path) -> DocumentRecord {
    let (frontmatter, body) = split_frontmatter(text);
    let kind = kind_for_path(relative);

    let mut record = DocumentRecord {
        layer: LibraryLayer::Own,
        shared_by: None,
        id: String::new(),
        kind,
        title: String::new(),
        summary: String::new(),
        tags: Vec::new(),
        series: None,
        sensitivity: "normal".to_string(),
        shareable: false,
        path: relative.to_string_lossy().replace('\\', "/"),
        body: body.trim().to_string(),
        cites: Vec::new(),
        related: Vec::new(),
        template: None,
        date: None,
    };
    // Date candidates in precedence order (see `DocumentRecord::date`).
    let mut dates: [Option<String>; 4] = [None, None, None, None];

    let mut list_key: Option<&str> = None;
    for line in frontmatter.lines() {
        let trimmed = line.trim();
        if let Some(item) = trimmed.strip_prefix("- ") {
            let item = scalar(item.trim());
            if !item.is_empty() {
                match list_key {
                    Some("cites") => record.cites.push(item),
                    Some("related") => record.related.push(item),
                    Some("tags") => record.tags.push(item),
                    _ => {}
                }
            }
            continue;
        }
        let Some((key, value)) = trimmed.split_once(':') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        list_key = None;
        match key {
            "id" => record.id = scalar(value),
            "shareable" => record.shareable = scalar(value) == "true",
            "title" => record.title = scalar(value),
            "summary" => record.summary = scalar(value),
            "tags" => {
                record.tags = inline_list(value);
                list_key = Some("tags");
            }
            "series" => {
                let value = scalar(value);
                if !value.is_empty() && value != "null" {
                    record.series = Some(value);
                }
            }
            "sensitivity" => {
                let value = scalar(value);
                if !value.is_empty() {
                    record.sensitivity = value;
                }
            }
            "template" => {
                record.template = Some(scalar(value)).filter(|v| !v.is_empty() && v != "null");
            }
            "occurred_at" => dates[0] = date_prefix(&scalar(value)),
            "captured_at" => dates[1] = date_prefix(&scalar(value)),
            "updated_at" => dates[2] = date_prefix(&scalar(value)),
            "last_verified" => dates[3] = date_prefix(&scalar(value)),
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

    finish_record(record, relative, dates)
}

/// Fill in the id/title fallbacks and the document date.
fn finish_record(
    mut record: DocumentRecord,
    relative: &Path,
    dates: [Option<String>; 4],
) -> DocumentRecord {
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
    record.date = dates
        .into_iter()
        .flatten()
        .next()
        .or_else(|| id_date(&record.id));
    record
}

/// The leading `YYYY-MM-DD` of a date or timestamp, if it has one.
fn date_prefix(value: &str) -> Option<String> {
    let head = value.get(..10)?;
    let bytes = head.as_bytes();
    let shaped = bytes.iter().enumerate().all(|(i, b)| match i {
        4 | 7 => *b == b'-',
        _ => b.is_ascii_digit(),
    });
    shaped.then(|| head.to_string())
}

/// The date in a dated id (`src-20261028-…`, `art-20261029-…`).
fn id_date(id: &str) -> Option<String> {
    let digits = id
        .strip_prefix("src-")
        .or_else(|| id.strip_prefix("art-"))?
        .get(..8)?;
    if !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    date_prefix(&format!(
        "{}-{}-{}",
        &digits[..4],
        &digits[4..6],
        &digits[6..]
    ))
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

/// `[a, b]` or a bare scalar, as a list of decoded items. Commas inside
/// quoted items (or nested flow collections) do not split the list.
fn inline_list(value: &str) -> Vec<String> {
    let uncommented = strip_comment(value);
    let trimmed = uncommented.trim();
    let inner = trimmed
        .strip_prefix('[')
        .and_then(|v| v.strip_suffix(']'))
        .unwrap_or(trimmed);
    if inner.trim().is_empty() {
        return Vec::new();
    }
    split_list_items(inner)
        .into_iter()
        .map(scalar)
        .filter(|item| !item.is_empty())
        .collect()
}

/// Split a flow list's contents on top-level commas only.
fn split_list_items(inner: &str) -> Vec<&str> {
    let mut items = Vec::new();
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut depth: usize = 0;
    let mut start = 0;
    let mut chars = inner.char_indices().peekable();
    while let Some((index, c)) = chars.next() {
        if escaped {
            escaped = false;
            continue;
        }
        match quote {
            Some('"') if c == '\\' => escaped = true,
            Some('\'') if c == '\'' => {
                if chars.peek().is_some_and(|(_, next)| *next == '\'') {
                    chars.next();
                } else {
                    quote = None;
                }
            }
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None => match c {
                '"' | '\'' if opens_quote(inner, index) => quote = Some(c),
                '[' | '{' => depth += 1,
                ']' | '}' => depth = depth.saturating_sub(1),
                ',' if depth == 0 => {
                    items.push(&inner[start..index]);
                    start = index + 1;
                }
                _ => {}
            },
        }
    }
    items.push(&inner[start..]);
    items
}

/// A scalar value with any trailing comment removed and, when quoted,
/// its quotes removed and escapes decoded (double-quoted `\"`, `\\`, …;
/// single-quoted `''` for an apostrophe).
fn scalar(value: &str) -> String {
    let trimmed = strip_comment(value).trim();
    decode_quoted(trimmed).unwrap_or_else(|| trimmed.to_string())
}

/// Whether a quote character at `index` opens a quoted string: at the
/// start, or after `:`, `[`, `{`, `,` or `-` (so an apostrophe inside a
/// plain scalar such as `Acme's` opens nothing).
fn opens_quote(text: &str, index: usize) -> bool {
    text[..index]
        .trim_end()
        .chars()
        .next_back()
        .is_none_or(|p| [':', '[', '{', ',', '-'].contains(&p))
}

/// Drop a `#` comment (at the start, or after whitespace) outside quotes.
fn strip_comment(line: &str) -> &str {
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut previous: Option<char> = None;
    let mut chars = line.char_indices().peekable();
    while let Some((index, c)) = chars.next() {
        if escaped {
            escaped = false;
            previous = Some(c);
            continue;
        }
        match quote {
            Some('"') if c == '\\' => escaped = true,
            Some('\'') if c == '\'' => {
                if chars.peek().is_some_and(|(_, next)| *next == '\'') {
                    chars.next();
                } else {
                    quote = None;
                }
            }
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None => {
                if c == '#' && previous.is_none_or(char::is_whitespace) {
                    return &line[..index];
                }
                if (c == '"' || c == '\'') && opens_quote(line, index) {
                    quote = Some(c);
                }
            }
        }
        previous = Some(c);
    }
    line
}

/// Decode a value that is exactly one quoted string, if it is one.
/// Malformed quoting falls back to stripping a matching outer pair.
fn decode_quoted(text: &str) -> Option<String> {
    let quote = text.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let mut escaped = false;
    let mut chars = text.char_indices().peekable();
    chars.next();
    while let Some((index, c)) = chars.next() {
        if escaped {
            escaped = false;
            continue;
        }
        match quote {
            '"' if c == '\\' => escaped = true,
            '\'' if c == '\'' => {
                if chars.peek().is_some_and(|(_, next)| *next == '\'') {
                    chars.next();
                } else {
                    return Some(decode_quoted_inner(quote, &text[1..index]));
                }
            }
            q if c == q => return Some(decode_quoted_inner(quote, &text[1..index])),
            _ => {}
        }
    }
    // Unterminated: strip a lone opening quote only when the rest has no
    // closing pair to preserve; otherwise keep the text as written.
    text.strip_prefix(quote)
        .filter(|rest| !rest.contains(quote))
        .map(str::to_string)
}

fn decode_quoted_inner(quote: char, inner: &str) -> String {
    match quote {
        '"' => serde_json::from_str::<String>(&format!("\"{inner}\""))
            .unwrap_or_else(|_| inner.to_string()),
        _ => inner.replace("''", "'"),
    }
}
