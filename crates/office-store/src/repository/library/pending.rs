//! Pending-write journal types: the durable decision replayed on recovery.

use std::path::{Path, PathBuf};

use office_core::domain::artifact::ArtifactFile;
use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// A durable decision to finish one write. Replayed idempotently while locked.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PendingWrite {
    pub(super) version: u32,
    pub(super) kind: WriteKind,
    pub(super) root: PathBuf,
    pub(super) relative: String,
    pub(super) text: String,
    pub(super) token: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) original: Option<PendingOriginal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) bundle: Option<Vec<ArtifactFile>>,
}

/// Upload bytes belong to the decision, never to the caller's temporary path.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PendingOriginal {
    pub(super) relative: String,
    pub(super) bytes: Vec<u8>,
}

/// Keep upload extensions safe for conversion and inline source frontmatter.
/// A non-alphanumeric suffix starts an untrusted tail, so retain only the
/// leading ASCII alphanumeric extension and normalize it to lowercase.
pub(super) fn normalize_upload_extension(extension: &str) -> String {
    extension
        .chars()
        .take_while(|character| character.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase()
}

/// A journal as read from disk, before its version gate is applied.
/// Version-1 journals predate `kind` and are always documents; version-2
/// journals must carry `kind` explicitly. Defaulting a missing `kind` on
/// a version-2 journal would silently change what the journal means, so
/// recovery refuses it instead. Version 3 adds a document's original upload;
/// earlier readers refuse that version rather than dropping its bytes.
/// Version 4 binds an artifact document and every immutable companion byte.
/// Version 5 binds a video source update and its mutable derived files.
/// Version 6 binds bootstrap phase ledgers and source privacy updates.
/// Version 7 binds model provenance, immutable transcript versions and extraction sidecars.
/// Version 8 commits non-indexed proposal records; older readers refuse it.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawPendingWrite {
    version: u32,
    #[serde(default)]
    kind: Option<WriteKind>,
    root: PathBuf,
    relative: String,
    text: String,
    token: String,
    #[serde(default)]
    original: Option<PendingOriginal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bundle: Option<Vec<ArtifactFile>>,
}

impl RawPendingWrite {
    /// Apply the version gate and produce the replayable decision.
    pub(super) fn into_pending(self, path: &Path) -> Result<PendingWrite> {
        let kind = match (self.version, self.kind, &self.original, &self.bundle) {
            (1, None | Some(WriteKind::Document), None, None) => WriteKind::Document,
            (2, Some(WriteKind::Document), None, None) => WriteKind::Document,
            (2, Some(WriteKind::Settings), None, None) => WriteKind::Settings,
            (3, Some(WriteKind::Document), Some(_), None) => WriteKind::Document,
            (4, Some(WriteKind::Artifact), None, Some(_)) => WriteKind::Artifact,
            (5, Some(WriteKind::Video), None, Some(_)) => WriteKind::Video,
            (6, Some(WriteKind::Bootstrap), None, Some(_)) => WriteKind::Bootstrap,
            (7, Some(WriteKind::SourceProcessing), None, Some(_)) => WriteKind::SourceProcessing,
            (8, Some(WriteKind::Proposal), None, None) => WriteKind::Proposal,
            _ => {
                return Err(Error::Journal {
                    path: path.to_path_buf(),
                });
            }
        };
        Ok(PendingWrite {
            version: self.version,
            kind,
            root: self.root,
            relative: self.relative,
            text: self.text,
            token: self.token,
            original: self.original,
            bundle: self.bundle,
        })
    }
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum WriteKind {
    #[default]
    Document,
    Settings,
    Artifact,
    Video,
    Bootstrap,
    SourceProcessing,
    Proposal,
}
