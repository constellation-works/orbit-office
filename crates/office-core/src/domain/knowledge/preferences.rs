//! Preference matter records and deterministic contradiction checks.

use super::{Page, frontmatter, parse_citation};
use crate::{Error, Result};
use chrono::{Days, NaiveDate};
use serde::{Deserialize, Serialize};

/// One current preference, encoded as scalar/list fields below `## <matter>`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preference {
    /// Lowercase kebab-case class of question.
    pub matter: String,
    /// Current statement, compared exactly after trimming outer whitespace.
    pub statement: String,
    /// SCR tag anchors and/or decision ledger entry ids.
    pub cites: Vec<String>,
    /// Date the preference was stated (`YYYY-MM-DD`).
    pub stated_at: String,
    /// Review date (`YYYY-MM-DD`).
    pub review_by: String,
}

impl Preference {
    /// Construct a record, defaulting review to 90 calendar days after statement.
    pub fn new(
        matter: String,
        statement: String,
        cites: Vec<String>,
        stated_at: String,
        review_by: Option<String>,
    ) -> Result<Self> {
        let date = date(&stated_at).ok_or_else(|| invalid("stated_at"))?;
        let review_by = match review_by {
            Some(value) => value,
            None => date
                .checked_add_days(Days::new(90))
                .ok_or_else(|| invalid("review_by"))?
                .to_string(),
        };
        let preference = Self {
            matter,
            statement,
            cites,
            stated_at,
            review_by,
        };
        preference.validate()?;
        Ok(preference)
    }

    fn validate(&self) -> Result<()> {
        if !super::page::is_slug(&self.matter)
            || self.statement.trim().is_empty()
            || self.statement.contains(['\r', '\n'])
            || self.cites.is_empty()
            || self.cites.iter().any(|cite| parse_citation(cite).is_none())
        {
            return Err(invalid("matter, statement or cites"));
        }
        let stated = date(&self.stated_at).ok_or_else(|| invalid("stated_at"))?;
        let review = date(&self.review_by).ok_or_else(|| invalid("review_by"))?;
        if review < stated {
            return Err(invalid("review_by precedes stated_at"));
        }
        Ok(())
    }

    /// Stable section encoding; JSON strings/lists are valid in the YAML subset.
    #[must_use]
    pub fn markdown(&self) -> String {
        format!(
            "## {}\nstatement: {}\ncites: {}\nstated_at: {}\nreview_by: {}\n",
            self.matter,
            serde_json::json!(self.statement),
            serde_json::json!(self.cites),
            self.stated_at,
            self.review_by
        )
    }
}

/// A current statement quoted on a proposal evidence page.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Conflict {
    /// Existing matter (may differ from the finding for a semantic conflict).
    pub matter: String,
    /// Current statement, not the replacement.
    pub statement: String,
    /// Current statement's citations.
    pub cites: Vec<String>,
}

/// Parse every section and refuse empty pages, missing fields and duplicate matters.
pub fn parse(id: &str, body: &str) -> Result<Vec<Preference>> {
    let mut sections: Vec<(&str, String)> = Vec::new();
    for line in body.lines() {
        if let Some(matter) = line.strip_prefix("## ") {
            sections.push((matter.trim(), String::new()));
        } else if let Some((_, content)) = sections.last_mut() {
            content.push_str(line);
            content.push('\n');
        } else if !line.trim().is_empty() {
            return Err(invalid("content outside a matter section"));
        }
    }
    if sections.is_empty() {
        return Err(missing(id, "<matter>"));
    }
    let mut records: Vec<Preference> = Vec::new();
    for (matter, content) in sections {
        if records.iter().any(|record| record.matter == matter) {
            return Err(invalid("duplicate matter"));
        }
        let fields = frontmatter::parse(content.trim())?;
        for field in ["statement", "cites", "stated_at", "review_by"] {
            if !fields.iter().any(|(key, value)| {
                key == field
                    && !value.is_null()
                    && value.as_str().is_none_or(|text| !text.trim().is_empty())
                    && value.as_array().is_none_or(|items| !items.is_empty())
            }) {
                return Err(Error::InvalidArgument(format!(
                    "preference page {id} matter {matter} requires a nonempty {field} field"
                )));
            }
        }
        let mut object: serde_json::Map<String, serde_json::Value> = fields.into_iter().collect();
        // `matter` comes only from the heading, never an embedded override.
        if object.contains_key("matter") {
            return Err(invalid("embedded matter"));
        }
        object.insert("matter".into(), serde_json::json!(matter));
        let record: Preference = serde_json::from_value(serde_json::Value::Object(object))
            .map_err(|_| invalid("matter fields"))?;
        record.validate()?;
        records.push(record);
    }
    Ok(records)
}

/// Compare only matching matters; evidence/date changes alone are not conflicts.
pub fn conflicts(current: &Page, proposed: &Page) -> Result<Vec<Conflict>> {
    let current = parse(&current.id, &current.body)?;
    let proposed = parse(&proposed.id, &proposed.body)?;
    Ok(current
        .into_iter()
        .filter(|old| {
            proposed
                .iter()
                .any(|new| old.matter == new.matter && old.statement.trim() != new.statement.trim())
        })
        .map(|old| Conflict {
            matter: old.matter,
            statement: old.statement,
            cites: old.cites,
        })
        .collect())
}

fn date(text: &str) -> Option<NaiveDate> {
    if text.len() != 10 {
        return None;
    }
    let date = NaiveDate::parse_from_str(text, "%Y-%m-%d").ok()?;
    (date.to_string() == text).then_some(date)
}
fn missing(id: &str, section: &str) -> Error {
    Error::MissingSection {
        id: id.into(),
        template: "preferences".into(),
        section: section.into(),
    }
}
fn invalid(field: &str) -> Error {
    Error::InvalidArgument(format!("invalid preference {field}"))
}
