//! Bundled templates and template-drift assessment.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::Value;

use crate::domain::knowledge::{DocumentKind, DocumentRecord};

use super::rules::{scalar, uncomment};
use super::types::Template;

/// A compiled-in template file, ready for init to copy without runtime core I/O.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BundledTemplate {
    /// File name relative to the Library's template directory.
    pub filename: &'static str,
    /// Complete frontmatter and Markdown skeleton.
    pub markdown: &'static str,
}

/// All twelve bundled templates, in spec table order. Invalid rules are never
/// silently skipped: consumers parse each file with [`Template::from_markdown`].
pub const BUNDLED: &[BundledTemplate] = &[
    BundledTemplate {
        filename: "user.md",
        markdown: include_str!("../../../templates/user.md"),
    },
    BundledTemplate {
        filename: "standing-directions.md",
        markdown: include_str!("../../../templates/standing-directions.md"),
    },
    BundledTemplate {
        filename: "systems.md",
        markdown: include_str!("../../../templates/systems.md"),
    },
    BundledTemplate {
        filename: "person.md",
        markdown: include_str!("../../../templates/person.md"),
    },
    BundledTemplate {
        filename: "client.md",
        markdown: include_str!("../../../templates/client.md"),
    },
    BundledTemplate {
        filename: "project.md",
        markdown: include_str!("../../../templates/project.md"),
    },
    BundledTemplate {
        filename: "topic.md",
        markdown: include_str!("../../../templates/topic.md"),
    },
    BundledTemplate {
        filename: "glossary.md",
        markdown: include_str!("../../../templates/glossary.md"),
    },
    BundledTemplate {
        filename: "goals.md",
        markdown: include_str!("../../../templates/goals.md"),
    },
    BundledTemplate {
        filename: "rhythm.md",
        markdown: include_str!("../../../templates/rhythm.md"),
    },
    BundledTemplate {
        filename: "scribe-rules.md",
        markdown: include_str!("../../../templates/scribe-rules.md"),
    },
    BundledTemplate {
        filename: "preferences.md",
        markdown: include_str!("../../../templates/preferences.md"),
    },
    BundledTemplate {
        filename: "runbook.md",
        markdown: include_str!("../../../templates/runbook.md"),
    },
];

/// One template file read from `.office/templates/`.
#[derive(Clone, Copy, Debug)]
pub struct TemplateFile<'a> {
    /// Library-relative path. Status names this path when the file does not parse.
    pub path: &'a str,
    /// Markdown when the file was read. `None` means it could not be read.
    pub markdown: Option<&'a str>,
}

/// A knowledge page checked against the template it names.
#[derive(Clone, Copy, Debug)]
pub struct KnowledgePage<'a> {
    /// Layer implied by the page's path when frontmatter omits `type`.
    pub kind: DocumentKind,
    /// Full Markdown, including frontmatter.
    pub markdown: &'a str,
}

/// Template drift for `office.status`. Parsing failures stay in this value.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TemplateAssessment {
    /// Pages that name a template and do not meet its current rules.
    pub pages_off_template: u32,
    /// Template files that do not parse, sorted by the supplied path.
    pub template_invalid: Vec<String>,
}

/// Parse template files and count pages that name a template they no longer
/// meet. A file that cannot be read or parsed is listed in
/// [`TemplateAssessment::template_invalid`] and does not fail this function.
/// Pages with no `template` field are free-form and are not counted.
/// When several valid files declare the same template name, the first one
/// in `templates` is the current template.
#[must_use]
pub fn assess_templates(
    templates: &[TemplateFile<'_>],
    pages: &[KnowledgePage<'_>],
) -> TemplateAssessment {
    let mut by_name = BTreeMap::new();
    let mut template_invalid = Vec::new();
    for file in templates {
        let Some(markdown) = file.markdown else {
            template_invalid.push(file.path.to_string());
            continue;
        };
        match Template::from_markdown(file.path, markdown) {
            Ok(template) => {
                let name = template.template.clone();
                by_name.entry(name).or_insert(template);
            }
            Err(_) => template_invalid.push(file.path.to_string()),
        }
    }
    template_invalid.sort();
    template_invalid.dedup();

    let mut pages_off_template = 0u32;
    for page in pages {
        let Some(name) = frontmatter_scalar(page.markdown, "template") else {
            continue;
        };
        let Some(template) = by_name.get(&name) else {
            pages_off_template = pages_off_template.saturating_add(1);
            continue;
        };
        let kind = match frontmatter_scalar(page.markdown, "type").as_deref() {
            Some("domain") => DocumentKind::Domain,
            Some("runbook") => DocumentKind::Runbook,
            Some(_) => {
                pages_off_template = pages_off_template.saturating_add(1);
                continue;
            }
            None => page.kind,
        };
        let relative = match kind {
            DocumentKind::Runbook => Path::new("knowledge/runbooks/page.md"),
            DocumentKind::Domain | DocumentKind::Source | DocumentKind::Artifact => {
                Path::new("knowledge/domain/page.md")
            }
        };
        let record = DocumentRecord::from_markdown(relative, page.markdown);
        if template
            .validate_page(&record.id, kind, &record.body)
            .is_err()
        {
            pages_off_template = pages_off_template.saturating_add(1);
        }
    }
    TemplateAssessment {
        pages_off_template,
        template_invalid,
    }
}

/// First scalar `field` in leading frontmatter, without list items.
pub(crate) fn frontmatter_scalar(text: &str, field: &str) -> Option<String> {
    let rest = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))?;
    let end = rest.find("\n---")?;
    for line in rest[..end].lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('-') {
            continue;
        }
        let Some((key, value)) = trimmed.split_once(':') else {
            continue;
        };
        if key.trim() != field {
            continue;
        }
        let value = value.trim();
        let value = match uncomment("", field, value) {
            Ok(value) => value.trim(),
            Err(_) => value,
        };
        let value = match scalar("", field, value) {
            Ok(Value::String(value)) => value,
            Ok(value) => value.to_string(),
            Err(_) => unquote_scalar(value),
        };
        if value.is_empty() {
            return None;
        }
        return Some(value);
    }
    None
}

fn unquote_scalar(value: &str) -> String {
    value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
        .unwrap_or(value)
        .to_string()
}
