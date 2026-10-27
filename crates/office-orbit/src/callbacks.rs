//! The `orbit_tools` callbacks the plugin may make during a call (specs/plugin-surface.md).

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Command;

use office_core::ports::Orbit;
use serde_json::{Map, Value};

use crate::{Error, Result};

/// Office's complete callback allowlist; no namespace or wildcard expansion.
pub const ORBIT_TOOLS: [&str; 8] = [
    "orbit.task.add",
    "orbit.task.update",
    "orbit.task.list",
    "orbit.task.show",
    "orbit.task.artifact.get",
    "orbit.task.artifact.put",
    "orbit.friction.add",
    "orbit.search",
];

/// Non-secret environment names needed by the callback CLI. Composition reads
/// only these names; construction filters again so no caller can pass an
/// operator override, claim token or ambient provider credential accidentally.
/// The credential itself remains on the host's inherited descriptor.
pub const CALLBACK_ENV_NAMES: &[&str] = &[
    "HOME",
    "PATH",
    "LANG",
    "LC_ALL",
    "TMPDIR",
    "TZ",
    "ORBIT_ROOT",
    "ORBIT_REGISTRY_ROOT",
    "ORBIT_WORKSPACE",
    "ORBIT_WORKTREE_ROOT",
    "ORBIT_RUN_ID",
    "ORBIT_MANAGED_RUN_CONTEXT",
    "ORBIT_AGENT_NAME",
    "ORBIT_AGENT_MODEL",
    "ORBIT_SESSION_ID",
    "ORBIT_TASK_ID",
    "ORBIT_ACTIVE_TASK_ID",
    "ORBIT_SCRATCH_DIR",
    "ORBIT_STEP_INDEX",
    "ORBIT_TASK_ACTOR_KIND",
    "ORBIT_PLUGIN",
    "ORBIT_PLUGIN_CALLBACK_FD",
    "ORBIT_PLUGIN_BROKER",
];

/// Synchronous Orbit plugin callback adapter. Roots, executable and environment
/// are resolved by composition. Construction performs no I/O.
///
/// Each call runs `orbit tool run <name> --json --full --input <json>` with a
/// cleared, allowlisted environment. The host's open callback descriptor is
/// inherited unchanged: it is neither read nor copied into argv or input.
/// Orbit authenticates it and rechecks the live grant and session ceiling.
///
/// Calls have a 30-second deadline and 512 KiB per-stream capture limit. They
/// are never retried: a missing reply to a write can mean the outcome is unknown.
pub struct OrbitCallbacks {
    program: PathBuf,
    cwd: PathBuf,
    environment: Vec<(OsString, OsString)>,
}

impl OrbitCallbacks {
    /// Configure the executable, call working directory and resolved environment.
    /// Only [`CALLBACK_ENV_NAMES`] are passed to the child.
    #[must_use]
    pub fn new(
        program: impl Into<PathBuf>,
        cwd: impl Into<PathBuf>,
        environment: impl IntoIterator<Item = (OsString, OsString)>,
    ) -> Self {
        Self {
            program: program.into(),
            cwd: cwd.into(),
            environment: environment
                .into_iter()
                .filter(|(name, _)| CALLBACK_ENV_NAMES.iter().any(|allowed| name == allowed))
                .collect(),
        }
    }

    /// Call an allowlisted tool. Every named port method goes through this guard
    /// before the process is created. Input keys and output shapes stay intact;
    /// Orbit owns schema validation, workspace routing and authorization.
    pub fn call(&self, tool: &str, input: &Map<String, Value>) -> Result<Value> {
        if !ORBIT_TOOLS.contains(&tool) {
            return Err(Error::ToolNotAllowed);
        }
        let mut command = Command::new(&self.program);
        command
            .current_dir(&self.cwd)
            .env_clear()
            .envs(self.environment.iter().cloned())
            // Even without a descriptor, identify this as a callback attempt.
            // Orbit must refuse it rather than treat it as an ordinary user CLI.
            .env("ORBIT_PLUGIN_CALLBACK", "descriptor-required")
            .args(["tool", "run", tool, "--json", "--full", "--input"])
            .arg(Value::Object(input.clone()).to_string());
        let output = office_common::process::run_captured(&mut command, &self.cwd)
            .map_err(|error| process_failure(tool, error))?;
        if !output.status.success() {
            // The CLI emits {"error": "diagnostic", "code": "…"} on stderr.
            // Classify only from that structure, never from prose or stdout.
            if let Ok(refusal) = serde_json::from_slice::<Value>(&output.stderr)
                && refusal.get("error").is_some_and(Value::is_string)
                && let Some(code) = refusal.get("code").and_then(Value::as_str)
                && !code.is_empty()
                && code.len() <= 64
                && code
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'.')
            {
                let error = Error::Refused {
                    code: code.to_string(),
                };
                return Err(if code == "outcome_unknown" {
                    Error::OutcomeUnknown(Box::new(error))
                } else {
                    error
                });
            }
            return Err(indeterminate(
                tool,
                Error::CommandFailed {
                    exit_code: output.status.code(),
                },
            ));
        }
        serde_json::from_slice(&output.stdout)
            .map_err(|_| indeterminate(tool, Error::InvalidResponse))
    }
}

// Common's public error is non-exhaustive. A future supervisor error cannot
// prove the child was never started, so conservatively preserve uncertainty.
#[allow(clippy::wildcard_enum_match_arm)]
fn process_failure(tool: &str, error: office_common::Error) -> Error {
    match error {
        office_common::Error::ProcessSpawn { .. } | office_common::Error::ProcessUnsupported => {
            Error::Process(error)
        }
        other => indeterminate(tool, Error::Process(other)),
    }
}

// A mutation with a missing, malformed or interrupted reply may already have
// committed. Keep that state distinct from a confirmed refusal (STD-03 R31).
fn indeterminate(tool: &str, failure: Error) -> Error {
    if matches!(
        tool,
        "orbit.task.add" | "orbit.task.update" | "orbit.task.artifact.put" | "orbit.friction.add"
    ) {
        Error::OutcomeUnknown(Box::new(failure))
    } else {
        failure
    }
}

impl Orbit for OrbitCallbacks {
    fn task_add(&self, input: &Map<String, Value>) -> office_core::Result<Value> {
        Ok(self.call("orbit.task.add", input)?)
    }
    fn task_update(&self, input: &Map<String, Value>) -> office_core::Result<Value> {
        Ok(self.call("orbit.task.update", input)?)
    }
    fn task_list(&self, input: &Map<String, Value>) -> office_core::Result<Value> {
        Ok(self.call("orbit.task.list", input)?)
    }
    fn task_show(&self, input: &Map<String, Value>) -> office_core::Result<Value> {
        Ok(self.call("orbit.task.show", input)?)
    }
    fn task_artifact_get(&self, input: &Map<String, Value>) -> office_core::Result<Value> {
        Ok(self.call("orbit.task.artifact.get", input)?)
    }
    fn task_artifact_put(&self, input: &Map<String, Value>) -> office_core::Result<Value> {
        Ok(self.call("orbit.task.artifact.put", input)?)
    }
    fn friction_add(&self, input: &Map<String, Value>) -> office_core::Result<Value> {
        Ok(self.call("orbit.friction.add", input)?)
    }
    fn search(&self, input: &Map<String, Value>) -> office_core::Result<Value> {
        Ok(self.call("orbit.search", input)?)
    }
}
