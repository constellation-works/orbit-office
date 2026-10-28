//! Pull the team's shared library into the read-only layer, file incoming
//! updates and conflicts as proposals, then push this member's shareable
//! documents as their shared view (specs/shared-library.md).
use crate::registry::{Call, Tool};
use crate::tools::proposals::create;
use office_core::domain::artifact::Artifact;
use office_core::domain::knowledge::{DocumentKind, DocumentRecord, LibraryLayer, Page};
use office_core::domain::proposal::{self, Create, Payload};
use office_core::domain::sharing::sync::{
    self, LAYOUT, LAYOUT_FILE, PathState, SharedPath, SyncState, classify, page_path,
};
use office_core::domain::sharing::{self, SharedSettings, SharingError};
use office_core::domain::source::{format_rfc3339_utc, sha256_hex};
use office_core::ports::{PushOutcome, PushRequest, SharedFile, SharedFileKind, SharedSnapshot};
use office_core::tool::{ExecutionKind, McpScope, ToolSpec};
use office_core::{Error, Result};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub(crate) const TOOL: Tool = Tool::built(
    ToolSpec {
        name: "library-sync",
        execution_kind: ExecutionKind::Mutating,
        mcp_scope: McpScope::Workspace,
    },
    &["direction", "dry_run"],
    run,
);

/// The lists the result reports.
#[derive(Default)]
struct Report {
    pulled: Vec<String>,
    pushed: Vec<String>,
    removed: Vec<String>,
    incoming: Vec<Value>,
    conflicts: Vec<Value>,
    skipped: Vec<Value>,
}

fn skip(phase: &str, path: &str, id: Option<&str>, reason: &str) -> Value {
    json!({"phase":phase,"path":path,"id":id,"reason":reason})
}

fn run(call: &Call<'_>) -> Result<Value> {
    let direction = match call.input.get("direction") {
        None => "both",
        Some(Value::String(value)) if ["both", "pull", "push"].contains(&value.as_str()) => {
            value.as_str()
        }
        Some(_) => {
            return Err(Error::InvalidArgument(
                "direction must be both, pull or push".into(),
            ));
        }
    };
    let dry_run = match call.input.get("dry_run") {
        None => false,
        Some(Value::Bool(value)) => *value,
        Some(_) => {
            return Err(Error::InvalidArgument(
                "dry_run must be true or false".into(),
            ));
        }
    };
    let settings = call
        .ports
        .library
        .settings()?
        .shared
        .ok_or(SharingError::NotConfigured)?;
    let shared = &call.ports.shared;
    shared.lock()?;
    let recorded = shared.state()?;
    // A changed remote, branch or member starts fresh: the old clone and
    // state describe another repository or namespace.
    if recorded
        .as_ref()
        .is_some_and(|state| !state.belongs_to(&settings))
        && !dry_run
    {
        shared.reset()?;
    }
    let mut state = recorded
        .filter(|state| state.belongs_to(&settings))
        .unwrap_or_else(|| SyncState::fresh(&settings));
    let mut snapshot = shared.fetch(&settings)?;
    check_layout(call, &snapshot)?;
    let mut report = Report::default();
    if direction != "push" {
        pull(call, &settings, &snapshot, &mut state, &mut report, dry_run)?;
        if !dry_run {
            state.head.clone_from(&snapshot.head);
            shared.save_state(&state)?;
        }
    }
    if direction != "pull" {
        snapshot = push(call, &settings, snapshot, &mut state, &mut report, dry_run)?;
    }
    let synced_at = (!dry_run).then(|| format_rfc3339_utc(call.ports.clock.now()));
    if !dry_run {
        state.head.clone_from(&snapshot.head);
        state.synced_at.clone_from(&synced_at);
        shared.save_state(&state)?;
    }
    Ok(json!({
        "direction": direction,
        "dry_run": dry_run,
        "pulled": report.pulled,
        "pushed": report.pushed,
        "removed": report.removed,
        "incoming": report.incoming,
        "conflicts": report.conflicts,
        "skipped": report.skipped,
        "commit": snapshot.head,
        "synced_at": synced_at,
    }))
}

