//! Session context, deterministic stored refs, and read-only recall selection.
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use super::{Session, invalid, scrub};
use crate::domain::source::{parse_source_meta, source_body};
use crate::domain::{knowledge::frontmatter::flow_value, metrics::metadata, session};
use crate::ports::LibraryText;
use crate::{Error, Result};

/// A provider resume target. Values are data, never executed by Office.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Resume {
    /// Provider token.
    pub provider: String,
    /// Command displayed to the user, never interpreted by a shell here.
    pub command: String,
    /// Provider link, when the hook can read one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// Optional hook context and a proposed or user-authored session title.
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Context {
    /// Working directory at session start; transcript cwd takes precedence.
    pub project: Option<String>,
    /// Git branch at session start; transcript gitBranch takes precedence.
    pub branch: Option<String>,
    /// Provider command and optional deep link.
    pub resume: Option<Resume>,
    /// Known GitHub owner/repo from the hook, for unqualified PR numbers.
    pub repository: Option<String>,
    /// Proposed title, at most eight words for a scribe.
    pub title: Option<String>,
    /// scribe (default) or user; private sessions ignore scribe proposals.
    pub title_by: Option<String>,
}

fn single_line(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}

/// Validate nullable context on both legacy and new sources.
pub fn validate_context(fields: &Map<String, Value>) -> Result<()> {
    for key in ["project", "repository", "title", "branch"] {
        if let Some(value) = fields.get(key).filter(|v| !v.is_null())
            && !value
                .as_str()
                .is_some_and(|s| single_line(s, if key == "branch" { 256 } else { 4096 }))
        {
            return Err(invalid("session context needs bounded single-line strings"));
        }
    }
    if let Some(value) = fields.get("title_by").filter(|v| !v.is_null())
        && !matches!(value.as_str(), Some("scribe" | "user"))
    {
        return Err(invalid("title_by must be scribe, user or null"));
    }
    for key in ["started_at", "last_active_at"] {
        if let Some(value) = fields.get(key).filter(|v| !v.is_null()) {
            date(
                value
                    .as_str()
                    .ok_or_else(|| invalid("invalid session time"))?,
            )?;
        }
    }
    if let Some(value) = fields.get("resume").filter(|v| !v.is_null()) {
        let resume: Resume = serde_json::from_value(value.clone())
            .map_err(|_| invalid("invalid session resume target"))?;
        if !single_line(&resume.provider, 64)
            || !single_line(&resume.command, 4096)
            || resume.url.as_ref().is_some_and(|v| !single_line(v, 4096))
        {
            return Err(invalid("invalid session resume target"));
        }
    }
    if let Some(value) = fields.get("repository").filter(|v| !v.is_null())
        && !value.as_str().is_some_and(valid_repository)
    {
        return Err(invalid("repository must be owner/repo"));
    }
    Ok(())
}

fn render(fields: &Map<String, Value>, body: &str) -> String {
    let mut out = String::from("---\n");
    for (key, value) in fields {
        out.push_str(&format!("{key}: {}\n", flow_value(value)));
    }
    out.push_str("---\n");
    out.push_str(body);
    out
}

