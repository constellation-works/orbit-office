use super::{Attendee, CalendarError, Event, Organizer, Window, plain, stamp, time};
use chrono::{DateTime, Datelike, Duration, FixedOffset, NaiveDate};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone)]
struct Property {
    name: String,
    params: BTreeMap<String, String>,
    value: String,
}
impl Property {
    fn tz(&self) -> &str {
        if self.value.ends_with('Z')
            || self.params.get("VALUE").is_some_and(|s| s == "DATE")
            || self.value.len() == 8
        {
            "UTC"
        } else {
            self.params.get("TZID").map(String::as_str).unwrap_or("")
        }
    }
    fn date(&self) -> Result<(DateTime<FixedOffset>, bool), CalendarError> {
        time::local(&self.value, self.tz())
    }
}
#[derive(Clone)]
struct Raw {
    props: Vec<Property>,
}
impl Raw {
    fn get(&self, name: &str) -> Option<&Property> {
        self.props.iter().find(|p| p.name == name)
    }
    fn text(&self, name: &str) -> String {
        self.get(name)
            .map(|p| unescape(&p.value))
            .unwrap_or_default()
    }
    fn start(&self) -> Result<&Property, CalendarError> {
        self.get("DTSTART")
            .or_else(|| self.get("RECURRENCE-ID"))
            .ok_or(CalendarError::Invalid)
    }
    fn duration(&self, base: Option<Duration>) -> Result<Duration, CalendarError> {
        let (start, all_day) = self.start()?.date()?;
        let duration = if let Some(end) = self.get("DTEND") {
            end.date()?.0 - start
        } else if let Some(p) = self.get("DURATION") {
            duration(&p.value)?
        } else {
            base.unwrap_or(if all_day {
                Duration::days(1)
            } else {
                Duration::zero()
            })
        };
        if duration < Duration::zero() {
            return Err(CalendarError::Invalid);
        }
        Ok(duration)
    }
    fn event(
        &self,
        start: DateTime<FixedOffset>,
        original: DateTime<FixedOffset>,
        duration: Duration,
        unsupported: bool,
    ) -> Result<Event, CalendarError> {
        let uid = self.text("UID");
        if uid.is_empty() || uid.len() > 1024 {
            return Err(CalendarError::Invalid);
        }
        let (_, all_day) = self.start()?.date()?;
        let attendees = self
            .props
            .iter()
            .filter(|p| p.name == "ATTENDEE")
            .map(|p| Attendee {
                name: plain(p.params.get("CN").map(String::as_str).unwrap_or("")),
                email: p
                    .value
                    .get(..7)
                    .filter(|s| s.eq_ignore_ascii_case("mailto:"))
                    .map(|_| p.value[7..].trim().to_ascii_lowercase()),
                response: p
                    .params
                    .get("PARTSTAT")
                    .map(|s| s.to_ascii_lowercase())
                    .unwrap_or_else(|| "needs-action".into()),
            })
            .collect();
        let agenda = self.text("DESCRIPTION");
        let location = self.text("LOCATION");
        let online = agenda.contains("://")
            || location.contains("://")
            || self.get("URL").is_some()
            || self.props.iter().any(|p| {
                p.name.starts_with("X-MICROSOFT-SKYPETEAMSMEETINGURL") || p.name == "CONFERENCE"
            });
        let status = match self.text("STATUS").as_str() {
            "CANCELLED" => "cancelled",
            "TENTATIVE" => "tentative",
            _ => "confirmed",
        };
        let end = start
            .checked_add_signed(duration)
            .ok_or(CalendarError::Invalid)?;
        Ok(Event {
            local: false,
            key: format!("{}:{}", uid, stamp(original)),
            series: uid,
            title: plain(&self.text("SUMMARY")),
            start: start.to_rfc3339(),
            end: end.to_rfc3339(),
            all_day,
            status: status.into(),
            location: plain(&location),
            online,
            organizer: Organizer {
                name: plain(
                    self.get("ORGANIZER")
                        .and_then(|p| p.params.get("CN"))
                        .map(String::as_str)
                        .unwrap_or(""),
                ),
            },
            attendees,
            agenda: plain(&agenda),
            sensitivity: if ["PRIVATE", "CONFIDENTIAL"].contains(&self.text("CLASS").as_str()) {
                "private"
            } else {
                "normal"
            }
            .into(),
            recurrence_unsupported: unsupported,
        })
    }
}
fn duration(text: &str) -> Result<Duration, CalendarError> {
    let mut seconds = 0i64;
    let mut digits = String::new();
    let mut in_time = false;
    let text = text.strip_prefix('P').ok_or(CalendarError::Invalid)?;
    for c in text.chars() {
        if c == 'T' {
            in_time = true;
            continue;
        }
        if c.is_ascii_digit() {
            digits.push(c);
            continue;
        }
        let n = digits.parse::<i64>().map_err(|_| CalendarError::Invalid)?;
        digits.clear();
        let unit = match c {
            'W' if !in_time => 604800,
            'D' if !in_time => 86400,
            'H' if in_time => 3600,
            'M' if in_time => 60,
            'S' if in_time => 1,
            _ => return Err(CalendarError::Invalid),
        };
        seconds = seconds
            .checked_add(n.checked_mul(unit).ok_or(CalendarError::Invalid)?)
            .ok_or(CalendarError::Invalid)?;
    }
    if !digits.is_empty() || seconds <= 0 {
        return Err(CalendarError::Invalid);
    }
    Duration::try_seconds(seconds).ok_or(CalendarError::Invalid)
}
fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n' | 'N') => out.push('\n'),
                Some(c) => out.push(c),
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}
fn split(s: &str, delimiter: char) -> Vec<&str> {
    let mut quote = false;
    let mut start = 0;
    let mut out = Vec::new();
    for (i, c) in s.char_indices() {
        if c == '"' {
            quote = !quote;
        }
        if c == delimiter && !quote {
            out.push(&s[start..i]);
            start = i + c.len_utf8();
        }
    }
    out.push(&s[start..]);
    out
}
fn records(text: &str) -> Result<Vec<Raw>, CalendarError> {
    if text.len() > 1_048_576 {
        return Err(CalendarError::TooLarge);
    }
    let mut lines: Vec<String> = Vec::new();
    for line in text.lines() {
        if line.starts_with([' ', '\t']) {
            lines
                .last_mut()
                .ok_or(CalendarError::Invalid)?
                .push_str(&line[1..]);
        } else if !line.is_empty() {
            lines.push(line.into());
        }
    }
    if lines.first().map(String::as_str) != Some("BEGIN:VCALENDAR")
        || lines.last().map(String::as_str) != Some("END:VCALENDAR")
    {
        return Err(CalendarError::Invalid);
    }
    let mut stack: Vec<String> = Vec::new();
    let mut events = Vec::new();
    let mut props = Vec::new();
    for line in lines {
        if let Some(name) = line.strip_prefix("BEGIN:") {
            if stack.len() >= 32 || (name == "VCALENDAR" && !stack.is_empty()) {
                return Err(CalendarError::Invalid);
            }
            if name == "VEVENT" {
                if stack.last().map(String::as_str) != Some("VCALENDAR") {
                    return Err(CalendarError::Invalid);
                }
                props.clear();
            }
            stack.push(name.into());
            continue;
        }
        if let Some(name) = line.strip_prefix("END:") {
            if stack.pop().as_deref() != Some(name) {
                return Err(CalendarError::Invalid);
            }
            if name == "VEVENT" {
                events.push(Raw {
                    props: std::mem::take(&mut props),
                });
            }
            continue;
        }
        let parts = split(&line, ':');
        if parts.len() < 2 {
            return Err(CalendarError::Invalid);
        }
        if stack.last().map(String::as_str) != Some("VEVENT") {
            continue;
        }
        let left = parts[0];
        let value = &line[left.len() + 1..];
        let mut keys = split(left, ';').into_iter();
        let name = keys
            .next()
            .ok_or(CalendarError::Invalid)?
            .to_ascii_uppercase();
        let mut params = BTreeMap::new();
        for key in keys {
            let (k, v) = key.split_once('=').ok_or(CalendarError::Invalid)?;
            params.insert(k.to_ascii_uppercase(), v.trim_matches('"').into());
        }
        if !["ATTENDEE", "EXDATE", "RDATE"].contains(&name.as_str())
            && props.iter().any(|p: &Property| p.name == name)
        {
            return Err(CalendarError::Invalid);
        }
        if props.len() >= 1024 {
            return Err(CalendarError::TooLarge);
        }
        props.push(Property {
            name,
            params,
            value: value.into(),
        });
    }
    if !stack.is_empty() || events.len() > 10000 {
        return Err(CalendarError::Invalid);
    }
    Ok(events)
}
struct Rule {
    freq: String,
    interval: i64,
    count: Option<u32>,
    until: Option<DateTime<FixedOffset>>,
    days: Vec<(i32, u32)>,
    monthdays: Vec<i32>,
}
impl Rule {
    fn parse(text: &str, start: &Property) -> Option<Self> {
        let mut map = BTreeMap::new();
        for field in text.split(';') {
            let (k, v) = field.split_once('=')?;
            if !["FREQ", "INTERVAL", "COUNT", "UNTIL", "BYDAY", "BYMONTHDAY"].contains(&k)
                || map.insert(k, v).is_some()
            {
                return None;
            }
        }
        let freq = *map.get("FREQ")?;
        if !["DAILY", "WEEKLY", "MONTHLY"].contains(&freq) {
            return None;
        }
        let interval = map
            .get("INTERVAL")
            .map(|v| v.parse().ok())
            .unwrap_or(Some(1))?;
        if !(1..=10000).contains(&interval) {
            return None;
        }
        let count = map
            .get("COUNT")
            .map(|s| s.parse::<u32>().ok().filter(|n| *n > 0))
            .transpose_option()?;
        let until = map
            .get("UNTIL")
            .map(|v| time::local(v, start.tz()).ok().map(|t| t.0))
            .transpose_option()?;
        if count.is_some() && until.is_some() {
            return None;
        }
        let mut days = Vec::new();
        if let Some(byday) = map.get("BYDAY") {
            for d in byday.split(',') {
                let n = d.len().checked_sub(2)?;
                let ord = d.get(..n)?;
                let day = d.get(n..)?;
                let weekday = ["MO", "TU", "WE", "TH", "FR", "SA", "SU"]
                    .iter()
                    .position(|s| *s == day)
                    .and_then(|n| u32::try_from(n).ok())?;
                let ordinal = if ord.is_empty() {
                    0
                } else {
                    ord.parse::<i32>()
                        .ok()
                        .filter(|n| *n != 0 && (-5..=5).contains(n))?
                };
                if ordinal != 0 && freq != "MONTHLY" {
                    return None;
                }
                days.push((ordinal, weekday));
            }
        }
        let mut monthdays = Vec::new();
        if let Some(by) = map.get("BYMONTHDAY") {
            if freq == "WEEKLY" {
                return None;
            }
            for d in by.split(',') {
                monthdays.push(
                    d.parse::<i32>()
                        .ok()
                        .filter(|n| *n != 0 && (-31..=31).contains(n))?,
                );
            }
        }
        Some(Self {
            freq: freq.into(),
            interval,
            count,
            until,
            days,
            monthdays,
        })
    }
    fn matches(&self, date: NaiveDate, start: NaiveDate) -> bool {
        let elapsed = (date - start).num_days();
        let weekday = date.weekday().num_days_from_monday();
        let months = i64::from(date.year() - start.year()) * 12 + i64::from(date.month())
            - i64::from(start.month());
        let period = match self.freq.as_str() {
            "DAILY" => elapsed % self.interval == 0,
            "WEEKLY" => {
                (elapsed + i64::from(start.weekday().num_days_from_monday())) / 7 % self.interval
                    == 0
                    && (!self.days.is_empty() || date.weekday() == start.weekday())
            }
            "MONTHLY" => {
                months % self.interval == 0
                    && (!self.days.is_empty()
                        || !self.monthdays.is_empty()
                        || date.day() == start.day())
            }
            _ => false,
        };
        if !period {
            return false;
        }
        let last = (28..=31)
            .rev()
            .find(|d| NaiveDate::from_ymd_opt(date.year(), date.month(), *d).is_some())
            .unwrap_or(28);
        let day = i32::try_from(date.day()).unwrap_or(0);
        let last = i32::try_from(last).unwrap_or(0);
        (self.monthdays.is_empty()
            || self.monthdays.iter().any(|d| {
                if *d > 0 {
                    day == *d
                } else {
                    day == last + *d + 1
                }
            }))
            && (self.days.is_empty()
                || self.days.iter().any(|(ord, w)| {
                    *w == weekday
                        && (*ord == 0
                            || if *ord > 0 {
                                (day - 1) / 7 + 1 == *ord
                            } else {
                                -((last - day) / 7 + 1) == *ord
                            })
                }))
    }
}
// Unlike Result::transpose, this helper distinguishes an absent bound from an invalid one.
trait Transpose<T> {
    fn transpose_option(self) -> Option<Option<T>>;
}
impl<T> Transpose<T> for Option<Option<T>> {
    fn transpose_option(self) -> Option<Option<T>> {
        match self {
            None => Some(None),
            Some(Some(x)) => Some(Some(x)),
            Some(None) => None,
        }
    }
}

