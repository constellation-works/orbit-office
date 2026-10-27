//! Library access port. Defaults refuse access for an unconfigured context.

use std::path::Path;

use crate::domain::endpoint::Capability;
use crate::domain::knowledge::{
    Backlink, DocumentKind, DocumentRecord, Revision, SearchFilter, SearchHit,
};
use crate::domain::settings::Settings;
use crate::{Error, Result};

/// What `office.init` created, or that it found a complete Library.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InitOutcome {
    /// True when a complete Library already existed and nothing changed.
    pub already_initialized: bool,
    /// Library-relative (or state-relative) paths created or upgraded, sorted.
    /// An existing `.gitignore` appears when its frames rule was added.
    pub created: Vec<String>,
}

/// One Markdown file read from the Library, by its Library-relative path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibraryText {
    /// Library-relative path, forward slashes.
    pub path: String,
    /// The file's Markdown. `None` when it could not be read (a symlink or
    /// an unreadable template), so a caller can name it rather than skip it.
    pub markdown: Option<String>,
}

/// Library and index health for `office.status`. Counts come from the index.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LibraryHealth {
    /// The Library's own repository and a valid settings file are present.
    pub initialized: bool,
    /// Indexed documents of every kind.
    pub documents_total: u32,
    /// Indexed `source` documents.
    pub source: u32,
    /// Indexed `domain` pages.
    pub domain: u32,
    /// Indexed `runbook` pages.
    pub runbook: u32,
    /// Indexed `artifact` documents.
    pub artifact: u32,
    /// Knowledge pages that name a template and no longer meet its current rules.
    pub pages_off_template: u32,
    /// Template files that do not parse, as Library-relative paths, sorted.
    /// Empty when every template file is valid.
    pub template_invalid: Vec<String>,
}

impl LibraryHealth {
    /// Health for a Library that is not initialized: no counts are read.
    #[must_use]
    pub fn uninitialized() -> Self {
        Self::default()
    }

    /// Count for one kind.
    #[must_use]
    pub fn count_for(&self, kind: DocumentKind) -> u32 {
        match kind {
            DocumentKind::Source => self.source,
            DocumentKind::Domain => self.domain,
            DocumentKind::Runbook => self.runbook,
            DocumentKind::Artifact => self.artifact,
        }
    }
}

/// The shared refusal for an uninitialized Library (STD-02 R24): one
/// definition, used by the dispatch chokepoint and by adapters.
#[must_use]
pub fn not_initialized_error() -> Error {
    Error::Library {
        code: "library_not_initialized",
        message: "Library is not initialized; run office.init".to_string(),
    }
}

/// The Library operations used by tools, independent of files, git and SQLite.
/// An adapter must override all operations; defaults support contexts that have
/// no Library (for example the status-only dispatch scaffold).
pub trait Library {
    /// Read a contained video's original and Markdown; verify original SHA-256.
    fn read_video(&self, _id: &str) -> Result<crate::domain::source::video::VideoSource> {
        Err(Error::LibraryUnavailable)
    }
    /// Compare the Markdown revision and apply a video mutation under the writer lock.
    /// Persist call reservations before inference; journal derived files with the source.
    fn mutate_video(
        &self,
        _id: &str,
        _revision: &str,
        _mutation: crate::domain::source::video::VideoMutation,
    ) -> Result<crate::domain::source::video::VideoState> {
        Err(Error::LibraryUnavailable)
    }

    /// Publish a new immutable artifact bundle, assigning its series version
    /// and unique id under the writer lock, then indexing and committing once.
    fn promote_artifact(
        &self,
        _artifact: &crate::domain::artifact::Artifact,
        _files: &[crate::domain::artifact::ArtifactFile],
    ) -> Result<crate::domain::artifact::Artifact> {
        Err(Error::LibraryUnavailable)
    }
    /// Every artifact frontmatter file, without a result limit, in path order.
    fn artifact_documents(&self) -> Result<Vec<LibraryText>> {
        Err(Error::LibraryUnavailable)
    }

