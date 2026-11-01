//! Supervised `gh` adapter for the live PR source port.
//!
//! Runs `gh pr view <owner/repo#n> --json state,mergedAt,mergeCommit`
//! once, under the shared bounded supervisor (deadline, capped output,
//! process group swept), and never retries. The child gets a cleared
//! environment with only the variables `gh` needs to find and
//! authenticate itself; no Office or Orbit variable is inherited, and
//! raw child output is never included in an error.

use std::process::Command;
use std::time::Duration;

use office_core::ports::{PrError, PrState, PrView, PullRequests, parse_pr_id};
use serde_json::Value;

/// Deadline for one `gh` reading.
const GH_DEADLINE: Duration = Duration::from_secs(15);
/// stdout bound for the small JSON reply.
const GH_OUTPUT_LIMIT: u64 = 64 * 1024;
/// Variables forwarded to `gh` so it can locate its configuration and
/// token; everything else is cleared.
const GH_ENVIRONMENT: [&str; 5] = ["PATH", "HOME", "XDG_CONFIG_HOME", "GH_TOKEN", "GH_HOST"];

/// PR source backed by the granted `gh` program.
#[derive(Clone, Copy, Debug, Default)]
pub struct GhPullRequests;

impl GhPullRequests {
    /// A source that resolves `gh` against `PATH` at call time.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

fn optional_string(value: &Value) -> Result<Option<String>, PrError> {
    match value {
        Value::Null => Ok(None),
        Value::String(text) if !text.is_empty() && text.len() <= 64 => Ok(Some(text.clone())),
        Value::String(_)
        | Value::Bool(_)
        | Value::Number(_)
        | Value::Array(_)
        | Value::Object(_) => Err(PrError::InvalidResponse),
    }
}

pub(crate) fn parse_view(stdout: &[u8]) -> Result<PrView, PrError> {
    let value: Value = serde_json::from_slice(stdout).map_err(|_| PrError::InvalidResponse)?;
    let state = match value.get("state").and_then(Value::as_str) {
        Some("OPEN") => PrState::Open,
        Some("CLOSED") => PrState::Closed,
        Some("MERGED") => PrState::Merged,
        _ => return Err(PrError::InvalidResponse),
    };
    let merged_at = optional_string(value.get("mergedAt").ok_or(PrError::InvalidResponse)?)?;
    let merge_commit = match value.get("mergeCommit").ok_or(PrError::InvalidResponse)? {
        Value::Null => None,
        Value::Object(commit) => {
            optional_string(commit.get("oid").ok_or(PrError::InvalidResponse)?)?
        }
        Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_) => {
            return Err(PrError::InvalidResponse);
        }
    };
    Ok(PrView {
        state,
        merged_at,
        merge_commit,
    })
}

impl PullRequests for GhPullRequests {
    fn pr_view(&self, id: &str) -> Result<PrView, PrError> {
        parse_pr_id(id)?;
        let scratch = tempfile::tempdir().map_err(|_| PrError::Failed)?;
        let mut command = Command::new("gh");
        command
            .args(["pr", "view", id, "--json", "state,mergedAt,mergeCommit"])
            .current_dir(scratch.path())
            .env_clear()
            .env("GH_PROMPT_DISABLED", "1");
        for name in GH_ENVIRONMENT {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        if std::env::var_os("HOME").is_none() {
            command.env("HOME", scratch.path());
        }
        let output = office_common::process::run_captured_with_deadline(
            &mut command,
            scratch.path(),
            GH_DEADLINE,
            GH_OUTPUT_LIMIT,
        )
        .map_err(|source| {
            if let office_common::Error::ProcessSpawn { source: ref io, .. } = source
                && io.kind() == std::io::ErrorKind::NotFound
            {
                PrError::Unavailable
            } else {
                PrError::Failed
            }
        })?;
        if !output.status.success() {
            return Err(PrError::Failed);
        }
        parse_view(&output.stdout)
    }
}
