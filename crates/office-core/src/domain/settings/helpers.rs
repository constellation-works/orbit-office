use serde_json::Value;

use super::model::{CHECK_ONLY, OPEN_WEIGHTS_CHECKED, OPEN_WEIGHTS_SETTABLE, OpenWeights};
use super::types::Settings;
use crate::domain::endpoint::{Capability, EndpointKind};
use crate::domain::metrics::date_seconds;
use crate::{Error, Result};

/// One Orbit workspace selector: nonempty, at most 256 bytes, no whitespace.
pub(super) fn workspace_selector<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value
        .as_str()
        .filter(|s| {
            !s.is_empty()
                && s.len() <= 256
                && s.bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"-._/:".contains(&c))
        })
        .ok_or_else(|| {
            invalid(
                key,
                "expected an Orbit workspace selector without whitespace",
            )
        })
}

pub(super) fn encode(settings: &Settings) -> Result<Value> {
    serde_json::to_value(settings).map_err(|_| invalid("config", "cannot encode settings"))
}

/// Apply one `open_weights` update to the encoded settings, before validation.
pub(super) fn set_open_weights(encoded: &mut Value, key: &str, value: Value) -> Result<()> {
    let Some((_, name)) = key.split_once('.') else {
        if let Some(block) = value.as_object()
            && let Some(checked) = OPEN_WEIGHTS_CHECKED
                .iter()
                .find(|checked| block.contains_key(**checked))
        {
            return Err(invalid(&format!("open_weights.{checked}"), CHECK_ONLY));
        }
        let mut value = value;
        if let (Some(previous), Some(replacement)) = (
            encoded.get("open_weights").and_then(Value::as_object),
            value.as_object_mut(),
        ) {
            let same_route = ["endpoint", "model"].into_iter().all(|name| {
                previous
                    .get(name)
                    .is_some_and(|old| replacement.get(name) == Some(old))
            });
            if same_route {
                for checked in OPEN_WEIGHTS_CHECKED {
                    if let Some(value) = previous.get(checked) {
                        replacement.insert(checked.into(), value.clone());
                    }
                }
            }
        }
        encoded["open_weights"] = value;
        return Ok(());
    };
    if OPEN_WEIGHTS_CHECKED.contains(&name) {
        return Err(invalid(key, CHECK_ONLY));
    }
    if !OPEN_WEIGHTS_SETTABLE.contains(&name) {
        return Err(Error::UnknownSetting { key: key.into() });
    }
    let block = encoded
        .get_mut("open_weights")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| {
            invalid(
                "open_weights",
                "set the whole open_weights block (endpoint, model, kind, label) first",
            )
        })?;
    // A new endpoint or model has not been checked: what the old one
    // accepted says nothing about it.
    if ["endpoint", "model"].contains(&name) && block.get(name) != Some(&value) {
        for checked in OPEN_WEIGHTS_CHECKED {
            block.remove(checked);
        }
    }
    block.insert(name.into(), value);
    Ok(())
}

