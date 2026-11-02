//! The `scribe:` block of the Library settings (specs/scribe.md,
//! "Configuration"): the pass cadence and the tag definitions.
//!
//! Pure validation. A definition names one of four sinks, all inside the
//! Library or Orbit; a definition that names a file target is refused with
//! `tag_sink_refused` so no tag can write polaris, almanac or any other
//! path outside the Library. The block is written only by
//! `office.config-set`, which is not agent-callable.

use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::{Error, Result};

/// Minutes between passes: 5 through 240.
const MINUTES: std::ops::RangeInclusive<u32> = 5..=240;
/// User turns between passes: 2 through 100.
const TURNS: std::ops::RangeInclusive<u32> = 2..=100;
/// Most tags a Library may define.
const MAX_TAGS: usize = 32;
/// Longest tag name, in bytes.
const MAX_NAME: usize = 32;
/// Longest tag instruction, in characters.
const MAX_INSTRUCTION: usize = 2000;
/// Longest Orbit workspace selector, in bytes.
const MAX_WORKSPACE: usize = 256;
/// Definition keys that would name a file target rather than a sink.
const FILE_TARGET_KEYS: [&str; 7] = [
    "path",
    "file",
    "target",
    "dir",
    "directory",
    "destination",
    "output",
];

/// The pass cadence: a pass starts when either threshold is reached.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct ScribeCadence {
    /// Minutes since the last pass (5–240, default 20).
    pub minutes: u32,
    /// User turns since the last pass (2–100, default 10).
    pub turns: u32,
}

impl Default for ScribeCadence {
    fn default() -> Self {
        Self {
            minutes: 20,
            turns: 10,
        }
    }
}

/// Where a tag's entries go.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TagSink {
    /// The scope's decision ledger.
    DecisionsLedger,
    /// The scope's preference page.
    PreferencePage,
    /// The session's SCR file only.
    ScrOnly,
    /// A proposed Orbit task.
    OrbitTask,
}

impl TagSink {
    fn parse(text: &str) -> Option<Self> {
        match text {
            "decisions_ledger" => Some(Self::DecisionsLedger),
            "preference_page" => Some(Self::PreferencePage),
            "scr_only" => Some(Self::ScrOnly),
            "orbit_task" => Some(Self::OrbitTask),
            _ => None,
        }
    }
}

/// How a tag's entries take effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TagMode {
    /// Entries are proposed and wait for approval.
    Propose,
    /// Entries take effect directly; only `preference_page` accepts this.
    Apply,
    /// Entries are appended as evidence.
    Append,
}

/// What the scribe may record for a tag.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum TagCapture {
    /// Only what the user marked.
    #[serde(rename = "markers")]
    Markers,
    /// Markers, plus unmarked candidates that are always proposals.
    #[serde(rename = "markers+inferred")]
    MarkersInferred,
}

/// Whose scope a tag's entries belong to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TagScope {
    /// Library-wide.
    Global,
    /// The session's project.
    Project,
    /// The session's scope.
    Session,
}

/// One validated tag definition.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ScribeTag {
    /// Lowercase kebab-case, unique.
    pub name: String,
    /// Whose scope the entries belong to.
    pub scope: TagScope,
    /// What the scribe records; passed to the scribe agent as data.
    pub instruction: String,
    /// Where entries go.
    pub sink: TagSink,
    /// How entries take effect.
    pub mode: TagMode,
    /// Markers only, or markers plus inferred proposals.
    pub capture: TagCapture,
    /// For `orbit_task` only: the Orbit workspace selector.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
}

/// The `scribe` block. `tags` is `None` while the key is absent, so the
/// defaults apply and stay current; [`ScribeSettings::effective_tags`] is
/// the list in force.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct ScribeSettings {
    /// Pass cadence.
    pub cadence: ScribeCadence,
    /// Configured tags; `None` means the two shipped defaults.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<ScribeTag>>,
}

impl ScribeSettings {
    /// Whether the block is entirely default (and so omitted from the file).
    #[must_use]
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// The tags in force: the configured list, else the two defaults.
    #[must_use]
    pub fn effective_tags(&self) -> Vec<ScribeTag> {
        self.tags.clone().unwrap_or_else(default_tags)
    }

    /// The effective block as returned by `office.status` and
    /// `office.scribe-pending`.
    #[must_use]
    pub fn effective(&self) -> Value {
        json!({
            "cadence": {"minutes": self.cadence.minutes, "turns": self.cadence.turns},
            "tags": self.effective_tags(),
            "tags_customized": self.tags.is_some(),
        })
    }

    /// Validate the `scribe` mapping read from the file or built by
    /// `with_setting`.
    pub(crate) fn parse(value: &Value) -> Result<Self> {
        let map = value
            .as_object()
            .ok_or_else(|| invalid("scribe", "expected a mapping of cadence and tags"))?;
        let mut out = Self::default();
        for (name, value) in map {
            match name.as_str() {
                "cadence" => out.cadence = parse_cadence(value)?,
                "tags" => out.tags = Some(parse_tags(value)?),
                _ => return Err(invalid(&format!("scribe.{name}"), "unknown scribe setting")),
            }
        }
        Ok(out)
    }
}

