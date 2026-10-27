//! Read only bounded, validated ZIP parts. No archive member reaches the filesystem.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read};

use quick_xml::{Reader, XmlVersion, events::Event};
use zip::ZipArchive;

use crate::{Error, Result};

struct Package<'a> {
    zip: ZipArchive<Cursor<&'a [u8]>>,
    format: &'static str,
}

impl<'a> Package<'a> {
    fn open(bytes: &'a [u8], format: &'static str) -> Result<Self> {
        let mut zip = ZipArchive::new(Cursor::new(bytes))
            .map_err(|_| super::invalid(format, "invalid ZIP container"))?;
        if zip.len() > 4096 {
            return Err(Error::ConversionLimit {
                limit: "4096 ZIP entries",
            });
        }
        let mut expanded = 0_u64;
        let mut names = BTreeSet::new();
        for index in 0..zip.len() {
            let entry = zip
                .by_index(index)
                .map_err(|_| super::invalid(format, "unreadable ZIP entry"))?;
            // zip 9 returns the decoded member name; a non-UTF-8 name is unsafe.
            let name = entry
                .name()
                .map_err(|_| super::invalid(format, "unreadable ZIP entry"))?;
            if name.starts_with('/')
                || name.contains(['\\', ':', '\0'])
                || name.split('/').any(|part| part == "..")
                || !names.insert(name.into_owned())
            {
                return Err(super::invalid(
                    format,
                    "unsafe or duplicate ZIP member path",
                ));
            }
            expanded = expanded.saturating_add(entry.size());
            if expanded > super::INPUT_LIMIT {
                return Err(Error::ConversionLimit {
                    limit: "32 MiB expanded ZIP data",
                });
            }
        }
        Ok(Self { zip, format })
    }

    fn has(&self, path: &str) -> bool {
        self.zip.index_for_name(path).is_some()
    }

    fn xml(&mut self, path: &str) -> Result<Node> {
        let entry = self
            .zip
            .by_name(path)
            .map_err(|_| super::invalid(self.format, "missing or unreadable XML part"))?;
        let mut bytes = Vec::new();
        entry
            .take(u64::try_from(super::TEXT_LIMIT).unwrap_or(u64::MAX) + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| super::invalid(self.format, "could not decompress XML part"))?;
        if bytes.len() > super::TEXT_LIMIT {
            return Err(Error::ConversionLimit {
                limit: "8 MiB XML part",
            });
        }
        parse(super::utf8(&bytes, self.format)?, self.format)
    }
}

#[derive(Default)]
struct Node {
    name: String,
    attributes: BTreeMap<String, String>,
    text: String,
    children: Vec<Node>,
}

impl Node {
    fn attribute(&self, key: &str) -> Option<&str> {
        self.attributes
            .get(key)
            .or_else(|| {
                self.attributes
                    .iter()
                    .find(|(name, _)| name.rsplit(':').next() == Some(key))
                    .map(|(_, value)| value)
            })
            .map(String::as_str)
    }

    fn elements<'a>(&'a self, name: &str, found: &mut Vec<&'a Self>) {
        if self.name == name {
            found.push(self);
        }
        for child in &self.children {
            child.elements(name, found);
        }
    }

    fn all(&self, name: &str) -> Vec<&Self> {
        let mut found = Vec::new();
        self.elements(name, &mut found);
        found
    }

    fn paragraph_text(&self, out: &mut String) {
        match self.name.as_str() {
            "t" => out.push_str(&self.text),
            "tab" => out.push('\t'),
            "br" | "cr" => out.push('\n'),
            _ => {
                for child in &self.children {
                    child.paragraph_text(out);
                }
            }
        }
    }
}

fn resolved_reference(
    reference: &quick_xml::events::BytesRef<'_>,
    format: &'static str,
) -> Result<String> {
    let invalid = || {
        super::invalid(
            format,
            "malformed XML part (DTDs and external entities are unsupported)",
        )
    };
    if let Some(character) = reference.resolve_char_ref().map_err(|_| invalid())? {
        return Ok(character.to_string());
    }
    let name = reference.decode().map_err(|_| invalid())?;
    quick_xml::escape::resolve_predefined_entity(name.as_ref())
        .map(str::to_owned)
        .ok_or_else(invalid)
}

fn push_xml_text(stack: &mut [Node], text: &str, format: &'static str) -> Result<()> {
    let invalid = || {
        super::invalid(
            format,
            "malformed XML part (DTDs and external entities are unsupported)",
        )
    };
    if stack.len() == 1 && !text.trim().is_empty() {
        return Err(invalid());
    }
    stack.last_mut().ok_or_else(invalid)?.text.push_str(text);
    Ok(())
}

