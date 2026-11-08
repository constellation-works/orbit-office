//! Frontmatter splitting and template rule parsing.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::{Error, Result};

pub(super) fn invalid(file: &str, field: &str, reason: &'static str) -> Error {
    Error::TemplateInvalid {
        file: file.into(),
        field: field.into(),
        reason,
    }
}

pub(super) fn is_slug(value: &str) -> bool {
    !value.is_empty()
        && value.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        })
}

pub(super) fn split_frontmatter<'a>(file: &str, text: &'a str) -> Result<(&'a str, &'a str)> {
    let rest = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
        .ok_or_else(|| invalid(file, "frontmatter", "expected an opening --- line"))?;
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end_matches(['\r', '\n']) == "---" {
            return Ok((&rest[..offset], &rest[offset + line.len()..]));
        }
        offset += line.len();
    }
    Err(invalid(file, "frontmatter", "expected a closing --- line"))
}

pub(super) fn parse_rules<'a>(file: &str, text: &'a str) -> Result<BTreeMap<&'a str, Value>> {
    let mut rules = BTreeMap::new();
    let mut active_list = None;
    let mut active_block = false;
    for raw in text.lines() {
        let diagnostic_field = if raw.starts_with(char::is_whitespace) {
            if active_block {
                raw.trim_start()
                    .split_once(':')
                    .map_or("submission", |(field, _)| field.trim())
            } else {
                active_list.unwrap_or("frontmatter")
            }
        } else {
            raw.split_once(':')
                .map_or("frontmatter", |(field, _)| field.trim())
        };
        let line = uncomment(file, diagnostic_field, raw)?.trim_end();
        if line.trim().is_empty() {
            continue;
        }
        if line.starts_with(char::is_whitespace) {
            if active_block {
                let trimmed = line.trim_start();
                if trimmed.starts_with("- ") {
                    return Err(invalid(file, "submission", "expected a nested mapping"));
                }
                let (subfield, subvalue) = trimmed
                    .split_once(':')
                    .ok_or_else(|| invalid(file, "submission", "expected field: value"))?;
                let subfield = subfield.trim();
                if !["system", "via", "connection"].contains(&subfield) {
                    return Err(invalid(file, subfield, "unknown rule"));
                }
                let block = rules
                    .get_mut("submission")
                    .and_then(Value::as_object_mut)
                    .ok_or_else(|| invalid(file, "submission", "expected a nested mapping"))?;
                if block.contains_key(subfield) {
                    return Err(invalid(file, subfield, "duplicate rule"));
                }
                let subvalue = subvalue.trim();
                if subvalue.is_empty() {
                    return Err(invalid(file, subfield, "expected a nonempty string rule"));
                }
                let value = scalar(file, subfield, subvalue)?;
                block.insert(subfield.to_owned(), value);
                continue;
            }
            let field = active_list
                .ok_or_else(|| invalid(file, "frontmatter", "unexpected indentation"))?;
            let item = line
                .trim_start()
                .strip_prefix("- ")
                .ok_or_else(|| invalid(file, field, "expected a block list item"))?;
            let value = scalar(file, field, item)?;
            let list = rules
                .get_mut(field)
                .and_then(Value::as_array_mut)
                .ok_or_else(|| invalid(file, field, "unexpected list item"))?;
            list.push(value);
            continue;
        }
        let (field, raw_value) = line
            .split_once(':')
            .ok_or_else(|| invalid(file, "frontmatter", "expected field: value"))?;
        let field = field.trim();
        if ![
            "template",
            "type",
            "id",
            "title",
            "one_per_library",
            "attach",
            "sensitivity",
            "sections",
            "required",
            "performable",
            "submission",
        ]
        .contains(&field)
        {
            return Err(invalid(file, field, "unknown rule"));
        }
        if rules.contains_key(field) {
            return Err(invalid(file, field, "duplicate rule"));
        }
        let raw_value = raw_value.trim();
        active_list = None;
        active_block = false;
        let value = if raw_value.is_empty() && field == "submission" {
            active_block = true;
            Value::Object(serde_json::Map::new())
        } else if raw_value.is_empty() && ["sections", "required"].contains(&field) {
            active_list = Some(field);
            Value::Array(Vec::new())
        } else if raw_value.starts_with('[') {
            let inner = raw_value
                .strip_prefix('[')
                .and_then(|v| v.strip_suffix(']'))
                .ok_or_else(|| invalid(file, field, "expected a closed inline list"))?;
            let mut values = Vec::new();
            let mut start = 0;
            for end in unquoted_positions(file, field, inner, ',')? {
                values.push(scalar(file, field, inner[start..end].trim())?);
                start = end + 1;
            }
            if !inner[start..].trim().is_empty() {
                values.push(scalar(file, field, inner[start..].trim())?);
            }
            Value::Array(values)
        } else {
            scalar(file, field, raw_value)?
        };
        rules.insert(field, value);
    }
    Ok(rules)
}