/// A non-empty branch must carry the layout marker at format 1.
fn check_layout(call: &Call<'_>, snapshot: &SharedSnapshot) -> Result<()> {
    if snapshot.head.is_none() {
        return Ok(());
    }
    let marker = snapshot
        .files
        .iter()
        .find(|file| file.path == LAYOUT_FILE && file.kind == SharedFileKind::Regular)
        .ok_or(SharingError::LayoutInvalid)?;
    if !sync::layout_valid(&call.ports.shared.read(&marker.blob)?) {
        return Err(SharingError::LayoutInvalid.into());
    }
    Ok(())
}

fn text(call: &Call<'_>, file: &SharedFile) -> Result<String> {
    String::from_utf8(call.ports.shared.read(&file.blob)?).map_err(|_| Error::Library {
        code: "shared_file_unreadable",
        message: "shared file is not UTF-8 text".into(),
    })
}

/// Validate one shared document with the Library's own rules.
fn validate_document(call: &Call<'_>, file: &SharedFile) -> Result<DocumentRecord> {
    let markdown = text(call, file)?;
    let record = sharing::shared_document(&file.path, &markdown)?;
    if matches!(record.kind, DocumentKind::Domain | DocumentKind::Runbook) {
        create::check_template(call, &Page::from_markdown(&markdown)?)?;
    }
    Ok(record)
}

/// Report what changed upstream, refresh the shared layer, and file a
/// proposal for every page whose own copy differs from a changed shared copy.
fn pull(
    call: &Call<'_>,
    settings: &SharedSettings,
    snapshot: &SharedSnapshot,
    state: &mut SyncState,
    report: &mut Report,
    dry_run: bool,
) -> Result<()> {
    let files: BTreeMap<&str, &SharedFile> = snapshot
        .files
        .iter()
        .map(|file| (file.path.as_str(), file))
        .collect();
    // Each document directory once: whether its document is valid and, for
    // an artifact, the companion files it lists.
    let mut documents: BTreeMap<String, Option<Vec<String>>> = BTreeMap::new();
    let mut document = |dir: &str, source: bool| -> Option<Vec<String>> {
        let name = if source { "source.md" } else { "artifact.md" };
        documents
            .entry(dir.to_string())
            .or_insert_with(|| {
                let file = files
                    .get(format!("{dir}/{name}").as_str())
                    .filter(|file| file.kind == SharedFileKind::Regular)?;
                validate_document(call, file).ok()?;
                if source {
                    return Some(Vec::new());
                }
                let markdown = text(call, file).ok()?;
                Artifact::from_markdown(&markdown)
                    .ok()
                    .map(|artifact| artifact.files)
            })
            .clone()
    };
    for file in &snapshot.files {
        let seen = state.paths.get(&file.path).and_then(|s| s.seen.as_deref());
        if seen == Some(file.blob.as_str()) {
            continue;
        }
        let class = classify(&file.path);
        let id = match &class {
            SharedPath::Page { id } => Some(id.clone()),
            SharedPath::Layout
            | SharedPath::Document { .. }
            | SharedPath::Companion { .. }
            | SharedPath::Outside => None,
        };
        let reason = match (file.kind, &class) {
            (_, SharedPath::Layout) => continue,
            (SharedFileKind::Symlink, _) => Some("shared_file_linked".to_string()),
            (SharedFileKind::Special, _) => Some("shared_file_special".to_string()),
            (SharedFileKind::Regular, SharedPath::Outside) => {
                Some("shared_path_outside_layout".to_string())
            }
            (SharedFileKind::Regular, SharedPath::Page { .. } | SharedPath::Document { .. }) => {
                validate_document(call, file)
                    .err()
                    .map(|error| error.code().to_string())
            }
            (
                SharedFileKind::Regular,
                SharedPath::Companion {
                    dir, name, source, ..
                },
            ) => match document(dir, *source) {
                None => Some("shared_document_invalid".to_string()),
                Some(listed) => (!*source && !listed.contains(name))
                    .then(|| "shared_path_outside_layout".to_string()),
            },
        };
        match reason {
            Some(reason) => report
                .skipped
                .push(skip("pull", &file.path, id.as_deref(), &reason)),
            None => report.pulled.push(file.path.clone()),
        }
    }
    if dry_run {
        incoming(call, settings, snapshot, state, report, true)?;
        return Ok(());
    }
    for file in &snapshot.files {
        state.paths.entry(file.path.clone()).or_default().seen = Some(file.blob.clone());
    }
    // Forget paths that left the shared library unless this member shares
    // them; the push decides about those.
    state
        .paths
        .retain(|path, entry| files.contains_key(path.as_str()) || entry.own.is_some());
    call.ports.shared.checkout(snapshot.head.as_deref())?;
    call.ports.library.reindex()?;
    incoming(call, settings, snapshot, state, report, false)
}

