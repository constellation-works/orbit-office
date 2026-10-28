//! Pure rules for `office.library-sync`: the shared repository layout, the
//! sync state record, localizing a shared page for the user's own Library,
//! content comparison and the git transport policy. No I/O.

use super::{SharedSettings, rewrite_citations, settings::validate_member};
use crate::Result;
use crate::domain::knowledge::frontmatter;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// The layout marker at the root of the shared repository.
pub const LAYOUT_FILE: &str = ".office-shared.yaml";
/// The marker's contents, written by the first push to an empty repository.
pub const LAYOUT: &str = "format: 1\n";
/// Deadline for each git command.
pub const GIT_DEADLINE_SECONDS: u64 = 120;
/// The only variables a git child inherits (STD-05@1), plus
/// [`GIT_TERMINAL_PROMPT`].
pub const GIT_ENVIRONMENT: [&str; 4] = ["PATH", "HOME", "LANG", "SSH_AUTH_SOCK"];
/// Set on every git child so a missing credential fails instead of prompting.
pub const GIT_TERMINAL_PROMPT: (&str, &str) = ("GIT_TERMINAL_PROMPT", "0");
/// `-c` overrides on every git command: only the ssh, https and file
/// transports, and no hooks.
pub const GIT_OVERRIDES: [&str; 5] = [
    "protocol.allow=never",
    "protocol.ssh.allow=always",
    "protocol.https.allow=always",
    "protocol.file.allow=always",
    "core.hooksPath=/dev/null",
];

/// Whether a layout marker is exactly a mapping with `format: 1`.
#[must_use]
pub fn layout_valid(bytes: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    let Ok(fields) = frontmatter::parse(text) else {
        return false;
    };
    matches!(fields.as_slice(), [(key, value)] if key == "format"
        && (value.as_str() == Some("1") || value.as_u64() == Some(1)))
}

/// Where a clone-relative path sits in the shared repository layout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SharedPath {
    /// `.office-shared.yaml`.
    Layout,
    /// `knowledge/domain/<id>.md` or `knowledge/runbooks/<id>.md`.
    Page {
        /// The page id.
        id: String,
    },
    /// `members/<member>/{sources,artifacts}/YYYY/MM/<id>/{source,artifact}.md`.
    Document {
        /// The sharing member.
        member: String,
        /// The document's directory, clone-relative.
        dir: String,
    },
    /// A file beside a shared source or artifact document.
    Companion {
        /// The sharing member.
        member: String,
        /// The document's directory, clone-relative.
        dir: String,
        /// The file name.
        name: String,
        /// True for a source sidecar, false for an artifact file.
        source: bool,
    },
    /// Anything else, never indexed.
    Outside,
}

/// The shared path of an own page id.
#[must_use]
pub fn page_path(id: &str) -> Option<String> {
    let valid = |rest: &str| {
        !rest.is_empty()
            && rest
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    };
    if id.strip_prefix("dom-").is_some_and(valid) {
        Some(format!("knowledge/domain/{id}.md"))
    } else if id.strip_prefix("rb-").is_some_and(valid) {
        Some(format!("knowledge/runbooks/{id}.md"))
    } else {
        None
    }
}

fn source_sidecar(name: &str) -> bool {
    ["frames.md", "chapters.md", "digest.json", "summary.md"].contains(&name)
        || name
            .strip_prefix('v')
            .and_then(|n| n.strip_suffix(".md"))
            .is_some_and(|n| {
                !n.is_empty() && n.len() <= 10 && n.bytes().all(|b| b.is_ascii_digit())
            })
}

/// Classify a clone-relative path. Content is validated separately.
#[must_use]
pub fn classify(path: &str) -> SharedPath {
    if path == LAYOUT_FILE {
        return SharedPath::Layout;
    }
    if let Some(name) = path
        .strip_prefix("knowledge/domain/")
        .or_else(|| path.strip_prefix("knowledge/runbooks/"))
        && let Some(id) = name.strip_suffix(".md")
        && page_path(id).as_deref() == Some(path)
    {
        return SharedPath::Page { id: id.into() };
    }
    let parts: Vec<_> = path.split('/').collect();
    let [
        "members",
        member,
        area @ ("sources" | "artifacts"),
        year,
        month,
        id,
        name,
    ] = parts.as_slice()
    else {
        return SharedPath::Outside;
    };
    let source = *area == "sources";
    let prefix = if source { "src-" } else { "art-" };
    if validate_member(member).is_err()
        || year.len() != 4
        || month.len() != 2
        || !year
            .bytes()
            .chain(month.bytes())
            .all(|b| b.is_ascii_digit())
        || !id.starts_with(prefix)
        || !id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        || name.is_empty()
        || name.starts_with('.')
    {
        return SharedPath::Outside;
    }
    let dir = parts[..6].join("/");
    let member = (*member).to_string();
    let document = if source { "source.md" } else { "artifact.md" };
    if *name == document {
        SharedPath::Document { member, dir }
    } else if (source && source_sidecar(name)) || (!source && !name.starts_with("original.")) {
        SharedPath::Companion {
            member,
            dir,
            name: (*name).to_string(),
            source,
        }
    } else {
        SharedPath::Outside
    }
}

