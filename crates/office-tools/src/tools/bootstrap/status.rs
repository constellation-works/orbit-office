//! `office.bootstrap-status`: complete per-phase progress and computed gaps.
use office_core::domain::bootstrap::status;
use office_core::domain::source::format_rfc3339_utc;
use office_core::tool::{ExecutionKind, McpScope, ToolSpec};
use office_core::{Error, Result};
use serde_json::{Value, json};

use crate::registry::{Call, Tool};

pub(crate) const TOOL: Tool = Tool::built(
    ToolSpec {
        name: "bootstrap-status",
        execution_kind: ExecutionKind::ReadOnly,
        mcp_scope: McpScope::Workspace,
    },
    &[],
    run,
);
fn run(call: &Call<'_>) -> Result<Value> {
    let snapshot = call.ports.library.bootstrap_snapshot()?;
    let settings = call.ports.library.settings()?;
    let revision = snapshot.revision(&settings)?;
    let input = json!({"limit":10000,"fields":["id","status","tags"]});
    let tasks = call
        .ports
        .orbit
        .task_list(input.as_object().ok_or(Error::OrbitUnavailable)?)?;
    let tasks = status::tasks(&tasks)?;
    let now = format_rfc3339_utc(call.ports.clock.now());
    let now = office_core::domain::metrics::timestamp(&now).ok_or_else(|| {
        Error::InvalidArgument("clock is outside the supported date range".into())
    })?;
    let phases = status::compute(&snapshot, tasks, now, settings.thresholds.stale_days)?;
    let after = call.ports.library.bootstrap_snapshot()?;
    if after.revision(&call.ports.library.settings()?)? != revision {
        return Err(Error::Library {
            code: "library_changed",
            message: "Library changed during bootstrap status; retry".into(),
        });
    }
    let pending_batches = office_core::domain::bootstrap::pending_batches(&after, 256);
    Ok(json!({"phases":phases,"pending_batches":pending_batches}))
}
