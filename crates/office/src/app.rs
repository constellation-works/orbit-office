//! Compose services from the call context and dispatch one request.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use office_core::domain::knowledge::{Backlink, DocumentRecord, Revision, SearchFilter, SearchHit};
use office_core::domain::selection::{PutRequest, PutResult, Selection as SelectionRecord};
use office_core::domain::settings::Settings;
use office_core::ports::{
    Clock, InitOutcome, Library, LibraryHealth, LibraryText, Models, Orbit, Ports, Routed,
    Selections,
};
use office_core::protocol::{self, Context};
use office_orbit::{CALLBACK_ENV_NAMES, OrbitCallbacks};
use office_store::LibraryStore;
use serde_json::Value;

use crate::error::Result;

// Adapters are constructed per request; constructing their handles does no I/O.
struct ContextHandle<'a> {
    #[expect(dead_code, reason = "adapter operations will use the call context")]
    context: &'a Context,
}
impl Orbit for ContextHandle<'_> {}

/// The real model adapter for one call. Construction does no I/O: the
/// routed client is built per operation from the context's secrets and
/// the Library's `open_weights` block, so a call without a Library or
/// secrets still constructs and only the operation itself can refuse.
struct ContextModels<'a> {
    context: &'a Context,
    library: RealLibrary,
}

impl ContextModels<'_> {
    fn planned_client(&self, expected: &str) -> office_core::Result<office_models::RoutedClient> {
        let settings = self.library.settings()?;
        if office_core::domain::source::video::settings_digest(&settings)? != expected {
            return Err(office_core::domain::source::video::refusal(
                "plan_changed",
                "model route changed after confirmation",
            ));
        }
        self.client_with_settings(settings)
    }
    fn client(&self) -> office_core::Result<office_models::RoutedClient> {
        self.client_with_settings(self.library.settings()?)
    }
    fn client_with_settings(
        &self,
        settings: office_core::domain::settings::Settings,
    ) -> office_core::Result<office_models::RoutedClient> {
        let secret = |name: &str| {
            self.context
                .get("secrets")
                .and_then(|secrets| secrets.get(name))
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        };
        let mut config =
            office_models::ModelConfig::new(secret("meta_api_key"), secret("open_weights_token"));
        // Settings that fail to load are reported as their own error (for
        // example `invalid_library_setting` naming the offending key), not
        // swallowed into an unset `open_weights` block: that would misreport
        // the verified cause as `private_endpoint_unconfigured`. A load that
        // succeeds with no block still refuses private material rather than
        // sending it hosted.
        config.careful_model = settings.models.careful;
        config.open_weights = settings.open_weights;
        Ok(office_models::RoutedClient::new(config))
    }
}

