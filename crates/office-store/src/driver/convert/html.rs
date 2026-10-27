//! HTML5 tokenization without executing scripts, resolving links or building a DOM.

use std::cell::RefCell;

use html5ever::tokenizer::{
    BufferQueue, Tag, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts,
    states::RawKind,
};

use crate::{Error, Result};

#[derive(Default)]
struct Markdown {
    text: String,
    skipped: Option<(String, usize)>,
    lists: Vec<(bool, usize)>,
    cells: usize,
    first_row: bool,
    in_cell: bool,
    limited: bool,
}

impl Markdown {
    fn newline(&mut self, count: usize) {
        while self.text.ends_with(' ') {
            self.text.pop();
        }
        if !self.text.is_empty() {
            while !self.text.ends_with(&"\n".repeat(count)) {
                self.text.push('\n');
            }
        }
    }

    fn characters(&mut self, text: &str) {
        for ch in text.chars() {
            if ch.is_whitespace() {
                if !self.text.is_empty() && !self.text.ends_with([' ', '\n']) {
                    self.text.push(' ');
                }
            } else if ch == '|' && self.in_cell {
                self.text.push_str("\\|");
            } else {
                self.text.push(ch);
            }
        }
    }

    fn tag(&mut self, tag: &Tag) {
        let name = tag.name.as_ref();
        let start = tag.kind == TagKind::StartTag;
        if let Some((skipped, depth)) = &mut self.skipped {
            if name == skipped {
                if start {
                    *depth += 1;
                } else {
                    *depth -= 1;
                }
                if *depth == 0 {
                    self.skipped = None;
                }
            }
            return;
        }
        if start && drop_element(tag) {
            self.skipped = Some((name.to_string(), 1));
            return;
        }
        match name {
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                self.newline(2);
                if start {
                    let level = usize::from(name.as_bytes()[1] - b'0');
                    self.text.push_str(&"#".repeat(level));
                    self.text.push(' ');
                }
            }
            "p" | "div" | "section" | "article" | "blockquote" | "pre" => {
                self.newline(if self.in_cell { 1 } else { 2 })
            }
            "br" => self.newline(1),
            "ul" | "ol" => {
                self.newline(1);
                if start {
                    if self.lists.len() >= 128 {
                        self.limited = true;
                    } else {
                        self.lists.push((name == "ol", 0));
                    }
                } else {
                    self.lists.pop();
                }
            }
            "li" => {
                self.newline(1);
                if start {
                    self.text
                        .push_str(&"  ".repeat(self.lists.len().saturating_sub(1)));
                    match self.lists.last_mut() {
                        Some((true, count)) => {
                            *count += 1;
                            self.text.push_str(&format!("{count}. "));
                        }
                        Some((false, _)) | None => self.text.push_str("- "),
                    }
                }
            }
            "table" => {
                self.newline(2);
                if start {
                    self.first_row = true;
                }
            }
            "tr" => {
                self.newline(1);
                if start {
                    self.cells = 0;
                    self.text.push('|');
                } else if self.first_row && self.cells > 0 {
                    self.text
                        .push_str(&format!("\n|{}", " --- |".repeat(self.cells)));
                    self.newline(1);
                    self.first_row = false;
                }
            }
            "th" | "td" => {
                self.in_cell = start;
                if start {
                    self.cells += 1;
                    self.text.push(' ');
                } else {
                    while self.text.ends_with(' ') {
                        self.text.pop();
                    }
                    self.text.push_str(" |");
                }
            }
            _ => {}
        }
    }
}

fn drop_element(tag: &Tag) -> bool {
    if matches!(
        tag.name.as_ref(),
        "head" | "script" | "style" | "nav" | "noscript" | "template"
    ) {
        return true;
    }
    tag.attrs.iter().any(|attr| match attr.name.local.as_ref() {
        "role" => attr.value.eq_ignore_ascii_case("navigation"),
        "id" | "class" => attr.value.split_whitespace().any(|name| {
            matches!(
                name.to_ascii_lowercase().as_str(),
                "navigation"
                    | "nav"
                    | "sidebar"
                    | "breadcrumbs"
                    | "page-metadata"
                    | "confluence-navigation"
            )
        }),
        _ => false,
    })
}

#[derive(Default)]
struct Sink(RefCell<Markdown>);

impl TokenSink for Sink {
    type Handle = ();

    fn process_token(&self, token: Token, _line: u64) -> TokenSinkResult<()> {
        let mut markdown = self.0.borrow_mut();
        if markdown.limited {
            return TokenSinkResult::Continue;
        }
        let mut result = TokenSinkResult::Continue;
        match token {
            Token::TagToken(tag) => {
                markdown.tag(&tag);
                if tag.kind == TagKind::StartTag {
                    result = match tag.name.as_ref() {
                        "script" => TokenSinkResult::RawData(RawKind::ScriptData),
                        "style" => TokenSinkResult::RawData(RawKind::Rawtext),
                        _ => TokenSinkResult::Continue,
                    };
                }
            }
            Token::CharacterTokens(text) => {
                if markdown.skipped.is_none() {
                    markdown.characters(&text);
                }
            }
            Token::NullCharacterToken
            | Token::CommentToken(_)
            | Token::DoctypeToken(_)
            | Token::EOFToken
            | Token::ParseError(_) => {}
        }
        markdown.limited |= markdown.text.len() > super::TEXT_LIMIT;
        result
    }
}

pub(super) fn convert(text: &str) -> Result<String> {
    let input = BufferQueue::default();
    input.push_back(text.into());
    let tokenizer = Tokenizer::new(Sink::default(), TokenizerOpts::default());
    let _ = tokenizer.feed(&input);
    tokenizer.end();
    let markdown = tokenizer.sink.0.into_inner();
    if markdown.limited {
        return Err(Error::ConversionLimit {
            limit: "8 MiB HTML text or 128 nested lists",
        });
    }
    Ok(markdown.text.trim().to_string())
}