/// Apply start context once, advance activity, and preserve user-owned titles.
/// Even an unchanged transcript can carry a new title or missing hook context.
pub fn with_context(markdown: &str, session: &Session, context: &Context) -> Result<String> {
    let proposed = serde_json::to_value(context).map_err(|_| invalid("invalid session context"))?;
    let proposed = proposed
        .as_object()
        .ok_or_else(|| invalid("invalid session context"))?;
    validate_context(proposed)?;
    if context.title_by.is_some() && context.title.is_none() {
        return Err(invalid("title_by requires title"));
    }
    if context.title_by.as_deref() != Some("user")
        && context
            .title
            .as_ref()
            .is_some_and(|s| s.split_whitespace().count() > 8)
    {
        return Err(invalid("a scribe title has at most eight words"));
    }
    // Hook fields, like transcript evidence, cannot become a credential channel.
    fn contains_credentials(value: &Value) -> bool {
        match value {
            Value::String(text) => scrub(text) != *text,
            Value::Array(items) => items.iter().any(contains_credentials),
            Value::Object(fields) => fields.values().any(contains_credentials),
            Value::Null | Value::Bool(_) | Value::Number(_) => false,
        }
    }
    if proposed.values().any(contains_credentials) {
        return Err(invalid("session context contains credentials"));
    }
    let mut fields = metadata(markdown)?;
    let project = session.project.as_ref().or(context.project.as_ref());
    let branch = session.branch.as_ref().or(context.branch.as_ref());
    let last = session.last_active_at.as_deref();
    let resume = context.resume.clone().unwrap_or(Resume {
        provider: session.orchestrator.clone(),
        command: format!(
            "claude --resume {}",
            if session
                .session_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
            {
                session.session_id.clone()
            } else {
                format!("'{}'", session.session_id.replace('\'', "'\"'\"'"))
            }
        ),
        url: None,
    });
    for (key, value) in [
        ("project", json!(project)),
        ("branch", json!(branch)),
        ("started_at", json!(session.started_at)),
        ("resume", json!(resume)),
        ("repository", json!(context.repository)),
    ] {
        if fields.get(key).is_none_or(Value::is_null) {
            fields.insert(key.into(), value);
        }
    }
    if let Some(resume) = &context.resume {
        fields.insert("resume".into(), json!(resume));
    }
    if let Some(last) = last {
        let old = fields
            .get("last_active_at")
            .and_then(Value::as_str)
            .map(date)
            .transpose()?;
        if old.is_none_or(|old| date(last).is_ok_and(|new| new > old)) {
            fields.insert("last_active_at".into(), json!(last));
        }
    }
    let user_title = context.title_by.as_deref() == Some("user");
    if user_title || fields.get("title_by").and_then(Value::as_str) != Some("user") {
        let title = if user_title || session.sensitivity != "private" {
            context.title.clone()
        } else {
            None
        };
        let title = title.or_else(|| {
            // No crew proposal: use the first user line, particularly on the
            // private route, where the crew never receives transcript text.
            fields
                .get("title_by")
                .is_none_or(Value::is_null)
                .then(|| {
                    session.lines.iter().find(|l| l.role == "user").map(|l| {
                        l.text
                            .split_whitespace()
                            .take(8)
                            .collect::<Vec<_>>()
                            .join(" ")
                    })
                })
                .flatten()
        });
        if let Some(title) = title {
            fields.insert("title".into(), json!(title));
            fields.insert(
                "title_by".into(),
                json!(if user_title { "user" } else { "scribe" }),
            );
        }
    }
    fields.entry("title_by").or_insert(Value::Null);
    validate_context(&fields)?;
    with_refs(&render(&fields, &source_body(markdown)))
}

/// Canonical reference identity. Status is deliberately absent from storage.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    /// task, pr, or commit.
    pub kind: String,
    /// Task id, owner/repo#number, or lowercase commit hash.
    pub id: String,
}

fn valid_repository(value: &str) -> bool {
    let Some((owner, repo)) = value.split_once('/') else {
        return false;
    };
    let valid = |s: &str| {
        !s.is_empty()
            && s != "."
            && s != ".."
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
    };
    valid(owner) && valid(repo)
}
fn number(value: &str) -> Option<u64> {
    (!value.is_empty() && value.bytes().all(|c| c.is_ascii_digit()))
        .then(|| value.parse::<u64>().ok())
        .flatten()
        .filter(|n| *n > 0)
}
fn task_id(value: &str) -> bool {
    value.split_once('-').is_some_and(|(prefix, n)| {
        !prefix.is_empty()
            && prefix.starts_with(|c: char| c.is_ascii_uppercase())
            && prefix
                .bytes()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
            && number(n).is_some()
    })
}
fn commit(value: &str) -> bool {
    (7..=64).contains(&value.len()) && value.bytes().all(|c| c.is_ascii_hexdigit())
}
fn pr(value: &str) -> Option<(String, u64)> {
    if let Some((repo, n)) = value.split_once('#')
        && valid_repository(repo)
        && let Some(n) = number(n)
    {
        return Some((repo.into(), n));
    }
    let url = url::Url::parse(value).ok()?;
    if url.scheme() != "https"
        || url.host_str() != Some("github.com")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return None;
    }
    let parts: Vec<_> = url.path_segments()?.collect();
    if parts.len() != 4 || parts[2] != "pull" {
        return None;
    }
    let repo = format!("{}/{}", parts[0], parts[1]);
    valid_repository(&repo)
        .then(|| number(parts[3]).map(|n| (repo, n)))
        .flatten()
}
fn repository(value: &str) -> Option<String> {
    if valid_repository(value) {
        return Some(value.into());
    }
    let url = url::Url::parse(value).ok()?;
    if url.host_str() != Some("github.com") {
        return None;
    }
    let repo = url.path().trim_matches('/').trim_end_matches(".git");
    valid_repository(repo).then(|| repo.into())
}

