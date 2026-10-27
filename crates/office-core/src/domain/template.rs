//! Pure template rules, bundled Markdown and checks shared by knowledge consumers.

use std::collections::BTreeMap;

use office_common::Sensitivity;
use serde_json::Value;

use super::knowledge::DocumentKind;
use crate::{Error, Result};

/// Knowledge layer allowed by a template's `type` rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TemplateKind {
    /// An About page.
    Domain,
    /// A How-to page.
    Runbook,
}

impl TemplateKind {
    /// Canonical frontmatter token.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Domain => "domain",
            Self::Runbook => "runbook",
        }
    }

    fn document_kind(self) -> DocumentKind {
        match self {
            Self::Domain => DocumentKind::Domain,
            Self::Runbook => DocumentKind::Runbook,
        }
    }
}

/// How automatic attachment selects pages using a template.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttachPolicy {
    /// Include current pages on every task.
    Always,
    /// Match the source's participants against person names and aliases.
    Participants,
    /// Match client, project or topic tags.
    Tagged,
    /// Match glossary terms in the source.
    Matched,
    /// No automatic policy attachment (runbooks may still be named by a task).
    Never,
}

/// A fixed id, or an id containing exactly one `<slug>` placeholder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdPattern(String);

impl IdPattern {
    /// The rule as written in frontmatter.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Match a fixed id exactly, or substitute a lowercase kebab-case slug.
    #[must_use]
    pub fn matches(&self, id: &str) -> bool {
        match self.0.split_once("<slug>") {
            Some((prefix, suffix)) => id
                .strip_prefix(prefix)
                .and_then(|rest| rest.strip_suffix(suffix))
                .is_some_and(is_slug),
            None => id == self.0,
        }
    }
}

/// Rules parsed from one template file, plus its Markdown skeleton.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Template {
    /// Template name, used by page frontmatter.
    pub template: String,
    /// Required knowledge layer (`type` in frontmatter).
    pub kind: TemplateKind,
    /// Fixed page id or slug pattern.
    pub id: IdPattern,
    /// Suggested page title, possibly containing a placeholder.
    pub title: String,
    /// Whether the Library may contain only one page using this template.
    pub one_per_library: bool,
    /// Automatic attachment policy.
    pub attach: AttachPolicy,
    /// Default sensitivity for new pages.
    pub sensitivity: Sensitivity,
    /// Suggested level-two headings, in order.
    pub sections: Vec<String>,
    /// Headings that must have content on a page.
    pub required: Vec<String>,
    /// Markdown after the closing frontmatter delimiter, preserved verbatim.
    pub body: String,
}

impl Template {
    /// Parse a flat YAML rules mapping followed by a Markdown skeleton.
    /// Scalars, inline lists and indented block lists are supported. Unknown,
    /// duplicate, missing and unsupported rules fail with the file and field.
    pub fn from_markdown(file: &str, text: &str) -> Result<Self> {
        let (frontmatter, body) = split_frontmatter(file, text)?;
        let rules = parse_rules(file, frontmatter)?;
        let string = |field| rule_string(file, &rules, field);
        let template = string("template")?;
        if !is_slug(&template) {
            return Err(invalid(
                file,
                "template",
                "expected a lowercase kebab-case name",
            ));
        }
        let kind = match string("type")?.as_str() {
            "domain" => TemplateKind::Domain,
            "runbook" => TemplateKind::Runbook,
            _ => return Err(invalid(file, "type", "expected domain or runbook")),
        };
        let pattern = string("id")?;
        let substituted = pattern.replacen("<slug>", "slug", 1);
        if !is_slug(&substituted) {
            return Err(invalid(
                file,
                "id",
                "expected a fixed id or one <slug> placeholder",
            ));
        }
        let one_per_library = rules
            .get("one_per_library")
            .and_then(Value::as_bool)
            .ok_or_else(|| invalid(file, "one_per_library", "expected true or false"))?;
        if one_per_library && pattern.contains("<slug>") {
            return Err(invalid(
                file,
                "id",
                "one-per-Library templates need a fixed id",
            ));
        }
        let attach = match string("attach")?.as_str() {
            "always" => AttachPolicy::Always,
            "participants" => AttachPolicy::Participants,
            "tagged" => AttachPolicy::Tagged,
            "matched" => AttachPolicy::Matched,
            "never" => AttachPolicy::Never,
            _ => {
                return Err(invalid(
                    file,
                    "attach",
                    "expected always, participants, tagged, matched or never",
                ));
            }
        };
        let sensitivity = Sensitivity::parse(&string("sensitivity")?)
            .ok_or_else(|| invalid(file, "sensitivity", "expected normal or private"))?;
        let sections = rule_list(file, &rules, "sections")?;
        let required = rule_list(file, &rules, "required")?;
        if required.iter().any(|section| !sections.contains(section)) {
            return Err(invalid(
                file,
                "required",
                "required headings must appear in sections",
            ));
        }
        Ok(Self {
            template,
            kind,
            id: IdPattern(pattern),
            title: string("title")?,
            one_per_library,
            attach,
            sensitivity,
            sections,
            required,
            body: body.into(),
        })
    }