/// The user's own copy of a page, if any: its Library path and Markdown.
fn own_page(call: &Call<'_>, id: &str) -> Result<Option<(String, String)>> {
    let Some(record) = call.ports.library.get(id)? else {
        return Ok(None);
    };
    if record.layer != LibraryLayer::Own {
        return Ok(None);
    }
    let markdown = call.ports.library.read_text(Path::new(&record.path))?;
    Ok(Some((record.path, markdown)))
}

fn shareable_own(call: &Call<'_>, id: &str) -> bool {
    call.ports
        .library
        .get(id)
        .ok()
        .flatten()
        .is_some_and(|record| {
            record.layer == LibraryLayer::Own && record.shareable && record.sensitivity == "normal"
        })
}

fn view(
    call: &Call<'_>,
    markdown: &str,
    member: &str,
    snapshot: &SharedSnapshot,
) -> Result<String> {
    let shared: BTreeSet<&str> = snapshot.files.iter().map(|f| f.path.as_str()).collect();
    sharing::shared_view(
        markdown,
        member,
        |id| shareable_own(call, id),
        |id| page_path(id).is_some_and(|path| shared.contains(path.as_str())),
    )
}

/// File one incoming update or conflict for every changed shared page whose
/// shareable own copy differs from it. Nothing is filed in a dry run.
fn incoming(
    call: &Call<'_>,
    settings: &SharedSettings,
    snapshot: &SharedSnapshot,
    state: &mut SyncState,
    report: &mut Report,
    dry_run: bool,
) -> Result<()> {
    for file in &snapshot.files {
        let SharedPath::Page { id } = classify(&file.path) else {
            continue;
        };
        if file.kind != SharedFileKind::Regular {
            continue;
        }
        let entry = state.paths.get(&file.path).cloned().unwrap_or_default();
        if entry.base.as_deref() == Some(file.blob.as_str()) {
            continue;
        }
        let Some((_, own)) = own_page(call, &id)? else {
            continue;
        };
        if !sharing::shareable(&own).unwrap_or(false) {
            // An unshared own copy stops tracking colleagues' edits.
            if !dry_run && entry.own.is_some() {
                let tracked = state.paths.entry(file.path.clone()).or_default();
                tracked.own = None;
                tracked.base = Some(file.blob.clone());
            }
            continue;
        }
        if validate_document(call, file).is_err() {
            continue;
        }
        let theirs = text(call, file)?;
        let Ok(mine) = view(call, &own, &settings.member, snapshot) else {
            continue;
        };
        let own_digest = sha256_hex(own.as_bytes());
        if sync::same_content(&theirs, &mine) {
            if !dry_run {
                let tracked = state.paths.entry(file.path.clone()).or_default();
                tracked.base = Some(file.blob.clone());
                tracked.own = Some(own_digest);
            }
            continue;
        }
        let markdown = sync::localized_view(&theirs, &own, &settings.member)?;
        let unchanged = entry.own.as_deref() == Some(own_digest.as_str());
        let page = Page::from_markdown(&theirs)?;
        let by = sharing::shared_document(&file.path, &theirs)?
            .shared_by
            .unwrap_or_default();
        let (title, description, payload) = if unchanged {
            (
                format!("{by} updated {} in the shared library", page.title),
                format!(
                    "The shared library has a newer version of {} from {by}. Accepting replaces your copy with it, keeping your own contact and path references.",
                    page.title
                ),
                Payload::KnowledgeUpdate {
                    shareable: None,
                    markdown,
                    base_sha256: own_digest,
                },
            )
        } else {
            (
                format!("{} changed here and in the shared library", page.title),
                format!(
                    "You and {by} both changed {}. Use theirs replaces your copy with the shared version; Keep mine pushes your version over theirs on the next sync. Nothing is pushed for this page until you choose.",
                    page.title
                ),
                Payload::SharedConflict {
                    page: id.clone(),
                    path: file.path.clone(),
                    blob: file.blob.clone(),
                    markdown,
                    base_sha256: own_digest,
                },
            )
        };
        let list = if unchanged {
            &mut report.incoming
        } else {
            &mut report.conflicts
        };
        if let Err(error) = create::validate_payload(call, &payload) {
            report
                .skipped
                .push(skip("pull", &file.path, Some(&id), error.code()));
            continue;
        }
        if dry_run {
            list.push(json!({"page":id,"proposal":null,"created":false}));
            continue;
        }
        let evidence = vec![id.clone(), format!("shared:{}@{}", file.path, file.blob)];
        let (proposal, created) = file_once(call, title, description, evidence, payload)?;
        list.push(json!({"page":id,"proposal":proposal,"created":created}));
    }
    Ok(())
}

