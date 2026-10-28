//! `office.attach`: Pick the knowledge, source and example artifact files
//! for a task and copy them into it as task artifacts under `context/`
//! (specs/library.md, "Attaching to a task"; knowledge-templates.md,
//! "Attach policies").
//!
//! Selection fills at most [`LIMIT`] slots, in this order: pages by their
//! template's attach policy (`always`, `participants`, `tagged`,
//! `matched`), runbooks the task names, the task's sources, example
//! artifacts in the task's series, index search hits over the task's title
//! and tags, then pages `related` to what was picked. Every knowledge page
//! is copied as its model view, so no `contact` ref value leaves the
//! machine. `private` documents are withheld. Every read happens before the
//! first copy, and the task is read first, so an unknown task copies
//! nothing. Each copy is staged through the Library port, put with
//! `orbit.task.artifact.put` under `context/<library path>`, and its
//! staging file removed. A manifest artifact records the Library commit
//! the copies were read at.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use office_core::domain::knowledge::{
    DocumentKind, DocumentRecord, Page, PageStatus, SearchFilter, page_kind_for_id,
};
use office_core::domain::source::source_body;
use office_core::domain::template::{AttachPolicy, Template};
use office_core::tool::{ExecutionKind, McpScope, ToolSpec};
use office_core::{Error, Result};
use serde_json::{Map, Value, json};

use crate::registry::{Call, Tool};

pub(crate) const TOOL: Tool = Tool::built(
    ToolSpec {
        name: "attach",
        execution_kind: ExecutionKind::Mutating,
        mcp_scope: McpScope::Workspace,
    },
    &["task", "text", "tags", "dry_run"],
    run,
);

/// The most files one call attaches under `context/`.
pub(crate) const LIMIT: usize = 8;
/// The artifact path prefix for every copy.
const CONTEXT_DIR: &str = "context";
/// The artifact that records what was attached and the Library commit read.
const MANIFEST: &str = "office-attach.json";
/// Hits read per index search (title, or one tag).
const SEARCH_LIMIT: usize = 20;

/// Why a file was picked, in selection order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reason {
    Always,
    Participants,
    Tagged,
    Matched,
    Runbook,
    Source,
    Example,
    Search,
    Related,
}

impl Reason {
    fn as_str(self) -> &'static str {
        match self {
            Self::Always => "always",
            Self::Participants => "participants",
            Self::Tagged => "tagged",
            Self::Matched => "matched",
            Self::Runbook => "runbook",
            Self::Source => "source",
            Self::Example => "example",
            Self::Search => "search",
            Self::Related => "related",
        }
    }
}

/// What the task asks for: its text, tags and the sources it came from.
struct Request {
    task: Option<String>,
    title: String,
    text: String,
    tags: Vec<String>,
    dry_run: bool,
}

/// A knowledge page as read, with the attach policy of its template.
struct KnownPage {
    path: String,
    page: Page,
    policy: Option<AttachPolicy>,
}

/// One chosen file and the text its copy carries.
struct Pick {
    id: String,
    kind: DocumentKind,
    path: String,
    layer: office_core::domain::knowledge::LibraryLayer,
    reason: Reason,
    text: String,
}

/// A candidate that was not attached, and why.
struct Omitted {
    id: String,
    reason: &'static str,
}

/// Selection state: the picks so far, and what was left out.
struct Selection<'a> {
    library: &'a dyn office_core::ports::Library,
    pages: &'a BTreeMap<String, KnownPage>,
    picks: Vec<Pick>,
    seen: BTreeSet<String>,
    omitted: Vec<Omitted>,
}

