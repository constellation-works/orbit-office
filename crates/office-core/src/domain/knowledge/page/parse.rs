//! Frontmatter parsing for [`Page`](super::types::Page).

use office_common::Sensitivity;
use serde_json::Value;

use super::citation::page_kind_for_id;
use super::types::{
    Page, PageRef, PageStatus, Performable, RefTarget, Submission, SubmissionVia, invalid,
};
use crate::Result;
use crate::domain::knowledge::DocumentKind;
use crate::domain::knowledge::frontmatter;

const FIELDS: &[&str] = &[
    "id",
    "type",
    "template",
    "title",
    "aliases",
    "summary",
    "tags",
    "cites",
    "refs",
    "related",
    "updated_at",
    "last_verified",
    "status",
    "sensitivity",
    "shareable",
    "shared_by",
    "contributors",
    "as_of",
    "performable",
    "submission",
    "scribe",
];

impl Page {
    /// Parse and validate a page's frontmatter. Unknown fields, malformed
    /// values and refs that are not single plain pointers are refused with
    /// `invalid_frontmatter`, naming the field but never echoing its value.
    pub fn from_markdown(markdown: &str) -> Result<Self> {
        let (frontmatter, body) = frontmatter::split(markdown)?;
        let fields = frontmatter::parse(frontmatter)?;
        if let Some((key, _)) = fields
            .iter()
            .find(|(key, _)| !FIELDS.contains(&key.as_str()))
        {
            return Err(invalid(key, "unknown field"));
        }
        let get = |name: &str| {
            fields
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value)
                .filter(|value| !value.is_null())
        };

        if let Some(member) = text(get("shared_by"), "shared_by")?
            && (member.len() > 32 || !is_slug(&member))
        {
            return Err(invalid("shared_by", "expected a member name"));
        }
        if list(get("contributors"), "contributors")?
            .iter()
            .any(|member| member.len() > 32 || !is_slug(member))
        {
            return Err(invalid("contributors", "expected member names"));
        }
        let id = text(get("id"), "id")?.ok_or_else(|| invalid("id", "required"))?;
        let prefix_kind = page_kind_for_id(&id)
            .ok_or_else(|| invalid("id", "expected dom-<slug> or rb-<slug>"))?;
        let kind = match text(get("type"), "type")?.as_deref() {
            None => prefix_kind,
            Some("domain") => DocumentKind::Domain,
            Some("runbook") => DocumentKind::Runbook,
            Some(_) => return Err(invalid("type", "expected domain or runbook")),
        };
        let template = text(get("template"), "template")?;
        if template.as_deref().is_some_and(|name| !is_slug(name)) {
            return Err(invalid("template", "expected a lowercase kebab-case name"));
        }
        let title = text(get("title"), "title")?
            .filter(|title| !title.trim().is_empty())
            .ok_or_else(|| invalid("title", "required"))?;
        let cites = list(get("cites"), "cites")?;
        let related = list(get("related"), "related")?;
        if related.iter().any(|id| page_kind_for_id(id).is_none()) {
            return Err(invalid("related", "expected page ids (dom-… or rb-…)"));
        }
        let status = match text(get("status"), "status")?.as_deref() {
            None | Some("current") => PageStatus::Current,
            Some("stale") => PageStatus::Stale,
            Some("retired") => PageStatus::Retired,
            Some(_) => return Err(invalid("status", "expected current, stale or retired")),
        };
        let sensitivity = match text(get("sensitivity"), "sensitivity")? {
            None => Sensitivity::Normal,
            Some(token) => Sensitivity::parse(&token)
                .ok_or_else(|| invalid("sensitivity", "expected normal or private"))?,
        };
        let last_verified = date(get("last_verified"), "last_verified")?;
        let as_of = date(get("as_of"), "as_of")?;
        let performable = match text(get("performable"), "performable")?.as_deref() {
            None => None,
            Some("agent") => Some(Performable::Agent),
            Some("you") => Some(Performable::You),
            Some(_) => return Err(invalid("performable", "expected agent or you")),
        };
        let submission = get("submission").map(submission).transpose()?;
        let scribe_handoff = get("scribe").map(scribe).transpose()?.flatten();
        let refs = match get("refs") {
            None => Vec::new(),
            Some(Value::Array(items)) => items.iter().map(page_ref).collect::<Result<_>>()?,
            Some(_) => return Err(invalid("refs", "expected a list of pointers")),
        };

        Ok(Self {
            id,
            kind,
            template,
            title,
            aliases: list(get("aliases"), "aliases")?,
            summary: text(get("summary"), "summary")?.unwrap_or_default(),
            tags: list(get("tags"), "tags")?,
            cites,
            refs,
            related,
            updated_at: text(get("updated_at"), "updated_at")?,
            last_verified,
            status,
            sensitivity,
            shareable: crate::domain::sharing::shareable(markdown)?,
            as_of,
            performable,
            submission,
            scribe_handoff,
            body: body.to_string(),
        })
    }
}

pub(in crate::domain::knowledge) fn is_slug(value: &str) -> bool {
    !value.is_empty()
        && value.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        })
}

