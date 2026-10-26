//! Sensitivity: how far a piece of content may travel. Shared by the
//! Library contracts, the model router, and selection sharing — one
//! definition, so the three can never disagree about which is stricter.

use serde::{Deserialize, Serialize};

/// A source's (and, by inheritance, an artifact's) sensitivity.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sensitivity {
    /// May go to hosted model APIs.
    #[default]
    Normal,
    /// Stays on the open-weight model; never quoted into a hosted prompt.
    Private,
}

impl Sensitivity {
    /// The canonical lowercase token, as stored in frontmatter.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Private => "private",
        }
    }

    /// Parse a canonical token; unknown tokens are refused, never guessed.
    #[must_use]
    pub fn parse(token: &str) -> Option<Self> {
        match token {
            "normal" => Some(Self::Normal),
            "private" => Some(Self::Private),
            _ => None,
        }
    }

    /// The stricter of two sensitivities (an artifact is the strictest of
    /// what it cites).
    #[must_use]
    pub fn strictest(self, other: Self) -> Self {
        self.max(other)
    }
}