/// The two tags that ship by default (D3).
#[must_use]
pub fn default_tags() -> Vec<ScribeTag> {
    vec![
        ScribeTag {
            name: "preferences".into(),
            scope: TagScope::Global,
            instruction: "A standing answer to a class of question, in the user's words.".into(),
            sink: TagSink::PreferencePage,
            mode: TagMode::Propose,
            capture: TagCapture::Markers,
            workspace: None,
        },
        ScribeTag {
            name: "decisions".into(),
            scope: TagScope::Session,
            instruction:
                "A choice that was made, with the question and options from the turns before it."
                    .into(),
            sink: TagSink::DecisionsLedger,
            mode: TagMode::Append,
            capture: TagCapture::Markers,
            workspace: None,
        },
    ]
}

/// Apply one `scribe.*` update to the encoded settings, before validation.
/// `scribe.cadence` and `scribe.tags` take null to restore the default;
/// `scribe.tags.<name>` takes one definition, or null to delete it.
pub(crate) fn set(encoded: &mut Value, key: &str, value: Value) -> Result<()> {
    let root = encoded
        .as_object_mut()
        .ok_or_else(|| invalid("config", "expected a mapping"))?;
    let block = root
        .entry("scribe")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| invalid("scribe", "expected a mapping of cadence and tags"))?;
    match key {
        "scribe.cadence" | "scribe.tags" => {
            let leaf = key.trim_start_matches("scribe.");
            if value.is_null() {
                block.remove(leaf);
            } else {
                block.insert(leaf.into(), value);
            }
        }
        _ => {
            let name = key
                .strip_prefix("scribe.tags.")
                .ok_or_else(|| Error::UnknownSetting { key: key.into() })?;
            let mut tags = match block.get("tags") {
                Some(Value::Array(list)) => list.clone(),
                _ => default_tags()
                    .iter()
                    .map(|tag| serde_json::to_value(tag).unwrap_or(Value::Null))
                    .collect(),
            };
            let position = tags
                .iter()
                .position(|tag| tag.get("name").and_then(Value::as_str) == Some(name));
            if value.is_null() {
                let position = position
                    .ok_or_else(|| invalid(key, "no tag with this name; nothing to delete"))?;
                tags.remove(position);
            } else {
                let mut definition = value;
                let object = definition
                    .as_object_mut()
                    .ok_or_else(|| invalid(key, "expected one tag definition mapping, or null"))?;
                match object.get("name") {
                    None => {
                        object.insert("name".into(), Value::String(name.into()));
                    }
                    Some(Value::String(own)) if own == name => {}
                    Some(_) => {
                        return Err(invalid(key, "the definition's name must match the key"));
                    }
                }
                match position {
                    Some(position) => tags[position] = definition,
                    None => tags.push(definition),
                }
            }
            block.insert("tags".into(), Value::Array(tags));
        }
    }
    Ok(())
}

fn parse_cadence(value: &Value) -> Result<ScribeCadence> {
    let map = value
        .as_object()
        .ok_or_else(|| invalid("scribe.cadence", "expected a mapping of minutes and turns"))?;
    let mut cadence = ScribeCadence::default();
    for (name, value) in map {
        let dotted = format!("scribe.cadence.{name}");
        let (slot, range, reason) = match name.as_str() {
            "minutes" => (
                &mut cadence.minutes,
                MINUTES,
                "expected an integer from 5 to 240",
            ),
            "turns" => (
                &mut cadence.turns,
                TURNS,
                "expected an integer from 2 to 100",
            ),
            _ => return Err(invalid(&dotted, "unknown cadence setting")),
        };
        *slot = value
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .filter(|n| range.contains(n))
            .ok_or_else(|| invalid(&dotted, reason))?;
    }
    Ok(cadence)
}

fn parse_tags(value: &Value) -> Result<Vec<ScribeTag>> {
    let list = value
        .as_array()
        .ok_or_else(|| invalid("scribe.tags", "expected a list of tag definitions"))?;
    if list.len() > MAX_TAGS {
        return Err(invalid("scribe.tags", "at most 32 tag definitions"));
    }
    let mut tags: Vec<ScribeTag> = Vec::with_capacity(list.len());
    for (index, item) in list.iter().enumerate() {
        let tag = parse_tag(item, &format!("scribe.tags[{index}]"))?;
        if tags.iter().any(|seen| seen.name == tag.name) {
            return Err(invalid(
                &format!("scribe.tags.{}", tag.name),
                "duplicate tag name",
            ));
        }
        tags.push(tag);
    }
    Ok(tags)
}

