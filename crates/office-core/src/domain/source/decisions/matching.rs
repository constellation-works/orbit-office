use super::Decision;
use super::decision::words;
use super::scope::is_slug;
use super::types::Options;

/// Parse `dec-<slug>-<n>` into its slug and number.
#[must_use]
pub fn parse_entry_id(id: &str) -> Option<(&str, usize)> {
    let rest = id.strip_prefix("dec-")?;
    let (slug, number) = rest.rsplit_once('-')?;
    if !is_slug(slug) || number.is_empty() || !number.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let number: usize = number.parse().ok()?;
    (number >= 1 && format!("dec-{slug}-{number}") == id).then_some((slug, number))
}

fn folded(text: &str) -> String {
    words(&text.to_lowercase())
}

/// Whether two chosen texts match after case and whitespace folding, or
/// one contains the other (the dedupe rule in decision-precedents.md).
#[must_use]
pub fn chosen_matches(left: &str, right: &str) -> bool {
    let left = folded(left);
    let right = folded(right);
    !left.is_empty() && (left == right || left.contains(&right) || right.contains(&left))
}

/// The one ingested entry `candidate` duplicates, if exactly one matches:
/// an entry carrying `orbit`, with the same `context.task`, a `decided_at`
/// within 30 minutes, and a `chosen` matching the candidate's chosen.
/// More than one candidate, or none, links nothing.
#[must_use]
pub fn same_as_match<'a>(
    candidate: &Decision,
    entries: impl IntoIterator<Item = &'a Decision>,
) -> Option<String> {
    let task = candidate.context.task.as_deref()?;
    let chosen = candidate.chosen.as_deref()?;
    let at = crate::domain::metrics::timestamp(&candidate.decided_at)?;
    let mut found: Option<&str> = None;
    for entry in entries {
        if entry.id == candidate.id || entry.orbit.is_none() {
            continue;
        }
        if entry.context.task.as_deref() != Some(task) {
            continue;
        }
        let Some(entry_at) = crate::domain::metrics::timestamp(&entry.decided_at) else {
            continue;
        };
        if (at - entry_at).abs() > 30 * 60 {
            continue;
        }
        let Some(entry_chosen) = entry.chosen.as_deref() else {
            continue;
        };
        // The candidate's chosen must match the ingested entry's chosen
        // answer: its `chosen`, or an option text that entry chose.
        let matches = chosen_matches(chosen, entry_chosen)
            || matches!(&entry.options, Some(Options::Listed(items))
                if items.iter().any(|item| chosen_matches(chosen, &item.text)
                    && chosen_matches(&item.text, entry_chosen)));
        if !matches {
            continue;
        }
        if found.is_some() {
            return None;
        }
        found = Some(entry.id.as_str());
    }
    found.map(str::to_owned)
}
