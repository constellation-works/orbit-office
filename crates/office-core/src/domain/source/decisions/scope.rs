use std::path::Path;

use crate::{Error, Result};

/// The source kind token and the directory under `sources/`.
pub const KIND: &str = "decisions";
/// The Library-relative directory holding every ledger file.
pub const LEDGER_DIR: &str = "sources/decisions";
/// The tag written on every ledger line.
pub(super) const LINE_TAG: &str = "#decision";

pub(super) fn refusal(code: &'static str, message: impl Into<String>) -> Error {
    Error::Library {
        code,
        message: message.into(),
    }
}

pub(super) fn invalid(message: impl Into<String>) -> Error {
    refusal("invalid_decision", message)
}

pub(super) fn is_slug(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        })
}

/// A ledger's scope: `global` or `project:<name>`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Scope {
    /// `global`, stored at `sources/decisions/global.md`.
    Global,
    /// `project:<name>`, stored at `sources/decisions/<name>.md`.
    Project(String),
}

impl Scope {
    /// Parse `global` or `project:<name>`, where the name is a lowercase
    /// kebab-case slug other than `global`.
    pub fn parse(text: &str) -> Result<Self> {
        if text == "global" {
            return Ok(Self::Global);
        }
        match text.strip_prefix("project:") {
            Some(name) if is_slug(name) && name != "global" => Ok(Self::Project(name.into())),
            _ => Err(invalid(format!(
                "decision scope {text:?} must be global or project:<name> with a lowercase kebab-case name"
            ))),
        }
    }

    /// The canonical token (`global`, `project:<name>`).
    #[must_use]
    pub fn token(&self) -> String {
        match self {
            Self::Global => "global".into(),
            Self::Project(name) => format!("project:{name}"),
        }
    }

    /// The slug naming the file and the ids (`global`, `<name>`).
    #[must_use]
    pub fn slug(&self) -> &str {
        match self {
            Self::Global => "global",
            Self::Project(name) => name,
        }
    }

    /// `decisions-<slug>`.
    #[must_use]
    pub fn ledger_id(&self) -> String {
        format!("{KIND}-{}", self.slug())
    }

    /// `sources/decisions/<slug>.md`.
    #[must_use]
    pub fn path(&self) -> String {
        format!("{LEDGER_DIR}/{}.md", self.slug())
    }

    /// `dec-<slug>-<n>`.
    #[must_use]
    pub fn entry_id(&self, number: usize) -> String {
        format!("dec-{}-{number}", self.slug())
    }
}

/// `office.capture` with no kind for input whose kind it cannot infer
/// (plain text): refused, naming the paste kinds and the `decision` input.
#[must_use]
pub fn source_kind_required() -> Error {
    refusal(
        "source_kind_required",
        "capture needs a kind for this input: use kind chat, email or doc for a paste; \
         a decision goes in the decision input, which appends to the decision ledger",
    )
}

/// `office.capture` with `kind: decisions`: ledgers take entries only
/// through the `decision` input.
#[must_use]
pub fn decision_input_required() -> Error {
    refusal(
        "decision_input_required",
        "decisions sources are ledgers: append an entry with the decision input, not kind decisions",
    )
}

/// Whether `relative` is a ledger file: exactly `sources/decisions/<slug>.md`.
#[must_use]
pub fn is_ledger_path(relative: &Path) -> bool {
    let mut components = relative.components();
    let names: Vec<&str> = components
        .by_ref()
        .map_while(|component| match component {
            std::path::Component::Normal(name) => name.to_str(),
            std::path::Component::Prefix(_)
            | std::path::Component::RootDir
            | std::path::Component::CurDir
            | std::path::Component::ParentDir => None,
        })
        .collect();
    if components.next().is_some() {
        return false;
    }
    match names.as_slice() {
        ["sources", KIND, file] => file.strip_suffix(".md").is_some_and(is_slug),
        _ => false,
    }
}
