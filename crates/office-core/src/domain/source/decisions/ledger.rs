use std::collections::BTreeMap;
use std::path::Path;

use serde_json::Value;

use super::scope::{KIND, LINE_TAG, is_ledger_path, refusal};
use super::{Decision, Scope};
use crate::domain::knowledge::DocumentKind;
use crate::{Error, Result};

/// What a decision `ref` names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EvidenceRef {
    /// `<source-id>#<anchor>`: a span of a Library source.
    Source {
        /// The source id as written.
        id: String,
        /// The anchor, without `#`.
        anchor: String,
    },
    /// `orbit://task/<task-id>#comment-<n>`: the n-th (1-based, oldest
    /// first) comment of an Orbit task.
    TaskComment {
        /// The task id.
        task: String,
        /// 1-based comment number.
        number: usize,
    },
}

impl EvidenceRef {
    /// Parse a ref; anything else is refused naming both forms.
    pub fn parse(reference: &str) -> Result<Self> {
        let unresolved = || {
            refusal(
                "decision_ref_unresolved",
                format!(
                    "decision ref {reference:?} must be <source-id>#<anchor> or orbit://task/<task-id>#comment-<n>"
                ),
            )
        };
        if let Some(rest) = reference.strip_prefix("orbit://task/") {
            let (task, comment) = rest.split_once('#').ok_or_else(unresolved)?;
            let number = comment
                .strip_prefix("comment-")
                .filter(|n| !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()))
                .and_then(|n| n.parse::<usize>().ok())
                .filter(|n| *n >= 1)
                .ok_or_else(unresolved)?;
            if task.is_empty()
                || task.len() > 64
                || !task.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
            {
                return Err(unresolved());
            }
            return Ok(Self::TaskComment {
                task: task.into(),
                number,
            });
        }
        let (id, anchor) = reference.split_once('#').ok_or_else(unresolved)?;
        let citation = crate::domain::knowledge::parse_citation(reference)
            .filter(|c| c.kind == DocumentKind::Source && !c.id.contains('/'))
            .ok_or_else(unresolved)?;
        if citation.anchor.as_deref() != Some(anchor) {
            return Err(unresolved());
        }
        Ok(Self::Source {
            id: id.into(),
            anchor: anchor.into(),
        })
    }

    /// The `source` value an entry citing this ref records: the source id,
    /// or `orbit-task:<task-id>` for a task comment.
    #[must_use]
    pub fn source_id(&self) -> String {
        match self {
            Self::Source { id, .. } => id.clone(),
            Self::TaskComment { task, .. } => format!("orbit-task:{task}"),
        }
    }
}

/// A ledger entry and the body line it occupies.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LedgerEntry {
    /// 1-based body line.
    pub line: usize,
    /// The entry.
    pub decision: Decision,
}

/// A parsed ledger file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ledger {
    /// Library-relative path.
    pub path: String,
    /// The scope.
    pub scope: Scope,
    /// `created_at` from the front matter.
    pub created_at: String,
    /// Migrated source ids mapped to their entry ids.
    pub moved_sources: BTreeMap<String, String>,
    /// Entries in line order.
    pub entries: Vec<LedgerEntry>,
}

/// A ledger whose file is malformed is refused, naming the file.
fn malformed(path: &str, reason: &str) -> Error {
    refusal(
        "decision_ledger_invalid",
        format!("decision ledger {path} is malformed: {reason}"),
    )
}

impl Ledger {
    /// An empty ledger for `scope`.
    #[must_use]
    pub fn new(scope: Scope, created_at: &str) -> Self {
        Self {
            path: scope.path(),
            scope,
            created_at: created_at.into(),
            moved_sources: BTreeMap::new(),
            entries: Vec::new(),
        }
    }

    /// The ledger's id, `decisions-<slug>`.
    #[must_use]
    pub fn id(&self) -> String {
        self.scope.ledger_id()
    }

    /// The ledger's title.
    #[must_use]
    pub fn title(&self) -> String {
        format!("Decisions ({})", self.scope.token())
    }

