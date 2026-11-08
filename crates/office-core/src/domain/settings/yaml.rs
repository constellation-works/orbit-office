use serde_json::{Map, Value};

use super::helpers::invalid;
use crate::Result;

pub(super) fn parse_yaml(text: &str) -> Result<Value> {
    if text.trim_start().starts_with('{') {
        return serde_json::from_str(text).map_err(|_| invalid("config", "invalid JSON mapping"));
    }
    let mut root = Map::new();
    let mut section: Option<String> = None;
    for raw in text.lines() {
        // Comments outside quotes only. Quoted hashes and endpoint fragments survive.
        let mut quote = None;
        let mut escaped = false;
        let line = raw.split_once('\r').map_or(raw, |(line, _)| line);
        let mut end = line.len();
        for (i, c) in line.char_indices() {
            if escaped {
                escaped = false;
                continue;
            }
            // In double quotes `\` escapes the next character; in single quotes `''`
            // is one quote. The escaped character stays inside the value.
            escaped = (quote == Some('"') && c == '\\')
                || (quote == Some('\'') && c == '\'' && line[i + 1..].starts_with('\''));
            if escaped {
                continue;
            }
            // A quote opens only at the start of a value or list item, so an
            // apostrophe inside a word (`Acme's server`) does not hide a comment.
            if (c == '\'' || c == '"')
                && (quote == Some(c)
                    || (quote.is_none()
                        && (i == 0 || matches!(line.as_bytes()[i - 1], b' ' | b'[' | b','))))
            {
                quote = if quote.is_none() { Some(c) } else { None };
            }
            if c == '#'
                && quote.is_none()
                && (i == 0 || line.as_bytes()[i - 1].is_ascii_whitespace())
            {
                end = i;
                break;
            }
        }
        let line = line[..end].trim_end();
        if line.trim().is_empty() || line == "---" || line == "..." {
            continue;
        }
        if line.contains('\t') {
            return Err(invalid(
                section.as_deref().unwrap_or("config"),
                "tabs are not supported",
            ));
        }
        let nested = line.starts_with("  ") && !line.starts_with("   ");
        if !nested && line.starts_with(' ') {
            return Err(invalid(
                section.as_deref().unwrap_or("config"),
                "use two-space indentation",
            ));
        }
        let line = line.trim();
        if nested {
            let parent = section
                .as_deref()
                .ok_or_else(|| invalid("config", "unexpected nested value"))?;
            if let Some(item) = line.strip_prefix("- ") {
                let list = root
                    .get_mut(parent)
                    .and_then(Value::as_array_mut)
                    .ok_or_else(|| invalid(parent, "unexpected list item"))?;
                list.push(scalar(item, parent)?);
                continue;
            }
            let (key, value) = line
                .split_once(':')
                .ok_or_else(|| invalid(parent, "expected key: value"))?;
            let dotted = format!("{parent}.{key}");
            let map = root
                .get_mut(parent)
                .and_then(Value::as_object_mut)
                .ok_or_else(|| invalid(parent, "unexpected mapping"))?;
            if map.contains_key(key) {
                return Err(invalid(&dotted, "duplicate key"));
            }
            map.insert(key.into(), scalar(value.trim(), &dotted)?);
        } else {
            let (key, value) = line
                .split_once(':')
                .ok_or_else(|| invalid("config", "expected key: value"))?;
            let value = value.trim();
            if root.contains_key(key) {
                return Err(invalid(key, "duplicate key"));
            }
            root.insert(
                key.into(),
                if value.is_empty() {
                    if key == "packs" {
                        Value::Array(Vec::new())
                    } else {
                        Value::Object(Map::new())
                    }
                } else {
                    scalar(value, key)?
                },
            );
            section = Some(key.into());
        }
    }
    Ok(Value::Object(root))
}

pub(super) fn scalar(text: &str, key: &str) -> Result<Value> {
    if text.starts_with('[') {
        let inner = text
            .strip_prefix('[')
            .and_then(|v| v.strip_suffix(']'))
            .ok_or_else(|| invalid(key, "unterminated list"))?;
        return Ok(Value::Array(if inner.trim().is_empty() {
            Vec::new()
        } else {
            inner
                .split(',')
                .map(|v| scalar(v.trim(), key))
                .collect::<Result<Vec<_>>>()?
        }));
    }
    if text.starts_with('"') {
        return serde_json::from_str(text).map_err(|_| invalid(key, "invalid quoted string"));
    }
    if text.starts_with('\'') {
        return text
            .strip_prefix('\'')
            .and_then(|v| v.strip_suffix('\''))
            .map(|v| Value::String(v.replace("''", "'")))
            .ok_or_else(|| invalid(key, "unterminated quoted string"));
    }
    if text == "{}" {
        return Ok(Value::Object(Map::new()));
    }
    if text == "null" || text == "~" {
        return Ok(Value::Null);
    }
    if text.is_empty() || text.starts_with(['&', '*', '!', '{', '|', '>']) {
        return Err(invalid(key, "unsupported YAML value"));
    }
    Ok(serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.into())))
}