/// Parse and expand an ICS file, retaining cancellations and original keys for overrides.
/// Unknown/unresolved zones skip whole VEVENT records; structural errors refuse the sync.
pub fn parse_ics(text: &str, window: &Window) -> Result<(Vec<Event>, usize), CalendarError> {
    let records = records(text)?;
    let mut skipped = 0;
    let mut masters = BTreeMap::new();
    let mut overrides = BTreeMap::new();
    for raw in records {
        let check = (|| {
            raw.start()?.date()?;
            raw.duration(None)?;
            if let Some(p) = raw.get("RECURRENCE-ID") {
                p.date()?;
            }
            for p in raw.props.iter().filter(|p| p.name == "EXDATE") {
                for v in p.value.split(',') {
                    let mut p = p.clone();
                    p.value = v.into();
                    p.date()?;
                }
            }
            Ok::<_, CalendarError>(())
        })();
        match check {
            Err(CalendarError::Zone) => {
                skipped += 1;
                continue;
            }
            Err(e) => return Err(e),
            Ok(()) => {}
        }
        let uid = raw.text("UID");
        if uid.is_empty() {
            return Err(CalendarError::Invalid);
        }
        if let Some(p) = raw.get("RECURRENCE-ID") {
            let original = p.date()?.0;
            if overrides.insert((uid, stamp(original)), raw).is_some() {
                return Err(CalendarError::Invalid);
            }
        } else if masters.insert(uid, raw).is_some() {
            return Err(CalendarError::Invalid);
        }
    }
    let mut out = BTreeMap::new();
    let mut output_bytes = 0usize;
    let mut work = 0usize;
    for (uid, raw) in &masters {
        let prop = raw.start()?;
        let start = prop.date()?.0;
        let duration = raw.duration(None)?;
        let rule = raw.get("RRULE").and_then(|r| Rule::parse(&r.value, prop));
        let unsupported =
            (raw.get("RRULE").is_some() && rule.is_none()) || raw.get("RDATE").is_some();
        let mut dates = vec![start];
        if let Some(rule) = rule.filter(|_| !unsupported) {
            let mut day = start.date_naive();
            let mut count = 1u32;
            while rule.count.is_none_or(|n| count < n) {
                day = day.succ_opt().ok_or(CalendarError::TooLarge)?;
                work += 1;
                if work > 1_000_000 {
                    return Err(CalendarError::TooLarge);
                }
                // The largest possible timezone displacement is under two days.
                if day
                    > window
                        .until
                        .date_naive()
                        .checked_add_signed(Duration::days(2))
                        .ok_or(CalendarError::Invalid)?
                {
                    break;
                }
                if !rule.matches(day, start.date_naive()) {
                    continue;
                }
                let at = match time::at(day.and_time(start.time()), prop.tz()) {
                    Ok(t) => t,
                    Err(CalendarError::Zone) => {
                        skipped += 1;
                        continue;
                    }
                    Err(e) => return Err(e),
                };
                if rule.until.is_some_and(|until| at > until) {
                    break;
                }
                count += 1;
                if at
                    >= window
                        .from
                        .checked_sub_signed(duration)
                        .ok_or(CalendarError::TooLarge)?
                {
                    dates.push(at);
                }
            }
        }
        let mut excluded = BTreeSet::new();
        for p in raw.props.iter().filter(|p| p.name == "EXDATE") {
            for v in p.value.split(',') {
                let mut p = p.clone();
                p.value = v.into();
                excluded.insert(stamp(p.date()?.0));
            }
        }
        for original in dates {
            let key = stamp(original);
            if overrides.contains_key(&(uid.clone(), key.clone())) || excluded.contains(&key) {
                continue;
            }
            let event = raw.event(original, original, duration, unsupported)?;
            if window.contains(&event) {
                insert(&mut out, &mut output_bytes, event)?;
            }
        }
    }
    for ((uid, _), raw) in overrides {
        let original = raw
            .get("RECURRENCE-ID")
            .ok_or(CalendarError::Invalid)?
            .date()?
            .0;
        let base = masters.get(&uid);
        let unsupported = if let Some(base) = base {
            let prop = base.start()?;
            (base
                .get("RRULE")
                .is_some_and(|r| Rule::parse(&r.value, prop).is_none()))
                || base.get("RDATE").is_some()
        } else {
            false
        };
        if unsupported
            && base.is_some_and(|b| {
                b.start()
                    .and_then(Property::date)
                    .is_ok_and(|t| t.0 != original)
            })
        {
            continue;
        }
        // Inherit unspecified fields from the master, as RFC 5545 overrides do.
        let mut effective = raw.clone();
        if let Some(base) = base {
            for p in &base.props {
                if !["RRULE", "EXDATE", "DTSTART", "DTEND", "DURATION"].contains(&p.name.as_str())
                    && raw.get(&p.name).is_none()
                {
                    effective.props.push(p.clone());
                }
            }
        }
        let event = effective.event(
            effective.start()?.date()?.0,
            original,
            effective.duration(base.map(|r| r.duration(None)).transpose()?)?,
            unsupported,
        )?;
        if window.contains(&event) {
            insert(&mut out, &mut output_bytes, event)?;
        }
    }
    let mut events: Vec<_> = out.into_values().collect();
    events.sort_by(|a, b| {
        parse_time_key(&a.start)
            .cmp(&parse_time_key(&b.start))
            .then(a.key.cmp(&b.key))
    });
    if events.len() > 10000 {
        return Err(CalendarError::TooLarge);
    }
    Ok((events, skipped))
}
fn parse_time_key(s: &str) -> Option<DateTime<FixedOffset>> {
    super::parse_time(s).ok()
}

// Enforce bounds during expansion, before amplified invite text can accumulate.
fn insert(
    out: &mut BTreeMap<String, Event>,
    bytes: &mut usize,
    event: Event,
) -> Result<(), CalendarError> {
    let size = serde_json::to_vec(&event)
        .map_err(|_| CalendarError::Invalid)?
        .len();
    *bytes = bytes.checked_add(size).ok_or(CalendarError::TooLarge)?;
    if *bytes > 16 * 1_048_576 || out.len() >= 10000 {
        return Err(CalendarError::TooLarge);
    }
    out.insert(event.key.clone(), event);
    Ok(())
}