fn parse(text: &str, format: &'static str) -> Result<Node> {
    let invalid = || {
        super::invalid(
            format,
            "malformed XML part (DTDs and external entities are unsupported)",
        )
    };
    let mut reader = Reader::from_str(text);
    reader.config_mut().expand_empty_elements = true;
    let mut stack = vec![Node::default()];
    let mut count = 0_usize;
    loop {
        match reader.read_event().map_err(|_| invalid())? {
            Event::Start(element) => {
                count += 1;
                if stack.len() > 128 || count > 100_000 {
                    return Err(Error::ConversionLimit {
                        limit: "128 XML levels or 100000 elements",
                    });
                }
                let mut attributes = BTreeMap::new();
                for attr in element.attributes() {
                    let attr = attr.map_err(|_| invalid())?;
                    let key = super::utf8(attr.key.as_ref(), format)?.to_string();
                    let value = attr
                        .decoded_and_normalized_value(XmlVersion::Implicit1_0, reader.decoder())
                        .map_err(|_| invalid())?
                        .into_owned();
                    attributes.insert(key, value);
                }
                stack.push(Node {
                    name: super::utf8(element.local_name().as_ref(), format)?.to_string(),
                    attributes,
                    ..Node::default()
                });
            }
            Event::End(_) => {
                if stack.len() <= 1 {
                    return Err(invalid());
                }
                let node = stack.pop().ok_or_else(invalid)?;
                stack.last_mut().ok_or_else(invalid)?.children.push(node);
            }
            Event::Text(text) => {
                // Entities are separate `GeneralRef` events in quick-xml 0.41.
                let text = text.xml10_content().map_err(|_| invalid())?;
                push_xml_text(&mut stack, &text, format)?;
            }
            Event::GeneralRef(reference) => {
                let text = resolved_reference(&reference, format)?;
                push_xml_text(&mut stack, &text, format)?;
            }
            Event::CData(text) => {
                stack
                    .last_mut()
                    .ok_or_else(invalid)?
                    .text
                    .push_str(&text.decode().map_err(|_| invalid())?);
            }
            Event::Decl(decl) => {
                if let Some(encoding) = decl.encoding() {
                    let encoding = encoding.map_err(|_| invalid())?;
                    if !encoding.eq_ignore_ascii_case(b"UTF-8")
                        && !encoding.eq_ignore_ascii_case(b"US-ASCII")
                    {
                        return Err(invalid());
                    }
                }
            }
            Event::DocType(_) => return Err(invalid()),
            Event::Eof => break,
            Event::PI(_) | Event::Comment(_) => {}
            Event::Empty(_) => return Err(invalid()), // expanded by the reader
        }
    }
    if stack.len() != 1 {
        return Err(invalid());
    }
    let mut root = stack.pop().ok_or_else(invalid)?;
    if root.children.len() != 1 {
        return Err(invalid());
    }
    root.children.pop().ok_or_else(invalid)
}

pub(super) fn docx(bytes: &[u8]) -> Result<String> {
    let mut package = Package::open(bytes, "docx")?;
    let document = package.xml("word/document.xml")?;
    if document.name != "document" || document.all("body").len() != 1 {
        return Err(super::invalid("docx", "missing document body"));
    }
    let mut body = String::new();
    for paragraph in document.all("p") {
        let mut text = String::new();
        paragraph.paragraph_text(&mut text);
        if text.trim().is_empty() {
            continue;
        }
        let level = paragraph
            .all("pStyle")
            .first()
            .and_then(|style| style.attribute("val"))
            .and_then(|style| {
                style
                    .to_ascii_lowercase()
                    .strip_prefix("heading")
                    .and_then(|n| n.parse::<usize>().ok())
            });
        if let Some(level) = level.filter(|level| (1..=6).contains(level)) {
            body.push_str(&format!("{} ", "#".repeat(level)));
        } else if !paragraph.all("numPr").is_empty() {
            let indent = paragraph
                .all("ilvl")
                .first()
                .and_then(|node| node.attribute("val"))
                .and_then(|n| n.parse::<usize>().ok())
                .unwrap_or(0)
                .min(8);
            body.push_str(&format!("{}- ", "  ".repeat(indent)));
        }
        body.push_str(text.trim());
        body.push_str("\n\n");
        if body.len() > super::TEXT_LIMIT {
            return Err(Error::ConversionLimit {
                limit: "8 MiB docx text",
            });
        }
    }
    Ok(body.trim().to_string())
}

struct Relationship {
    id: String,
    target: String,
}

