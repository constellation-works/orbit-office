//! In-memory Library port with document, search and history behavior.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::Path;
use std::rc::Rc;

use office_common::Sensitivity;
use office_core::domain::knowledge::{
    Backlink, DocumentKind, DocumentRecord, LinkKind, Revision, SearchFilter, SearchHit,
};
use office_core::domain::settings::Settings;
use office_core::domain::source::{
    CaptureSourceRequest, LibraryText, RenderSource, SourceKind, is_video_extension,
    media_type_for_extension, normalize_text, render_source, sha256_hex, source_id_base,
    unique_source_id, vtt_to_body,
};
use office_core::domain::template::BUNDLED;
use office_core::ports::{
    InitOutcome, Library, LibraryHealth, SourceOutcome, SourceView, not_initialized_error,
};
use office_core::{Error, Result};
use std::path::PathBuf;

/// A Library fake with explicit readiness and settings, and no host I/O.
#[derive(Default)]
pub struct FakeLibrary {
    bootstrap_batches: RefCell<BTreeMap<String, office_core::domain::bootstrap::Batch>>,
    bootstrap_digests: RefCell<BTreeMap<String, serde_json::Value>>,
    originals: RefCell<BTreeMap<String, Vec<u8>>>,
    artifact_files: RefCell<BTreeMap<String, Vec<u8>>>,
    documents: RefCell<BTreeMap<String, DocumentRecord>>,
    texts: RefCell<BTreeMap<String, String>>,
    revisions: RefCell<BTreeMap<String, Vec<Revision>>>,
    now_initialized: RefCell<Option<bool>>,
    current_settings: RefCell<Option<Settings>>,
    /// Whether the fixture represents an initialized Library.
    pub initialized: bool,
    /// Settings returned by the fake (validated on every load).
    pub settings: Settings,
    /// Pages the fixture reports as no longer meeting their template.
    pub pages_off_template: u32,
    /// Template files the fixture reports as unreadable or invalid.
    pub template_invalid: Vec<String>,
    sources: RefCell<BTreeMap<String, SourceView>>,
    /// Canonical roots upload/transcript paths must stay inside. Empty
    /// means any existing file may be read (unit-test convenience); the
    /// real adapter always sets its workspace root.
    pub allowed_upload_roots: Vec<PathBuf>,
    /// Virtual session paths mapped to raw JSONL; session tests never read host files.
    pub session_transcripts: BTreeMap<String, String>,
    /// Files returned by `templates()`, in the order given.
    pub templates: Vec<LibraryText>,
    commits: RefCell<usize>,
    staging: FakeStaging,
    /// SCR files by Library path, in memory.
    scribe_texts: RefCell<BTreeMap<String, String>>,
    /// Explicit normalized FTS scores for precedent-provider fixtures.
    pub decision_precedent_scores: BTreeMap<String, f64>,
}

/// What `stage_attachment` wrote, shared across clones so a test keeps an
/// observer after the fake moves into boxed ports.
#[derive(Clone, Default)]
pub struct FakeStaging {
    state: Rc<RefCell<StagingState>>,
}

#[derive(Default)]
struct StagingState {
    written: BTreeMap<String, String>,
    removed: Vec<String>,
}

impl FakeStaging {
    /// The text staged at `path`, kept after removal so a test can read
    /// what an artifact copy carried.
    #[must_use]
    pub fn text(&self, path: &str) -> Option<String> {
        self.state.borrow().written.get(path).cloned()
    }

    /// Every staged path, in path order.
    #[must_use]
    pub fn written(&self) -> Vec<String> {
        self.state.borrow().written.keys().cloned().collect()
    }

    /// Staged paths not yet removed.
    #[must_use]
    pub fn live(&self) -> Vec<String> {
        let state = self.state.borrow();
        state
            .written
            .keys()
            .filter(|path| !state.removed.contains(path))
            .cloned()
            .collect()
    }
}

mod capture_session;
mod capture_source;
mod documents;
mod helpers;
mod media;
mod port;
mod scribe;
mod search;