impl Selection<'_> {
    /// Add a page (as its model view) unless it is already picked, private
    /// or over the limit.
    fn page(&mut self, id: &str, reason: Reason) {
        let Some(known) = self.pages.get(id) else {
            return;
        };
        if !self.admit(id, known.page.sensitivity.as_str() == "private") {
            return;
        }
        let mut view = known.page.model_view();
        view.path.clone_from(&known.path);
        self.picks.push(Pick {
            id: id.to_string(),
            kind: known.page.kind,
            path: known.path.clone(),
            layer: if known.path.starts_with("shared/") {
                office_core::domain::knowledge::LibraryLayer::Shared
            } else {
                office_core::domain::knowledge::LibraryLayer::Own
            },
            reason,
            text: view.to_markdown(),
        });
    }

    /// Add an indexed source or artifact as its file's Markdown.
    fn document(&mut self, record: &DocumentRecord, reason: Reason) -> Result<()> {
        if matches!(record.kind, DocumentKind::Domain | DocumentKind::Runbook) {
            self.page(&record.id, reason);
            return Ok(());
        }
        if !self.admit(&record.id, record.sensitivity == "private") {
            return Ok(());
        }
        let text = self.library.read_text(Path::new(&record.path))?;
        self.picks.push(Pick {
            id: record.id.clone(),
            kind: record.kind,
            path: record.path.clone(),
            layer: record.layer,
            reason,
            text,
        });
        Ok(())
    }

    /// Whether `id` takes a slot. A repeat is skipped silently; a private
    /// document or one past the limit is recorded once as omitted.
    fn admit(&mut self, id: &str, private: bool) -> bool {
        if !self.seen.insert(id.to_string()) {
            return false;
        }
        let reason = if private {
            "private"
        } else if self.picks.len() >= LIMIT {
            "limit"
        } else {
            return true;
        };
        self.omitted.push(Omitted {
            id: id.to_string(),
            reason,
        });
        false
    }

    /// Pages whose template has `policy`, that are in use, and that `admit`
    /// accepts, in id order.
    fn by_policy(&mut self, policy: AttachPolicy, reason: Reason, admit: impl Fn(&Page) -> bool) {
        let ids: Vec<String> = self
            .pages
            .iter()
            .filter(|(_, known)| known.policy == Some(policy))
            .filter(|(_, known)| known.page.status != PageStatus::Retired)
            .filter(|(_, known)| admit(&known.page))
            .map(|(id, _)| id.clone())
            .collect();
        for id in ids {
            self.page(&id, reason);
        }
    }

    /// Index hits for a query and filter, resolved to documents. A page
    /// whose template attaches `never`, or a retired page, is not a hit.
    fn search(&mut self, query: &str, filter: &SearchFilter) -> Result<()> {
        for hit in self.library.search(query, filter)? {
            if let Some(known) = self.pages.get(&hit.id) {
                let never = known.policy == Some(AttachPolicy::Never)
                    && known.page.kind == DocumentKind::Domain;
                if !never && known.page.status != PageStatus::Retired {
                    self.page(&hit.id, Reason::Search);
                }
                continue;
            }
            if matches!(hit.kind, DocumentKind::Source | DocumentKind::Artifact)
                && let Some(record) = self.library.get(&hit.id)?
            {
                self.document(&record, Reason::Search)?;
            }
        }
        Ok(())
    }
}