impl Models for ContextModels<'_> {
    fn media_audio(
        &self,
        bytes: &[u8],
        start_seconds: u64,
        length_seconds: u64,
    ) -> office_core::Result<Vec<u8>> {
        office_models::media::VideoProcessor::default().audio(bytes, start_seconds, length_seconds)
    }
    fn transcribe_planned(
        &self,
        request: &office_core::ports::TranscribeRequest,
        expected: &str,
    ) -> office_core::Result<Routed<office_core::ports::Transcript>> {
        self.planned_client(expected)?
            .transcribe_planned(request, expected)
    }
    fn extract_planned(
        &self,
        request: &office_core::ports::ExtractRequest,
        expected: &str,
    ) -> office_core::Result<Routed<Value>> {
        self.planned_client(expected)?.extract(request)
    }
    fn analyze_video(
        &self,
        bytes: &[u8],
        threshold: &serde_json::Number,
    ) -> office_core::Result<office_core::domain::source::video::VideoAnalysis> {
        office_models::media::VideoProcessor::default().analyze(bytes, threshold)
    }
    fn video_frame(&self, bytes: &[u8], time_ms: u64) -> office_core::Result<Vec<u8>> {
        office_models::media::VideoProcessor::default().frame(bytes, time_ms)
    }

    fn transcribe(
        &self,
        request: &office_core::ports::TranscribeRequest,
    ) -> office_core::Result<Routed<office_core::ports::Transcript>> {
        self.client()?.transcribe(request)
    }
    fn extract(
        &self,
        request: &office_core::ports::ExtractRequest,
    ) -> office_core::Result<Routed<Value>> {
        self.client()?.extract(request)
    }
    fn read_planned_image(
        &self,
        request: &office_core::ports::ImageRequest,
        expected: &str,
    ) -> office_core::Result<Routed<office_core::ports::ImageReading>> {
        self.planned_client(expected)?.read_image(request)
    }
    fn read_image(
        &self,
        request: &office_core::ports::ImageRequest,
    ) -> office_core::Result<Routed<office_core::ports::ImageReading>> {
        self.client()?.read_image(request)
    }
    fn check_private_route(&self) -> office_core::Result<office_core::ports::RouteCheck> {
        self.client()?.check_private_route()
    }
}
// Resolve the same grant as every Library caller before constructing a runner.
struct ContextScripts<'a> {
    context: &'a Context,
}
impl office_core::ports::ScriptRunner for ContextScripts<'_> {
    fn prepare(
        &self,
        script: &office_core::domain::script::ScriptPreparation,
    ) -> office_core::Result<()> {
        self.runner()?.prepare(script)
    }
    fn run(
        &self,
        execution: &office_core::domain::script::ScriptExecution,
        bounds: &office_core::domain::script::ScriptBounds,
    ) -> office_core::Result<(
        office_core::domain::script::ScriptResult,
        Vec<office_core::domain::script::ScriptFile>,
    )> {
        let mut execution = execution.clone();
        if execution.run_id.is_none() {
            execution.run_id = std::env::var("ORBIT_RUN_ID").ok();
        }
        self.runner()?.run(&execution, bounds)
    }
}
impl ContextScripts<'_> {
    fn runner(&self) -> office_core::Result<office_store::ScriptStore> {
        let library = RealLibrary::from_context(self.context);
        library.ensure_grant()?;
        let root = library
            .library
            .ok_or(office_core::Error::LibraryUnavailable)?;
        let state = library
            .state
            .ok_or(office_core::Error::LibraryUnavailable)?;
        let environment = ["PATH", "LANG", "TZ"]
            .iter()
            .filter_map(|name| std::env::var_os(name).map(|value| ((*name).into(), value)))
            .collect();
        // Match the existing Library attachment adapter's granted staging root.
        let staging = root.join(".office/attach");
        Ok(office_store::ScriptStore::new(
            root,
            state,
            staging,
            environment,
        ))
    }
}
struct SystemClock;
impl Clock for SystemClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}

fn context_path(context: &Context, keys: &[&str]) -> Option<PathBuf> {
    keys.iter()
        .find_map(|key| context.get(*key).and_then(Value::as_str))
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn context_state_dir(context: &Context) -> Option<PathBuf> {
    context_path(context, &["plugin_state", "plugin_state_dir", "state_dir"]).or_else(|| {
        let workspace = context_path(context, &["workspace_root", "workspace", "workspace_dir"])?;
        let library =
            workspace.join(context_library_dir(context).unwrap_or_else(|| "library".to_string()));
        Some(library.join(".office/state"))
    })
}

/// Selection tray at `plugin_state/selections.sqlite`, separate from the index.
struct RealCalendar<'a> {
    context: &'a Context,
}
impl RealCalendar<'_> {
    fn store(&self) -> office_core::Result<office_store::CalendarStore> {
        let state = context_state_dir(self.context)
            .ok_or(office_core::domain::calendar::CalendarError::Storage)?;
        let workspace = context_path(
            self.context,
            &["workspace_root", "workspace", "workspace_dir"],
        )
        .ok_or(office_core::domain::calendar::CalendarError::Storage)?;
        Ok(office_store::CalendarStore::new(state, workspace))
    }
}
impl office_core::ports::Calendar for RealCalendar<'_> {
    fn load(
        &self,
    ) -> office_core::Result<(office_core::domain::calendar::Snapshot, Option<String>)> {
        self.store()?.load()
    }
    fn replace(
        &self,
        snapshot: &office_core::domain::calendar::Snapshot,
    ) -> office_core::Result<()> {
        self.store()?.replace(snapshot)
    }
    fn record_error(&self, code: &str) -> office_core::Result<()> {
        self.store()?.record_error(code)
    }
    fn read_ics(&self, path: &str) -> office_core::Result<String> {
        self.store()?.read_ics(path)
    }
}

struct RealSelections<'a> {
    context: &'a Context,
}