/// Extract identities from copied text, without hardcoding workspace prefixes.
/// Orbit's registered task router validates task identities at query time.
#[must_use]
pub fn extract(text: &str, kind: &str, known_repository: Option<&str>) -> Vec<Reference> {
    let tokens: Vec<_> = text
        .split(|c: char| !(c.is_ascii_alphanumeric() || "-_.:/#".contains(c)))
        .map(|s| s.trim_end_matches(['.', ':', '/']))
        .filter(|s| !s.is_empty())
        .collect();
    let mut refs = BTreeSet::new();
    let mut repositories = BTreeSet::new();
    for token in &tokens {
        if let Some((repo, n)) = pr(token) {
            refs.insert(Reference {
                kind: "pr".into(),
                id: format!("{repo}#{n}"),
            });
            repositories.insert(repo);
        } else if let Some(repo) = repository(token) {
            repositories.insert(repo);
        }
    }
    // Task ids and hashes have different boundaries from repository/URL tokens.
    for token in text.split(|c: char| !(c.is_ascii_alphanumeric() || "_-".contains(c))) {
        if task_id(token) {
            refs.insert(Reference {
                kind: "task".into(),
                id: token.into(),
            });
        }
        if kind == "action" && commit(token) {
            refs.insert(Reference {
                kind: "commit".into(),
                id: token.to_ascii_lowercase(),
            });
        }
    }
    let repo = if repositories.len() == 1 {
        repositories.first().map(String::as_str)
    } else if repositories.is_empty() {
        known_repository
    } else {
        None
    };
    if let Some(repo) = repo.filter(|repo| valid_repository(repo)) {
        for token in &tokens {
            if let Some(n) = token.strip_prefix('#').and_then(number) {
                refs.insert(Reference {
                    kind: "pr".into(),
                    id: format!("{repo}#{n}"),
                });
            }
        }
    }
    refs.into_iter().collect()
}

fn stored_refs(fields: &Map<String, Value>) -> Result<BTreeMap<String, Vec<Reference>>> {
    let refs: BTreeMap<String, Vec<Reference>> = fields
        .get("entry_refs")
        .filter(|v| !v.is_null())
        .map(|v| serde_json::from_value(v.clone()))
        .transpose()
        .map_err(|_| invalid("invalid stored entry refs"))?
        .unwrap_or_default();
    for (anchor, values) in &refs {
        if anchor.strip_prefix('L').and_then(number).is_none()
            || values.iter().any(|r| match r.kind.as_str() {
                "task" => !task_id(&r.id),
                "pr" => pr(&r.id).is_none(),
                "commit" => !commit(&r.id),
                _ => true,
            })
        {
            return Err(invalid("invalid stored entry ref identity"));
        }
    }
    Ok(refs)
}

/// Index newly appended entries by their body anchor. Existing indexes are
/// retained, so a later repository context cannot reinterpret earlier #n text.
pub fn with_refs(markdown: &str) -> Result<String> {
    if parse_source_meta(markdown).kind != "conversation" {
        return Ok(markdown.into());
    }
    let mut fields = metadata(markdown)?;
    let mut refs = stored_refs(&fields)?;
    let repo = fields
        .get("repository")
        .and_then(Value::as_str)
        .or_else(|| fields.get("project").and_then(Value::as_str));
    let repo = repo.and_then(repository);
    let body = source_body(markdown);
    for entry in session::entries(&body)? {
        refs.entry(format!("L{}", entry.line))
            .or_insert_with(|| extract(&entry.text, &entry.kind, repo.as_deref()));
    }
    fields.insert("entry_refs".into(), json!(refs));
    Ok(render(&fields, &body))
}

/// Parsed and validated query. The tool performs only port reads afterwards.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Query {
    /// Literal case-insensitive terms, all required in text/title/refs.
    pub question: String,
    /// Inclusive RFC 3339 timestamp or UTC date lower bound.
    pub since: Option<String>,
    /// Inclusive RFC 3339 timestamp or UTC date upper bound.
    pub until: Option<String>,
    /// Exact project selector.
    pub project: Option<String>,
    /// Maximum public entries plus private matching entries considered.
    pub limit: Option<usize>,
}
fn date(value: &str) -> Result<chrono::DateTime<chrono::FixedOffset>> {
    chrono::DateTime::parse_from_rfc3339(value).map_err(|_| invalid("invalid recall timestamp"))
}
fn bound(value: &str, end: bool) -> Result<chrono::DateTime<chrono::FixedOffset>> {
    let value = if value.len() == 10 {
        format!(
            "{value}T{}Z",
            if end {
                "23:59:59.999999999"
            } else {
                "00:00:00"
            }
        )
    } else {
        value.into()
    };
    date(&value).map_err(|_| {
        Error::InvalidArgument(
            "recall date filters must be RFC 3339 timestamps or YYYY-MM-DD".into(),
        )
    })
}

/// A recall response before refs receive optional live status.
pub struct Matches {
    /// Public entries only, in descending time order.
    pub entries: Vec<Value>,
    /// One metadata-only item per contributing source.
    pub sessions: Vec<Value>,
    /// Distinct refs from public and private contributions.
    pub refs: BTreeSet<Reference>,
    /// Capture extent and explicit privacy/provider gaps.
    pub coverage: Value,
}

