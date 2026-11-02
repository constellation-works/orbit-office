//! Library access port. Defaults refuse access for an unconfigured context.

use std::path::Path;

use crate::domain::endpoint::Capability;
use crate::domain::knowledge::{Backlink, DocumentRecord, Revision, SearchFilter, SearchHit};
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

/// One Markdown file read from either layer, by its virtual Library path.
/// A `shared/` prefix addresses the read-only clone under plugin state.
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
    /// Update only sharing approval under the writer lock. Refusals and repeats
    /// write nothing; success reindexes and commits exactly once.
    fn set_shareable(&self, _id: &str, _shareable: bool) -> Result<DocumentRecord> {
        Err(Error::LibraryUnavailable)
    }
    /// Commit a non-indexed proposal record under `proposals/`. Other paths
    /// are refused. This does not change knowledge or source enumeration.
    fn write_proposal(&self, _relative: &Path, _text: &str) -> Result<()> {
        Err(Error::LibraryUnavailable)
    }
    /// Write `text` at the proposal page `to` and remove every page in `from`
    /// in one commit, so a page changes status folder in the same commit that
    /// records the outcome. Every path must be a proposal page path.
    fn move_proposal(&self, _to: &Path, _text: &str, _from: &[String]) -> Result<()> {
        Err(Error::LibraryUnavailable)
    }
    /// Every proposal page in any status folder or the legacy flat layout,
    /// in path order, with its Markdown. Other files under `proposals/` are
    /// not listed.
    fn proposal_pages(&self) -> Result<Vec<LibraryText>> {
        Err(Error::LibraryUnavailable)
    }
    /// Write reviewed knowledge under the writer lock only if the current
    /// Markdown hash matches. `None` requires an absent destination.
    fn write_reviewed_page(
        &self,
        _relative: &Path,
        _text: &str,
        _expected_sha256: Option<&str>,
    ) -> Result<DocumentRecord> {
        Err(Error::LibraryUnavailable)
    }
    /// Read a contained video or meeting original, checking its recorded digest.
    fn read_media(&self, id: &str) -> Result<crate::domain::source::video::VideoSource> {
        self.read_video(id)
    }
    /// Read an optional allowlisted source sidecar (including summary.md); missing is distinct from unreadable.
    fn source_sidecar(&self, _id: &str, _name: &str) -> Result<Option<String>> {
        Err(Error::LibraryUnavailable)
    }
    /// Publish model provenance, an optional versioned transcript, and derived sidecars
    /// in one journaled compare-and-swap. Existing version files are immutable.
    fn write_source_processing(
        &self,
        _id: &str,
        _revision: &str,
        _body: Option<&str>,
        _derived: &crate::ports::DerivedBy,
        _files: &[(String, Vec<u8>)],
    ) -> Result<SourceView> {
        Err(Error::LibraryUnavailable)
    }
    /// Read every bootstrap source/digest, knowledge page and durable phase ledger.
    /// This is a read-only file scan, never a bounded index search.
    fn bootstrap_snapshot(&self) -> Result<crate::domain::bootstrap::Snapshot> {
        Err(Error::LibraryUnavailable)
    }
    /// Compare the full snapshot/settings revision under the writer lock, then
    /// journal the ledger and all source privacy updates as one recoverable write.
    fn mutate_batch(
        &self,
        _revision: &str,
        _mutation: crate::domain::bootstrap::Mutation,
    ) -> Result<crate::domain::bootstrap::Batch> {
        Err(Error::LibraryUnavailable)
    }

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
    /// One artifact companion file's exact bytes, by its Library-relative
    /// path, under the adapter's containment rules. Unlike `read_text`,
    /// this does not require Markdown or UTF-8, so a caller can tell a
    /// binary companion from a text one.
    fn read_artifact_file(&self, _relative: &Path) -> Result<Vec<u8>> {
        Err(Error::LibraryUnavailable)
    }

    /// The shareable companions of an own source or artifact document, by
    /// its Library-relative path: source sidecars (`vN.md`, `frames.md`,
    /// `chapters.md`, `digest.json`, `summary.md`) or the artifact's listed
    /// files, as `(file name, bytes)` in name order. Originals, `frames/`
    /// and processing state are never returned; symlinks are never followed.
    fn evidence_files(&self, _relative: &Path) -> Result<Vec<(String, Vec<u8>)>> {
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
    /// present. Plain page ids prefer own copies; `<member>/<id>` addresses
    /// shared evidence/pages, or own documents for the configured member.
    /// Does not wait for the writer lock.
    fn get(&self, _id: &str) -> Result<Option<DocumentRecord>> {
        Err(Error::LibraryUnavailable)
    }
    /// Indexed documents whose frontmatter `cites` (`<id>` or
    /// `<id>#<anchor>`) or `related` names `id`, sorted by id.
    fn backlinks(&self, _id: &str) -> Result<Vec<Backlink>> {
        Err(Error::LibraryUnavailable)
    }
    /// Every own and valid shared knowledge page, in virtual path order.
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
    /// Read and scrub a contained session transcript, without writing it.
    fn session_transcript(
        &self,
        _path: &str,
        _session_id: &str,
    ) -> Result<crate::domain::source::conversation::Session> {
        Err(Error::LibraryUnavailable)
    }
    /// Read and scrub a contained session transcript for local, model-free
    /// marker detection (specs/scribe.md, "Privacy"). Unlike
    /// [`Library::session_transcript`], a private session keeps its lines:
    /// the caller writes them only to the private scribe parse and never
    /// returns them.
    fn scribe_transcript(
        &self,
        _path: &str,
        _session_id: &str,
    ) -> Result<crate::domain::source::conversation::Session> {
        Err(Error::LibraryUnavailable)
    }
    /// Append one decision ledger entry under the writer lock and commit it
    /// (specs/scribe.md, Decision ledger). The adapter assigns the next
    /// `dec-<scope>-<n>` id over `decision.id`, creates the scope's ledger on
    /// its first entry (`captured_at` becomes its `created_at`), refuses a
    /// `reverses` naming no entry, and never edits an earlier line. The
    /// caller has already checked the verbatim fields against their refs.
    fn append_decision(
        &self,
        _decision: &crate::domain::source::decisions::Decision,
        _captured_at: &str,
    ) -> Result<DecisionOutcome> {
        Err(Error::LibraryUnavailable)
    }
    /// Re-read a transcript and atomically create or version its conversation.
    fn capture_session(
        &self,
        _request: &crate::domain::source::conversation::Request,
        _captured_at: &str,
    ) -> Result<SourceOutcome> {
        Err(Error::LibraryUnavailable)
    }
    /// Read one source by id for display at an optional anchor.
    fn read_source(&self, _id: &str) -> Result<SourceView> {
        Err(Error::LibraryUnavailable)
    }
    /// Read an original or named frame up to source::access::MEDIA_LIMIT bytes.
    /// Validate the latest source's private-route rules before payload access;
    /// hold a shared writer lock so a sensitivity upgrade cannot race the read.
    fn source_media(
        &self,
        _id: &str,
        _selection: &crate::domain::source::access::MediaSelection,
    ) -> Result<crate::domain::source::access::SourceMedia> {
        Err(Error::LibraryUnavailable)
    }
    /// Rewrite sensitivity under the writer lock; private cannot become normal.
    /// Preserve unrelated source metadata, reindex and commit once; no-op updates
    /// do not commit. A refusal leaves the source, index and history unchanged.
    fn set_source_sensitivity(&self, _id: &str, _sensitivity: &str) -> Result<SensitivityOutcome> {
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
    /// Rebuild both layers from files; a missing clone is empty. Returns the
    /// total indexed document count, including own and shared copies.
    fn reindex(&self) -> Result<u32> {
        Err(Error::LibraryUnavailable)
    }
    /// The workspace-relative path of a session's parsed message file,
    /// `<library>/.office/state/scribe/<id>.jsonl`, without I/O.
    fn scribe_parse_path(&self, _session_id: &str) -> Result<String> {
        Err(Error::LibraryUnavailable)
    }
    /// Write a session's parsed scribe messages and its session facts
    /// (`<id>.jsonl` and `<id>.meta.json`) under the Library's git-ignored
    /// `.office/state/scribe/`, owner-only, replacing earlier copies.
    /// Returns the workspace-relative path of the `.jsonl`.
    fn write_scribe_parse(
        &self,
        _session_id: &str,
        _messages: &str,
        _meta: &str,
    ) -> Result<String> {
        Err(Error::LibraryUnavailable)
    }
    /// Read a session's parsed messages and session facts, if present.
    fn read_scribe_parse(&self, _session_id: &str) -> Result<Option<ScribeParse>> {
        Err(Error::LibraryUnavailable)
    }
    /// Every SCR file (`sources/conversation/<yyyy>/<mm>/SCR-*.md`), in
    /// path order. They are not indexed; symlinks are never followed.
    fn scribe_files(&self) -> Result<Vec<LibraryText>> {
        Err(Error::LibraryUnavailable)
    }
    /// Create, update or rename one SCR file and commit it, under the
    /// writer lock. Refused with `scribe_changed` when the file no longer
    /// holds `previous` (or exists when `previous` is `None`), and with
    /// `scribe_exists` when a rename target exists.
    fn write_scribe_file(&self, _write: &ScribeWrite) -> Result<()> {
        Err(Error::LibraryUnavailable)
    }
    /// Every decision ledger file (`sources/decisions/<scope>.md`) with its
    /// Markdown, in path order. Not parsed: the caller refuses a malformed
    /// one (`decision_ledger_invalid`).
    fn decision_ledgers(&self) -> Result<Vec<LibraryText>> {
        Err(Error::LibraryUnavailable)
    }
    /// Provider scan: retain unreadable ledger identities with no Markdown,
    /// so partial retrieval can report omissions instead of hiding a gap.
    fn decision_precedent_ledgers(&self) -> Result<Vec<LibraryText>> {
        self.decision_ledgers()
    }
    /// FTS5/BM25 lexical overlap on an ephemeral index of eligible decision
    /// texts. Returns only matching ids, with scores normalized to (0, 1].
    /// Never persists an index or ranks private/excluded decision text.
    fn decision_precedent_scores(
        &self,
        _query: &str,
        _entries: &[(String, String)],
    ) -> Result<std::collections::BTreeMap<String, f64>> {
        Err(Error::LibraryUnavailable)
    }
    /// One scribe v2 pass (specs/scribe.md, "Pipeline" step 4): the SCR and
    /// every ledger and preference page file it plans, written and
    /// committed once under the writer lock. Refused with `scribe_changed`,
    /// writing nothing, when the SCR or any file no longer holds what the
    /// pass planned against, or, for a new SCR, when the scope's latest SCR
    /// is no longer the planned `previous`.
    fn write_scribe_pass(&self, _pass: &ScribePass) -> Result<()> {
        Err(Error::LibraryUnavailable)
    }
    /// Write planned decision ledger files and commit them once under the
    /// writer lock (Orbit decision ingestion, decision-precedents.md).
    /// Each file is compare-and-set against `expected` (`None`: a new
    /// ledger) and refused with `decision_ledger_changed`, writing nothing,
    /// when any differs. Only ledger paths are accepted; no files: no write.
    fn write_decision_ledgers(
        &self,
        _files: &[crate::domain::source::conversation::scribe::v2::PlannedFile],
    ) -> Result<()> {
        Err(Error::LibraryUnavailable)
    }
    /// The host's name for SCR provenance; `None` when unknown.
    fn host_name(&self) -> Option<String> {
        None
    }
    /// Library and index health, without creating Library state.
    fn health(&self) -> Result<LibraryHealth> {
        Ok(LibraryHealth {
            initialized: self.is_initialized()?,
            ..LibraryHealth::default()
        })
    }
}

/// A session's parsed scribe files, as read back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScribeParse {
    /// The `.jsonl` message file.
    pub messages: String,
    /// The `.meta.json` session facts.
    pub meta: String,
}