fn is_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit())
}

fn text(value: Option<&Value>, field: &str) -> Result<Option<String>> {
    match value {
        None => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.clone())),
        Some(_) => Err(invalid(field, "expected a string")),
    }
}

fn date(value: Option<&Value>, field: &str) -> Result<Option<String>> {
    let value = text(value, field)?;
    if value.as_deref().is_some_and(|v| !is_date(v)) {
        return Err(invalid(field, "expected a YYYY-MM-DD date"));
    }
    Ok(value)
}

fn list(value: Option<&Value>, field: &str) -> Result<Vec<String>> {
    match value {
        None => Ok(Vec::new()),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| {
                item.as_str()
                    .filter(|text| !text.trim().is_empty())
                    .map(str::to_owned)
                    .ok_or_else(|| invalid(field, "expected a list of nonempty strings"))
            })
            .collect(),
        Some(Value::String(text)) if !text.trim().is_empty() => Ok(vec![text.clone()]),
        Some(_) => Err(invalid(field, "expected a list of strings")),
    }
}

/// The `scribe` mapping: only `handoff`, a boolean.
fn scribe(value: &Value) -> Result<Option<bool>> {
    let Value::Object(map) = value else {
        return Err(invalid("scribe", "expected a mapping"));
    };
    if map.keys().any(|key| key != "handoff") {
        return Err(invalid("scribe", "unknown key; expected handoff"));
    }
    match map.get("handoff") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(on)) => Ok(Some(*on)),
        Some(_) => Err(invalid("scribe", "handoff must be true or false")),
    }
}

fn submission(value: &Value) -> Result<Submission> {
    let Value::Object(map) = value else {
        return Err(invalid("submission", "expected a mapping"));
    };
    if map
        .keys()
        .any(|key| !["system", "via", "connection"].contains(&key.as_str()))
    {
        return Err(invalid(
            "submission",
            "unknown key; expected system, via, connection",
        ));
    }
    let string = |key: &str| text(map.get(key).filter(|v| !v.is_null()), "submission");
    let system = string("system")?
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| invalid("submission", "system is required"))?;
    let via = match string("via")?.as_deref() {
        Some("draft") => SubmissionVia::Draft,
        Some("gateway") => SubmissionVia::Gateway,
        _ => return Err(invalid("submission", "via must be draft or gateway")),
    };
    let connection = string("connection")?;
    if via == SubmissionVia::Gateway && connection.is_none() {
        return Err(invalid(
            "submission",
            "a gateway submission names its connection",
        ));
    }
    Ok(Submission {
        system,
        via,
        connection,
    })
}

fn page_ref(value: &Value) -> Result<PageRef> {
    let Value::Object(map) = value else {
        return Err(invalid("refs", "expected { label, <target> } mappings"));
    };
    let mut target = None;
    for key in map.keys() {
        if let Some(kind) = RefTarget::parse(key) {
            if target.is_some() {
                return Err(invalid("refs", "a ref has exactly one target"));
            }
            target = Some((kind, key.as_str()));
        } else if !["label", "authoritative", "as_of"].contains(&key.as_str()) {
            return Err(invalid(
                "refs",
                "unknown key; refs are pointers (label, path, url, contact, account, artifact, authoritative, as_of)",
            ));
        }
    }
    let (kind, key) = target.ok_or_else(|| {
        invalid(
            "refs",
            "a ref needs one of path, url, contact, account, artifact",
        )
    })?;
    let label = text(map.get("label"), "refs")?
        .filter(|label| !label.trim().is_empty())
        .ok_or_else(|| invalid("refs", "a ref needs a label"))?;
    let value = text(map.get(key), "refs")?
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| invalid("refs", "a ref's target must be a nonempty string"))?;
    match kind {
        RefTarget::Account => {
            let valid = value
                .split_once(':')
                .is_some_and(|(system, key)| is_slug(system) && !key.trim().is_empty());
            if !valid {
                return Err(invalid("refs", "an account target is system:key"));
            }
        }
        RefTarget::Artifact => {
            if !value.strip_prefix("art-").is_some_and(is_slug) {
                return Err(invalid("refs", "an artifact target is an art- id"));
            }
        }
        RefTarget::Url => {
            let authority = value
                .split_once("://")
                .map(|(_, rest)| rest.split(['/', '?', '#']).next().unwrap_or_default());
            match authority {
                None => return Err(invalid("refs", "a url target needs a scheme")),
                Some(authority) if authority.contains('@') => {
                    return Err(invalid("refs", "a url target never carries credentials"));
                }
                Some(_) => {}
            }
        }
        RefTarget::Path | RefTarget::Contact => {}
    }
    let authoritative = match map.get("authoritative") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(flag)) => *flag,
        Some(_) => return Err(invalid("refs", "authoritative is true or false")),
    };
    Ok(PageRef {
        label,
        target: kind,
        value,
        authoritative,
        as_of: date(map.get("as_of").filter(|v| !v.is_null()), "refs")?,
    })
}