/// Validate the `open_weights` block (null means unconfigured).
pub(super) fn open_weights(value: &Value) -> Result<Option<OpenWeights>> {
    if value.is_null() {
        return Ok(None);
    }
    let map = value.as_object().ok_or_else(|| {
        invalid(
            "open_weights",
            "expected a mapping of endpoint, model, kind and label, or null",
        )
    })?;
    if let Some(unknown) = map.keys().find(|key| {
        !OPEN_WEIGHTS_SETTABLE.contains(&key.as_str())
            && !OPEN_WEIGHTS_CHECKED.contains(&key.as_str())
    }) {
        return Err(invalid(
            &format!("open_weights.{unknown}"),
            "unknown open_weights key",
        ));
    }
    let text = |name: &str, max: usize| -> Result<String> {
        let dotted = format!("open_weights.{name}");
        map.get(name)
            .ok_or_else(|| invalid(&dotted, "required in the open_weights block"))?
            .as_str()
            .filter(|text| {
                !text.trim().is_empty()
                    && text.chars().count() <= max
                    && !text.chars().any(char::is_control)
            })
            .map(str::to_string)
            .ok_or_else(|| {
                invalid(
                    &dotted,
                    "expected a nonempty single-line string within the limits in docs/design/office/specs/plugin-surface.md#private-route-field-limits",
                )
            })
    };
    let endpoint = text("endpoint", 2048)?;
    crate::domain::endpoint::allow_private_endpoint(&endpoint)
        .map_err(|reason| invalid("open_weights.endpoint", reason))?;
    let model = text("model", 200)?;
    if model.chars().any(char::is_whitespace) {
        return Err(invalid(
            "open_weights.model",
            "expected the model name as the server names it, without whitespace (see docs/design/office/specs/plugin-surface.md#private-route-field-limits)",
        ));
    }
    let kind = EndpointKind::parse(&text("kind", 32)?).ok_or_else(|| {
        invalid(
            "open_weights.kind",
            "expected company, provider or own_hardware",
        )
    })?;
    let label = text("label", 80)?;
    let capabilities = map
        .get("capabilities")
        .map(|value| {
            let list = value.as_array().ok_or_else(|| {
                invalid(
                    "open_weights.capabilities",
                    "expected a list of text, image or audio",
                )
            })?;
            let mut seen = Vec::new();
            for item in list {
                let capability = item.as_str().and_then(Capability::parse).ok_or_else(|| {
                    invalid(
                        "open_weights.capabilities",
                        "expected a list of text, image or audio",
                    )
                })?;
                if seen.contains(&capability) {
                    return Err(invalid("open_weights.capabilities", "duplicate capability"));
                }
                seen.push(capability);
            }
            Ok(seen)
        })
        .transpose()?;
    let verified_at = map
        .get("verified_at")
        .map(|value| {
            value
                .as_str()
                .filter(|text| is_rfc3339(text))
                .map(str::to_string)
                .ok_or_else(|| {
                    invalid("open_weights.verified_at", "expected an RFC 3339 timestamp")
                })
        })
        .transpose()?;
    // The check writes both together; one without the other was typed in.
    match (&capabilities, &verified_at) {
        (Some(_), None) => return Err(invalid("open_weights.capabilities", CHECK_ONLY)),
        (None, Some(_)) => return Err(invalid("open_weights.verified_at", CHECK_ONLY)),
        (Some(_), Some(_)) | (None, None) => {}
    }
    Ok(Some(OpenWeights {
        endpoint,
        model,
        kind,
        label,
        capabilities,
        verified_at,
    }))
}

/// `YYYY-MM-DDTHH:MM:SS[.frac](Z|±HH:MM)`.
pub(super) fn is_rfc3339(text: &str) -> bool {
    let bytes = text.as_bytes();
    let digits = |range: std::ops::Range<usize>| {
        bytes
            .get(range)
            .is_some_and(|part| part.iter().all(u8::is_ascii_digit))
    };
    let at = |index: usize, byte: u8| bytes.get(index) == Some(&byte);
    if !(digits(0..4)
        && at(4, b'-')
        && digits(5..7)
        && at(7, b'-')
        && digits(8..10)
        && (at(10, b'T') || at(10, b't'))
        && digits(11..13)
        && at(13, b':')
        && digits(14..16)
        && at(16, b':')
        && digits(17..19))
    {
        return false;
    }
    let mut rest = &text[19..];
    if let Some(fraction) = rest.strip_prefix('.') {
        let count = fraction.bytes().take_while(u8::is_ascii_digit).count();
        if count == 0 {
            return false;
        }
        rest = &fraction[count..];
    }
    let offset = rest.as_bytes();
    rest == "Z"
        || rest == "z"
        || (offset.len() == 6
            && matches!(offset[0], b'+' | b'-')
            && offset[1..3].iter().all(u8::is_ascii_digit)
            && offset[3] == b':'
            && offset[4..6].iter().all(u8::is_ascii_digit))
}

/// A real `YYYY-MM-DD` date, or null for no simulated history.
pub(super) fn simulated_until(value: &Value, key: &str) -> Result<Option<String>> {
    if value.is_null() {
        return Ok(None);
    }
    value
        .as_str()
        .filter(|text| date_seconds(text).is_some())
        .map(|text| Some(text.to_string()))
        .ok_or_else(|| invalid(key, "expected a YYYY-MM-DD date or null"))
}

pub(super) fn positive(value: &Value, key: &str, max: u32) -> Result<u32> {
    value
        .as_u64()
        .and_then(|v| u32::try_from(v).ok())
        .filter(|v| (1..=max).contains(v))
        .ok_or_else(|| {
            invalid(
                key,
                "expected a positive integer within the documented bound",
            )
        })
}

pub(super) fn invalid(key: &str, reason: &'static str) -> Error {
    Error::InvalidSetting {
        key: key.into(),
        reason,
    }
}