/// What this member last knew about one shared path.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PathState {
    /// The blob last seen by a pull (`pulled` lists only changes since).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seen: Option<String>,
    /// The blob this member last reconciled with: pushed, matched, or chose
    /// to override with **Keep mine**. A push writes only while the shared
    /// copy is still this blob.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
    /// SHA-256 of the own document (or file) when it was last pushed or
    /// matched. Present only for paths this member shares.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub own: Option<String>,
}

/// Sync state, `{{plugin_state}}/shared/state.json`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyncState {
    /// Record format, 1.
    pub format: u32,
    /// The remote this state belongs to.
    pub remote: String,
    /// The branch this state belongs to.
    pub branch: String,
    /// The member this state belongs to.
    pub member: String,
    /// The shared head after the last sync.
    #[serde(default)]
    pub head: Option<String>,
    /// When the last sync finished.
    #[serde(default)]
    pub synced_at: Option<String>,
    /// Per clone-relative path.
    #[serde(default)]
    pub paths: BTreeMap<String, PathState>,
}

impl SyncState {
    /// An empty state for these settings.
    #[must_use]
    pub fn fresh(settings: &SharedSettings) -> Self {
        Self {
            format: 1,
            remote: settings.remote.clone(),
            branch: settings.branch.clone(),
            member: settings.member.clone(),
            ..Self::default()
        }
    }

    /// Whether this state was recorded for the same remote, branch and
    /// member. Any change starts fresh.
    #[must_use]
    pub fn belongs_to(&self, settings: &SharedSettings) -> bool {
        self.format == 1
            && self.remote == settings.remote
            && self.branch == settings.branch
            && self.member == settings.member
    }
}

/// The remote's host for diagnostics: never a user, password or path.
#[must_use]
pub fn remote_host(remote: &str) -> String {
    if remote.starts_with('/') || remote.starts_with("file://") {
        return "local".into();
    }
    if remote.starts_with("https://") || remote.starts_with("ssh://") {
        return url::Url::parse(remote)
            .ok()
            .and_then(|url| url.host_str().map(str::to_string))
            .unwrap_or_else(|| "unknown".into());
    }
    remote
        .split_once(':')
        .map(|(host, _)| host.rsplit('@').next().unwrap_or(host).to_string())
        .unwrap_or_else(|| "unknown".into())
}

fn unqualify(cite: &str, member: &str) -> String {
    match cite
        .strip_prefix(member)
        .and_then(|rest| rest.strip_prefix('/'))
    {
        Some(plain) if plain.starts_with("src-") || plain.starts_with("art-") => plain.into(),
        Some(_) | None => cite.into(),
    }
}

/// Render a shared page as the user's own copy: the user's own `contact` and
/// `path` refs are kept, citations of the user's own member become plain ids,
/// `shared_by` is dropped, and the own copy's `shareable` and `sensitivity`
/// are retained. Body text outside citations stays verbatim.
pub fn localized_view(shared: &str, own: &str, member: &str) -> Result<String> {
    validate_member(member)?;
    let (header, body) = frontmatter::split(shared)?;
    let mut fields = frontmatter::parse(header)?;
    let (own_header, _) = frontmatter::split(own)?;
    let own_fields = frontmatter::parse(own_header)?;
    fields.retain(|(key, _)| !["shared_by", "shareable", "sensitivity"].contains(&key.as_str()));
    let personal: Vec<Value> = own_fields
        .iter()
        .filter(|(key, _)| key == "refs")
        .filter_map(|(_, value)| value.as_array())
        .flatten()
        .filter(|reference| reference.get("contact").is_some() || reference.get("path").is_some())
        .cloned()
        .collect();
    let mut has_refs = false;
    for (key, value) in &mut fields {
        if key == "refs"
            && let Some(refs) = value.as_array_mut()
        {
            has_refs = true;
            for reference in refs.iter_mut() {
                if let Some(Value::String(id)) = reference.get_mut("artifact") {
                    *id = unqualify(id, member);
                }
            }
            refs.extend(personal.iter().cloned());
        }
        if key == "cites"
            && let Some(cites) = value.as_array_mut()
        {
            for cite in cites {
                if let Value::String(text) = cite {
                    *text = unqualify(text, member);
                }
            }
        }
    }
    if !has_refs && !personal.is_empty() {
        fields.push(("refs".into(), Value::Array(personal)));
    }
    for (key, value) in own_fields {
        if key == "shareable" || key == "sensitivity" {
            fields.push((key, value));
        }
    }
    let mut output = String::from("---\n");
    for (key, value) in fields {
        output.push_str(&format!(
            "{key}: {}\n",
            crate::domain::artifact::yaml_value(&value)
        ));
    }
    output.push_str("---\n");
    output.push_str(&rewrite_citations(body, |cite| unqualify(cite, member)));
    Ok(output)
}

/// Whether two shared views carry the same content: the same fields other
/// than `shared_by` and `contributors`, and the same body.
#[must_use]
pub fn same_content(left: &str, right: &str) -> bool {
    let normalized = |text: &str| -> Option<(BTreeMap<String, Value>, String)> {
        let (header, body) = frontmatter::split(text).ok()?;
        let fields = frontmatter::parse(header)
            .ok()?
            .into_iter()
            .filter(|(key, _)| key != "shared_by" && key != "contributors")
            .collect();
        Some((fields, body.to_string()))
    };
    match (normalized(left), normalized(right)) {
        (Some(left), Some(right)) => left == right,
        (Some(_) | None, None) | (None, Some(_)) => false,
    }
}