fn run(call: &Call<'_>) -> Result<Value> {
    let request = request(call)?;
    let library = call.ports.library.as_ref();
    let commit = library.head_commit()?;

    let policies = template_policies(library)?;
    let mut skipped = Vec::new();
    let mut pages = BTreeMap::new();
    for file in library.knowledge_pages()? {
        let parsed = file
            .markdown
            .as_deref()
            .map(Page::from_markdown)
            .transpose();
        match parsed {
            Ok(Some(page)) => {
                let policy = page
                    .template
                    .as_ref()
                    .and_then(|name| policies.get(name).copied());
                pages.entry(page.id.clone()).or_insert(KnownPage {
                    path: file.path,
                    page,
                    policy,
                });
            }
            Ok(None) => skipped.push(json!({ "path": file.path, "code": "unreadable" })),
            Err(error) => skipped.push(json!({ "path": file.path, "code": error.code() })),
        }
    }

    // The task's sources: their participants and text drive two policies.
    let mut sources = Vec::new();
    for id in tag_values(&request.tags, "source") {
        if let Some(record) = library
            .get(&id)?
            .filter(|record| record.kind == DocumentKind::Source)
        {
            let markdown = library.read_text(Path::new(&record.path))?;
            sources.push((record, markdown));
        }
    }
    let participants: Vec<String> = sources
        .iter()
        .flat_map(|(_, markdown)| frontmatter_list(markdown, "participants"))
        .map(|name| person_name(&name))
        .filter(|name| !name.is_empty())
        .collect();
    let mut haystack = request.text.clone();
    for (_, markdown) in &sources {
        haystack.push('\n');
        haystack.push_str(&source_body(markdown));
    }
    let haystack = haystack.to_lowercase();

    let mut selection = Selection {
        library,
        pages: &pages,
        picks: Vec::new(),
        seen: BTreeSet::new(),
        omitted: Vec::new(),
    };
    selection.by_policy(AttachPolicy::Always, Reason::Always, |page| {
        page.status == PageStatus::Current
    });
    selection.by_policy(AttachPolicy::Participants, Reason::Participants, |page| {
        std::iter::once(&page.title)
            .chain(&page.aliases)
            .map(|name| person_name(name))
            .any(|name| participants.contains(&name))
    });
    selection.by_policy(AttachPolicy::Tagged, Reason::Tagged, |page| {
        page.tags.iter().any(|tag| request.tags.contains(tag))
    });
    selection.by_policy(AttachPolicy::Matched, Reason::Matched, |page| {
        glossary_terms(&page.body).any(|term| contains_word(&haystack, &term))
    });
    for id in named_runbooks(&request) {
        if pages
            .get(&id)
            .is_some_and(|known| known.page.status != PageStatus::Retired)
        {
            selection.page(&id, Reason::Runbook);
        }
    }
    for (record, _) in &sources {
        selection.document(record, Reason::Source)?;
    }
    for series in tag_values(&request.tags, "series") {
        let mut examples = library.search(
            "",
            &SearchFilter {
                kind: Some(DocumentKind::Artifact),
                series: Some(series),
                limit: Some(SEARCH_LIMIT),
                ..SearchFilter::default()
            },
        )?;
        // Artifact ids are dated, so the newest version sorts last.
        examples.sort_by(|left, right| right.id.cmp(&left.id));
        for hit in examples {
            if let Some(record) = library.get(&hit.id)? {
                selection.document(&record, Reason::Example)?;
            }
        }
    }
    if !request.title.trim().is_empty() {
        selection.search(
            &request.title,
            &SearchFilter {
                limit: Some(SEARCH_LIMIT),
                ..SearchFilter::default()
            },
        )?;
    }
    for tag in &request.tags {
        selection.search(
            "",
            &SearchFilter {
                tag: Some(tag.clone()),
                limit: Some(SEARCH_LIMIT),
                ..SearchFilter::default()
            },
        )?;
    }
    let mut related = Vec::new();
    for pick in &selection.picks {
        if let Some(known) = pages.get(&pick.id) {
            related.extend(known.page.related.iter().cloned());
        }
    }
    for id in related {
        if pages.get(&id).is_some_and(|known| {
            known.page.status != PageStatus::Retired
                && !(known.policy == Some(AttachPolicy::Never)
                    && known.page.kind == DocumentKind::Domain)
        }) {
            selection.page(&id, Reason::Related);
        }
    }

    // Every file was read at one commit, or the call refuses before copying.
    if library.head_commit()? != commit {
        return Err(Error::Library {
            code: "library_changed",
            message: "the Library changed while attach read it; nothing was copied".into(),
        });
    }

    let Selection { picks, omitted, .. } = selection;
    let attached: Vec<Value> = picks
        .iter()
        .map(|pick| {
            json!({
                "id": pick.id,
                "layer": pick.layer,
                "kind": pick.kind,
                "library_path": pick.path,
                "artifact_path": format!("{CONTEXT_DIR}/{}", pick.path),
                "reason": pick.reason.as_str(),
            })
        })
        .collect();
    let omitted: Vec<Value> = omitted
        .iter()
        .map(|omitted| json!({ "id": omitted.id, "reason": omitted.reason }))
        .collect();
    let mut output = json!({
        "task": request.task,
        "dry_run": request.dry_run,
        "library_commit": commit,
        "attached": attached,
        "omitted": omitted,
        "skipped": skipped,
        "manifest": Value::Null,
    });

    let Some(task) = request.task.as_deref().filter(|_| !request.dry_run) else {
        return Ok(output);
    };
    for pick in &picks {
        copy(
            call,
            task,
            &pick.path,
            &format!("{CONTEXT_DIR}/{}", pick.path),
            &pick.text,
        )?;
    }
    let manifest = json!({
        "task": task,
        "library_commit": commit,
        "attached": output["attached"],
    });
    copy(call, task, MANIFEST, MANIFEST, &format!("{manifest:#}\n"))?;
    output["manifest"] = json!(MANIFEST);
    Ok(output)
}

