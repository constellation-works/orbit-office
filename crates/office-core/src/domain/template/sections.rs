//! Required-section detection in Markdown page bodies.

pub(super) fn populated_sections(body: &str) -> Vec<String> {
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
