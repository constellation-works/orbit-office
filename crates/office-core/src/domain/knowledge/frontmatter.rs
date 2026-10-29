//! The YAML subset knowledge page frontmatter uses, parsed without I/O.
//!
//! Supported: a flat top-level mapping; plain, single- or double-quoted
//! scalars; `true`/`false`/`null`; inline flow lists and flow mappings
//! (`[a, b]`, `{ label: X, url: Y }`); indented block lists whose items are
//! scalars, flow values or `key: value` mappings; one level of indented
//! block mapping under a key; and `#` comments. Anything else (anchors,
//! tags, block scalars, deeper nesting) is refused with the field it was
//! found in, never guessed.

use serde_json::{Map, Value};

use crate::{Error, Result};

/// Deepest flow nesting accepted (`[{ … }]` is two).
const MAX_FLOW_DEPTH: usize = 3;

fn invalid(field: &str, reason: &'static str) -> Error {
    Error::InvalidFrontmatter {
        field: field.into(),
        reason,
    }
}

/// Split leading `---` frontmatter from the body (the closing delimiter is
/// a line holding only `---`). The body is returned verbatim.
pub(crate) fn split(text: &str) -> Result<(&str, &str)> {
    let rest = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
        .ok_or_else(|| invalid("frontmatter", "expected an opening --- line"))?;
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end_matches(['\r', '\n']) == "---" {
            return Ok((&rest[..offset], &rest[offset + line.len()..]));
        }
        offset += line.len();
    }
    Err(invalid("frontmatter", "expected a closing --- line"))
}

/// One significant line: its indentation and its text without the comment.
struct Line<'a> {
    indent: usize,
    text: &'a str,
}

/// Parse the frontmatter into its top-level fields, in file order.
/// Duplicate keys are refused.
pub(crate) fn parse(text: &str) -> Result<Vec<(String, Value)>> {
    let mut lines = Vec::new();
    let mut field = "frontmatter".to_string();
    for raw in text.lines() {
        if raw.contains('\t') {
            return Err(invalid(&field, "tabs are not valid YAML indentation"));
        }
        let indent = raw.len() - raw.trim_start().len();
        if indent == 0
            && let Some((key, _)) = raw.split_once(':')
        {
            field = key.trim().to_string();
        }
        let text = strip_comment(&field, raw)?.trim_end();
        if text.trim().is_empty() {
            continue;
        }
        lines.push(Line {
            indent,
            text: text.trim_start(),
        });
    }

    let mut fields: Vec<(String, Value)> = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let line = &lines[index];
        if line.indent != 0 {
            return Err(invalid(&field_before(&fields), "unexpected indentation"));
        }
        let (key, rest) =
            split_key(line.text).ok_or_else(|| invalid("frontmatter", "expected field: value"))?;
        if fields.iter().any(|(existing, _)| existing == key) {
            return Err(invalid(key, "duplicate field"));
        }
        index += 1;
        let value = if rest.is_empty() {
            let start = index;
            while index < lines.len() && lines[index].indent > 0 {
                index += 1;
            }
            block(key, &lines[start..index])?
        } else {
            inline(key, rest)?
        };
        fields.push((key.to_string(), value));
    }
    Ok(fields)
}