/// Validate one definition. `position` is its list key (`scribe.tags[<i>]`),
/// used for errors raised before the name is known.
fn parse_tag(value: &Value, position: &str) -> Result<ScribeTag> {
    let map = value
        .as_object()
        .ok_or_else(|| invalid(position, "expected each tag as a mapping"))?;
    let name = map
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| well_formed_name(name))
        .ok_or_else(|| {
            invalid(
                &format!("{position}.name"),
                "expected a lowercase kebab-case name of at most 32 characters",
            )
        })?;
    let key = format!("scribe.tags.{name}");
    for field in map.keys() {
        let known = [
            "name",
            "scope",
            "instruction",
            "sink",
            "mode",
            "capture",
            "workspace",
        ];
        if FILE_TARGET_KEYS.contains(&field.as_str()) {
            return Err(Error::TagSinkRefused {
                key: format!("{key}.{field}"),
                reason: "a tag writes to a sink inside the Library or to an Orbit task, never to a file path",
            });
        }
        if !known.contains(&field.as_str()) {
            return Err(invalid(&format!("{key}.{field}"), "unknown tag key"));
        }
    }
    let text = |field: &str| map.get(field).and_then(Value::as_str);
    let scope = match text("scope") {
        Some("global") => TagScope::Global,
        Some("project") => TagScope::Project,
        Some("session") => TagScope::Session,
        _ => {
            return Err(invalid(
                &format!("{key}.scope"),
                "expected global, project or session",
            ));
        }
    };
    let instruction = text("instruction")
        .filter(|text| {
            !text.trim().is_empty()
                && text.chars().count() <= MAX_INSTRUCTION
                && !text.chars().any(|c| c.is_control() && c != '\n')
        })
        .ok_or_else(|| {
            invalid(
                &format!("{key}.instruction"),
                "expected a nonempty instruction of at most 2000 characters",
            )
        })?;
    let sink_text = text("sink").ok_or_else(|| {
        invalid(
            &format!("{key}.sink"),
            "required: decisions_ledger, preference_page, scr_only or orbit_task",
        )
    })?;
    let sink = TagSink::parse(sink_text).ok_or_else(|| {
        if names_a_path(sink_text) {
            Error::TagSinkRefused {
                key: format!("{key}.sink"),
                reason: "a tag writes to a sink inside the Library or to an Orbit task, never to a file path",
            }
        } else {
            invalid(
                &format!("{key}.sink"),
                "expected decisions_ledger, preference_page, scr_only or orbit_task",
            )
        }
    })?;
    let mode = match text("mode") {
        None => match sink {
            TagSink::PreferencePage | TagSink::OrbitTask => TagMode::Propose,
            TagSink::DecisionsLedger | TagSink::ScrOnly => TagMode::Append,
        },
        Some("propose") => TagMode::Propose,
        Some("apply") => TagMode::Apply,
        Some("append") => TagMode::Append,
        Some(_) => {
            return Err(invalid(
                &format!("{key}.mode"),
                "expected propose, apply or append",
            ));
        }
    };
    let paired = match sink {
        TagSink::PreferencePage => matches!(mode, TagMode::Propose | TagMode::Apply),
        TagSink::OrbitTask => mode == TagMode::Propose,
        TagSink::DecisionsLedger | TagSink::ScrOnly => mode == TagMode::Append,
    };
    if !paired {
        return Err(invalid(
            &format!("{key}.mode"),
            "preference_page takes propose or apply, orbit_task takes propose, and decisions_ledger and scr_only take append",
        ));
    }
    let capture = match text("capture") {
        None | Some("markers") => TagCapture::Markers,
        Some("markers+inferred") => TagCapture::MarkersInferred,
        Some(_) => {
            return Err(invalid(
                &format!("{key}.capture"),
                "expected markers or markers+inferred",
            ));
        }
    };
    let workspace = match map.get("workspace") {
        None | Some(Value::Null) => None,
        Some(Value::String(selector)) => {
            if sink != TagSink::OrbitTask {
                return Err(invalid(
                    &format!("{key}.workspace"),
                    "workspace is accepted only on the orbit_task sink",
                ));
            }
            if selector.contains('/') || selector.contains("..") || selector.starts_with(['.', '~'])
            {
                return Err(Error::TagSinkRefused {
                    key: format!("{key}.workspace"),
                    reason: "an orbit_task workspace is an Orbit workspace selector, never a path",
                });
            }
            if selector.is_empty()
                || selector.len() > MAX_WORKSPACE
                || !selector
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"-._/:".contains(&c))
            {
                return Err(invalid(
                    &format!("{key}.workspace"),
                    "expected an Orbit workspace selector without whitespace",
                ));
            }
            Some(selector.clone())
        }
        Some(_) => {
            return Err(invalid(
                &format!("{key}.workspace"),
                "expected an Orbit workspace selector without whitespace",
            ));
        }
    };
    Ok(ScribeTag {
        name: name.into(),
        scope,
        instruction: instruction.into(),
        sink,
        mode,
        capture,
        workspace,
    })
}

/// Lowercase kebab-case: letters and digits in hyphen-separated groups,
/// starting with a letter.
fn well_formed_name(name: &str) -> bool {
    name.len() <= MAX_NAME
        && name.starts_with(|c: char| c.is_ascii_lowercase())
        && name.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        })
}

/// A sink value shaped like a file target rather than a sink name.
fn names_a_path(text: &str) -> bool {
    text.contains(['/', '\\', '.', '~', ':']) || text.starts_with("..")
}

fn invalid(key: &str, reason: &'static str) -> Error {
    Error::InvalidSetting {
        key: key.into(),
        reason,
    }
}
