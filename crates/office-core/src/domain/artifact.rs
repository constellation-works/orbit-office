//! Approved, immutable Library artifacts and their versioned file bundles.

use std::collections::BTreeSet;

use office_common::Sensitivity;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::knowledge::frontmatter;
use super::source::slugify;

/// Orbit's per-file artifact limit, checked before publishing any files.
pub const MAX_FILE_BYTES: usize = 1_048_576;

/// Artifact refusals; diagnostics never contain file contents or metadata values.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ArtifactError {
    /// Invalid metadata or an unsafe file name.
    #[error("invalid artifact field {field}")]
    Invalid {
        /// The field that failed validation.
        field: &'static str,
    },
    /// Routine approval is restricted to the two built report routines.
    #[error("routine approval is allowed only for daily digest and meeting prep reports")]
    RoutineApproval,
    /// Another caller already published this routine's daily report.
    #[error("daily digest already exists; read the existing report")]
    DigestExists,
    /// Another caller already published prep for this event key.
    #[error("meeting prep already exists; read the existing report")]
    MeetingPrepExists,
    /// The task has no output to promote.
    #[error("task has no artifacts to promote")]
    NoArtifacts,
    /// Only reviewed task outputs can be approved.
    #[error(
        "task output must be in review or done, or proposed with script-output tag, before promotion"
    )]
    NotReviewable,
    /// A file exceeds Orbit's bound.
    #[error("artifact file exceeds the 1 MiB limit")]
    TooLarge,
    /// A callback supplied neither valid UTF-8 text nor valid base64 bytes.
    #[error("invalid Orbit artifact payload")]
    Payload,
    /// A requested Library artifact is absent.
    #[error("artifact was not found")]
    NotFound,
}

impl ArtifactError {
    /// Stable tool envelope code.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Invalid { .. } => "invalid_artifact",
            Self::RoutineApproval => "artifact_routine_approval_refused",
            Self::DigestExists => "digest_already_exists",
            Self::MeetingPrepExists => "meeting_prep_already_exists",
            Self::NoArtifacts => "task_has_no_artifacts",
            Self::NotReviewable => "task_not_reviewable",
            Self::TooLarge => "artifact_too_large",
            Self::Payload => "artifact_payload_invalid",
            Self::NotFound => "artifact_not_found",
        }
    }
}

type Result<T> = std::result::Result<T, ArtifactError>;

fn invalid(field: &'static str) -> ArtifactError {
    ArtifactError::Invalid { field }
}

/// The seven Library artifact kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    /// Research or status report.
    Report,
    /// Document.
    Doc,
    /// Message awaiting sending.
    Draft,
    /// Script, never executed by promotion.
    Script,
    /// Structured data.
    Data,
    /// Review.
    Review,
    /// Other approved output.
    Other,
}

/// Approval provenance. Only daily digest and meeting prep reports use routine approval.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Approver {
    /// Explicit user approval.
    User,
    /// Daily digest or meeting prep routine.
    Routine,
}

/// Producer recorded from the source task, separately from approval.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Producer {
    /// The task's recorded crew, if known.
    pub crew: Option<String>,
    /// Runbook named by the output manifest, if any.
    pub runbook: Option<String>,
    /// Task's recorded production/update timestamp, if known.
    pub at: Option<String>,
}

/// Approval stamped by the promoting operation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Approval {
    /// Approving actor category.
    pub by: Approver,
    /// Time of approval.
    pub at: String,
    /// Whether the user edited the task output before approving it.
    pub edited: bool,
}

/// Closed manifest stored alongside the task output as artifact-manifest.json.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactManifest {
    /// Output kind.
    pub kind: ArtifactKind,
    /// User-facing title.
    pub title: String,
    /// Searchable summary.
    #[serde(default)]
    pub summary: String,
    /// Task artifact paths, copied under the same relative names.
    pub files: Vec<String>,
    /// Evidence citations.
    #[serde(default)]
    pub cites: Vec<String>,
    /// Deliverable series across revisions.
    pub series: String,
    /// Calendar event identity for meeting preparation, absent on older artifacts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_key: Option<String>,
    /// Search tags.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Runbook that produced this output, if known.
    #[serde(default)]
    pub runbook: Option<String>,
    /// Explicit sensitivity; promotion also inherits cited sensitivity.
    #[serde(default)]
    pub sensitivity: Sensitivity,
    /// Explicit sharing approval; older artifacts default to false.
    #[serde(default)]
    pub shareable: bool,
}