impl RealSelections<'_> {
    fn open(&self) -> office_core::Result<office_store::SelectionStore> {
        let state = context_state_dir(self.context).ok_or(office_core::Error::Library {
            code: "selection_unavailable",
            message: "selection storage is unavailable in this call context".to_string(),
        })?;
        Ok(office_store::SelectionStore::open(&state)?)
    }
}

impl Selections for RealSelections<'_> {
    fn put(&self, request: &PutRequest, now: SystemTime) -> office_core::Result<PutResult> {
        Ok(self.open()?.put(request, now)?)
    }

    fn peek(&self, now: SystemTime) -> office_core::Result<Vec<SelectionRecord>> {
        Ok(self.open()?.peek(now)?)
    }

    fn get(&self, now: SystemTime) -> office_core::Result<Vec<SelectionRecord>> {
        Ok(self.open()?.get(now)?)
    }
}

fn context_library_dir(context: &Context) -> Option<String> {
    context
        .get("config")
        .and_then(|config| config.get("library_dir"))
        .and_then(Value::as_str)
        .or_else(|| context.get("library_dir").and_then(Value::as_str))
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// The real Library adapter for one call. Construction does no I/O: the
/// store is opened lazily by operations that need it, and health checks
/// that find no Library create nothing (so `office.status` writes nothing).
struct RealLibrary {
    bootstrap_read_only: bool,
    workspace: Option<PathBuf>,
    library: Option<PathBuf>,
    state: Option<PathBuf>,
}

impl RealLibrary {
    fn from_context(context: &Context) -> Self {
        let workspace = context_path(context, &["workspace_root", "workspace", "workspace_dir"]);
        let library = workspace.as_ref().map(|root| {
            root.join(context_library_dir(context).unwrap_or_else(|| "library".to_string()))
        });
        let state = context_state_dir(context);
        Self {
            bootstrap_read_only: false,
            workspace,
            library,
            state,
        }
    }

    /// The grant `initialize_at` enforces, applied before any open.
    ///
    /// `office.init` is not the only caller that can create a Library root:
    /// `LibraryStore::open` creates the layout. Dispatch treats
    /// `is_initialized == false` as `library_not_initialized` and never
    /// reaches the handler, so this also runs before that short-circuit.
    fn ensure_grant(&self) -> office_core::Result<()> {
        let (Some(library), Some(workspace)) = (&self.library, &self.workspace) else {
            return Err(office_core::Error::LibraryUnavailable);
        };
        LibraryStore::confine_to_grant(library, workspace)?;
        Ok(())
    }

    fn store(&self) -> office_core::Result<LibraryStore> {
        let (Some(library), Some(state)) = (&self.library, &self.state) else {
            return Err(office_core::Error::LibraryUnavailable);
        };
        self.ensure_grant()?;
        let store = if self.bootstrap_read_only {
            LibraryStore::open_for_bootstrap_read(library, state)?
        } else {
            LibraryStore::open(library, state)?
        };
        // Uploads live under the workspace (the plugin's read grant); the
        // Library root itself is also an allowed upload root.
        let mut roots = vec![library.clone()];
        if let Some(workspace) = &self.workspace {
            roots.push(workspace.clone());
        }
        Ok(store.with_upload_roots(roots))
    }

    fn looks_initialized(&self) -> bool {
        self.library.as_ref().is_some_and(|root| {
            root.join(".office/config.yaml").exists() && root.join(".git").exists()
        })
    }
}

impl Library for RealLibrary {
    fn source_media(
        &self,
        id: &str,
        selection: &office_core::domain::source::access::MediaSelection,
    ) -> office_core::Result<office_core::domain::source::access::SourceMedia> {
        <LibraryStore as Library>::source_media(&self.store()?, id, selection)
    }
    fn set_source_sensitivity(
        &self,
        id: &str,
        sensitivity: &str,
    ) -> office_core::Result<office_core::ports::SourceView> {
        <LibraryStore as Library>::set_source_sensitivity(&self.store()?, id, sensitivity)
    }
    fn write_reviewed_page(
        &self,
        relative: &Path,
        text: &str,
        expected: Option<&str>,
    ) -> office_core::Result<DocumentRecord> {
        Ok(self
            .store()?
            .write_reviewed_page(relative, text, expected)?)
    }
    fn write_proposal(&self, relative: &Path, text: &str) -> office_core::Result<()> {
        Ok(self.store()?.write_proposal(relative, text)?)
    }

    fn read_media(
        &self,
        id: &str,
    ) -> office_core::Result<office_core::domain::source::video::VideoSource> {
        <LibraryStore as Library>::read_media(&self.store()?, id)
    }
    fn source_sidecar(&self, id: &str, name: &str) -> office_core::Result<Option<String>> {
        <LibraryStore as Library>::source_sidecar(&self.store()?, id, name)
    }
    fn write_source_processing(
        &self,
        id: &str,
        revision: &str,
        body: Option<&str>,
        derived: &office_core::ports::DerivedBy,
        files: &[(String, Vec<u8>)],
    ) -> office_core::Result<office_core::ports::SourceView> {
        <LibraryStore as Library>::write_source_processing(
            &self.store()?,
            id,
            revision,
            body,
            derived,
            files,
        )
    }

    fn bootstrap_snapshot(&self) -> office_core::Result<office_core::domain::bootstrap::Snapshot> {
        <LibraryStore as Library>::bootstrap_snapshot(&self.store()?)
    }
    fn mutate_batch(
        &self,
        revision: &str,
        mutation: office_core::domain::bootstrap::Mutation,
    ) -> office_core::Result<office_core::domain::bootstrap::Batch> {
        <LibraryStore as Library>::mutate_batch(&self.store()?, revision, mutation)
    }
    fn read_video(
        &self,
        id: &str,
    ) -> office_core::Result<office_core::domain::source::video::VideoSource> {
        <LibraryStore as Library>::read_video(&self.store()?, id)
    }
    fn mutate_video(
        &self,
        id: &str,
        revision: &str,
        mutation: office_core::domain::source::video::VideoMutation,
    ) -> office_core::Result<office_core::domain::source::video::VideoState> {
        <LibraryStore as Library>::mutate_video(&self.store()?, id, revision, mutation)
    }

    fn promote_artifact(
        &self,
        artifact: &office_core::domain::artifact::Artifact,
        files: &[office_core::domain::artifact::ArtifactFile],
    ) -> office_core::Result<office_core::domain::artifact::Artifact> {
        Ok(self.store()?.promote_artifact(artifact, files)?)
    }
    fn artifact_documents(&self) -> office_core::Result<Vec<LibraryText>> {
        Ok(self.store()?.artifact_documents()?)
    }
    fn read_artifact_file(&self, relative: &Path) -> office_core::Result<Vec<u8>> {
        Ok(self.store()?.read_artifact_file(relative)?)
    }

    fn read_document(&self, relative: &Path) -> office_core::Result<DocumentRecord> {
        Ok(LibraryStore::read_document(&self.store()?, relative)?)
    }
    fn write_document(&self, relative: &Path, text: &str) -> office_core::Result<DocumentRecord> {
        Ok(LibraryStore::write_document(
            &self.store()?,
            relative,
            text,
        )?)
    }
    fn get(&self, id: &str) -> office_core::Result<Option<DocumentRecord>> {
        Ok(LibraryStore::get(&self.store()?, id)?)
    }
    fn backlinks(&self, id: &str) -> office_core::Result<Vec<Backlink>> {
        Ok(LibraryStore::backlinks(&self.store()?, id)?)
    }
    fn knowledge_pages(&self) -> office_core::Result<Vec<LibraryText>> {
        Ok(LibraryStore::knowledge_pages(&self.store()?)?)
    }
    fn source_documents(&self) -> office_core::Result<Vec<LibraryText>> {
        Ok(LibraryStore::source_documents(&self.store()?)?)
    }
    fn templates(&self) -> office_core::Result<Vec<LibraryText>> {
        Ok(LibraryStore::templates(&self.store()?)?)
    }
    fn search(&self, query: &str, filter: &SearchFilter) -> office_core::Result<Vec<SearchHit>> {
        Ok(LibraryStore::search(&self.store()?, query, filter)?)
    }
    fn history(&self, relative: &Path) -> office_core::Result<Vec<Revision>> {
        Ok(LibraryStore::history(&self.store()?, relative)?)
    }
    fn read_text(&self, relative: &Path) -> office_core::Result<String> {
        Ok(LibraryStore::read_text(&self.store()?, relative)?)
    }
    fn head_commit(&self) -> office_core::Result<Option<String>> {
        Ok(LibraryStore::head_commit(&self.store()?)?)
    }
    fn stage_attachment(&self, name: &str, text: &str) -> office_core::Result<String> {
        <LibraryStore as Library>::stage_attachment(&self.store()?, name, text)
    }
    fn remove_staged(&self, staged: &str) -> office_core::Result<()> {
        <LibraryStore as Library>::remove_staged(&self.store()?, staged)
    }
    fn is_initialized(&self) -> office_core::Result<bool> {
        if self.library.is_none() {
            return Err(office_core::Error::LibraryUnavailable);
        }
        self.ensure_grant()?;
        if !self.looks_initialized() {
            return Ok(false);
        }
        Ok(LibraryStore::is_initialized(&self.store()?)?)
    }
    fn settings(&self) -> office_core::Result<Settings> {
        Ok(LibraryStore::settings(&self.store()?)?)
    }
    fn set_setting(&self, key: &str, value: Value) -> office_core::Result<()> {
        Ok(LibraryStore::set_setting(&self.store()?, key, value)?)
    }
    fn record_route_check(
        &self,
        endpoint: &str,
        model: &str,
        capabilities: &[office_core::domain::endpoint::Capability],
        verified_at: &str,
    ) -> office_core::Result<()> {
        Ok(LibraryStore::record_route_check(
            &self.store()?,
            endpoint,
            model,
            capabilities,
            verified_at,
        )?)
    }
    fn capture_source(
        &self,
        request: &office_core::ports::CaptureSourceRequest,
    ) -> office_core::Result<office_core::ports::SourceOutcome> {
        LibraryStore::capture_source(&self.store()?, request)
    }
    fn read_source(&self, id: &str) -> office_core::Result<office_core::ports::SourceView> {
        LibraryStore::read_source(&self.store()?, id)
    }
    fn initialize(&self, library_dir: Option<&str>) -> office_core::Result<InitOutcome> {
        let (Some(workspace), Some(library), Some(state)) =
            (&self.workspace, &self.library, &self.state)
        else {
            return Err(office_core::Error::LibraryUnavailable);
        };
        Ok(LibraryStore::initialize_at(
            library,
            state,
            library_dir,
            workspace,
        )?)
    }
    fn reindex(&self) -> office_core::Result<u32> {
        let store = self.store()?;
        <LibraryStore as Library>::reindex(&store)
    }
    fn health(&self) -> office_core::Result<LibraryHealth> {
        // No Library in context stays uninitialized. A configured root
        // outside the grant is the typed refusal, including when the
        // escaped tree is absent, so status does not report it as healthy.
        if self.library.is_some() {
            self.ensure_grant()?;
        }
        if !self.looks_initialized() {
            return Ok(LibraryHealth::uninitialized());
        }
        Ok(LibraryStore::health(&self.store()?)?)
    }
}

fn ports(context: &Context, bootstrap_read_only: bool) -> Ports<'_> {
    let orbit: Box<dyn Orbit> = if let Some(workspace) =
        context_path(context, &["workspace_root", "workspace", "workspace_dir"])
    {
        let program = std::env::var_os("ORBIT_BIN").unwrap_or_else(|| "orbit".into());
        let environment = CALLBACK_ENV_NAMES.iter().filter_map(|name| {
            std::env::var_os(name).map(|value| (std::ffi::OsString::from(name), value))
        });
        Box::new(OrbitCallbacks::new(program, workspace, environment))
    } else {
        Box::new(ContextHandle { context })
    };
    let mut library = RealLibrary::from_context(context);
    library.bootstrap_read_only = bootstrap_read_only;
    Ports {
        scripts: Box::new(ContextScripts { context }),
        library: Box::new(library),
        models: Box::new(ContextModels {
            context,
            library: RealLibrary::from_context(context),
        }),
        orbit,
        clock: Box::new(SystemClock),
        selections: Box::new(RealSelections { context }),
        calendar: Box::new(RealCalendar { context }),
    }
}

/// Serve one Orbit exec call and return the payload for stdout.
pub(crate) fn serve_exec() -> Result<String> {
    let mut raw = String::new();
    let _ = std::io::stdin().read_to_string(&mut raw);
    Ok(match protocol::parse(&raw) {
        Ok(request) => office_tools::serve(
            &request,
            &ports(&request.context, request.tool == "office.bootstrap-status"),
        ),
        Err(error) => protocol::respond(Err(error)),
    })
}
