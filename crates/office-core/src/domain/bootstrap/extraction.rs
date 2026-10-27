//! Shared document window policy for extraction and its confirmed call budget.
use serde_json::{Value, json};

/// Maximum characters of annotated source text in a digest window. Prompts and
/// JSON Schema are additional; character splitting preserves UTF-8 and line ids.
pub const INPUT_CHARS: usize = 48_000;

/// Annotate absolute body line numbers, splitting even a single oversized line.
/// The same policy is used at preview and execution, before any paid call.
#[must_use]
pub fn windows(body: &str) -> Vec<String> {
    let mut windows = Vec::new();
    let mut text = String::new();
    let mut size = 0;
    for (index, line) in body.lines().enumerate() {
        let label = format!("#L{} ", index + 1);
        let mut chars = line.chars().peekable();
        loop {
            if size + label.len() + 1 >= INPUT_CHARS {
                windows.push(std::mem::take(&mut text));
                size = 0;
            }
            text.push_str(&label);
            size += label.len() + 1;
            for ch in chars.by_ref().take(INPUT_CHARS - size) {
                text.push(ch);
                size += 1;
            }
            text.push('\n');
            if chars.peek().is_none() {
                break;
            }
        }
    }
    if !text.is_empty() || windows.is_empty() {
        windows.push(text);
    }
    windows
}

/// Window calls and exactly one merge for multiple windows.
#[must_use]
pub fn calls(windows: usize) -> usize {
    windows + usize::from(windows > 1)
}

fn object(properties: Value, required: &[&str]) -> Value {
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}
fn array(items: Value) -> Value {
    json!({"type":"array","items":items})
}

/// Shared digest JSON Schema. Citations may be absent/empty in model output:
/// the caller drops and counts such items before publishing the digest.
#[must_use]
pub fn schema() -> Value {
    let text = json!({"type":"string"});
    let name = json!({"type":"string","minLength":1});
    let cites = array(text.clone());
    let step = object(json!({"text":text,"cites":cites}), &["text"]);
    object(
        json!({
            "summary":text,"kind_hint":text,"as_of":{"type":["string","null"]},
            "subjects":array(object(json!({"name":name,"template":name,"aliases":array(text.clone()),"role":text}), &["name","template"])),
            "claims":array(object(json!({"subject":name,"text":text,"cites":cites,"volatile":{"type":"boolean"}}), &["subject","text","volatile"])),
            "procedures":array(object(json!({"title":name,"inputs":array(text.clone()),"steps":array(step),"done_when":text}), &["title","inputs","steps","done_when"])),
            "terms":array(object(json!({"term":name,"definition":text,"cites":cites}), &["term","definition"])),
            "refs":array(object(json!({"label":text,"url":text}), &["label","url"]))
        }),
        &[
            "summary",
            "kind_hint",
            "as_of",
            "subjects",
            "claims",
            "procedures",
            "terms",
            "refs",
        ],
    )
}