/// Render one value in the flow subset [`parse`] accepts: objects as
/// `{ k: v, … }` with unquoted keys, arrays as `[…]`, and scalars through
/// their JSON spelling. Callers that store larger nested state as quoted
/// JSON strings keep that encoding; this only renders the value given.
pub(crate) fn flow_value(value: &Value) -> String {
    match value {
        Value::Object(map) => format!(
            "{{ {} }}",
            map.iter()
                .map(|(key, value)| format!("{key}: {}", flow_value(value)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::Array(items) => format!(
            "[{}]",
            items.iter().map(flow_value).collect::<Vec<_>>().join(", ")
        ),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => value.to_string(),
    }
}

fn field_before(fields: &[(String, Value)]) -> String {
    fields
        .last()
        .map_or_else(|| "frontmatter".to_string(), |(key, _)| key.clone())
}

/// `key: rest`, with a plain key of letters, digits and underscores.
fn split_key(text: &str) -> Option<(&str, &str)> {
    let (key, rest) = text.split_once(':')?;
    let key = key.trim();
    if key.is_empty()
        || !key
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
    {
        return None;
    }
    if !rest.is_empty() && !rest.starts_with(' ') {
        return None;
    }
    Some((key, rest.trim()))
}

/// The value of a key with nothing after the colon: an indented block list,
/// an indented one-level mapping, or (with no lines) null.
fn block(field: &str, lines: &[Line<'_>]) -> Result<Value> {
    let Some(first) = lines.first() else {
        return Ok(Value::Null);
    };
    if first.text == "-" || first.text.starts_with("- ") {
        return block_list(field, lines);
    }
    let mut map = Map::new();
    for line in lines {
        if line.indent != first.indent {
            return Err(invalid(field, "nested mappings are not supported"));
        }
        let (key, rest) =
            split_key(line.text).ok_or_else(|| invalid(field, "expected key: value"))?;
        if rest.is_empty() {
            return Err(invalid(field, "nested mappings are not supported"));
        }
        if map.insert(key.into(), inline(field, rest)?).is_some() {
            return Err(invalid(field, "duplicate key"));
        }
    }
    Ok(Value::Object(map))
}

fn block_list(field: &str, lines: &[Line<'_>]) -> Result<Value> {
    let dash = lines[0].indent;
    let mut items = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let line = &lines[index];
        if line.indent != dash {
            return Err(invalid(field, "inconsistent list indentation"));
        }
        let item = line
            .text
            .strip_prefix('-')
            .map(str::trim)
            .ok_or_else(|| invalid(field, "expected a list item"))?;
        index += 1;
        if item.is_empty() {
            return Err(invalid(field, "empty list item"));
        }
        // `- key: value` opens a block mapping item; deeper lines continue it.
        if !item.starts_with(['"', '\'', '[', '{'])
            && let Some((key, rest)) = split_key(item)
        {
            let mut map = Map::new();
            if rest.is_empty() {
                return Err(invalid(field, "nested mappings are not supported"));
            }
            map.insert(key.into(), inline(field, rest)?);
            while index < lines.len() && lines[index].indent > dash {
                let (key, rest) = split_key(lines[index].text)
                    .ok_or_else(|| invalid(field, "expected key: value"))?;
                if rest.is_empty() {
                    return Err(invalid(field, "nested mappings are not supported"));
                }
                if map.insert(key.into(), inline(field, rest)?).is_some() {
                    return Err(invalid(field, "duplicate key"));
                }
                index += 1;
            }
            items.push(Value::Object(map));
            continue;
        }
        items.push(inline(field, item)?);
    }
    Ok(Value::Array(items))
}

/// A value written on the key's own line: a flow collection or a scalar.
fn inline(field: &str, text: &str) -> Result<Value> {
    if text.starts_with('[') || text.starts_with('{') {
        let mut flow = Flow {
            field,
            chars: text.char_indices().collect(),
            at: 0,
            text,
        };
        let value = flow.value(0)?;
        flow.skip_spaces();
        if flow.at != flow.chars.len() {
            return Err(invalid(field, "unexpected text after a flow collection"));
        }
        return Ok(value);
    }
    scalar(field, text)
}

/// A complete scalar: quoted, or plain (with `true`, `false` and `null`).
fn scalar(field: &str, text: &str) -> Result<Value> {
    let text = text.trim();
    if text.starts_with('"') {
        return serde_json::from_str::<String>(text)
            .map(Value::String)
            .map_err(|_| invalid(field, "invalid double-quoted string"));
    }
    if text.starts_with('\'') {
        return text
            .strip_prefix('\'')
            .and_then(|v| v.strip_suffix('\''))
            .filter(|v| !v.replace("''", "").contains('\''))
            .map(|v| Value::String(v.replace("''", "'")))
            .ok_or_else(|| invalid(field, "invalid single-quoted string"));
    }
    if text.is_empty() {
        return Err(invalid(field, "expected a value"));
    }
    if text.starts_with(['&', '*', '!', '|', '>', '@', '`', '%', ']', '}', ','])
        || text.contains(": ")
    {
        return Err(invalid(
            field,
            "unsupported scalar; quote strings with YAML punctuation",
        ));
    }
    Ok(match text {
        "true" => Value::Bool(true),
        "false" => Value::Bool(false),
        "null" | "~" => Value::Null,
        _ => Value::String(text.into()),
    })
}

/// A recursive-descent reader over one flow collection.
struct Flow<'a> {
    field: &'a str,
    chars: Vec<(usize, char)>,
    at: usize,
    text: &'a str,
}

impl Flow<'_> {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.at).map(|(_, c)| *c)
    }

    fn skip_spaces(&mut self) {
        while self.peek().is_some_and(|c| c == ' ') {
            self.at += 1;
        }
    }

    fn offset(&self) -> usize {
        self.chars
            .get(self.at)
            .map_or(self.text.len(), |(offset, _)| *offset)
    }

    fn value(&mut self, depth: usize) -> Result<Value> {
        self.skip_spaces();
        match self.peek() {
            Some('[') | Some('{') if depth >= MAX_FLOW_DEPTH => {
                Err(invalid(self.field, "flow collections nest too deeply"))
            }
            Some('[') => self.list(depth + 1),
            Some('{') => self.map(depth + 1),
            Some('"') | Some('\'') => self.quoted(),
            Some(_) => self.plain(),
            None => Err(invalid(self.field, "unterminated flow collection")),
        }
    }

    fn list(&mut self, depth: usize) -> Result<Value> {
        self.at += 1;
        let mut items = Vec::new();
        loop {
            self.skip_spaces();
            if self.peek() == Some(']') {
                self.at += 1;
                return Ok(Value::Array(items));
            }
            items.push(self.value(depth)?);
            self.skip_spaces();
            match self.peek() {
                Some(',') => self.at += 1,
                Some(']') => {}
                _ => return Err(invalid(self.field, "expected , or ] in a flow list")),
            }
        }
    }

    fn map(&mut self, depth: usize) -> Result<Value> {
        self.at += 1;
        let mut map = Map::new();
        loop {
            self.skip_spaces();
            if self.peek() == Some('}') {
                self.at += 1;
                return Ok(Value::Object(map));
            }
            let start = self.offset();
            while self
                .peek()
                .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            {
                self.at += 1;
            }
            let key = self.text[start..self.offset()].to_string();
            if key.is_empty() || self.peek() != Some(':') {
                return Err(invalid(self.field, "expected key: value in a flow mapping"));
            }
            self.at += 1;
            let value = self.value(depth)?;
            if map.insert(key, value).is_some() {
                return Err(invalid(self.field, "duplicate key"));
            }
            self.skip_spaces();
            match self.peek() {
                Some(',') => self.at += 1,
                Some('}') => {}
                _ => return Err(invalid(self.field, "expected , or } in a flow mapping")),
            }
        }
    }

    fn quoted(&mut self) -> Result<Value> {
        let start = self.offset();
        let quote = self.peek();
        self.at += 1;
        loop {
            match self.peek() {
                None => return Err(invalid(self.field, "unterminated quoted string")),
                Some('\\') if quote == Some('"') => self.at += 2,
                Some('\'') if quote == Some('\'') => {
                    self.at += 1;
                    if self.peek() == Some('\'') {
                        self.at += 1;
                    } else {
                        break;
                    }
                }
                Some(c) if Some(c) == quote => {
                    self.at += 1;
                    break;
                }
                Some(_) => self.at += 1,
            }
        }
        let end = self.offset();
        scalar(self.field, &self.text[start..end])
    }

    fn plain(&mut self) -> Result<Value> {
        let start = self.offset();
        while self.peek().is_some_and(|c| ![',', ']', '}'].contains(&c)) {
            self.at += 1;
        }
        let end = self.offset();
        let text = self.text[start..end].trim();
        if text.starts_with(['[', '{']) || text.contains(['[', '{']) {
            return Err(invalid(self.field, "unexpected bracket in a plain value"));
        }
        scalar(self.field, text)
    }
}

/// Drop a `#` comment (at the start, or after whitespace) outside quotes.
fn strip_comment<'a>(field: &str, line: &'a str) -> Result<&'a str> {
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut previous: Option<char> = None;
    let mut chars = line.char_indices().peekable();
    while let Some((index, c)) = chars.next() {
        if escaped {
            escaped = false;
            previous = Some(c);
            continue;
        }
        match quote {
            Some('"') if c == '\\' => escaped = true,
            Some(q) if c == q => {
                if q == '\'' && chars.peek().is_some_and(|(_, next)| *next == '\'') {
                    chars.next();
                } else {
                    quote = None;
                }
            }
            Some(_) => {}
            None => {
                if c == '#' && previous.is_none_or(char::is_whitespace) {
                    return Ok(&line[..index]);
                }
                if (c == '"' || c == '\'')
                    && line[..index]
                        .trim_end()
                        .chars()
                        .next_back()
                        .is_none_or(|p| [':', '[', '{', ',', '-'].contains(&p))
                {
                    quote = Some(c);
                }
            }
        }
        previous = Some(c);
    }
    if quote.is_some() {
        return Err(invalid(field, "unterminated quoted string"));
    }
    Ok(line)
}