/// File a proposal unless an equivalent one (page + shared blob) is already
/// open; dismissal memory is applied by proposal filing itself.
fn file_once(
    call: &Call<'_>,
    title: String,
    description: String,
    evidence: Vec<String>,
    payload: Payload,
) -> Result<(Option<String>, bool)> {
    let cluster = proposal::cluster(payload.kind(), &evidence)?;
    let listed = call.ports.orbit.task_list(&create::object(json!({
        "tag":cluster,"limit":1000,"fields":["id","status","tags","updated_at"]
    }))?)?;
    if listed.get("truncated").and_then(Value::as_bool) != Some(false) {
        return Err(proposal::refused(
            "proposal_history_incomplete",
            "proposal history is missing or truncated",
        ));
    }
    let open = listed
        .get("tasks")
        .and_then(Value::as_array)
        .ok_or_else(|| proposal::invalid("proposal tasks"))?
        .iter()
        .find(|task| {
            task.get("status").and_then(Value::as_str) != Some("rejected")
                && task
                    .get("tags")
                    .and_then(Value::as_array)
                    .is_some_and(|tags| tags.iter().any(|tag| tag.as_str() == Some(&cluster)))
        })
        .and_then(|task| task.get("id").and_then(Value::as_str));
    if let Some(id) = open {
        return Ok((Some(id.to_string()), false));
    }
    let filed = create::file(
        call,
        Create {
            title,
            description,
            evidence,
            payload,
            tags: vec!["shared-library".into()],
        },
    )?;
    Ok((
        filed.get("id").and_then(Value::as_str).map(str::to_string),
        filed.get("created").and_then(Value::as_bool) == Some(true),
    ))
}

/// One own shareable document and the files its shared view consists of.
struct Own {
    id: String,
    path: String,
    kind: DocumentKind,
    markdown: String,
    files: Vec<(String, Vec<u8>)>,
}

