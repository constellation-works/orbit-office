//! Citation and anchor parsing.

use super::parse::is_slug;
use super::types::Citation;
use crate::domain::knowledge::DocumentKind;
use crate::domain::source::parse_clock_seconds;

/// One knowledge page file, as listed from the Library.
#[derive(Clone, Copy, Debug)]
pub struct PageFile<'a> {
    /// Library-relative path.
    pub path: &'a str,
    /// Full Markdown, including frontmatter.
    pub markdown: &'a str,
}

/// The knowledge layer an id's prefix names, if it is a page id.
#[must_use]
pub fn page_kind_for_id(id: &str) -> Option<DocumentKind> {
    if id.strip_prefix("dom-").is_some_and(is_slug) {
        Some(DocumentKind::Domain)
    } else if id.strip_prefix("rb-").is_some_and(is_slug) {
        Some(DocumentKind::Runbook)
    } else {
        None
    }
}

/// Parse `<id>` or `<id>#<anchor>` where the id is `src-`, `dom-`, `rb-`,
/// `art-`, `SCR-` (a conversation record), `dec-` (a decision ledger entry)
/// or `decisions-` (a ledger)
/// followed by a lowercase kebab-case slug.
#[must_use]
pub fn parse_citation(text: &str) -> Option<Citation> {
    let (id, anchor) = match text.split_once('#') {
        Some((id, anchor)) => (id, Some(anchor)),
        None => (text, None),
    };
    if anchor.is_some_and(|a| a.is_empty() || a.contains(char::is_whitespace)) {
        return None;
    }
    let local = if let Some((member, local)) = id.split_once('/') {
        if member.is_empty() || member.len() > 32 || !is_slug(member) {
            return None;
        }
        local
    } else {
        id
    };
    let (prefix, slug) = local.split_once('-')?;
    let kind = match prefix {
        "src" | "SCR" | "dec" | "decisions" => DocumentKind::Source,
        "dom" => DocumentKind::Domain,
        "rb" => DocumentKind::Runbook,
        "art" => DocumentKind::Artifact,
        _ => return None,
    };
    is_slug(slug).then(|| Citation {
        text: text.into(),
        id: id.into(),
        kind,
        anchor: anchor.map(str::to_owned),
    })
}

/// Check that `anchor` names a place in a source body (specs/library.md,
/// Sources): `t=hh:mm:ss` within the speaker turns (from the first turn's
/// timestamp to the last's), `L<n>` or `L<n>-<m>` within the body's lines,
/// and `slide=<n>` or `page=<n>` naming a `Slide <n>` or `Page <n>` heading.
/// Frame and chapter anchors live beside the source and are refused until
/// those files are readable here. Returns the refusal reason.
pub fn resolve_anchor(body: &str, anchor: &str) -> std::result::Result<(), &'static str> {
    if let Some(number) = anchor.strip_prefix('T') {
        if number.is_empty()
            || number.starts_with('0')
            || !number.bytes().all(|b| b.is_ascii_digit())
        {
            return Err("expected T<n> with a positive entry number");
        }
        let mut tags = false;
        let mut fence = None;
        for line in body.lines() {
            let trimmed = line.trim_start();
            if let Some(marker) = trimmed.chars().next()
                && matches!(marker, '`' | '~')
                && trimmed.chars().take_while(|c| *c == marker).count() >= 3
            {
                if fence == Some(marker) {
                    fence = None;
                } else if fence.is_none() {
                    fence = Some(marker);
                }
                continue;
            }
            if fence.is_some() {
                continue;
            }
            if line.starts_with("## ") {
                tags = line.trim() == "## Tags";
            }
            if tags
                && line
                    .strip_prefix("- ")
                    .is_some_and(|entry| entry.split_whitespace().next() == Some(anchor))
            {
                return Ok(());
            }
        }
        return Err("no such tag entry in the SCR");
    }
    if let Some(time) = anchor.strip_prefix("t=") {
        let at = parse_clock_seconds(time).ok_or("expected t=hh:mm:ss within u64 seconds")?;
        let turns: Vec<u64> = body
            .lines()
            .filter_map(|line| {
                let rest = line.trim_start().strip_prefix('[')?;
                let (stamp, _) = rest.split_once(']')?;
                parse_clock_seconds(stamp)
            })
            .collect();
        let (Some(first), Some(last)) = (turns.iter().min(), turns.iter().max()) else {
            return Err("the source has no timestamped turns");
        };
        return if (*first..=*last).contains(&at) {
            Ok(())
        } else {
            Err("the time is outside the source's turns")
        };
    }
    if let Some(range) = anchor.strip_prefix('L') {
        let (start, end) = match range.split_once('-') {
            Some((start, end)) => (start, end),
            None => (range, range),
        };
        let (Ok(start), Ok(end)) = (start.parse::<usize>(), end.parse::<usize>()) else {
            return Err("expected L<n> or L<n>-<m>");
        };
        let lines = body.lines().count();
        return if start >= 1 && start <= end && end <= lines {
            Ok(())
        } else {
            Err("the line range is outside the source")
        };
    }
    if anchor.starts_with("slide=") || anchor.starts_with("page=") {
        let parsed = crate::domain::source::parse_anchor(anchor)
            .map_err(|_| "expected a positive slide or page number")?;
        return crate::domain::source::anchor_span(body, parsed, anchor)
            .map(|_| ())
            .map_err(|_| "no such slide or page heading in the source");
    }
    if anchor.starts_with("frame=") || anchor.starts_with("chapter=") {
        return Err("frame and chapter anchors are not resolvable yet");
    }
    Err("unknown anchor form")
}

/// `[id#anchor]` citations in a body: bracketed text whose every `;`- or
/// `,`-separated item parses as a citation.
pub(crate) fn inline_citations(body: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = body;
    while let Some(open) = rest.find('[') {
        let after = &rest[open + 1..];
        let Some(close) = after.find([']', '[', '\n']) else {
            break;
        };
        if after.as_bytes()[close] == b']' {
            let inner = &after[..close];
            let items: Vec<&str> = inner
                .split([';', ','])
                .map(|item| item.trim().trim_matches(['"', '\'']))
                .collect();
            if !items.is_empty() && items.iter().all(|item| parse_citation(item).is_some()) {
                found.extend(items.into_iter().map(str::to_owned));
            }
        }
        rest = &after[close..];
        if rest.starts_with(']') {
            rest = &rest[1..];
        }
    }
    found
}
