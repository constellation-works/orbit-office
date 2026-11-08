//! Credential scrubbing and harness tag stripping.

/// Remove high-confidence credential shapes and secret assignment values.
/// Operates before previews as well as before persistence; does not read env.
pub fn scrub(text: &str) -> String {
    let mut out = String::new();
    let mut pem = false;
    let mut next_secret = false;
    for part in text.split_inclusive(char::is_whitespace) {
        let word = part.trim_end_matches(char::is_whitespace);
        let space = &part[word.len()..];
        if word.contains("-----BEGIN") {
            pem = true;
        }
        let lower = word.to_ascii_lowercase();
        let prefix = [
            "sk-",
            "ghp_",
            "gho_",
            "github_pat_",
            "xoxb-",
            "xoxp-",
            "AKIA",
            "eyJ",
        ]
        .iter()
        .any(|prefix| {
            word.match_indices(prefix).any(|(at, _)| {
                let boundary = word[..at]
                    .chars()
                    .next_back()
                    .is_none_or(|c| !c.is_ascii_alphanumeric() && c != '_' && c != '-');
                let credential = word[at + prefix.len()..]
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || "_-./+=".contains(*c))
                    .count();
                boundary && credential >= 16
            })
        });
        let assignment = word.split_once('=').or_else(|| word.split_once(':'));
        let secret_assignment = assignment.is_some_and(|(key, value)| {
            let key = key
                .trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                .to_ascii_lowercase();
            [
                "token",
                "password",
                "secret",
                "api_key",
                "apikey",
                "access_token",
            ]
            .contains(&key.as_str())
                && value.len() >= 8
        });
        let url_credentials = url::Url::parse(word).is_ok_and(|url| url.password().is_some());
        if pem || prefix || next_secret || secret_assignment || url_credentials {
            out.push_str("[REDACTED]");
        } else {
            out.push_str(word);
        }
        out.push_str(space);
        next_secret = lower.trim_matches(['"', '\'', ':']) == "bearer"
            || [
                "token=",
                "password=",
                "secret=",
                "api_key=",
                "token:",
                "password:",
                "secret:",
                "api_key:",
            ]
            .contains(&lower.as_str());
        if word.contains("-----END") {
            pem = false;
        }
    }
    out
}

/// The inner text of the first `<tag>…</tag>` block in `text`, if any.
pub(crate) fn tag_inner(text: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}");
    let start = text.find(&open)?;
    let after = &text[start + open.len()..];
    if !after.starts_with('>') && !after.starts_with(char::is_whitespace) {
        return None;
    }
    let content_start = start + text[start..].find('>')? + 1;
    let close = format!("</{tag}>");
    let end = text[content_start..].find(&close)? + content_start;
    Some(text[content_start..end].trim().to_owned())
}

/// Remove every `<tag>…</tag>` block (and any stray tag) from `text`.
/// An unclosed block removes the rest of the text: harness blocks are
/// always closed, so a missing close means truncated wrapper text.
pub(crate) fn strip_tag_blocks(text: &str, tag: &str) -> String {
    let mut out = text.to_owned();
    loop {
        let open = format!("<{tag}");
        let Some(start) = out.find(&open) else {
            break;
        };
        let after = &out[start + open.len()..];
        if !after.starts_with('>') && !after.starts_with(char::is_whitespace) {
            break;
        }
        let Some(gt) = out[start..].find('>') else {
            out.truncate(start);
            break;
        };
        let content_start = start + gt + 1;
        let close = format!("</{tag}>");
        match out[content_start..].find(&close) {
            Some(at) => out.replace_range(start..content_start + at + close.len(), ""),
            None => out.truncate(start),
        }
    }
    out.replace(&format!("</{tag}>"), "")
}

/// Remove every block whose tag starts with `prefix`, such as
/// `<local-command-stdout>` and `<local-command-stderr>`.
pub(crate) fn strip_prefixed_blocks(text: &str, prefix: &str) -> String {
    let mut out = text.to_owned();
    loop {
        let open = format!("<{prefix}");
        let Some(start) = out.find(&open) else {
            break;
        };
        let Some(gt) = out[start..].find('>') else {
            out.truncate(start);
            break;
        };
        let tag: String = out[start + 1..start + gt]
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_owned();
        let content_start = start + gt + 1;
        let close = format!("</{tag}>");
        match out[content_start..].find(&close) {
            Some(at) => out.replace_range(start..content_start + at + close.len(), ""),
            None => out.truncate(start),
        }
    }
    out
}

/// Strip harness wrapper blocks from one user text, leaving only the
/// words the user typed. Slash-command tags are removed here too; the
/// caller renders the invocation from the extracted name and args.
pub(crate) fn strip_harness(text: &str) -> String {
    let out = strip_tag_blocks(text, "system-reminder");
    let out = strip_tag_blocks(&out, "task-notification");
    let out = strip_prefixed_blocks(&out, "local-command-");
    let out = strip_tag_blocks(&out, "command-message");
    let out = strip_tag_blocks(&out, "command-name");
    strip_tag_blocks(&out, "command-args")
}
