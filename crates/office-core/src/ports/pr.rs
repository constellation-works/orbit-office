//! Live pull-request state, read through the `gh` program.
//!
//! A PR reference is a canonical `owner/repo#number` id. Status is read
//! live and never stored; every failure is a typed [`PrError`] so callers
//! can name the cause without echoing program output.

/// State of a pull request, normalised from `gh`'s uppercase spelling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrState {
    /// Still open.
    Open,
    /// Closed without merging.
    Closed,
    /// Merged.
    Merged,
}

impl PrState {
    /// Lowercase wire spelling used in recall refs.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Closed => "closed",
            Self::Merged => "merged",
        }
    }
}

/// One live PR reading: state, merge time and merge commit when merged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrView {
    /// Current PR state.
    pub state: PrState,
    /// RFC 3339 merge time, when `gh` reports one.
    pub merged_at: Option<String>,
    /// Merge commit id, when `gh` reports one.
    pub merge_commit: Option<String>,
}

/// Why a PR reading could not be produced. Messages name only the cause
/// class; raw program output is never carried here.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PrError {
    /// `gh` is not installed or not granted, so it could not be started.
    #[error("gh is unavailable")]
    Unavailable,
    /// `gh` ran but failed, timed out or exceeded its output bound.
    #[error("gh lookup failed")]
    Failed,
    /// `gh` succeeded but its reply did not match the expected shape.
    #[error("gh returned an invalid response")]
    InvalidResponse,
    /// The reference is not a canonical `owner/repo#number` id.
    #[error("invalid pull request reference")]
    InvalidRef,
}

/// Live PR source. Implementations run `gh` supervised, bounded and
/// never retried.
pub trait PullRequests {
    /// Read one PR by canonical `owner/repo#number` id.
    fn pr_view(&self, _id: &str) -> Result<PrView, PrError> {
        Err(PrError::Unavailable)
    }
}

/// Refusing adapter for call contexts without a PR source.
pub struct UnavailablePullRequests;

impl PullRequests for UnavailablePullRequests {
    fn pr_view(&self, _id: &str) -> Result<PrView, PrError> {
        Err(PrError::Unavailable)
    }
}

/// Split a canonical PR id into `(owner/repo, number)`.
///
/// Returns [`PrError::InvalidRef`] for anything else: a missing `#`,
/// an empty owner or repo, extra path segments, or a non-numeric,
/// zero or overflowing number.
pub fn parse_pr_id(id: &str) -> Result<(&str, u64), PrError> {
    let (repo, number) = id.split_once('#').ok_or(PrError::InvalidRef)?;
    let (owner, name) = repo.split_once('/').ok_or(PrError::InvalidRef)?;
    if owner.is_empty()
        || name.is_empty()
        || name.contains('/')
        || owner.contains(['#', '/', ' ', '\t'])
        || name.contains(['#', ' ', '\t'])
    {
        return Err(PrError::InvalidRef);
    }
    if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
        return Err(PrError::InvalidRef);
    }
    let number: u64 = number.parse().map_err(|_| PrError::InvalidRef)?;
    if number == 0 {
        return Err(PrError::InvalidRef);
    }
    Ok((repo, number))
}