/// Stage `text`, put it on the task at `artifact`, then remove the staging
/// file whether or not the put succeeded.
fn copy(call: &Call<'_>, task: &str, name: &str, artifact: &str, text: &str) -> Result<()> {
    let library = &call.ports.library;
    let staged = library.stage_attachment(name, text)?;
    let mut input = Map::new();
    input.insert("id".into(), json!(task));
    input.insert("source_path".into(), json!(staged));
    input.insert("path".into(), json!(artifact));
    let put = call.ports.orbit.task_artifact_put(&input);
    let removed = library.remove_staged(&staged);
    put?;
    removed
}

/// Validate input and read the task, refusing an unknown task first.
fn request(call: &Call<'_>) -> Result<Request> {
    let input = call.input;
    let task = match input.get("task") {
        None | Some(Value::Null) => None,
        Some(Value::String(id)) if !id.trim().is_empty() => Some(id.trim().to_string()),
        Some(_) => {
            return Err(Error::InvalidArgument(
                "attach task must be an Orbit task id".into(),
            ));
        }
    };
    let extra_text = match input.get("text") {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(text)) => text.clone(),
        Some(_) => return Err(Error::InvalidArgument("attach text must be text".into())),
    };
    let mut tags = strings(input.get("tags"))
        .ok_or_else(|| Error::InvalidArgument("attach tags must be a list of tags".into()))?;
    let dry_run = match input.get("dry_run") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(flag)) => *flag,
        Some(_) => {
            return Err(Error::InvalidArgument(
                "attach dry_run must be true or false".into(),
            ));
        }
    };
    let Some(id) = task else {
        if !dry_run || (extra_text.trim().is_empty() && tags.is_empty()) {
            return Err(Error::InvalidArgument(
                "attach needs a task, or dry_run with text or tags".into(),
            ));
        }
        return Ok(Request {
            task: None,
            title: extra_text.clone(),
            text: extra_text,
            tags,
            dry_run,
        });
    };

    let mut show = Map::new();
    show.insert("id".into(), json!(id));
    let shown = match call.ports.orbit.task_show(&show) {
        Ok(value) => value,
        Err(Error::OrbitRefused { code }) if code == "task_not_found" => {
            return Err(Error::TaskNotFound { id });
        }
        Err(error) => return Err(error),
    };
    let record = shown.get("task").unwrap_or(&shown);
    let field = |key: &str| record.get(key).and_then(Value::as_str).unwrap_or_default();
    let title = field("title").to_string();
    let text = format!("{title}\n{}\n{extra_text}", field("description"));
    for tag in strings(record.get("tags")).unwrap_or_default() {
        if !tags.contains(&tag) {
            tags.push(tag);
        }
    }
    Ok(Request {
        task: Some(id),
        title,
        text,
        tags,
        dry_run,
    })
}

/// A JSON list of nonempty strings (absent or null is empty).
fn strings(value: Option<&Value>) -> Option<Vec<String>> {
    match value {
        None | Some(Value::Null) => Some(Vec::new()),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| {
                item.as_str()
                    .map(str::trim)
                    .filter(|tag| !tag.is_empty())
                    .map(str::to_string)
            })
            .collect(),
        Some(_) => None,
    }
}