    /// Validate the resolved page id, frontmatter type and Markdown body.
    /// Template lookup and Library uniqueness checks belong to the caller.
    pub fn validate_page(&self, id: &str, kind: DocumentKind, body: &str) -> Result<()> {
        if !self.id.matches(id) {
            return Err(Error::PageIdMismatch {
                id: id.into(),
                template: self.template.clone(),
                pattern: self.id.as_str().into(),
            });
        }
        if kind != self.kind.document_kind() {
            return Err(Error::TemplateTypeMismatch {
                id: id.into(),
                template: self.template.clone(),
                expected: self.kind.as_str(),
                actual: kind.as_str(),
            });
        }
        let populated = populated_sections(body);
        for section in &self.required {
            if !populated.iter().any(|heading| heading == section) {
                return Err(Error::MissingSection {
                    id: id.into(),
                    template: self.template.clone(),
                    section: section.clone(),
                });
            }
        }
        Ok(())
    }
}

fn invalid(file: &str, field: &str, reason: &'static str) -> Error {
    Error::TemplateInvalid {
        file: file.into(),
        field: field.into(),
        reason,
    }
}

fn is_slug(value: &str) -> bool {
    !value.is_empty()
        && value.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        })
}

fn split_frontmatter<'a>(file: &str, text: &'a str) -> Result<(&'a str, &'a str)> {
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

fn parse_rules<'a>(file: &str, text: &'a str) -> Result<BTreeMap<&'a str, Value>> {
    let mut rules = BTreeMap::new();
    let mut active_list = None;
    for raw in text.lines() {
        let diagnostic_field = if raw.starts_with(char::is_whitespace) {
            active_list.unwrap_or("frontmatter")
        } else {
            raw.split_once(':')
                .map_or("frontmatter", |(field, _)| field.trim())
        };
        let line = uncomment(file, diagnostic_field, raw)?.trim_end();
        if line.trim().is_empty() {
            continue;
        }
        if line.starts_with(char::is_whitespace) {
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
        let value = if raw_value.is_empty() && ["sections", "required"].contains(&field) {
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

fn uncomment<'a>(file: &str, field: &str, text: &'a str) -> Result<&'a str> {
    let end = unquoted_positions(file, field, text, '#')?
        .first()
        .copied()
        .unwrap_or(text.len());
    Ok(&text[..end])
}

fn scalar(file: &str, field: &str, text: &str) -> Result<Value> {
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

fn rule_string(file: &str, rules: &BTreeMap<&str, Value>, field: &str) -> Result<String> {
    rules
        .get(field)
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
        .map(str::to_owned)
        .ok_or_else(|| invalid(file, field, "expected a nonempty string rule"))
}

fn rule_list(file: &str, rules: &BTreeMap<&str, Value>, field: &str) -> Result<Vec<String>> {
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

fn populated_sections(body: &str) -> Vec<String> {
    let mut populated = Vec::new();
    let mut section = None;
    let mut fence: Option<(char, usize)> = None;
    let mut in_comment = false;
    for raw in body.lines() {
        let visible;
        let line = if fence.is_some() {
            raw
        } else {
            visible = without_comments(raw, &mut in_comment);
            visible.as_str()
        };
        let trimmed = line.trim();
        let indented_code = line.starts_with("    ") || line.starts_with('\t');
        let first = trimmed.chars().next();
        let fence_size = trimmed.chars().take_while(|c| Some(*c) == first).count();
        if let Some((marker, size)) = fence {
            if !indented_code
                && first == Some(marker)
                && fence_size >= size
                && trimmed.chars().all(|c| c == marker)
            {
                fence = None;
            } else if !trimmed.is_empty()
                && let Some(title) = section.take()
            {
                populated.push(title);
            }
            continue;
        }
        if !indented_code && matches!(first, Some('`' | '~')) && fence_size >= 3 {
            fence = first.map(|marker| (marker, fence_size));
            continue;
        }
        // Four-space-indented code cannot create a Markdown heading.
        let heading = if indented_code {
            None
        } else {
            atx_heading(line.trim_start())
        };
        if let Some((level, title)) = heading {
            if level <= 2 {
                section = if level == 2 {
                    Some(title.to_owned())
                } else {
                    None
                };
            }
            continue;
        }
        if !trimmed.is_empty()
            && let Some(title) = section.take()
        {
            populated.push(title);
        }
    }
    populated
}

fn without_comments(line: &str, in_comment: &mut bool) -> String {
    let mut visible = String::new();
    let mut rest = line;
    while !rest.is_empty() {
        if *in_comment {
            if let Some((_, after)) = rest.split_once("-->") {
                *in_comment = false;
                rest = after;
            } else {
                break;
            }
        } else if let Some((before, after)) = rest.split_once("<!--") {
            visible.push_str(before);
            *in_comment = true;
            rest = after;
        } else {
            visible.push_str(rest);
            break;
        }
    }
    visible
}

fn atx_heading(line: &str) -> Option<(usize, &str)> {
    let level = line.bytes().take_while(|c| *c == b'#').count();
    if !(1..=6).contains(&level) {
        return None;
    }
    let rest = &line[level..];
    if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let title = rest.trim();
    let without_closing = title.trim_end_matches('#');
    let title = if without_closing.ends_with(char::is_whitespace) {
        without_closing.trim_end()
    } else {
        title
    };
    Some((level, title))
}

/// A compiled-in template file, ready for init to copy without runtime core I/O.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BundledTemplate {
    /// File name relative to the Library's template directory.
    pub filename: &'static str,
    /// Complete frontmatter and Markdown skeleton.
    pub markdown: &'static str,
}

/// All eleven bundled templates, in spec table order. Invalid rules are never
/// silently skipped: consumers parse each file with [`Template::from_markdown`].
pub const BUNDLED: &[BundledTemplate] = &[
    BundledTemplate {
        filename: "user.md",
        markdown: include_str!("../../templates/user.md"),
    },
    BundledTemplate {
        filename: "standing-directions.md",
        markdown: include_str!("../../templates/standing-directions.md"),
    },
    BundledTemplate {
        filename: "systems.md",
        markdown: include_str!("../../templates/systems.md"),
    },
    BundledTemplate {
        filename: "person.md",
        markdown: include_str!("../../templates/person.md"),
    },
    BundledTemplate {
        filename: "client.md",
        markdown: include_str!("../../templates/client.md"),
    },
    BundledTemplate {
        filename: "project.md",
        markdown: include_str!("../../templates/project.md"),
    },
    BundledTemplate {
        filename: "topic.md",
        markdown: include_str!("../../templates/topic.md"),
    },
    BundledTemplate {
        filename: "glossary.md",
        markdown: include_str!("../../templates/glossary.md"),
    },
    BundledTemplate {
        filename: "goals.md",
        markdown: include_str!("../../templates/goals.md"),
    },
    BundledTemplate {
        filename: "rhythm.md",
        markdown: include_str!("../../templates/rhythm.md"),
    },
    BundledTemplate {
        filename: "runbook.md",
        markdown: include_str!("../../templates/runbook.md"),
    },
];
