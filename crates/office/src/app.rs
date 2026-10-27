//! Compose services from the call context and dispatch one request.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use office_core::domain::knowledge::{DocumentRecord, Revision, SearchFilter, SearchHit};
use office_core::domain::settings::Settings;
use office_core::ports::{Clock, InitOutcome, Library, LibraryHealth, Models, Orbit, Ports};
use office_core::protocol::{self, Context};
use office_store::LibraryStore;
use serde_json::Value;

use crate::error::Result;

// Models and Orbit adapters arrive with their first port operations.
// In particular, constructing these handles does no I/O.
struct ContextHandle<'a> {
    #[expect(dead_code, reason = "adapter operations will use the call context")]
    context: &'a Context,
}
impl Models for ContextHandle<'_> {}
impl Orbit for ContextHandle<'_> {}
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
        Ok(LibraryStore::open(library, state)?)
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
    fn search(&self, query: &str, filter: &SearchFilter) -> office_core::Result<Vec<SearchHit>> {
        Ok(LibraryStore::search(&self.store()?, query, filter)?)
    }
    fn history(&self, relative: &Path) -> office_core::Result<Vec<Revision>> {
        Ok(LibraryStore::history(&self.store()?, relative)?)
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
    Ports {
        library: Box::new(RealLibrary::from_context(context)),
        models: Box::new(ContextHandle { context }),
        orbit: Box::new(ContextHandle { context }),
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
