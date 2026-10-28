//! Closed, credential-free shared Library configuration.
use crate::{Error, Result};
use serde::Serialize;
use serde_json::Value;

/// A complete configured shared Library. Absence disables sharing sync.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SharedSettings {
    /// SSH, HTTPS without user information, or an absolute local repository path.
    pub remote: String,
    /// Git branch to sync.
    pub branch: String,
    /// Stable lowercase kebab-case member name, at most 32 ASCII bytes.
    pub member: String,
}
fn invalid(key: &str) -> Error {
    Error::InvalidSetting {
        key: format!("shared.{key}"),
        reason: "invalid shared Library setting",
    }
}

pub(super) fn validate_member(member: &str) -> Result<()> {
    if member.is_empty()
        || member.len() > 32
        || !member.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
    {
        return Err(invalid("member"));
    }
    Ok(())
}
impl SharedSettings {
    /// Validate on every load and update, never including raw values in errors.
    pub fn parse(value: &Value) -> Result<Option<Self>> {
        if value.is_null() {
            return Ok(None);
        }
        let map = value.as_object().ok_or_else(|| invalid("remote"))?;
        if let Some(key) = map
            .keys()
            .find(|key| !["remote", "branch", "member"].contains(&key.as_str()))
        {
            return Err(invalid(key));
        }
        let text = |key: &str| {
            map.get(key)
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty() && !value.chars().any(char::is_control))
                .map(str::to_string)
                .ok_or_else(|| invalid(key))
        };
        let remote = text("remote")?;
        if !valid_remote(&remote) {
            return Err(invalid("remote"));
        }
        let branch = text("branch")?;
        if branch.starts_with(['-', '/'])
            || branch.ends_with(['/', '.'])
            || branch.ends_with(".lock")
            || branch.contains([' ', '~', '^', ':', '?', '*', '[', '\\'])
            || branch.contains("..")
            || branch.contains("@{")
            || branch
                .split('/')
                .any(|part| part.is_empty() || part.starts_with('.') || part.ends_with(".lock"))
            || branch == "@"
        {
            return Err(invalid("branch"));
        }
        let member = text("member")?;
        validate_member(&member)?;
        Ok(Some(Self {
            remote,
            branch,
            member,
        }))
    }
}
fn valid_remote(remote: &str) -> bool {
    if remote.starts_with('/') {
        return true;
    }
    if remote.chars().any(char::is_whitespace) || remote.contains(['?', '#']) {
        return false;
    }
    if remote.starts_with("https://") || remote.starts_with("ssh://") {
        let Ok(url) = url::Url::parse(remote) else {
            return false;
        };
        return url.host_str().is_some()
            && url.password().is_none()
            && (url.scheme() == "ssh"
                || (url.username().is_empty()
                    && !remote
                        .trim_start_matches("https://")
                        .split('/')
                        .next()
                        .is_some_and(|authority| authority.contains('@'))));
    }
    // scp syntax has one host:path separator, with an optional SSH user.
    let Some((host, path)) = remote.split_once(':') else {
        return false;
    };
    !path.is_empty()
        && !path.starts_with("//")
        && !path.starts_with(':')
        && !host.is_empty()
        && !host.contains('/')
        && host.split('@').count() <= 2
        && host.split('@').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-._".contains(&b))
        })
}
