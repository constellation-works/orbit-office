//! Deterministic page identities and evidence for the careful consolidation call.

mod evidence;
mod group;
mod groups;
mod reply;

use serde::Deserialize;

pub use group::Group;
pub use groups::groups;
pub use reply::{Claim, Conflict, Reply, base_sha256, checked_reply, markdown, schema};

pub(in crate::domain::bootstrap) use evidence::{candidate_subjects, procedure};

/// Closed consolidation request; batch ids are phase-ledger keys.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// Confirmed batch ledger key.
    pub batch_id: String,
    /// Explicitly requested subjects may bypass the source-count bar.
    #[serde(default)]
    pub requested: Vec<String>,
}
