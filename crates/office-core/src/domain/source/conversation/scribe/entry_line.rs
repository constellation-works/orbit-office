//! Validate and render one cited message or note as an SCR entry.

use chrono::{DateTime, FixedOffset};

use super::super::scrub;
use super::entries::{matter_at, tag_at};
use super::rules::token;
use super::time::valid_due;
use super::{
    Authors, Entry, Message, NOTE_MAX, PREFERENCE_REVIEW_DAYS, ScrFile, ScribeRules, WhyNote,
    line_time, local, message_index,
};

fn valid_crew(by: &str) -> bool {
    by != "user"
        && by.len() <= 64
        && by.bytes().next().is_some_and(|b| b.is_ascii_alphanumeric())
        && by
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.:/".contains(&b))
}

fn valid_ref(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 128
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"#-_.:/@".contains(&b))
}

fn escape(text: &str) -> String {
    text.replace("\r\n", "\\n").replace(['\n', '\r'], "\\n")
}

pub(super) struct Ctx<'a> {
    pub(super) messages: &'a [Message],
    pub(super) rules: &'a ScribeRules,
    pub(super) files: &'a [ScrFile],
    pub(super) after: Option<usize>,
    pub(super) through: Option<usize>,
    pub(super) zone: Option<&'a str>,
    pub(super) now: DateTime<FixedOffset>,
}