/// Select matches from captured conversation documents; private text is never
/// returned. Stored refs are read, never inferred anew during recall.
pub fn select(files: &[LibraryText], query: &Query) -> Result<Matches> {
    let limit = query.limit.unwrap_or(20);
    if query.question.trim().is_empty() || query.question.len() > 4096 || !(1..=50).contains(&limit)
    {
        return Err(Error::InvalidArgument(
            "recall needs question (1–4096 bytes) and limit (1–50)".into(),
        ));
    }
    let since = query
        .since
        .as_deref()
        .map(|s| bound(s, false))
        .transpose()?;
    let until = query.until.as_deref().map(|s| bound(s, true)).transpose()?;
    if since.zip(until).is_some_and(|(a, b)| a > b) {
        return Err(Error::InvalidArgument(
            "recall since must not be after until".into(),
        ));
    }
    let question = query.question.to_lowercase();
    let terms: Vec<_> = question.split_whitespace().collect();
    let mut candidates = Vec::new();
    let mut sources = BTreeMap::new();
    let mut earliest = None;
    let mut private_count = 0usize;
    let mut source_ids = BTreeSet::new();
    for file in files {
        let markdown = session::markdown(file)?;
        let meta = parse_source_meta(markdown);
        if meta.kind != "conversation" {
            continue;
        }
        super::validate_provenance(markdown)?;
        let fields = metadata(markdown)?;
        let start = fields
            .get("started_at")
            .and_then(Value::as_str)
            .or_else(|| fields.get("occurred_at").and_then(Value::as_str))
            .or_else(|| fields.get("captured_at").and_then(Value::as_str));
        if let Some(start) = start {
            let at = date(start)?;
            if earliest.as_ref().is_none_or(|(old, _)| at < *old) {
                earliest = Some((at, start.to_owned()));
            }
        }
        if !matches!(
            fields.get("sensitivity").and_then(Value::as_str),
            Some("normal" | "private")
        ) {
            return Err(invalid("invalid recall source sensitivity"));
        }
        if !source_ids.insert(meta.id.clone()) {
            return Err(invalid("duplicate recall source identity"));
        }
        let private = meta.sensitivity == "private";
        private_count += usize::from(private);
        if query
            .project
            .as_ref()
            .is_some_and(|p| fields.get("project").and_then(Value::as_str) != Some(p))
        {
            continue;
        }
        let refs = stored_refs(&fields)?;
        for entry in session::entries(&source_body(markdown))? {
            let at = date(&entry.time)?;
            if since.is_some_and(|s| at < s) || until.is_some_and(|u| at > u) {
                continue;
            }
            let anchor = format!("#L{}", entry.line);
            let refs = refs.get(&anchor[1..]).cloned().unwrap_or_default();
            let haystack = format!(
                "{} {} {}",
                meta.title,
                entry.text,
                refs.iter()
                    .map(|r| r.id.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            )
            .to_lowercase();
            if terms.iter().all(|term| haystack.contains(term)) {
                candidates.push((
                    at,
                    meta.id.clone(),
                    entry.line,
                    private,
                    json!({
                        "source_id":meta.id,"anchor":anchor,"kind":entry.kind,"text":entry.text,
                        "time":entry.time,"refs":refs,
                    }),
                    refs,
                ));
            }
        }
        let mut source = json!({"source_id":meta.id,"path":file.path,"title":fields.get("title").cloned().unwrap_or(Value::Null),"content_withheld":private});
        for key in [
            "project",
            "branch",
            "started_at",
            "last_active_at",
            "resume",
            "title_by",
        ] {
            source[key] = fields.get(key).cloned().unwrap_or(Value::Null);
        }
        sources.insert(meta.id, source);
    }
    candidates.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| a.1.cmp(&b.1))
            .then_with(|| b.2.cmp(&a.2))
    });
    candidates.truncate(limit);
    let mut entries = Vec::new();
    let mut sessions = Vec::new();
    let mut seen = BTreeSet::new();
    let mut refs = BTreeSet::new();
    for (_, id, _, private, entry, entry_refs) in candidates {
        if seen.insert(id.clone())
            && let Some(source) = sources.remove(&id)
        {
            sessions.push(source);
        }
        refs.extend(entry_refs);
        if !private {
            entries.push(entry);
        }
    }
    Ok(Matches {
        entries,
        sessions,
        refs,
        coverage: json!({
            "earliest_session_at":earliest.map(|(_, text)| text),
            "private_sessions_skipped":private_count,"providers_not_captured":["codex"],
        }),
    })
}
