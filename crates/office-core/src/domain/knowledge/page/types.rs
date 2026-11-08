//! Page types and lifecycle tokens.

use office_common::Sensitivity;
use serde::Serialize;

use crate::Error;
use crate::domain::knowledge::DocumentKind;

/// The word cap on a page body (library-bootstrap.md, "Short pages").
pub const MAX_PAGE_WORDS: usize = 1_200;

/// The tag that marks a page, or (as a trailing `[policy]` on its heading)
/// a section, as a policy summary.
pub const POLICY_TAG: &str = "policy";

/// The one page whose frontmatter may carry `scribe` settings
/// (specs/sessions.md, Live scribe).
pub const SCRIBE_RULES_PAGE: &str = "dom-scribe-rules";

/// A page's lifecycle (`status`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PageStatus {
    /// In use.
    #[default]
    Current,
    /// Due for re-verification.
    Stale,
    /// Kept for history only.
    Retired,
}

/// Who carries out a runbook (`performable`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Performable {
    /// The agent does every step up to submission.
    Agent,
    /// The user does it (the default when absent).
    You,
}

/// How a performable runbook's request is submitted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SubmissionVia {
    /// A `draft` artifact the user submits.
    Draft,
    /// A `gateway.http-draft` the user approves.
    Gateway,
}

/// A runbook's `submission` mapping.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Submission {
    /// Where the request goes, for example `ServiceNow`.
    pub system: String,
    /// Draft or gateway.
    pub via: SubmissionVia,
    /// The gateway connection; required when `via` is `gateway`.
    pub connection: Option<String>,
}

/// Which kind of pointer a ref is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RefTarget {
    /// A file path.
    Path,
    /// A link.
    Url,
    /// An email address or phone number. Never reaches a model.
    Contact,
    /// A key in another system, `system:key`.
    Account,
    /// An artifact id.
    Artifact,
}

impl RefTarget {
    pub(super) fn parse(token: &str) -> Option<Self> {
        match token {
            "path" => Some(Self::Path),
            "url" => Some(Self::Url),
            "contact" => Some(Self::Contact),
            "account" => Some(Self::Account),
            "artifact" => Some(Self::Artifact),
            _ => None,
        }
    }
}

/// One pointer in a domain page's `refs`. Pointers are never resolved or
/// followed, and never carry credentials.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageRef {
    /// What the pointer is.
    pub label: String,
    /// The target kind.
    pub target: RefTarget,
    /// The target value.
    pub value: String,
    /// Whether this is the official source of a policy summary.
    pub authoritative: bool,
    /// The date the pointed-to material is current as of, `YYYY-MM-DD`.
    pub as_of: Option<String>,
}

/// One citation, from frontmatter `cites` or inline in the body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Citation {
    /// The citation as written.
    pub text: String,
    /// The cited document id.
    pub id: String,
    /// The kind of document the id prefix names.
    pub kind: DocumentKind,
    /// The anchor after `#`, if any.
    pub anchor: Option<String>,
}

/// A parsed knowledge page: frontmatter fields plus the verbatim body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Page {
    /// `dom-…` or `rb-…`.
    pub id: String,
    /// `Domain` or `Runbook` (frontmatter `type`, else the id prefix).
    pub kind: DocumentKind,
    /// The template the page follows, if any.
    pub template: Option<String>,
    /// Title.
    pub title: String,
    /// Names the subject goes by in sources.
    pub aliases: Vec<String>,
    /// One-line summary.
    pub summary: String,
    /// Tags, for example `client:acme`.
    pub tags: Vec<String>,
    /// Frontmatter citations, as written.
    pub cites: Vec<String>,
    /// Pointers (domain pages only).
    pub refs: Vec<PageRef>,
    /// Related page ids.
    pub related: Vec<String>,
    /// Last update time, as written.
    pub updated_at: Option<String>,
    /// Last verification date, `YYYY-MM-DD`.
    pub last_verified: Option<String>,
    /// Lifecycle.
    pub status: PageStatus,
    /// Sensitivity (`normal` when absent).
    pub sensitivity: Sensitivity,
    /// Explicit approval to reach the shared Library.
    pub shareable: bool,
    /// The date the page's policy content is current as of, `YYYY-MM-DD`.
    pub as_of: Option<String>,
    /// Who performs a runbook.
    pub performable: Option<Performable>,
    /// How a performable runbook is submitted.
    pub submission: Option<Submission>,
    /// `scribe.handoff` on the scribe rules page; `None` when absent.
    pub scribe_handoff: Option<bool>,
    /// Markdown after the frontmatter, verbatim.
    pub body: String,
}

pub(super) fn invalid(field: &str, reason: &'static str) -> Error {
    Error::InvalidFrontmatter {
        field: field.into(),
        reason,
    }
}