/// One SCR file write: the new text at `path`, replacing `previous`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScribeWrite {
    /// Library-relative SCR path to write.
    pub path: String,
    /// Full new Markdown.
    pub text: String,
    /// The file as read before planning (`path`, Markdown); a different
    /// path is removed in the same commit (a rename).
    pub previous: Option<(String, String)>,
}

/// One scribe v2 pass write.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScribePass {
    /// The SCR file: `previous` is the file as read (`None`: a new SCR).
    pub scr: ScribeWrite,
    /// Ledger (`sources/decisions/<scope>.md`) and preference page
    /// (`knowledge/domain/dom-preferences-<scope>.md`) files.
    pub files: Vec<crate::domain::source::conversation::scribe::v2::PlannedFile>,
    /// For a new SCR: its scope token, its session id and the `previous`
    /// it names, rechecked under the lock.
    pub chain: Option<ScribeChain>,
}

/// The `previous` link a new v2 SCR names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScribeChain {
    /// `global` or `project:<name>`.
    pub scope: String,
    /// The new SCR's session id.
    pub session: String,
    /// The scope's latest other SCR as planned.
    pub previous: Option<String>,
}

/// One conversation entry to append (specs/sessions.md).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConversationEntry {
    /// A decision-log kind, including correction.
    pub kind: String,
    /// Entry text, or a canonical correction JSON payload.
    pub text: String,
    /// Explicit user attribution (a caller claim, not authority).
    pub by: Option<String>,
    /// `scope: global` on a handoff (specs/sessions.md#scopes); `None` is
    /// the project thread. Stored as the `(global …)` marker leading the text.
    pub global: Option<crate::domain::session::GlobalScope>,
}