impl ArtifactManifest {
    /// Raise sensitivity to that of cited evidence; unknown values fail closed.
    pub fn inherit_sensitivity(&mut self, token: &str) -> Result<()> {
        let sensitivity = Sensitivity::parse(token).ok_or_else(|| invalid("cites.sensitivity"))?;
        self.sensitivity = self.sensitivity.strictest(sensitivity);
        Ok(())
    }

    /// Validate before reading the output files.
    pub fn validate(&self) -> Result<()> {
        if self.title.trim().is_empty() {
            return Err(invalid("title"));
        }
        if !safe_slug(&self.series) {
            return Err(invalid("series"));
        }
        validate_files(&self.files)?;
        Ok(())
    }
}

/// Complete artifact frontmatter, persisted losslessly as YAML.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    /// art-YYYYMMDD-slug identity.
    pub id: String,
    /// Output kind.
    pub kind: ArtifactKind,
    /// Title.
    pub title: String,
    /// Summary.
    pub summary: String,
    /// Files beside artifact.md, relative to its directory.
    pub files: Vec<String>,
    /// Source Orbit task.
    pub task: String,
    /// Producer, from the source task and manifest.
    pub produced_by: Producer,
    /// Approval provenance.
    pub approved: Approval,
    /// Evidence citations.
    pub cites: Vec<String>,
    /// Version series.
    pub series: String,
    /// Positive monotonic version in the series.
    pub version: u32,
    /// Previous artifact id, retained unchanged.
    pub supersedes: Option<String>,
    /// Strictest cited sensitivity.
    pub sensitivity: Sensitivity,
    /// Explicit sharing approval; older artifacts default to false.
    #[serde(default)]
    pub shareable: bool,
    /// Calendar event identity for meeting preparation, absent on older artifacts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_key: Option<String>,
    /// Search tags.
    pub tags: Vec<String>,
}

/// An output file, already read from Orbit and bounded before promotion.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactFile {
    /// Relative task artifact path and destination within the artifact directory.
    pub path: String,
    /// Exact file bytes, including binary data.
    pub bytes: Vec<u8>,
}

impl Artifact {
    /// Build a first-version candidate. The Library assigns its final version
    /// and collision-free id under its writer lock.
    pub fn approved(
        manifest: ArtifactManifest,
        task: String,
        produced_by: Producer,
        approved: Approval,
    ) -> Result<Self> {
        manifest.validate()?;
        let date: String = approved.at.chars().take(10).filter(|c| *c != '-').collect();
        let artifact = Self {
            id: format!("art-{date}-{}", slugify(&manifest.title)),
            kind: manifest.kind,
            title: manifest.title,
            summary: manifest.summary,
            files: manifest.files,
            task,
            produced_by,
            approved,
            cites: manifest.cites,
            series: manifest.series,
            version: 1,
            supersedes: None,
            sensitivity: manifest.sensitivity,
            shareable: manifest.shareable,
            tags: manifest.tags,
            event_key: manifest.event_key,
        };
        artifact.validate()?;
        Ok(artifact)
    }

    /// One shared validation path for callers, stores, parsing and recovery.
    pub fn validate(&self) -> Result<()> {
        match self.approved.by {
            Approver::User => {}
            Approver::Routine => {
                if self.kind != ArtifactKind::Report
                    || !matches!(self.series.as_str(), "digest-daily" | "meeting-prep")
                    || (self.series == "meeting-prep"
                        && self
                            .event_key
                            .as_deref()
                            .is_none_or(|key| key.trim().is_empty()))
                {
                    return Err(ArtifactError::RoutineApproval);
                }
            }
        }
        if !valid_id(&self.id) {
            return Err(invalid("id"));
        }
        if self.title.trim().is_empty() {
            return Err(invalid("title"));
        }
        if self.task.trim().is_empty() {
            return Err(invalid("task"));
        }
        if !safe_slug(&self.series) {
            return Err(invalid("series"));
        }
        if self.approved.at.trim().is_empty() {
            return Err(invalid("approved.at"));
        }
        if self.version == 0
            || (self.version == 1) != self.supersedes.is_none()
            || self
                .supersedes
                .as_deref()
                .is_some_and(|id| !valid_id(id) || id == self.id)
        {
            return Err(invalid("version"));
        }
        validate_files(&self.files)
    }