    /// Read the current document by Library-relative path, without a writer lock.
    fn read_document(&self, _relative: &Path) -> Result<DocumentRecord> {
        Err(Error::LibraryUnavailable)
    }
    /// Write a document, update its index and commit under one writer lock.
    fn write_document(&self, _relative: &Path, _text: &str) -> Result<DocumentRecord> {
        Err(Error::LibraryUnavailable)
    }
    /// One indexed document by id (with its body, cites and related), if
    /// present. Does not wait for the writer lock.
    fn get(&self, _id: &str) -> Result<Option<DocumentRecord>> {
        Err(Error::LibraryUnavailable)
    }
    /// Indexed documents whose frontmatter `cites` (`<id>` or
    /// `<id>#<anchor>`) or `related` names `id`, sorted by id.
    fn backlinks(&self, _id: &str) -> Result<Vec<Backlink>> {
        Err(Error::LibraryUnavailable)
    }
    /// Every knowledge page file (`knowledge/**/*.md`), in path order.
    fn knowledge_pages(&self) -> Result<Vec<LibraryText>> {
        Err(Error::LibraryUnavailable)
    }
    /// Every source document (`sources/**/source.md`), with full frontmatter
    /// and body, without a result limit, in path order. Originals and version
    /// sidecars are excluded; symlinks are never followed.
    fn source_documents(&self) -> Result<Vec<LibraryText>> {
        Err(Error::LibraryUnavailable)
    }
    /// Every template file in `.office/templates/`, in path order.
    fn templates(&self) -> Result<Vec<LibraryText>> {
        Err(Error::LibraryUnavailable)
    }
    /// Search the index, applying filters before the limit.
    fn search(&self, _query: &str, _filter: &SearchFilter) -> Result<Vec<SearchHit>> {
        Err(Error::LibraryUnavailable)
    }
    /// Newest-first commits affecting a document (empty for an untracked path).
    fn history(&self, _relative: &Path) -> Result<Vec<Revision>> {
        Err(Error::LibraryUnavailable)
    }
    /// A document's current Markdown, frontmatter included, by its
    /// Library-relative path, without a writer lock.
    fn read_text(&self, _relative: &Path) -> Result<String> {
        Err(Error::LibraryUnavailable)
    }
    /// The Library repository's current commit (`None` before the first commit).
    fn head_commit(&self) -> Result<Option<String>> {
        Err(Error::LibraryUnavailable)
    }
    /// Write `text` to a staging file for one Orbit artifact copy and return
    /// the path to hand `orbit.task.artifact.put` as its `source_path`.
    /// `name` is the Library-relative path the copy stands for. Staging files
    /// are outside the indexed layout and never committed.
    fn stage_attachment(&self, _name: &str, _text: &str) -> Result<String> {
        Err(Error::LibraryUnavailable)
    }
    /// Remove a staging file returned by [`Library::stage_attachment`].
    fn remove_staged(&self, _staged: &str) -> Result<()> {
        Err(Error::LibraryUnavailable)
    }
    /// Check for the Library's own repository and settings file, without creating them.
    fn is_initialized(&self) -> Result<bool> {
        Err(Error::LibraryUnavailable)
    }
    /// Load and validate settings on every call.
    fn settings(&self) -> Result<Settings> {
        Err(Error::LibraryUnavailable)
    }
    /// Apply one typed setting to the latest config and commit under one
    /// writer lock. Unknown keys and invalid values leave it unchanged.
    fn set_setting(&self, _key: &str, _value: serde_json::Value) -> Result<()> {
        Err(Error::LibraryUnavailable)
    }
    /// Record a private-endpoint check (`Settings::with_route_check`) in the
    /// latest config and commit under one writer lock. Refused with
    /// `private_route_changed` when the endpoint or model is no longer the
    /// one checked; nothing changes on error.
    fn record_route_check(
        &self,
        _endpoint: &str,
        _model: &str,
        _capabilities: &[Capability],
        _verified_at: &str,
    ) -> Result<()> {
        Err(Error::LibraryUnavailable)
    }
    /// Capture a source (upload copy, pasted text, conversation create or
    /// append). Upload/transcript paths are contained by the adapter.
    fn capture_source(&self, _request: &CaptureSourceRequest) -> Result<SourceOutcome> {
        Err(Error::LibraryUnavailable)
    }
    /// Read one source by id for display at an optional anchor.
    fn read_source(&self, _id: &str) -> Result<SourceView> {
        Err(Error::LibraryUnavailable)
    }
    /// Scaffold the Library (layout, git, config, `.gitignore`, index, one
    /// commit). `library_dir`, when given, is the requested Library location
    /// (workspace-relative or absolute); an adapter refuses one outside its
    /// granted `{{workspace}}/library` root with `library_dir_outside_workspace`
    /// and a missing `git` with `git_missing`. Idempotent.
    fn initialize(&self, _library_dir: Option<&str>) -> Result<InitOutcome> {
        Err(Error::LibraryUnavailable)
    }
    /// Rebuild the index from the files; returns the indexed document count.
    fn reindex(&self) -> Result<u32> {
        Err(Error::LibraryUnavailable)
    }
    /// Library and index health, without creating Library state.
    fn health(&self) -> Result<LibraryHealth> {
        Ok(LibraryHealth {
            initialized: self.is_initialized()?,
            ..LibraryHealth::default()
        })
    }
}

