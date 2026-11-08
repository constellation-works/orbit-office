//! Contact detection and withholding.

/// Email addresses and phone numbers in `text`, as byte ranges and kinds.
pub(super) fn find_contacts(text: &str) -> Vec<(std::ops::Range<usize>, &'static str)> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    // Emails: local@domain.tld.
    for (at, _) in text.match_indices('@') {
        let local = |c: u8| c.is_ascii_alphanumeric() || b"._%+-".contains(&c);
        let mut start = at;
        while start > 0 && local(bytes[start - 1]) {
            start -= 1;
        }
        let mut end = at + 1;
        while end < bytes.len()
            && (bytes[end].is_ascii_alphanumeric() || b".-".contains(&bytes[end]))
        {
            end += 1;
        }
        while end > at + 1 && b".-".contains(&bytes[end - 1]) {
            end -= 1;
        }
        let domain = &text[at + 1..end];
        let tld_ok = domain.rsplit_once('.').is_some_and(|(host, tld)| {
            !host.is_empty() && tld.len() >= 2 && tld.bytes().all(|c| c.is_ascii_alphabetic())
        });
        if start < at && tld_ok {
            found.push((start..end, "email"));
        }
    }
    // Phone numbers: 10–15 digits in groups joined by single separators.
    let mut index = 0;
    while index < bytes.len() {
        let c = bytes[index];
        let boundary = index == 0 || {
            let before = bytes[index - 1];
            !(before.is_ascii_alphanumeric() || b"-_#=/:.@".contains(&before))
        };
        if !(boundary && (c.is_ascii_digit() || c == b'+' || c == b'(')) {
            index += 1;
            continue;
        }
        let start = index;
        let mut end = index;
        let mut digits = 0;
        let mut last_digit_end = index;
        while end < bytes.len() {
            let b = bytes[end];
            let next = bytes.get(end + 1).copied();
            let joins = match b {
                b'0'..=b'9' => {
                    digits += 1;
                    last_digit_end = end + 1;
                    true
                }
                // A closing parenthesis may be followed by a separator.
                b')' => next.is_some_and(|n| n.is_ascii_digit() || b" -.".contains(&n)),
                // Every other separator is single and leads to a digit group.
                b'(' | b'+' | b'-' | b'.' | b' ' => {
                    next.is_some_and(|n| n.is_ascii_digit() || n == b'(')
                }
                _ => false,
            };
            if !joins {
                break;
            }
            end += 1;
        }
        let run = &text[start..last_digit_end];
        let after_ok = bytes
            .get(last_digit_end)
            .is_none_or(|b| !b.is_ascii_alphanumeric());
        if (10..=15).contains(&digits) && after_ok && !contains_iso_date(run) {
            found.push((start..last_digit_end, "phone"));
        }
        index = end.max(index + 1);
    }
    found.sort_by_key(|(range, _)| range.start);
    found
}

fn contains_iso_date(run: &str) -> bool {
    let bytes = run.as_bytes();
    bytes.windows(10).any(|w| {
        w[..4].iter().all(u8::is_ascii_digit)
            && w[4] == b'-'
            && w[5..7].iter().all(u8::is_ascii_digit)
            && w[7] == b'-'
            && w[8..].iter().all(u8::is_ascii_digit)
    })
}

/// Remove contact values before unstructured evidence reaches a model.
#[must_use]
pub fn withhold_contacts(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut at = 0;
    for (range, _) in find_contacts(body) {
        if range.start < at {
            continue;
        }
        out.push_str(&body[at..range.start]);
        out.push_str("[contact withheld]");
        at = range.end;
    }
    out.push_str(&body[at..]);
    out
}