    /// Check the entire file set before the first durable side effect.
    pub fn validate_bundle(&self, files: &[ArtifactFile]) -> Result<()> {
        self.validate()?;
        if files.len() != self.files.len() {
            return Err(invalid("files"));
        }
        let names: BTreeSet<_> = files.iter().map(|file| file.path.as_str()).collect();
        if names.len() != files.len()
            || self.files.iter().any(|name| !names.contains(name.as_str()))
        {
            return Err(invalid("files"));
        }
        if files.iter().any(|file| file.bytes.len() > MAX_FILE_BYTES) {
            return Err(ArtifactError::TooLarge);
        }
        Ok(())
    }

    /// Allocate a revision and avoid every occupied id (including non-doc entries).
    /// Called with a complete Library snapshot while its writer lock is held.
    pub fn next_version(
        &self,
        history: &[Self],
        mut occupied: impl FnMut(&str) -> bool,
    ) -> Result<Self> {
        self.validate()?;
        // Called under the Library writer lock, including by concurrent digest
        // callers that gathered before either published. Other artifact series
        // continue to allocate ordinary versions.
        if self.approved.by == Approver::Routine
            && self.series == "digest-daily"
            && self.task.starts_with("routine:office-daily-digest/")
            && history
                .iter()
                .any(|a| a.series == self.series && a.task == self.task)
        {
            return Err(ArtifactError::DigestExists);
        }
        if self.series == "meeting-prep"
            && self.event_key.is_some()
            && history
                .iter()
                .any(|a| a.series == self.series && a.event_key == self.event_key)
        {
            return Err(ArtifactError::MeetingPrepExists);
        }
        let latest = history
            .iter()
            .filter(|a| a.series == self.series)
            .max_by_key(|a| a.version);
        let mut next = self.clone();
        next.version = 1;
        next.supersedes = None;
        if let Some(latest) = latest {
            next.version = latest
                .version
                .checked_add(1)
                .ok_or_else(|| invalid("version"))?;
            next.supersedes = Some(latest.id.clone());
        }
        let base = self.id.clone();
        let mut suffix = 2u32;
        while history.iter().any(|a| a.id == next.id) || occupied(&next.id) {
            next.id = format!("{base}-{suffix}");
            suffix = suffix.checked_add(1).ok_or_else(|| invalid("id"))?;
        }
        next.validate()?;
        Ok(next)
    }

    /// Library-relative artifact.md path, derived only from the validated id.
    pub fn path(&self) -> Result<String> {
        self.validate()?;
        Ok(format!(
            "artifacts/{}/{}/{}/artifact.md",
            &self.id[4..8],
            &self.id[8..10],
            self.id
        ))
    }

    /// Render YAML in the shared supported subset (with quoted scalar values).
    pub fn to_markdown(&self) -> Result<String> {
        self.validate()?;
        let value = serde_json::to_value(self).map_err(|_| invalid("frontmatter"))?;
        let fields = value.as_object().ok_or_else(|| invalid("frontmatter"))?;
        let mut text = String::from("---\n");
        for (key, value) in fields {
            text.push_str(&format!("{key}: {}\n", yaml_value(value)));
        }
        text.push_str("---\n");
        Ok(text)
    }

    /// Bind loaded metadata to its canonical Library location.
    pub fn from_document(relative: &str, text: &str) -> Result<Self> {
        let artifact = Self::from_markdown(text)?;
        if artifact.path()? != relative {
            return Err(invalid("path"));
        }
        Ok(artifact)
    }

    /// Read nested provenance and version metadata using the shared YAML parser.
    pub fn from_markdown(text: &str) -> Result<Self> {
        let (header, _) = frontmatter::split(text).map_err(|_| invalid("frontmatter"))?;
        let mut fields: Map<String, Value> = frontmatter::parse(header)
            .map_err(|_| invalid("frontmatter"))?
            .into_iter()
            .collect();
        if let Some(Value::String(version)) = fields.get("version") {
            let version: u32 = version.parse().map_err(|_| invalid("version"))?;
            fields.insert("version".into(), Value::from(version));
        }
        let artifact: Self =
            serde_json::from_value(Value::Object(fields)).map_err(|_| invalid("frontmatter"))?;
        artifact.validate()?;
        Ok(artifact)
    }
}