    /// Parse a ledger file. The path must be a ledger path and agree with
    /// the front matter; every body line must be one entry whose id is the
    /// next in sequence.
    pub fn parse(path: &str, markdown: &str) -> Result<Self> {
        use crate::domain::knowledge::frontmatter;
        if !is_ledger_path(Path::new(path)) {
            return Err(malformed(path, "not a sources/decisions/<scope>.md path"));
        }
        let (header, body) =
            frontmatter::split(markdown).map_err(|_| malformed(path, "no front matter"))?;
        let fields =
            frontmatter::parse(header).map_err(|_| malformed(path, "unreadable front matter"))?;
        let field = |key: &str| fields.iter().find(|(k, _)| k == key).map(|(_, v)| v);
        let text = |key: &str| field(key).and_then(Value::as_str).map(str::to_owned);
        if text("kind").as_deref() != Some(KIND) {
            return Err(malformed(path, "kind is not decisions"));
        }
        let scope = Scope::parse(&text("scope").unwrap_or_default())
            .map_err(|_| malformed(path, "scope is missing or invalid"))?;
        if scope.path() != path || text("id") != Some(scope.ledger_id()) {
            return Err(malformed(path, "id, scope and path disagree"));
        }
        let mut moved_sources = BTreeMap::new();
        match field("moved_sources") {
            None | Some(Value::Null) => {}
            Some(Value::Object(map)) => {
                for (old, new) in map {
                    let new = new
                        .as_str()
                        .ok_or_else(|| malformed(path, "moved_sources values must be entry ids"))?;
                    moved_sources.insert(old.clone(), new.to_owned());
                }
            }
            Some(_) => return Err(malformed(path, "moved_sources must be a mapping")),
        }
        let mut entries = Vec::new();
        for (index, line) in body.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let decision = parse_line(line).ok_or_else(|| {
                malformed(path, &format!("line {} is not a decision entry", index + 1))
            })?;
            if decision.id != scope.entry_id(entries.len() + 1) || decision.scope != scope.token() {
                return Err(malformed(
                    path,
                    &format!("line {} is out of sequence or scope", index + 1),
                ));
            }
            entries.push(LedgerEntry {
                line: index + 1,
                decision,
            });
        }
        Ok(Self {
            path: path.into(),
            scope,
            created_at: text("created_at").unwrap_or_default(),
            moved_sources,
            entries,
        })
    }

    /// The next entry id.
    #[must_use]
    pub fn next_id(&self) -> String {
        self.scope.entry_id(self.entries.len() + 1)
    }

    /// The front matter, rendered.
    #[must_use]
    pub fn frontmatter(&self) -> String {
        let quote = |text: &str| serde_json::to_string(text).unwrap_or_else(|_| "\"\"".into());
        let mut out = String::from("---\n");
        out.push_str(&format!("id: {}\n", self.id()));
        out.push_str(&format!("kind: {KIND}\n"));
        out.push_str(&format!("title: {}\n", quote(&self.title())));
        out.push_str(&format!("scope: {}\n", quote(&self.scope.token())));
        out.push_str(&format!("created_at: {}\n", quote(&self.created_at)));
        out.push_str("sensitivity: normal\n");
        if self.moved_sources.is_empty() {
            out.push_str("moved_sources: {}\n");
        } else {
            out.push_str("moved_sources:\n");
            for (old, new) in &self.moved_sources {
                out.push_str(&format!("  {old}: {new}\n"));
            }
        }
        out.push_str("---\n");
        out
    }

    /// The whole file: front matter, then one line per entry. Entries are
    /// re-rendered canonically, so callers append to the existing body
    /// instead when one exists ([`append_line`]).
    pub fn render(&self) -> Result<String> {
        let mut out = self.frontmatter();
        for entry in &self.entries {
            out.push_str(&entry.decision.line()?);
            out.push('\n');
        }
        Ok(out)
    }

    /// The entry with this id, if any.
    #[must_use]
    pub fn entry(&self, id: &str) -> Option<&LedgerEntry> {
        self.entries.iter().find(|entry| entry.decision.id == id)
    }
}

/// Parse one `- <id> <time> #decision <json>` line.
fn parse_line(line: &str) -> Option<Decision> {
    let rest = line.strip_prefix("- ")?;
    let (id, rest) = rest.split_once(' ')?;
    let (_time, rest) = rest.split_once(' ')?;
    let json = rest.strip_prefix(LINE_TAG)?.strip_prefix(' ')?;
    let decision: Decision = serde_json::from_str(json).ok()?;
    (decision.id == id).then_some(decision)
}

/// `existing` (a ledger file's full text) with one more line, keeping every
/// earlier byte.
#[must_use]
pub fn append_line(existing: &str, line: &str) -> String {
    let mut out = existing.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(line);
    out.push('\n');
    out
}

/// One decision ledger file to write, compare-and-set: `expected` is the
/// text read before planning (`None`: the file must not exist yet).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedFile {
    /// Library-relative path.
    pub path: String,
    /// Full new text.
    pub text: String,
    /// The text planned against.
    pub expected: Option<String>,
}