fn relationships(package: &mut Package<'_>, owner: &str, kind: &str) -> Result<Vec<Relationship>> {
    let (parent, name) = owner
        .rsplit_once('/')
        .ok_or_else(|| super::invalid("pptx", "invalid part name"))?;
    let path = format!("{parent}/_rels/{name}.rels");
    if !package.has(&path) {
        return Ok(Vec::new());
    }
    let root = package.xml(&path)?;
    if root.name != "Relationships" {
        return Err(super::invalid("pptx", "invalid relationship part"));
    }
    let mut found = Vec::new();
    for relation in root.all("Relationship") {
        if !relation
            .attribute("Type")
            .is_some_and(|value| value.ends_with(&format!("/{kind}")))
        {
            continue;
        }
        if relation
            .attribute("TargetMode")
            .is_some_and(|value| value.eq_ignore_ascii_case("External"))
        {
            return Err(super::invalid(
                "pptx",
                "external slide or notes relationship",
            ));
        }
        let id = relation
            .attribute("Id")
            .ok_or_else(|| super::invalid("pptx", "missing relationship id"))?;
        let target = relation
            .attribute("Target")
            .ok_or_else(|| super::invalid("pptx", "missing relationship target"))?;
        if target.contains(['\\', ':', '\0']) {
            return Err(super::invalid("pptx", "unsafe relationship target"));
        }
        let joined = if target.starts_with('/') {
            target.trim_start_matches('/').to_string()
        } else {
            format!("{parent}/{target}")
        };
        let mut parts = Vec::new();
        for part in joined.split('/') {
            match part {
                "" | "." => {}
                ".." => {
                    if parts.pop().is_none() {
                        return Err(super::invalid("pptx", "relationship escapes package"));
                    }
                }
                _ => parts.push(part),
            }
        }
        if found.iter().any(|item: &Relationship| item.id == id) {
            return Err(super::invalid("pptx", "duplicate relationship id"));
        }
        found.push(Relationship {
            id: id.to_string(),
            target: parts.join("/"),
        });
    }
    Ok(found)
}

fn slide_text(root: &Node, title: bool) -> String {
    let mut body = String::new();
    for (index, paragraph) in root.all("p").into_iter().enumerate() {
        let mut text = String::new();
        paragraph.paragraph_text(&mut text);
        if text.trim().is_empty() {
            continue;
        }
        if index == 0 && title {
            body.push_str("## ");
        } else if !paragraph.all("buChar").is_empty()
            || !paragraph.all("buAutoNum").is_empty()
            || paragraph
                .all("pPr")
                .iter()
                .any(|node| node.attribute("lvl").is_some())
        {
            body.push_str("- ");
        }
        body.push_str(text.trim());
        body.push_str("\n\n");
    }
    body
}

pub(super) fn pptx(bytes: &[u8]) -> Result<String> {
    let mut package = Package::open(bytes, "pptx")?;
    let presentation = package.xml("ppt/presentation.xml")?;
    if presentation.name != "presentation" {
        return Err(super::invalid("pptx", "missing presentation"));
    }
    let slides = relationships(&mut package, "ppt/presentation.xml", "slide")?;
    let order = presentation.all("sldId");
    if order.is_empty() {
        return Err(super::invalid("pptx", "presentation has no slides"));
    }
    let mut body = String::new();
    let mut has_text = false;
    for (index, slide) in order.into_iter().enumerate() {
        // r:id is distinct from the unqualified numeric slide id. Namespace
        // prefixes may vary in a conforming Office package.
        let id = slide
            .attributes
            .iter()
            .find(|(key, _)| key.contains(':') && key.rsplit(':').next() == Some("id"))
            .map(|(_, value)| value);
        let relation = slides
            .iter()
            .find(|relation| Some(&relation.id) == id)
            .ok_or_else(|| super::invalid("pptx", "missing slide relationship"))?;
        let root = package.xml(&relation.target)?;
        if root.name != "sld" {
            return Err(super::invalid("pptx", "invalid slide part"));
        }
        body.push_str(&format!("# Slide {}\n\n", index + 1));
        let text = slide_text(&root, true);
        has_text |= !text.trim().is_empty();
        body.push_str(&text);
        for notes in relationships(&mut package, &relation.target, "notesSlide")? {
            let root = package.xml(&notes.target)?;
            if root.name != "notes" {
                return Err(super::invalid("pptx", "invalid notes part"));
            }
            let notes = slide_text(&root, false);
            if !notes.trim().is_empty() {
                has_text = true;
                body.push_str("## Speaker notes\n\n");
                body.push_str(&notes);
            }
        }
        if body.len() > super::TEXT_LIMIT {
            return Err(Error::ConversionLimit {
                limit: "8 MiB pptx text",
            });
        }
    }
    if !has_text {
        body.push_str("> needs_ocr: no text was found during document conversion.");
    }
    Ok(body.trim().to_string())
}