/// Read every shareable own document at one Library commit, retrying once
/// when the Library changes while it is read.
fn read_library(call: &Call<'_>) -> Result<Vec<Own>> {
    for _ in 0..2 {
        let before = call.ports.library.head_commit()?;
        let mut documents = Vec::new();
        let texts = call
            .ports
            .library
            .knowledge_pages()?
            .into_iter()
            .chain(call.ports.library.source_documents()?)
            .chain(call.ports.library.artifact_documents()?);
        for text in texts {
            if text.path.starts_with("shared/") {
                continue;
            }
            let Some(markdown) = text.markdown else {
                continue;
            };
            if !sharing::shareable(&markdown).unwrap_or(false) {
                continue;
            }
            let record = DocumentRecord::from_markdown(Path::new(&text.path), &markdown);
            let files = match record.kind {
                DocumentKind::Source | DocumentKind::Artifact => {
                    call.ports.library.evidence_files(Path::new(&text.path))?
                }
                DocumentKind::Domain | DocumentKind::Runbook => Vec::new(),
            };
            documents.push(Own {
                id: record.id,
                path: text.path,
                kind: record.kind,
                markdown,
                files,
            });
        }
        if call.ports.library.head_commit()? == before {
            return Ok(documents);
        }
    }
    Err(Error::Library {
        code: "library_changed",
        message: "the Library changed while it was read; sync again".into(),
    })
}

/// One file the push wants in the shared library.
struct Desired {
    bytes: Vec<u8>,
    document: String,
    digest: String,
    page: bool,
}

/// The computed push: one commit's writes and removals, and the state
/// updates that hold once it lands (or at once when nothing is pushed).
#[derive(Default)]
struct Plan {
    writes: Vec<(String, Vec<u8>, String, String)>,
    removes: Vec<String>,
    updates: Vec<(String, Option<PathState>)>,
    skipped: Vec<Value>,
    layout: bool,
}

