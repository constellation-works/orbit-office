//! Compose services from the call context and dispatch one request.

use std::io::Read;
use std::path::PathBuf;
use std::time::SystemTime;

use office_core::domain::selection::{PutRequest, PutResult, Selection as SelectionRecord};
use office_core::ports::{
    Clock, InitOutcome, LibraryHealth, Models, Orbit, Ports, Routed, Selections,
};
use office_core::ports::{SharedRepository, UnavailableShared};
use office_core::protocol::{self, Context};
use office_orbit::{CALLBACK_ENV_NAMES, OrbitCallbacks};
use office_store::{GhPullRequests, LibraryStore, SharedStore};
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
        let settings = self.library.store()?.settings()?;
        if office_core::domain::source::video::settings_digest(&settings)? != expected {
            return Err(office_core::domain::source::video::refusal(
                "plan_changed",
                "model route changed after confirmation",
            ));
        }
        self.client_with_settings(settings)
    }
    fn client(&self) -> office_core::Result<office_models::RoutedClient> {
        self.client_with_settings(self.library.store()?.settings()?)
    }
    fn client_with_settings(
        &self,
        settings: office_core::domain::settings::Settings,
    ) -> office_core::Result<office_models::RoutedClient> {
        let secret =
            |name: &str| office_core::protocol::secret(self.context, name).map(str::to_string);
        let mut config = office_models::ModelConfig::with_models(
            secret("meta_api_key"),
            secret("open_weights_token"),
            &settings.models,
        );
        // Settings that fail to load are reported as their own error (for
        // example `invalid_library_setting` naming the offending key), not
        // swallowed into an unset `open_weights` block: that would misreport
        // the verified cause as `private_endpoint_unconfigured`. A load that
        // succeeds with no block still refuses private material rather than
        // sending it hosted.
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
        // Authored local events live in the Library, inside its write grant.
        let library = workspace
            .join(context_library_dir(self.context).unwrap_or_else(|| "library".to_string()));
        LibraryStore::confine_to_grant(&library, &workspace)?;
        Ok(office_store::CalendarStore::new(state, workspace, library))
    }
}
impl office_core::ports::Calendar for RealCalendar<'_> {
    fn sources(&self) -> office_core::Result<office_core::domain::calendar::CalendarSources> {
        self.store()?.sources()
    }
    fn change_local(
        &self,
        change: &office_core::domain::calendar::LocalChange,
    ) -> office_core::Result<Option<String>> {
        self.store()?.change_local(change)
    }

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

/// The real Library opener for one call. Construction does no I/O: the
/// store is opened lazily by operations that need it, and health checks
/// that find no Library create nothing (so `office.status` writes nothing).
/// [`office_store::LazyLibrary`] turns this opener into the Library port.
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

impl RealLibrary {
    fn library_is_initialized(&self) -> office_core::Result<bool> {
        if self.library.is_none() {
            return Err(office_core::Error::LibraryUnavailable);
        }
        self.ensure_grant()?;
        if !self.looks_initialized() {
            return Ok(false);
        }
        Ok(LibraryStore::is_initialized(&self.store()?)?)
    }

    fn library_health(&self) -> office_core::Result<LibraryHealth> {
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

    fn library_initialize(&self, library_dir: Option<&str>) -> office_core::Result<InitOutcome> {
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
    let library = std::sync::Arc::new(library);
    let opened = {
        let library = std::sync::Arc::clone(&library);
        move || library.store()
    };
    let checked_initialized = {
        let library = std::sync::Arc::clone(&library);
        move || library.library_is_initialized()
    };
    let checked_health = {
        let library = std::sync::Arc::clone(&library);
        move || library.library_health()
    };
    let initialize = move |library_dir: Option<&str>| library.library_initialize(library_dir);
    Ports {
        scripts: Box::new(ContextScripts { context }),
        library: Box::new(office_store::LazyLibrary::new(
            opened,
            checked_initialized,
            checked_health,
            initialize,
        )),
        models: Box::new(ContextModels {
            context,
            library: RealLibrary::from_context(context),
        }),
        orbit,
        clock: Box::new(SystemClock),
        selections: Box::new(RealSelections { context }),
        shared: shared(context),
        calendar: Box::new(RealCalendar { context }),
        pr: Box::new(GhPullRequests::new()),
    }
}

/// The shared library clone under `plugin_state/shared/`. Git children get
/// only the allowlisted environment (STD-05@1).
fn shared(context: &Context) -> Box<dyn SharedRepository + '_> {
    match context_state_dir(context) {
        Some(state) => Box::new(SharedStore::new(
            state,
            office_store::git_environment(|name| std::env::var_os(name)),
        )),
        None => Box::new(UnavailableShared),
    }
}

/// Serve one Orbit exec call and return the payload for stdout.
pub(crate) fn serve_exec() -> Result<String> {
    let mut raw = String::new();
    let _ = std::io::stdin().read_to_string(&mut raw);
    Ok(match protocol::parse(&raw) {
        Ok(request) => office_tools::serve(
            &request,
            &ports(
                &request.context,
                matches!(
                    request.tool.as_str(),
                    "office.bootstrap-status" | "office.decision-precedents"
                ),
            ),
        ),
        Err(error) => protocol::respond(Err(error)),
    })
}