pub(crate) fn yaml_value(value: &Value) -> String {
    match value {
        Value::Object(map) => format!(
            "{{ {} }}",
            map.iter()
                .map(|(k, v)| format!("{k}: {}", yaml_value(v)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::Array(items) => format!(
            "[{}]",
            items.iter().map(yaml_value).collect::<Vec<_>>().join(", ")
        ),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => value.to_string(),
    }
}

/// Whether an artifact id has its fixed dated prefix and a safe slug.
#[must_use]
pub fn valid_id(id: &str) -> bool {
    let Some(rest) = id.strip_prefix("art-") else {
        return false;
    };
    rest.len() > 9
        && rest.as_bytes().get(8) == Some(&b'-')
        && rest.as_bytes()[..8].iter().all(u8::is_ascii_digit)
        && safe_slug(&rest[9..])
        && matches!(
            rest.get(4..6),
            Some("01" | "02" | "03" | "04" | "05" | "06" | "07" | "08" | "09" | "10" | "11" | "12")
        )
        && rest
            .get(6..8)
            .and_then(|day| day.parse::<u8>().ok())
            .is_some_and(|day| (1..=31).contains(&day))
}

fn safe_slug(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('-')
        && !value.ends_with('-')
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn validate_files(files: &[String]) -> Result<()> {
    if files.is_empty() {
        return Err(ArtifactError::NoArtifacts);
    }
    let mut seen = BTreeSet::new();
    for file in files {
        if file.is_empty()
            || file.contains('\\')
            || !seen.insert(file)
            || file.split('/').any(|part| {
                part.is_empty()
                    || part.starts_with('.')
                    || part == "artifact.md"
                    || part.chars().any(|c| c.is_control() || c == ':')
            })
        {
            return Err(invalid("files"));
        }
    }
    // No file may also be another file's parent directory.
    if files
        .iter()
        .any(|a| files.iter().any(|b| b.starts_with(&format!("{a}/"))))
    {
        return Err(invalid("files"));
    }
    Ok(())
}

/// Decode Orbit's text/base64 artifact envelope without retaining raw errors.
/// Both forms are bounded before allocation; ambiguous payloads are refused.
pub fn decode_payload(value: &Value) -> Result<Vec<u8>> {
    let text = value.get("content").filter(|v| !v.is_null());
    let base64 = value.get("content_base64").filter(|v| !v.is_null());
    let bytes = match (text, base64) {
        (Some(Value::String(text)), None) => {
            if text.len() > MAX_FILE_BYTES {
                return Err(ArtifactError::TooLarge);
            }
            text.as_bytes().to_vec()
        }
        (None, Some(Value::String(encoded))) => decode_base64(encoded)?,
        _ => return Err(ArtifactError::Payload),
    };
    if bytes.len() > MAX_FILE_BYTES {
        return Err(ArtifactError::TooLarge);
    }
    if let Some(size) = value.get("size")
        && size.as_u64() != u64::try_from(bytes.len()).ok()
    {
        return Err(ArtifactError::Payload);
    }
    Ok(bytes)
}

// Standard, padded RFC 4648 alphabet. Kept local because the pure artifact
// domain has no codec dependency; reject padding/noncanonical trailing bits.
fn decode_base64(text: &str) -> Result<Vec<u8>> {
    if text.len() > MAX_FILE_BYTES.div_ceil(3) * 4 {
        return Err(ArtifactError::TooLarge);
    }
    if !text.len().is_multiple_of(4) {
        return Err(ArtifactError::Payload);
    }
    let mut out = Vec::new();
    for (i, chunk) in text.as_bytes().chunks_exact(4).enumerate() {
        let mut values = [0u8; 4];
        let mut padding = 0;
        for (at, byte) in chunk.iter().enumerate() {
            values[at] = match byte {
                b'A'..=b'Z' if padding == 0 => byte - b'A',
                b'a'..=b'z' if padding == 0 => byte - b'a' + 26,
                b'0'..=b'9' if padding == 0 => byte - b'0' + 52,
                b'+' if padding == 0 => 62,
                b'/' if padding == 0 => 63,
                b'=' if at >= 2 => {
                    padding += 1;
                    0
                }
                _ => return Err(ArtifactError::Payload),
            };
        }
        if padding > 0
            && (i + 1 != text.len() / 4
                || (padding == 2 && values[1] & 15 != 0)
                || (padding == 1 && values[2] & 3 != 0))
        {
            return Err(ArtifactError::Payload);
        }
        out.push((values[0] << 2) | (values[1] >> 4));
        if padding < 2 {
            out.push((values[1] << 4) | (values[2] >> 2));
        }
        if padding == 0 {
            out.push((values[2] << 6) | values[3]);
        }
    }
    Ok(out)
}
