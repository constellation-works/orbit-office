//! The careful model reply: conflict checks and the proposed page markdown.
use office_common::Sensitivity;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::evidence::{date_seconds, strings};
use super::group::Group;
use crate::domain::knowledge::Page;
use crate::domain::source::sha256_hex;
use crate::domain::template::Template;
use crate::{Error, Result};

/// Exact evidence returned by the model when flagging a contradiction.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Claim {
    /// Exact source claim text.
    pub text: String,
    /// Full source citations.
    pub cites: Vec<String>,
}
/// A contradiction with the newer source designated current.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Conflict {
    /// Claim from the newer source.
    pub current: Claim,
    /// Claim from the older source.
    pub superseded: Claim,
}
/// Closed careful model response. Target and template are never model-selected.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reply {
    /// Markdown body using the selected template.
    pub body: String,
    /// Explicit conflicts, with both sides cited.
    pub conflicts: Vec<Conflict>,
}
/// Schema validated by the real Models adapter before returning.
#[must_use]
pub fn schema() -> Value {
    let claim = json!({"type":"object","properties":{"text":{"type":"string","minLength":1},"cites":{"type":"array","minItems":1,"items":{"type":"string"}}},"required":["text","cites"],"additionalProperties":false});
    json!({"type":"object","properties":{"body":{"type":"string","minLength":1},"conflicts":{"type":"array","items":{"type":"object","properties":{"current":claim,"superseded":claim},"required":["current","superseded"],"additionalProperties":false}}},"required":["body","conflicts"],"additionalProperties":false})
}

/// Verify model conflict provenance and chronology, then materialize both sides in the body.
pub fn checked_reply(group: &Group, value: Value) -> Result<Reply> {
    let invalid = || {
        Error::Model { code:"model_invalid_response",message:"consolidation needs a page body and conflicts citing exact claims with the newer source current".into(),retryable:false }
    };
    let mut reply: Reply = serde_json::from_value(value).map_err(|_| invalid())?;
    if reply.body.trim().is_empty() || reply.body.trim_start().starts_with("---") {
        return Err(invalid());
    }
    for conflict in &reply.conflicts {
        let find = |claim: &Claim| {
            group.claims.iter().find(|e| {
                e["text"].as_str() == Some(&claim.text)
                    && !claim.cites.is_empty()
                    && claim.cites.iter().all(|c| strings(&e["cites"]).contains(c))
            })
        };
        let current = find(&conflict.current).ok_or_else(invalid)?;
        let old = find(&conflict.superseded).ok_or_else(invalid)?;
        if current["source"] == old["source"]
            || date_seconds(&current["as_of"])
                .zip(date_seconds(&old["as_of"]))
                .is_none_or(|(a, b)| a <= b)
        {
            return Err(invalid());
        }
        reply.body.push_str(&format!(
            "\n\n### Conflict\nCurrent ({}): {} [{}]\n\nSuperseded ({}): {} [{}]\n",
            current["as_of"],
            conflict.current.text,
            conflict.current.cites.join(", "),
            old["as_of"],
            conflict.superseded.text,
            conflict.superseded.cites.join(", ")
        ));
    }
    Ok(reply)
}

/// A template-valid proposed page. Existing frontmatter (including contacts) is preserved.
pub fn markdown(group: &Group, template: &Template, body: &str) -> Result<String> {
    let mut page = if let Some((page, _)) = &group.existing {
        page.clone()
    } else {
        Page::from_markdown(&format!(
            "---\nid: {}\ntitle: {}\ntemplate: {}\n---\n",
            json!(group.page_id(template)),
            json!(group.title),
            json!(template.template)
        ))?
    };
    page.template = Some(template.template.clone());
    page.sensitivity = if group.private || template.sensitivity == Sensitivity::Private {
        Sensitivity::Private
    } else {
        Sensitivity::Normal
    };
    page.aliases.extend(group.names.iter().cloned());
    page.aliases.sort();
    page.aliases.dedup();
    page.cites.extend(group.cites());
    page.cites.sort();
    page.cites.dedup();
    // Reuse the canonical frontmatter writer, restoring contact refs only in the stored page.
    let mut view = page.model_view();
    for (view, original) in view.refs.iter_mut().zip(&page.refs) {
        view.value = Some(original.value.clone());
    }
    view.body = body.into();
    Ok(view.to_markdown())
}

/// Reviewed base identity for a knowledge_update payload.
#[must_use]
pub fn base_sha256(group: &Group) -> Option<String> {
    group
        .existing
        .as_ref()
        .map(|(_, markdown)| sha256_hex(markdown.as_bytes()))
}
