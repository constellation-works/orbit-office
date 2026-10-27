//! Compose services from the call context and dispatch one request.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use office_core::domain::knowledge::{Backlink, DocumentRecord, Revision, SearchFilter, SearchHit};
use office_core::domain::settings::Settings;
use office_core::ports::{
    Clock, InitOutcome, Library, LibraryHealth, LibraryText, Models, Orbit, Ports, Routed,
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
    fn client(&self) -> office_models::RoutedClient {
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
        // No Library, or settings that fail validation, mean no private
        // route, which refuses private material rather than sending it hosted.
        config.open_weights = self
            .library
            .settings()
            .ok()
            .and_then(|settings| settings.open_weights);
        office_models::RoutedClient::new(config)
    }
}

impl Models for ContextModels<'_> {
    fn transcribe(
        &self,
        request: &office_core::ports::TranscribeRequest,
    ) -> office_core::Result<Routed<office_core::ports::Transcript>> {
        self.client().transcribe(request)
    }
    fn extract(
        &self,
        request: &office_core::ports::ExtractRequest,
    ) -> office_core::Result<Routed<Value>> {
        self.client().extract(request)
    }
    fn read_image(
        &self,
        request: &office_core::ports::ImageRequest,
    ) -> office_core::Result<Routed<office_core::ports::ImageReading>> {
        self.client().read_image(request)
    }
    fn check_private_route(&self) -> office_core::Result<office_core::ports::RouteCheck> {
        self.client().check_private_route()
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
        let state = context_path(context, &["plugin_state", "plugin_state_dir", "state_dir"])
            .or_else(|| library.as_ref().map(|root| root.join(".office/state")));
        Self {
            workspace,
            library,
            state,
        }
    }

    fn store(&self) -> office_core::Result<LibraryStore> {
        let (Some(library), Some(state)) = (&self.library, &self.state) else {
            return Err(office_core::Error::LibraryUnavailable);
        };
        let store = LibraryStore::open(library, state)?;
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
        if !self.looks_initialized() {
            return Ok(LibraryHealth::uninitialized());
        }
        Ok(LibraryStore::health(&self.store()?)?)
    }
}

fn ports(context: &Context) -> Ports<'_> {
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
    Ports {
        library: Box::new(RealLibrary::from_context(context)),
        models: Box::new(ContextModels {
            context,
            library: RealLibrary::from_context(context),
        }),
        orbit,
        clock: Box::new(SystemClock),
    }
}

/// Serve one Orbit exec call and return the payload for stdout.
pub(crate) fn serve_exec() -> Result<String> {
    let mut raw = String::new();
    let _ = std::io::stdin().read_to_string(&mut raw);
    Ok(match protocol::parse(&raw) {
        Ok(request) => office_tools::serve(&request, &ports(&request.context)),
        Err(error) => protocol::respond(Err(error)),
    })
}