/// Each template's attach policy, by template name. A template file that
/// cannot be read or does not parse is refused, never skipped.
fn template_policies(
    library: &dyn office_core::ports::Library,
) -> Result<BTreeMap<String, AttachPolicy>> {
    let mut policies = BTreeMap::new();
    for file in library.templates()? {
        let markdown = file
            .markdown
            .as_deref()
            .ok_or_else(|| Error::TemplateInvalid {
                file: file.path.clone(),
                field: "file".into(),
                reason: "could not be read",
            })?;
        let template = Template::from_markdown(&file.path, markdown)?;
        policies.entry(template.template).or_insert(template.attach);
    }
    Ok(policies)
}

/// Values of `<prefix>:<value>` tags, in order.
fn tag_values(tags: &[String], prefix: &str) -> Vec<String> {
    tags.iter()
        .filter_map(|tag| tag.strip_prefix(prefix)?.strip_prefix(':'))
        .map(str::to_string)
        .filter(|value| !value.is_empty())
        .collect()
}

/// Runbook ids the task names: `runbook:<id>` or bare `rb-…` tags, and
/// `rb-…` ids in its text.
fn named_runbooks(request: &Request) -> Vec<String> {
    let mut ids: Vec<String> = tag_values(&request.tags, "runbook");
    ids.extend(request.tags.iter().cloned());
    ids.extend(
        request
            .text
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
            .map(|word| word.trim_end_matches('-').to_string()),
    );
    let mut seen = BTreeSet::new();
    ids.into_iter()
        .filter(|id| page_kind_for_id(id) == Some(DocumentKind::Runbook))
        .filter(|id| seen.insert(id.clone()))
        .collect()
}

/// A participant or page name for comparison: lowercase, without a
/// trailing `(Organization)`.
fn person_name(name: &str) -> String {
    let name = name.trim();
    let name = match name.rfind('(') {
        Some(open) if name.ends_with(')') => &name[..open],
        _ => name,
    };
    name.trim().to_lowercase()
}

/// A frontmatter list field (inline `[a, b]` or an indented block list).
fn frontmatter_list(markdown: &str, key: &str) -> Vec<String> {
    let Some(rest) = markdown
        .strip_prefix("---\n")
        .or_else(|| markdown.strip_prefix("---\r\n"))
    else {
        return Vec::new();
    };
    let Some(end) = rest.find("\n---") else {
        return Vec::new();
    };
    let unquote = |item: &str| {
        let item = item.trim();
        item.strip_prefix('"')
            .and_then(|i| i.strip_suffix('"'))
            .unwrap_or(item)
            .to_string()
    };
    let mut lines = rest[..end].lines();
    while let Some(line) = lines.next() {
        let Some(value) = line
            .strip_prefix(key)
            .and_then(|after| after.strip_prefix(':'))
        else {
            continue;
        };
        let value = value.trim();
        if let Some(inner) = value.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
            return inner
                .split(',')
                .map(unquote)
                .filter(|item| !item.is_empty())
                .collect();
        }
        if !value.is_empty() {
            return vec![unquote(value)];
        }
        return lines
            .map(str::trim)
            .map_while(|line| line.strip_prefix("- "))
            .map(unquote)
            .collect();
    }
    Vec::new()
}

/// Glossary terms: one `### Term` heading each, lowercased.
fn glossary_terms(body: &str) -> impl Iterator<Item = String> + '_ {
    body.lines()
        .filter_map(|line| line.trim().strip_prefix("### "))
        .map(|term| term.trim().to_lowercase())
        .filter(|term| !term.is_empty())
}

/// Whether lowercase `haystack` contains `term` as a whole word.
fn contains_word(haystack: &str, term: &str) -> bool {
    let boundary = |c: Option<char>| c.is_none_or(|c| !c.is_alphanumeric());
    haystack.match_indices(term).any(|(at, _)| {
        boundary(haystack[..at].chars().next_back())
            && boundary(haystack[at + term.len()..].chars().next())
    })
}
