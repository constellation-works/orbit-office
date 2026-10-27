//! Approved script execution contracts; filesystem and process work lives in adapters.

use super::artifact::{ArtifactKind, ArtifactManifest};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};

/// Closed routine step input. Identity and run ids come from the host context.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptRequest {
    /// Approved Library script artifact.
    pub artifact: String,
    /// Approved script bytes, pinned by the proposal.
    pub sha256: String,
    /// Library paths or artifact ids to copy read-only.
    #[serde(default)]
    pub inputs: Vec<String>,
    /// Literal argv, never shell source.
    #[serde(default)]
    pub args: Vec<String>,
    /// Output promotion metadata.
    pub output: ScriptOutput,
}

/// Output metadata fixed in the approved routine definition.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptOutput {
    /// Review task title.
    pub title: String,
    /// Promoted artifact kind.
    pub kind: ArtifactKind,
    /// Promoted artifact series.
    pub series: String,
}

impl ScriptRequest {
    /// Validate before any callback or filesystem mutation.
    pub fn validate(&self) -> Result<()> {
        if !super::artifact::valid_id(&self.artifact)
            || self.sha256.len() != 64
            || !self
                .sha256
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            || self
                .inputs
                .iter()
                .chain(&self.args)
                .any(|s| s.contains('\0'))
        {
            return Err(Error::InvalidArgument(
                "script-run needs an artifact id, lowercase SHA-256 and NUL-free inputs/args"
                    .into(),
            ));
        }
        self.manifest(vec!["output".into()]).validate()?;
        Ok(())
    }

    /// Promotion-compatible manifest; provenance stays alongside it in script-run.json.
    #[must_use]
    pub fn manifest(&self, files: Vec<String>) -> ArtifactManifest {
        ArtifactManifest {
            kind: self.output.kind,
            title: self.output.title.clone(),
            summary: String::new(),
            files,
            cites: vec![self.artifact.clone()],
            series: self.output.series.clone(),
            tags: vec!["script-output".into()],
            runbook: None,
            // File/folder inputs need not have citable metadata. Preserve their
            // confidentiality conservatively instead of assuming normal sensitivity.
            sensitivity: if self.inputs.is_empty() {
                Default::default()
            } else {
                office_common::Sensitivity::Private
            },
        }
    }
}

/// Library policy bounds. Operators may lower any bound, never raise the caps.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ScriptBounds {
    /// Wall-clock seconds (default 300, maximum 1800).
    pub timeout_seconds: u64,
    /// Combined stdout/stderr bytes (maximum 1 MiB).
    pub log_bytes: u64,
    /// Output file count (maximum eight).
    pub output_files: usize,
    /// Per-output-file bytes (maximum 1 MiB).
    pub output_file_bytes: u64,
}
impl Default for ScriptBounds {
    fn default() -> Self {
        Self {
            timeout_seconds: 300,
            log_bytes: 1_048_576,
            output_files: 8,
            output_file_bytes: 1_048_576,
        }
    }
}
impl ScriptBounds {
    /// The same validation at settings load and adapter entry.
    pub fn validate(&self) -> Result<()> {
        for (key, valid) in [
            (
                "timeout_seconds",
                (1..=1800).contains(&self.timeout_seconds),
            ),
            ("log_bytes", (1..=1_048_576).contains(&self.log_bytes)),
            ("output_files", (1..=8).contains(&self.output_files)),
            (
                "output_file_bytes",
                (1..=1_048_576).contains(&self.output_file_bytes),
            ),
        ] {
            if !valid {
                return Err(Error::InvalidSetting {
                    key: format!("scripts.{key}"),
                    reason: "must be positive and no greater than the script bound",
                });
            }
        }
        Ok(())
    }
}

/// Resolved Library-relative script file and its approved hash.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptPreparation {
    /// Library-relative script filename.
    pub path: String,
    /// SHA-256 of the exact approved bytes.
    pub sha256: String,
}

/// Everything the runner receives after proposal verification.
#[derive(Clone, Debug)]
pub struct ScriptExecution {
    /// Original closed request.
    pub request: ScriptRequest,
    /// Resolved script file.
    pub script: ScriptPreparation,
    /// Expanded Library-relative input files or folders.
    pub inputs: Vec<String>,
    /// Host run identity, if present; never supplied through tool input.
    pub run_id: Option<String>,
}

/// One bounded file captured after the child has been reaped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptFile {
    /// Relative task artifact path.
    pub path: String,
    /// Local source for Orbit's artifact callback.
    pub source_path: String,
}

/// Completed execution, including failed child exits (which the tool refuses).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptResult {
    /// Exit code; null means terminated by a signal.
    pub exit_code: Option<i32>,
    /// Elapsed wall-clock milliseconds.
    pub duration_ms: u64,
    /// Last 50 lines of the capped combined log.
    pub log_tail: String,
    /// Local run directory.
    pub run_dir: String,
    /// Output review task, if any.
    pub output_task: Option<String>,
    /// Output filenames (excluding manifests).
    pub files: Vec<String>,
}

/// Runner errors; no raw dependency output or ambient credentials are retained.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ScriptError {
    /// Missing script kind or accepted proposal pin.
    #[error(
        "script artifact and version are not named by an accepted routine or auto-task proposal"
    )]
    NotApproved,
    /// Script file differs from the approved hash.
    #[error("script file SHA-256 differs from the approved version")]
    DigestMismatch,
    /// Input physically escapes the Library or is an unsafe special entry.
    #[error("script input resolves outside the Library or is not a regular Library file: {path}")]
    InputOutsideLibrary {
        /// Refused Library-relative selector.
        path: String,
    },
    /// A policy bound stopped execution.
    #[error("script exceeded {bound} bound ({limit})")]
    Bound {
        /// Name of the bound.
        bound: &'static str,
        /// Effective limit.
        limit: u64,
    },
    /// Dependency resolution failed.
    #[error("script dependencies could not be prepared (uv exit {exit_code:?})")]
    Prepare {
        /// Actual uv exit, if available.
        exit_code: Option<i32>,
    },
    /// Child failure keeps its bounded tail available to routine diagnostics.
    #[error("script exited with code {exit_code:?}; last 50 log lines:\n{log_tail}")]
    Exit {
        /// Actual child exit.
        exit_code: Option<i32>,
        /// Bounded log tail.
        log_tail: String,
    },
    /// Adapter failure, with no raw process output.
    #[error("script runner {operation} failed")]
    Adapter {
        /// Checked operation.
        operation: &'static str,
    },
    /// No script service in this call.
    #[error("script runner is unavailable in this call context")]
    Unavailable,
}
impl ScriptError {
    /// Stable envelope classification.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotApproved => "script_not_approved",
            Self::DigestMismatch => "script_digest_mismatch",
            Self::InputOutsideLibrary { .. } => "input_outside_library",
            Self::Bound { .. } => "script_bound_exceeded",
            Self::Prepare { .. } => "script_prepare_failed",
            Self::Exit { .. } => "script_exit_failed",
            Self::Adapter { .. } => "script_runner_failed",
            Self::Unavailable => "script_runner_unavailable",
        }
    }
}