impl Ctx<'_> {
    /// Render one entry or say why it is refused. `own` is this file's
    /// planned Markdown so far, with its current and earlier ids.
    pub(super) fn line(
        &self,
        entry: &Entry,
        own: &str,
        own_ids: &[String],
    ) -> std::result::Result<String, (&'static str, String)> {
        let fail = |reason: &'static str, message: &str| Err((reason, message.to_owned()));
        // The cited message and its author.
        let (head, at, quote, user) = if let Some(msg) = &entry.msg {
            let Some(index) = message_index(msg).filter(|i| *i < self.messages.len()) else {
                return fail("unknown_message", "no parsed message has this id");
            };
            if self.after.is_some_and(|after| index <= after) {
                return fail(
                    "context_message",
                    "the message is at or before scribed_through; context is read-only",
                );
            }
            if self.through.is_none_or(|through| index > through) {
                return fail("after_through", "the message is after through");
            }
            let message = &self.messages[index];
            let user = message.role == "user";
            match entry.by.as_deref() {
                Some("user") if !user => {
                    return fail("author_mismatch", "by: user cites an assistant message");
                }
                Some(_) if user => {
                    return fail("author_mismatch", "a crew by cites a user message");
                }
                _ => {}
            }
            let quote = entry.quote.as_deref().unwrap_or_default();
            if quote.is_empty() || !message.text.contains(quote) {
                return fail(
                    "quote_not_verbatim",
                    "the quote is not an exact substring of the cited message",
                );
            }
            let at = local(&message.at, self.zone).map_err(|_| {
                (
                    "unknown_message",
                    "the cited message has no valid time".to_owned(),
                )
            })?;
            (msg.clone(), at, quote.to_owned(), user)
        } else {
            let note = entry.text.as_deref().unwrap_or_default();
            if note.trim().is_empty() || note.len() > NOTE_MAX {
                return fail("invalid_note", "a note needs 1 to 4096 bytes of text");
            }
            let user = match entry.by.as_deref() {
                Some("user") => true,
                Some(_) => false,
                None => return fail("missing_by", "a note needs by: user or by: <crew>"),
            };
            ("note".to_owned(), self.now, scrub(note), user)
        };
        if let Some(by) = entry.by.as_deref()
            && by != "user"
            && !valid_crew(by)
        {
            return fail("invalid_by", "by is user or a crew name");
        }
        let Some(tag) = self.rules.tags.iter().find(|tag| tag.name == entry.tag) else {
            return fail("unknown_tag", "the tag is not in the scribe rules");
        };
        let mut topics: Vec<&str> = Vec::new();
        for topic in &entry.topics {
            if !self.rules.topics.contains(topic) {
                return fail("unknown_topic", "a topic is not in the scribe rules");
            }
            if !topics.contains(&topic.as_str()) {
                topics.push(topic);
            }
        }
        match tag.authors {
            Authors::User if !user => {
                return fail(
                    "tag_not_permitted",
                    "this tag is only for the user's messages and by: user notes",
                );
            }
            Authors::Agent if user => {
                return fail(
                    "tag_not_permitted",
                    "this tag is only for assistant messages and agent notes",
                );
            }
            Authors::Agent if entry.by.is_none() => {
                return fail("missing_by", "an agent tag needs by: <crew>");
            }
            Authors::User | Authors::Agent | Authors::Any => {}
        }
        if entry.refs.iter().any(|r| !valid_ref(r)) {
            return fail(
                "invalid_ref",
                "refs are single tokens such as ORB-1 or PR#2",
            );
        }
        if entry.due.as_deref().is_some_and(|due| !valid_due(due)) {
            return fail(
                "invalid_due",
                "due is an RFC 3339 time or a YYYY-MM-DD date",
            );
        }
        match (tag.name.as_str(), entry.resolves.as_deref()) {
            ("resolved", None) => {
                return fail(
                    "missing_resolves",
                    "a resolved entry names its pending line",
                );
            }
            ("resolved", Some(target)) => {
                if !self.resolves(target, own, own_ids) {
                    return fail(
                        "unresolved_pending",
                        "resolves does not name an existing pending line",
                    );
                }
            }
            ("why", None) => {
                return fail("missing_resolves", "a why entry names its decision line");
            }
            ("why", Some(target)) => {
                let named = self.referenced(target, own, own_ids);
                if !matches!(named.as_deref(), Some("decision" | "agent-decision")) {
                    return fail(
                        "unresolved_decision",
                        "resolves does not name an existing decision line",
                    );
                }
            }
            ("preference", Some(target)) => {
                let named = self.referenced(target, own, own_ids);
                if named.as_deref() != Some("decision") {
                    return fail(
                        "unresolved_decision",
                        "resolves does not name an existing decision line",
                    );
                }
            }
            ("preference", None) => {}
            (_, Some(_)) => {
                return fail(
                    "resolves_not_allowed",
                    "only resolved, why and preference entries take resolves",
                );
            }
            (_, None) => {}
        }
        // Why: checked verbatim against its cited message or note.
        let why_rendered: Option<(String, String)> = match &entry.why {
            None => None,
            Some(why) => {
                if !matches!(
                    tag.name.as_str(),
                    "decision" | "agent-decision" | "preference"
                ) {
                    return fail(
                        "why_not_allowed",
                        "only decision, agent-decision and preference entries take why",
                    );
                }
                match (&why.msg, &why.note) {
                    (Some(msg), None) => {
                        let quote = why.quote.as_deref().unwrap_or_default();
                        let Some(index) = message_index(msg).filter(|i| *i < self.messages.len())
                        else {
                            return fail("unknown_message", "no parsed message has this id");
                        };
                        if quote.is_empty() || !self.messages[index].text.contains(quote) {
                            return fail(
                                "why_not_verbatim",
                                "the why quote is not an exact substring of the cited message",
                            );
                        }
                        Some((msg.clone(), quote.to_owned()))
                    }
                    (None, Some(WhyNote::Text(text))) => {
                        let quote = why.quote.as_deref().unwrap_or(text.as_str());
                        if quote.is_empty() || !text.contains(quote) {
                            return fail(
                                "why_not_verbatim",
                                "the why quote is not an exact substring of the cited note",
                            );
                        }
                        Some(("note".to_owned(), quote.to_owned()))
                    }
                    (None, Some(WhyNote::Flag(true))) => {
                        let quote = why.quote.as_deref().unwrap_or_default();
                        let note = entry.text.as_deref().unwrap_or_default();
                        if entry.text.is_none() || quote.is_empty() || !note.contains(quote) {
                            return fail(
                                "why_not_verbatim",
                                "the why quote is not an exact substring of the cited note",
                            );
                        }
                        Some(("note".to_owned(), quote.to_owned()))
                    }
                    _ => {
                        return fail(
                            "why_not_verbatim",
                            "the why quote is not an exact substring of the cited message",
                        );
                    }
                }
            }
        };
        // Preference matter, review_by and supersedes.
        let mut review_by: Option<String> = None;
        if tag.name == "preference" {
            let Some(matter) = entry.matter.as_deref() else {
                return fail("missing_matter", "a preference entry names its matter");
            };
            if token(matter).as_deref() != Some(matter) {
                return fail("unknown_matter", "matter is a short slug");
            }
            if !self.rules.matters.iter().any(|m| m == matter) {
                return fail("unknown_matter", "the matter is not in the scribe rules");
            }
            if let Some(explicit) = &entry.review_by {
                if !valid_due(explicit) {
                    return fail(
                        "invalid_review_by",
                        "review_by is an RFC 3339 time or a YYYY-MM-DD date",
                    );
                }
                review_by = Some(explicit.clone());
            } else {
                review_by = Some(
                    (at + chrono::Duration::days(PREFERENCE_REVIEW_DAYS))
                        .format("%Y-%m-%d")
                        .to_string(),
                );
            }
            if let Some(target) = &entry.supersedes {
                let ok = self.referenced_matter(target, own, own_ids).is_some_and(
                    |(named_tag, named_matter)| {
                        named_tag == "preference" && named_matter.as_deref() == Some(matter)
                    },
                );
                if !ok {
                    return fail(
                        "invalid_supersedes",
                        "supersedes does not name an existing preference line on the same matter",
                    );
                }
            }
        } else {
            if entry.matter.is_some() {
                return fail("matter_not_allowed", "only a preference entry takes matter");
            }
            if entry.review_by.is_some() {
                return fail(
                    "review_by_not_allowed",
                    "only a preference entry takes review_by",
                );
            }
            if entry.supersedes.is_some() {
                return fail(
                    "supersedes_not_allowed",
                    "only a preference entry takes supersedes",
                );
            }
        }
        let mut line = format!("- {head} {} #{}", line_time(at), tag.name);
        if matches!(tag.name.as_str(), "decision" | "agent-decision") && why_rendered.is_none() {
            line.push_str(" #no-why");
        }
        for topic in topics {
            line.push_str(&format!(" #{topic}"));
        }
        line.push_str(&format!(" \"{}\"", escape(&quote)));
        if let Some((source, reason)) = &why_rendered {
            line.push_str(&format!(" why: {source} \"{}\"", escape(reason)));
        }
        if !entry.refs.is_empty() {
            line.push_str(&format!(" refs: {}", entry.refs.join(", ")));
        }
        if let Some(due) = &entry.due {
            line.push_str(&format!(" due: {due}"));
        }
        if let Some(matter) = &entry.matter {
            line.push_str(&format!(" matter: {matter}"));
        }
        if let Some(review) = &review_by {
            line.push_str(&format!(" review_by: {review}"));
        }
        if let Some(target) = &entry.supersedes {
            line.push_str(&format!(" supersedes: {target}"));
        }
        if let Some(target) = &entry.resolves {
            line.push_str(&format!(" resolves: {target}"));
        }
        if let Some(by) = &entry.by {
            line.push_str(&format!(" by: {by}"));
        }
        Ok(line)
    }

    fn resolves(&self, target: &str, own: &str, own_ids: &[String]) -> bool {
        self.referenced(target, own, own_ids).as_deref() == Some("pending")
    }

    /// The tag of the line `target` (`<SCR id>#L<n>`) names, if it exists.
    fn referenced(&self, target: &str, own: &str, own_ids: &[String]) -> Option<String> {
        self.referenced_matter(target, own, own_ids)
            .map(|(tag, _)| tag)
    }

    /// The tag and matter of the line `target` names, if it exists.
    fn referenced_matter(
        &self,
        target: &str,
        own: &str,
        own_ids: &[String],
    ) -> Option<(String, Option<String>)> {
        let (id, line) = target.split_once("#L")?;
        let n = line
            .parse::<usize>()
            .ok()
            .filter(|_| line.bytes().all(|b| b.is_ascii_digit()))?;
        if own_ids.iter().any(|own_id| own_id == id) {
            return tag_at(own, n).map(|tag| (tag, matter_at(own, n)));
        }
        self.files
            .iter()
            .filter(|f| f.front.id == id || f.front.aliases.iter().any(|a| a == id))
            .find_map(|f| tag_at(&f.markdown, n).map(|tag| (tag, matter_at(&f.markdown, n))))
    }
}
