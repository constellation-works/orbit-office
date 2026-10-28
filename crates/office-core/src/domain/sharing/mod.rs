//! Sharing approval and the pure projection sent to a team's shared Library.

mod settings;
pub use settings::SharedSettings;

use crate::domain::knowledge::{DocumentRecord, frontmatter, inline_citations, parse_citation};
use crate::{Error, Result};
use serde_json::Value;
use std::collections::BTreeSet;

/// Checked sharing refusals; diagnostics never contain document contents.
#[derive(Debug, thiserror::Error)]
pub enum SharingError {
    /// Private documents cannot be shared.
    #[error("sensitivity is private; sharing is refused")]
    Private,
    /// Own citations must have sharing approval first.
    #[error("cites unshared own documents: {}", .ids.join(", "))]
    CitesUnshared {
        /// Sorted, distinct own document ids, without anchors.
        ids: Vec<String>,
    },
    /// No indexed document has the requested id.
    #[error("document was not found")]
    DocumentNotFound,
}

impl SharingError {
    /// Stable tool envelope code.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Private => "share_private_refused",
            Self::CitesUnshared { .. } => "share_cites_unshared",
            Self::DocumentNotFound => "document_not_found",
        }
    }
}

fn invalid(field: &str, reason: &'static str) -> Error {
    Error::InvalidFrontmatter {
        field: field.into(),
        reason,
    }
}

/// Parse the field strictly. Only absence defaults to false.
pub fn shareable(markdown: &str) -> Result<bool> {
    let (header, _) = frontmatter::split(markdown)?;
    let fields = frontmatter::parse(header)?;
    flag(&fields)
}

fn flag(fields: &[(String, Value)]) -> Result<bool> {
    fields
        .iter()
        .find(|(key, _)| key == "shareable")
        .map_or(Ok(false), |(_, value)| {
            value
                .as_bool()
                .ok_or_else(|| invalid("shareable", "expected true or false"))
        })
}