impl ConversationEntry {
    /// Format one physical append-only line, preserving attributed user text.
    pub fn format(&self, at: &str) -> Result<String> {
        use crate::domain::session::looks_global;
        use crate::domain::source::{conversation::corrections, format_conversation_entry};
        if let Some(scope) = &self.global {
            if self.kind != "handoff" || self.by.is_some() {
                return Err(crate::Error::InvalidArgument(
                    "scope: global applies only to handoff entries without by".into(),
                ));
            }
            let text = self.text.trim();
            if text.is_empty() || looks_global(text) {
                return Err(crate::Error::InvalidArgument(
                    "a global handoff needs nonempty text without its own (global) marker".into(),
                ));
            }
            if let Some(until) = scope.until.as_deref() {
                let until_at = crate::domain::metrics::timestamp(until);
                let now = crate::domain::metrics::timestamp(at);
                if until_at.is_none()
                    || until.contains(char::is_whitespace)
                    || until.contains(')')
                    || until_at <= now
                {
                    return Err(crate::Error::InvalidArgument(
                        "handoff until must be an RFC 3339 time after the capture time".into(),
                    ));
                }
            }
            return format_conversation_entry(at, "handoff", &format!("{} {text}", scope.marker()));
        }
        if self.kind == "handoff" && looks_global(self.text.trim()) {
            return Err(crate::Error::InvalidArgument(
                "a handoff becomes global only through scope: global, never through its text"
                    .into(),
            ));
        }
        match self.by.as_deref() {
            Some(by) => {
                corrections::validate_user_text(&self.kind, &self.text, by)?;
                let text = serde_json::to_string(&self.text).map_err(|_| {
                    crate::Error::InvalidArgument("cannot encode user entry".into())
                })?;
                Ok(format!("[{at}] {} (by: user): {text}", self.kind))
            }
            None => format_conversation_entry(at, &self.kind, &self.text),
        }
    }
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
    /// Calendar identity copied by event-backed capture.
    pub event: Option<crate::domain::source::SourceEvent>,
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
    /// Session capture only: `written`, or why a crew-written scribe
    /// handoff was dropped. `None` when no handoff was offered.
    pub handoff: Option<String>,
}

/// The ledger entry `office.capture`'s `decision` input appended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecisionOutcome {
    /// The entry as read back, with its derived fields.
    pub entry: crate::domain::source::decisions::EntryView,
    /// Library-relative ledger path.
    pub path: String,
    /// The ledger's title.
    pub title: String,
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
    /// Decision ledger entries: every entry for a ledger id, the one entry
    /// for an entry id or a migrated source id; empty for other sources.
    pub decisions: Vec<crate::domain::source::decisions::EntryView>,
    /// The migrated source id this read resolved through, if any.
    pub moved_from: Option<String>,
}

/// Source privacy update and whether it revoked sharing in the same commit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SensitivityOutcome {
    /// Source after the atomic update.
    pub source: SourceView,
    /// True only when this call cleared sharing approval.
    pub unshared: bool,
}