// The scanner keeps quoted commas and hashes intact, including escaped double
// quotes and YAML's doubled single quotes. Unsupported syntax fails closed.
fn unquoted_positions(file: &str, field: &str, text: &str, separator: char) -> Result<Vec<usize>> {
    let mut quote = None;
    let mut escaped = false;
    let mut positions = Vec::new();
    let mut chars = text.char_indices().peekable();
    while let Some((index, c)) = chars.next() {
        if escaped {
            escaped = false;
            continue;
        }
        if quote == Some('"') && c == '\\' {
            escaped = true;
            continue;
        }
        if quote == Some(c) {
            if c == '\'' && chars.peek().is_some_and(|(_, next)| *next == '\'') {
                chars.next();
            } else {
                quote = None;
            }
        } else if quote.is_none() {
            if c == separator {
                if separator != '#'
                    || index == 0
                    || text.as_bytes()[index - 1].is_ascii_whitespace()
                {
                    positions.push(index);
                    if separator == '#' {
                        break;
                    }
                }
            } else if (c == '\'' || c == '"')
                && text[..index]
                    .trim_end()
                    .chars()
                    .next_back()
                    .is_none_or(|previous| [':', '[', ',', '-'].contains(&previous))
            {
                quote = Some(c);
            }
        }
    }
    if quote.is_some() {
        return Err(invalid(file, field, "unterminated quoted string"));
    }
    Ok(positions)
}

pub(super) fn uncomment<'a>(file: &str, field: &str, text: &'a str) -> Result<&'a str> {
    let end = unquoted_positions(file, field, text, '#')?
        .first()
        .copied()
        .unwrap_or(text.len());
    Ok(&text[..end])
}

pub(super) fn scalar(file: &str, field: &str, text: &str) -> Result<Value> {
    if text.starts_with('"') {
        return serde_json::from_str::<String>(text)
            .map(Value::String)
            .map_err(|_| invalid(file, field, "invalid double-quoted string"));
    }
    if text.starts_with('\'') {
        return text
            .strip_prefix('\'')
            .and_then(|v| v.strip_suffix('\''))
            .filter(|v| !v.replace("''", "").contains('\''))
            .map(|v| Value::String(v.replace("''", "'")))
            .ok_or_else(|| invalid(file, field, "invalid single-quoted string"));
    }
    if text.is_empty()
        || text.starts_with(['&', '*', '!', '[', '{', '|', '>'])
        || text.contains(": ")
    {
        return Err(invalid(
            file,
            field,
            "unsupported scalar; quote strings with YAML punctuation",
        ));
    }
    Ok(serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.into())))
}

pub(super) fn rule_string(
    file: &str,
    rules: &BTreeMap<&str, Value>,
    field: &str,
) -> Result<String> {
    rules
        .get(field)
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
        .map(str::to_owned)
        .ok_or_else(|| invalid(file, field, "expected a nonempty string rule"))
}

pub(super) fn rule_list(
    file: &str,
    rules: &BTreeMap<&str, Value>,
    field: &str,
) -> Result<Vec<String>> {
    let values = rules
        .get(field)
        .and_then(Value::as_array)
        .ok_or_else(|| invalid(file, field, "expected a list rule"))?;
    let mut sections = Vec::new();
    for value in values {
        let section = value
            .as_str()
            .filter(|s| !s.trim().is_empty() && !s.contains(['\r', '\n']))
            .ok_or_else(|| invalid(file, field, "expected nonempty heading names"))?;
        if sections.iter().any(|s| s == section) {
            return Err(invalid(file, field, "duplicate heading"));
        }
        sections.push(section.into());
    }
    Ok(sections)
}