/// Rewrite only the field's scalar (or insert one line before the closing
/// delimiter), retaining unrelated bytes, comments, whitespace and line endings.
/// The adapter calls this with the current document under its writer lock.
pub fn with_shareable(markdown: &str, requested: bool) -> Result<String> {
    let (header, _) = frontmatter::split(markdown)?;
    let fields = frontmatter::parse(header)?;
    if flag(&fields)? == requested {
        return Ok(markdown.into());
    }
    let start = markdown.len()
        - markdown
            .strip_prefix("---\r\n")
            .or_else(|| markdown.strip_prefix("---\n"))
            .ok_or_else(|| invalid("frontmatter", "expected delimiter"))?
            .len();
    let mut offset = start;
    for line in header.split_inclusive('\n') {
        if let Some((key, value)) = line.split_once(':')
            && key.trim_end() == "shareable"
        {
            let leading = value.len() - value.trim_start().len();
            let value_start = offset + key.len() + 1 + leading;
            let scalar = value.trim_start();
            let old = if scalar.starts_with("true") {
                "true"
            } else {
                "false"
            };
            let mut next = markdown.to_string();
            next.replace_range(
                value_start..value_start + old.len(),
                if requested { "true" } else { "false" },
            );
            return Ok(next);
        }
        offset += line.len();
    }
    let newline = if markdown.starts_with("---\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut next = markdown.to_string();
    next.insert_str(
        start + header.len(),
        &format!("shareable: {requested}{newline}"),
    );
    Ok(next)
}

/// Enforce sharing approval against current own documents. A missing own
/// citation fails closed; already qualified citations belong to the team.
/// Include artifact refs as well as frontmatter and inline citations.
pub fn validate_sharing(
    markdown: &str,
    mut lookup: impl FnMut(&str) -> Result<Option<DocumentRecord>>,
) -> Result<()> {
    let (header, body) = frontmatter::split(markdown)?;
    let fields = frontmatter::parse(header)?;
    if !flag(&fields)? {
        return Ok(());
    }
    if fields
        .iter()
        .any(|(key, value)| key == "sensitivity" && value.as_str() != Some("normal"))
    {
        return Err(SharingError::Private.into());
    }
    let mut cited = BTreeSet::new();
    for (key, value) in &fields {
        if key == "cites" {
            let list = value
                .as_array()
                .ok_or_else(|| invalid("cites", "expected a list"))?;
            for value in list {
                cited.insert(
                    value
                        .as_str()
                        .ok_or_else(|| invalid("cites", "expected strings"))?
                        .to_string(),
                );
            }
        }
        if key == "refs"
            && let Some(refs) = value.as_array()
        {
            for reference in refs {
                if let Some(id) = reference.get("artifact").and_then(Value::as_str) {
                    cited.insert(id.into());
                }
            }
        }
    }
    cited.extend(inline_citations(body));
    let mut own_ids = BTreeSet::new();
    for cite in cited {
        let citation =
            parse_citation(&cite).ok_or_else(|| invalid("cites", "expected a Library citation"))?;
        if !citation.id.contains('/') {
            own_ids.insert(citation.id);
        }
    }
    let mut unshared = BTreeSet::new();
    for id in own_ids {
        if lookup(&id)?.is_none_or(|doc| !doc.shareable || doc.sensitivity != "normal") {
            unshared.insert(id);
        }
    }
    if !unshared.is_empty() {
        return Err(SharingError::CitesUnshared {
            ids: unshared.into_iter().collect(),
        }
        .into());
    }
    Ok(())
}

fn qualify(cite: &str, member: &str) -> String {
    if (cite.starts_with("src-") || cite.starts_with("art-")) && parse_citation(cite).is_some() {
        format!("{member}/{cite}")
    } else {
        cite.into()
    }
}

/// Project a document for a push without I/O. The caller supplies the current
/// approval set and the ids already in the shared Library. Never mutates input.
/// Shared metadata is rendered canonically; body bytes outside citations survive.
pub fn shared_view(
    markdown: &str,
    member: &str,
    mut is_shareable: impl FnMut(&str) -> bool,
    mut is_shared: impl FnMut(&str) -> bool,
) -> Result<String> {
    settings::validate_member(member)?;
    let (header, body) = frontmatter::split(markdown)?;
    let mut fields = frontmatter::parse(header)?;
    if fields
        .iter()
        .any(|(key, value)| key == "sensitivity" && value.as_str() != Some("normal"))
    {
        return Err(SharingError::Private.into());
    }
    fields.retain(|(key, _)| !["sensitivity", "shareable", "shared_by"].contains(&key.as_str()));
    for (key, value) in &mut fields {
        if key == "refs"
            && let Some(refs) = value.as_array_mut()
        {
            refs.retain(|reference| {
                reference.get("contact").is_none()
                    && reference.get("path").is_none()
                    && reference
                        .get("artifact")
                        .and_then(Value::as_str)
                        .is_none_or(&mut is_shareable)
            });
            for reference in refs {
                if let Some(Value::String(id)) = reference.get_mut("artifact") {
                    *id = qualify(id, member);
                }
            }
        }
        if key == "cites"
            && let Some(cites) = value.as_array_mut()
        {
            for cite in cites {
                if let Value::String(text) = cite {
                    *text = qualify(text, member);
                }
            }
        }
        if key == "related"
            && let Some(related) = value.as_array_mut()
        {
            related.retain(|id| {
                id.as_str()
                    .is_some_and(|id| is_shareable(id) || is_shared(id))
            });
        }
    }
    let contributors = fields.iter_mut().find(|(key, _)| key == "contributors");
    if let Some((_, value)) = contributors {
        let list = value
            .as_array_mut()
            .ok_or_else(|| invalid("contributors", "expected a list"))?;
        if !list.iter().all(Value::is_string) {
            return Err(invalid("contributors", "expected strings"));
        }
        if !list.iter().any(|value| value.as_str() == Some(member)) {
            list.push(Value::String(member.into()));
        }
    } else {
        fields.push(("contributors".into(), serde_json::json!([member])));
    }
    fields.push(("shared_by".into(), Value::String(member.into())));
    let mut output = String::from("---\n");
    for (key, value) in fields {
        output.push_str(&format!(
            "{key}: {}\n",
            crate::domain::artifact::yaml_value(&value)
        ));
    }
    output.push_str("---\n");
    // The citation scanner defines exactly which bracket groups are citations.
    // Replace their tokens only inside those groups, preserving punctuation.
    let mut rest = body;
    while let Some(open) = rest.find('[') {
        output.push_str(&rest[..open + 1]);
        let after = &rest[open + 1..];
        let Some(close) = after.find([']', '[', '\n']) else {
            rest = after;
            break;
        };
        let inner = &after[..close];
        if after.as_bytes()[close] == b']'
            && inner
                .split([';', ','])
                .all(|part| parse_citation(part.trim()).is_some())
        {
            for part in inner.split_inclusive([';', ',']) {
                let text = part.trim().trim_end_matches([';', ',']).trim_end();
                if let Some(at) = part.find(text) {
                    output.push_str(&part[..at]);
                    output.push_str(&qualify(text, member));
                    output.push_str(&part[at + text.len()..]);
                }
            }
        } else {
            output.push_str(inner);
        }
        rest = &after[close..];
        if rest.starts_with(']') {
            output.push(']');
            rest = &rest[1..];
        }
    }
    output.push_str(rest);
    Ok(output)
}