/// One conversation entry to append (specs/sessions.md).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConversationEntry {
    /// One of the seven decision-log kinds.
    pub kind: String,
    /// One line of entry text.
    pub text: String,
}

/// Everything `office.capture` asks the Library to do in one call.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CaptureSourceRequest {
    /// The kind.
    pub kind: Option<String>,
    /// The title.
    pub title: Option<String>,
    /// The text.
    pub text: Option<String>,
    /// The upload path.
    pub upload_path: Option<String>,
    /// The transcript path.
    pub transcript_path: Option<String>,
    /// The append to.
    pub append_to: Option<String>,
    /// The entries.
    pub entries: Vec<ConversationEntry>,
    /// The captured at.
    pub captured_at: String,
    /// The captured by.
    pub captured_by: Option<String>,
    /// The origin.
    pub origin: Option<String>,
    /// The occurred at.
    pub occurred_at: Option<String>,
    /// The participants.
    pub participants: Vec<String>,
    /// Source tags, such as `bootstrap:orientation`.
    pub tags: Vec<String>,
    /// The sensitivity.
    pub sensitivity: Option<String>,
}

/// The source a capture created, deduplicated to, or appended to.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SourceOutcome {
    /// The id.
    pub id: String,
    /// The kind.
    pub kind: String,
    /// The title.
    pub title: String,
    /// Library-relative path of `source.md`.
    pub path: String,
    /// The sha256.
    pub sha256: Option<String>,
    /// The bytes.
    pub bytes: Option<u64>,
    /// The media type.
    pub media_type: Option<String>,
    /// The sensitivity.
    pub sensitivity: String,
    /// The deduplicated.
    pub deduplicated: bool,
    /// Entries appended by this call (0 for a create).
    pub appended: usize,
    /// The version.
    pub version: u32,
}

/// A source read back for `office.source-show`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SourceView {
    /// The id.
    pub id: String,
    /// The kind.
    pub kind: String,
    /// The title.
    pub title: String,
    /// The path.
    pub path: String,
    /// The body.
    pub body: String,
    /// The sensitivity.
    pub sensitivity: String,
    /// The sha256.
    pub sha256: Option<String>,
    /// The bytes.
    pub bytes: Option<u64>,
    /// The media type.
    pub media_type: Option<String>,
    /// The version.
    pub version: u32,
}