fn plan(
    call: &Call<'_>,
    settings: &SharedSettings,
    snapshot: &SharedSnapshot,
    state: &SyncState,
    documents: &[Own],
) -> Result<Plan> {
    let member = settings.member.as_str();
    let mut plan = Plan {
        layout: snapshot.head.is_none(),
        ..Plan::default()
    };
    let upstream: BTreeMap<&str, &SharedFile> = snapshot
        .files
        .iter()
        .map(|file| (file.path.as_str(), file))
        .collect();
    let mut desired: BTreeMap<String, Desired> = BTreeMap::new();
    for own in documents {
        let checked = sharing::validate_sharing(&own.markdown, |id| call.ports.library.get(id))
            .and_then(|()| view(call, &own.markdown, member, snapshot));
        let shared = match checked {
            Ok(shared) => shared,
            Err(error) => {
                plan.skipped
                    .push(skip("push", &own.path, Some(&own.id), error.code()));
                continue;
            }
        };
        let digest = sha256_hex(own.markdown.as_bytes());
        match own.kind {
            DocumentKind::Domain | DocumentKind::Runbook => {
                let Some(path) = page_path(&own.id) else {
                    plan.skipped.push(skip(
                        "push",
                        &own.path,
                        Some(&own.id),
                        "shared_path_outside_layout",
                    ));
                    continue;
                };
                desired.insert(
                    path,
                    Desired {
                        bytes: shared.into_bytes(),
                        document: own.id.clone(),
                        digest,
                        page: true,
                    },
                );
            }
            DocumentKind::Source | DocumentKind::Artifact => {
                let path = format!("members/{member}/{}", own.path);
                let SharedPath::Document { dir, .. } = classify(&path) else {
                    plan.skipped.push(skip(
                        "push",
                        &own.path,
                        Some(&own.id),
                        "shared_path_outside_layout",
                    ));
                    continue;
                };
                desired.insert(
                    path,
                    Desired {
                        bytes: shared.into_bytes(),
                        document: own.id.clone(),
                        digest,
                        page: false,
                    },
                );
                for (name, bytes) in &own.files {
                    let path = format!("{dir}/{name}");
                    if !matches!(classify(&path), SharedPath::Companion { .. }) {
                        continue;
                    }
                    desired.insert(
                        path,
                        Desired {
                            digest: sha256_hex(bytes),
                            bytes: bytes.clone(),
                            document: own.id.clone(),
                            page: false,
                        },
                    );
                }
            }
        }
    }
    for (path, want) in desired {
        let entry = state.paths.get(&path).cloned().unwrap_or_default();
        let current = upstream.get(path.as_str()).copied();
        let matched = |blob: &str| PathState {
            seen: Some(blob.to_string()),
            base: Some(blob.to_string()),
            own: Some(want.digest.clone()),
        };
        if let Some(file) = current.filter(|file| file.kind == SharedFileKind::Regular) {
            // Unchanged on both sides since the last push or match.
            if entry.base.as_deref() == Some(file.blob.as_str())
                && entry.own.as_deref() == Some(want.digest.as_str())
            {
                continue;
            }
            let bytes = call.ports.shared.read(&file.blob)?;
            let same = if want.page {
                std::str::from_utf8(&bytes).is_ok_and(|theirs| {
                    std::str::from_utf8(&want.bytes)
                        .is_ok_and(|mine| sync::same_content(theirs, mine))
                })
            } else {
                bytes == want.bytes
            };
            if same {
                plan.updates.push((path, Some(matched(&file.blob))));
                continue;
            }
            // A page a colleague changed since this member last saw it waits
            // for its incoming update or conflict to be resolved.
            if want.page && entry.base.as_deref() != Some(file.blob.as_str()) {
                continue;
            }
        }
        plan.writes
            .push((path, want.bytes, want.document, want.digest));
    }
    let wanted: BTreeSet<&str> = plan.writes.iter().map(|w| w.0.as_str()).collect();
    for (path, entry) in &state.paths {
        if entry.own.is_none()
            || wanted.contains(path.as_str())
            || documents
                .iter()
                .any(|own| shared_paths(own, member).contains(path))
        {
            continue;
        }
        let class = classify(path);
        let mine = match &class {
            SharedPath::Page { .. } => true,
            SharedPath::Document { member: owner, .. }
            | SharedPath::Companion { member: owner, .. } => owner == member,
            SharedPath::Layout | SharedPath::Outside => false,
        };
        if !mine {
            continue;
        }
        match upstream.get(path.as_str()) {
            None => plan.updates.push((path.clone(), None)),
            Some(file)
                if matches!(class, SharedPath::Page { .. })
                    && entry.base.as_deref() != Some(file.blob.as_str()) =>
            {
                // A colleague changed it since: the page is theirs now.
                plan.updates.push((
                    path.clone(),
                    Some(PathState {
                        own: None,
                        ..entry.clone()
                    }),
                ));
            }
            Some(_) => plan.removes.push(path.clone()),
        }
    }
    Ok(plan)
}

/// The shared paths an own document maps to (its view and its files).
fn shared_paths(own: &Own, member: &str) -> Vec<String> {
    match own.kind {
        DocumentKind::Domain | DocumentKind::Runbook => page_path(&own.id).into_iter().collect(),
        DocumentKind::Source | DocumentKind::Artifact => {
            let path = format!("members/{member}/{}", own.path);
            let mut paths = vec![path.clone()];
            if let Some((dir, _)) = path.rsplit_once('/') {
                paths.extend(own.files.iter().map(|(name, _)| format!("{dir}/{name}")));
            }
            paths
        }
    }
}

fn message(member: &str, plan: &Plan) -> String {
    let shared: BTreeSet<&str> = plan.writes.iter().map(|w| w.2.as_str()).collect();
    let removed: BTreeSet<String> = plan
        .removes
        .iter()
        .map(|path| match classify(path) {
            SharedPath::Document { dir, .. } | SharedPath::Companion { dir, .. } => dir,
            SharedPath::Page { .. } | SharedPath::Layout | SharedPath::Outside => path.clone(),
        })
        .collect();
    if shared.is_empty() && removed.is_empty() {
        return format!("{member}: initialize the shared library");
    }
    let noun = |count: usize| if count == 1 { "document" } else { "documents" };
    format!(
        "{member}: share {} {}, remove {}",
        shared.len(),
        noun(shared.len()),
        removed.len()
    )
}

fn apply(state: &mut SyncState, updates: Vec<(String, Option<PathState>)>) {
    for (path, update) in updates {
        match update {
            Some(entry) => {
                state.paths.insert(path, entry);
            }
            None => {
                state.paths.remove(&path);
            }
        }
    }
}

/// Push this member's shareable documents in one commit. A rejected push
/// fetches, recomputes and retries once.
fn push(
    call: &Call<'_>,
    settings: &SharedSettings,
    mut snapshot: SharedSnapshot,
    state: &mut SyncState,
    report: &mut Report,
    dry_run: bool,
) -> Result<SharedSnapshot> {
    let documents = read_library(call)?;
    let mut retried = false;
    loop {
        let plan = plan(call, settings, &snapshot, state, &documents)?;
        if plan.writes.is_empty() && plan.removes.is_empty() && !plan.layout {
            report.skipped.extend(plan.skipped);
            if !dry_run {
                apply(state, plan.updates);
            }
            return Ok(snapshot);
        }
        if dry_run {
            report.skipped.extend(plan.skipped);
            report
                .pushed
                .extend(plan.writes.iter().map(|w| w.0.clone()));
            report.removed.extend(plan.removes.iter().cloned());
            return Ok(snapshot);
        }
        let mut writes: Vec<(String, Vec<u8>)> = plan
            .writes
            .iter()
            .map(|(path, bytes, _, _)| (path.clone(), bytes.clone()))
            .collect();
        if plan.layout {
            writes.push((LAYOUT_FILE.to_string(), LAYOUT.as_bytes().to_vec()));
        }
        let request = PushRequest {
            base: snapshot.head.clone(),
            writes,
            removes: plan.removes.clone(),
            message: message(&settings.member, &plan),
        };
        match call.ports.shared.push(settings, &request)? {
            PushOutcome::Pushed(pushed) => {
                let blobs: BTreeMap<&str, &str> = pushed
                    .files
                    .iter()
                    .map(|file| (file.path.as_str(), file.blob.as_str()))
                    .collect();
                apply(state, plan.updates);
                for (path, _, _, digest) in &plan.writes {
                    let blob = blobs.get(path.as_str()).map(|blob| (*blob).to_string());
                    state.paths.insert(
                        path.clone(),
                        PathState {
                            seen: blob.clone(),
                            base: blob,
                            own: Some(digest.clone()),
                        },
                    );
                }
                for path in &plan.removes {
                    state.paths.remove(path);
                }
                if let Some(blob) = blobs.get(LAYOUT_FILE) {
                    state.paths.insert(
                        LAYOUT_FILE.to_string(),
                        PathState {
                            seen: Some((*blob).to_string()),
                            ..PathState::default()
                        },
                    );
                }
                report.skipped.extend(plan.skipped);
                report
                    .pushed
                    .extend(plan.writes.iter().map(|w| w.0.clone()));
                report.removed.extend(plan.removes.iter().cloned());
                call.ports.shared.checkout(pushed.head.as_deref())?;
                call.ports.library.reindex()?;
                return Ok(pushed);
            }
            PushOutcome::Rejected if retried => return Err(SharingError::PushRejected.into()),
            PushOutcome::Rejected => {
                retried = true;
                snapshot = call.ports.shared.fetch(settings)?;
                check_layout(call, &snapshot)?;
            }
        }
    }
}
