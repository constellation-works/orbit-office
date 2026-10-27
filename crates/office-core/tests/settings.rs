//! Settings through the public, pure parsing/deserialization boundary.
#![allow(clippy::expect_used, clippy::unwrap_used)]
use office_core::domain::endpoint::{Capability, EndpointKind};
use office_core::{Error, domain::settings::Settings};
use serde_json::json;

const BLOCK: &str = "open_weights:\n  endpoint: https://muse.internal.acme.example/v1\n  model: meta-models/Muse-Spark-Open-816B-A42B\n  kind: company\n  label: Acme's server\n";

fn block_value() -> serde_json::Value {
    json!({
        "endpoint": "https://muse.internal.acme.example/v1",
        "model": "meta-models/Muse-Spark-Open-816B-A42B",
        "kind": "company",
        "label": "Acme's server",
    })
}

fn refused_key(result: office_core::Result<Settings>) -> String {
    let error = result.expect_err("refused");
    let Error::InvalidSetting { key, .. } = error else {
        panic!("wrong error: {error:?}")
    };
    key
}

#[test]
fn single_setting_updates_roundtrip_and_preserve_other_settings() {
    let mut settings = Settings::default();
    for (key, value) in [
        ("retention.meeting", serde_json::json!(36500)),
        ("retention.note", serde_json::json!(null)),
        ("sensitivity.email", serde_json::json!("private")),
        ("packs", serde_json::json!(["receipts", "meetings"])),
        ("thresholds.repetition_count", serde_json::json!(10000)),
        ("thresholds.window_days", serde_json::json!(36500)),
        ("thresholds.stale_days", serde_json::json!(1)),
        ("thresholds.friction_count", serde_json::json!(10)),
        ("thresholds.new_subject_count", serde_json::json!(5)),
        ("thresholds.unused_days", serde_json::json!(20)),
        ("open_weights", block_value()),
        (
            "open_weights.endpoint",
            serde_json::json!("http://localhost:9/v1"),
        ),
        ("open_weights.kind", serde_json::json!("own_hardware")),
        ("open_weights.label", serde_json::json!("Your hardware")),
        ("open_weights", serde_json::json!(null)),
        ("packs", serde_json::json!([])),
    ] {
        let before = serde_json::to_value(&settings).unwrap();
        let updated = settings.with_setting(key, value.clone()).unwrap();
        let mut expected = before;
        if let Some((section, name)) = key.split_once('.')
            && section != "open_weights"
        {
            expected[section][name] = value;
        } else if let Some(("open_weights", name)) = key.split_once('.') {
            expected["open_weights"][name] = value;
        } else {
            expected[key] = value;
        }
        assert_eq!(serde_json::to_value(&updated).unwrap(), expected);
        assert_eq!(
            Settings::from_yaml(&updated.to_yaml().unwrap()).unwrap(),
            updated
        );
        settings = updated;
    }
}

#[test]
fn single_setting_refuses_unknown_leaves_and_bad_values_without_mutation() {
    let settings = Settings::default();
    for key in [
        "retention",
        "sensitivity",
        "thresholds",
        "retention.unknown",
        "thresholds.unknown",
        "retention.meeting.extra",
        "meta_api_key",
        "baseten_token",
        "open_weights_token",
        "open_weights_endpoint",
        "open_weights.token",
        "secrets.meta_api_key",
        "",
    ] {
        assert!(
            matches!(
                settings.with_setting(key, serde_json::json!(10)),
                Err(Error::UnknownSetting { .. })
            ),
            "{key}"
        );
    }
    for (key, value) in [
        ("retention.meeting", serde_json::json!(0)),
        ("retention.meeting", serde_json::json!(36501)),
        ("retention.meeting", serde_json::json!("30")),
        ("retention.meeting", serde_json::json!(1.5)),
        ("sensitivity.note", serde_json::json!(null)),
        ("sensitivity.note", serde_json::json!("secret-value")),
        ("packs", serde_json::json!(["meetings", "meetings"])),
        ("packs", serde_json::json!(["unknown"])),
        ("thresholds.friction_count", serde_json::json!(10001)),
        ("thresholds.unused_days", serde_json::json!(36501)),
        ("thresholds.window_days", serde_json::json!(false)),
        (
            "open_weights",
            serde_json::json!({"endpoint": "https://secret-value@example.test", "model": "m", "kind": "company", "label": "x"}),
        ),
        ("open_weights", serde_json::json!("secret-value")),
    ] {
        let error = settings.with_setting(key, value).unwrap_err();
        assert_eq!(error.code(), "invalid_library_setting");
        assert!(!error.to_string().contains("secret-value"));
        assert_eq!(settings, Settings::default());
    }
}

#[test]
fn pre_batch_default_config_loads() {
    // Exact default emitted by office.init before the private-route change.
    let yaml = "retention:
  video: 90
  meeting: 90
  chat: null
  email: null
  doc: null
  receipt: null
  bill: null
  conversation: null
  note: null
sensitivity:
  video: normal
  meeting: normal
  chat: normal
  email: normal
  doc: normal
  receipt: normal
  bill: normal
  conversation: normal
  note: normal
packs: [meetings]
thresholds:
  repetition_count: 3
  window_days: 30
  stale_days: 90
  friction_count: 2
  new_subject_count: 2
  unused_days: 30
open_weights_endpoint: null
";
    let expected = Settings {
        packs: vec!["meetings".into()],
        ..Settings::default()
    };
    assert_eq!(Settings::from_yaml(yaml).unwrap(), expected);
}

#[test]
fn partial_yaml_merges_policy_defaults_without_creating_observations() {
    assert_eq!(Settings::from_yaml("").expect("empty"), Settings::default());
    assert_eq!(
        Settings::from_yaml("{}").expect("empty mapping"),
        Settings::default()
    );
    let settings = Settings::from_yaml("retention:\n  meeting: null\nsensitivity:\n  note: private\npacks:\n  - meetings\n  - receipts\nthresholds:\n  repetition_count: 4\nopen_weights:\n  endpoint: https://muse.internal.acme.example/v1   # OpenAI-compatible base URL\n  model: meta-models/Muse-Spark-Open-816B-A42B       # as the server names it\n  kind: company            # company | provider | own_hardware\n  label: Acme's server     # shown in the app\n  capabilities: [text, image, audio]   # written by the check below, never typed in\n  verified_at: 2026-10-27T10:02:00-04:00\n").expect("settings");
    assert_eq!(settings.retention["meeting"], None);
    assert_eq!(settings.retention["note"], None);
    assert_eq!(settings.sensitivity["note"].as_str(), "private");
    assert_eq!(settings.sensitivity["meeting"].as_str(), "normal");
    assert_eq!(settings.thresholds.repetition_count, 4);
    assert_eq!(settings.thresholds.window_days, 30);
    assert_eq!(settings.packs, ["meetings", "receipts"]);
    let route = settings.open_weights.as_ref().expect("open_weights block");
    assert_eq!(route.endpoint, "https://muse.internal.acme.example/v1");
    assert_eq!(route.model, "meta-models/Muse-Spark-Open-816B-A42B");
    assert_eq!(route.kind, EndpointKind::Company);
    assert_eq!(route.label, "Acme's server");
    assert_eq!(route.capabilities, Some(Capability::ALL.to_vec()));
    assert_eq!(
        route.verified_at.as_deref(),
        Some("2026-10-27T10:02:00-04:00")
    );
    let encoded = serde_json::to_value(&settings).expect("serialize");
    assert_eq!(
        serde_json::from_value::<Settings>(encoded).expect("roundtrip"),
        settings
    );
}

#[test]
fn a_single_quoted_value_with_a_doubled_quote_keeps_a_later_hash_as_text() {
    // `''` is an escaped quote, not a close and reopen, so ` #` stays in the value.
    let settings = Settings::from_yaml(&BLOCK.replace(
        "label: Acme's server",
        "label: 'Acme''s #1 server'   # shown in the app",
    ))
    .expect("single-quoted label");
    assert_eq!(
        settings.open_weights.expect("block").label,
        "Acme's #1 server"
    );
}

#[test]
fn a_double_quoted_value_with_escaped_quotes_keeps_a_later_hash_as_text() {
    // A backslash-escaped quote does not close the value, so ` # x` stays in it.
    let settings = Settings::from_yaml(&BLOCK.replace(
        "label: Acme's server",
        r#"label: "say \"hi\" # x"   # shown in the app"#,
    ))
    .expect("double-quoted label");
    assert_eq!(
        settings.open_weights.expect("block").label,
        r#"say "hi" # x"#
    );
}

#[test]
fn a_backslash_does_not_escape_inside_a_single_quoted_value() {
    // YAML single quotes have no backslash escapes: the quote after `\` closes.
    let settings = Settings::from_yaml(&BLOCK.replace(
        "label: Acme's server",
        r"label: 'Acme\'   # shown in the app",
    ))
    .expect("backslash label");
    assert_eq!(settings.open_weights.expect("block").label, r"Acme\");
}

#[test]
fn a_doubled_quote_with_no_closing_quote_is_refused_not_cut_at_the_hash() {
    // `''` is an escaped quote, so this single-quoted value never closes and
    // ` # shown` is text, not a comment. YAML refuses the value.
    let key = refused_key(Settings::from_yaml(
        &BLOCK.replace("label: Acme's server", "label: 'Acme'' # shown in the app"),
    ));
    assert_eq!(key, "open_weights.label");
}

#[test]
fn invalid_values_name_the_exact_key_and_never_echo_the_value() {
    for (yaml, key) in [
        ("retention:\n  meeting: -1", "retention.meeting"),
        ("retention:\n  meeting: 4294967296", "retention.meeting"),
        (
            "sensitivity:\n  meeting: secret-token",
            "sensitivity.meeting",
        ),
        (
            "thresholds:\n  repetition_count: 0",
            "thresholds.repetition_count",
        ),
        ("thresholds:\n  window_days: 1.5", "thresholds.window_days"),
        ("thresholds:\n  typo: 5", "thresholds.typo"),
        ("packs: [unrecognized]", "packs"),
        ("retention:\n  typo: 2", "retention.typo"),
        (
            "open_weights:\n  endpoint: https://secret-token@example.test\n  model: m\n  kind: company\n  label: x",
            "open_weights.endpoint",
        ),
        (
            "open_weights_endpoint: https://example.test",
            "open_weights_endpoint",
        ),
        ("unknown: secret-token", "unknown"),
        ("packs: [meetings, meetings]", "packs"),
        (
            "retention:\n  meeting: 1\n  meeting: 2",
            "retention.meeting",
        ),
    ] {
        let error = Settings::from_yaml(yaml).unwrap_err();
        assert!(!error.to_string().contains("secret-token"));
        let Error::InvalidSetting { key: actual, .. } = error else {
            panic!("wrong error: {error:?}")
        };
        assert_eq!(actual, key);
    }
    assert!(serde_json::from_str::<Settings>(r#"{"thresholds":{"window_days":0}}"#).is_err());
}

#[test]
fn accepts_any_https_host_or_loopback_as_the_private_endpoint() {
    for endpoint in [
        "https://muse.internal.acme.example/v1",
        "https://inference.baseten.co/v1",
        "https://model-abc.api.baseten.co/v1",
        "https://10.0.0.7:8443/v1",
        "http://127.0.0.1:9/v1",
        "http://localhost:9/v1",
        "http://[::1]:9/v1",
        "https://[::1]/v1",
    ] {
        let yaml = BLOCK.replace("https://muse.internal.acme.example/v1", endpoint);
        let settings =
            Settings::from_yaml(&yaml).unwrap_or_else(|error| panic!("{endpoint}: {error}"));
        assert_eq!(settings.open_weights.expect("block").endpoint, endpoint);
    }
}

#[test]
fn refuses_malformed_endpoint_authorities_at_every_settings_boundary() {
    for endpoint in [
        "http://localhost:invalid/v1",
        "http://localhost:65536/v1",
        "http://localhost:/v1",
        "http://localhost:80:80/v1",
        "https://inference.baseten.co:secret-value/v1",
        "https://inference.baseten.co:99999/v1",
        "http://[::1]garbage/v1",
        "http://[::1]:invalid/v1",
        "http://[::1]:65536/v1",
        "http://[::1]:/v1",
        "http://[::1]9/v1",
        "https://[not-an-ip]/v1",
    ] {
        // Load (from_yaml).
        let yaml = BLOCK.replace("https://muse.internal.acme.example/v1", endpoint);
        let error = Settings::from_yaml(&yaml).expect_err(endpoint);
        assert_eq!(error.code(), "invalid_library_setting", "{endpoint}");
        assert!(!error.to_string().contains(endpoint), "echoed: {error}");
        assert!(!error.to_string().contains("secret-value"));
        assert_eq!(refused_key(Err(error)), "open_weights.endpoint");

        // Deserialization.
        let json = serde_json::to_string(&serde_json::json!({
            "open_weights": {
                "endpoint": endpoint,
                "model": "m",
                "kind": "company",
                "label": "x",
            }
        }))
        .unwrap();
        let error = serde_json::from_str::<Settings>(&json)
            .expect_err(&format!("deserialization accepted {endpoint}"));
        assert!(
            !error.to_string().contains(endpoint),
            "deserialization echoed {endpoint}: {error}"
        );

        // with_setting: whole block and endpoint leaf.
        let mut block = block_value();
        block["endpoint"] = json!(endpoint);
        let error = Settings::default()
            .with_setting("open_weights", block)
            .expect_err(endpoint);
        assert_eq!(error.code(), "invalid_library_setting", "{endpoint}");
        assert!(!error.to_string().contains(endpoint), "echoed: {error}");
        assert!(!error.to_string().contains("secret-value"));

        let configured = Settings::default()
            .with_setting("open_weights", block_value())
            .expect("valid block");
        let error = configured
            .with_setting("open_weights.endpoint", json!(endpoint))
            .expect_err(endpoint);
        assert_eq!(error.code(), "invalid_library_setting", "{endpoint}");
        assert!(!error.to_string().contains(endpoint), "echoed: {error}");
        let Error::InvalidSetting { key, .. } = error else {
            panic!("wrong error: {error:?}")
        };
        assert_eq!(key, "open_weights.endpoint", "{endpoint}");
    }
    // Valid authorities with explicit ports remain accepted.
    for endpoint in [
        "https://inference.baseten.co/v1",
        "https://inference.baseten.co:443/v1",
        "https://model-abc.api.baseten.co:8443/v1",
        "http://localhost:8080/v1",
        "http://127.0.0.1:65535/v1",
        "http://[::1]:8080/v1",
        "https://[::1]:443/v1",
    ] {
        let yaml = BLOCK.replace("https://muse.internal.acme.example/v1", endpoint);
        Settings::from_yaml(&yaml)
            .unwrap_or_else(|error| panic!("{endpoint} should be accepted: {error}"));
    }
}

#[test]
fn refuses_plain_http_off_loopback_meta_and_credentials_at_load_naming_the_key() {
    for endpoint in [
        "http://muse.internal.acme.example/v1",
        "http://10.0.0.7:8000/v1",
        "https://api.meta.ai/v1",
        "https://secret-token@muse.internal.acme.example/v1",
        "https://muse.internal.acme.example/v1?key=secret-token",
        "ftp://muse.internal.acme.example/v1",
    ] {
        let yaml = BLOCK.replace("https://muse.internal.acme.example/v1", endpoint);
        let error = Settings::from_yaml(&yaml).expect_err(endpoint);
        assert!(!error.to_string().contains(endpoint), "echoed: {error}");
        assert!(!error.to_string().contains("secret-token"));
        let Error::InvalidSetting { key, .. } = error else {
            panic!("wrong error: {error:?}")
        };
        assert_eq!(key, "open_weights.endpoint", "{endpoint}");
    }
}

#[test]
fn refuses_an_unknown_kind_and_incomplete_or_unknown_block_keys() {
    assert_eq!(
        refused_key(Settings::from_yaml(
            &BLOCK.replace("kind: company", "kind: laptop")
        )),
        "open_weights.kind"
    );
    assert_eq!(
        refused_key(Settings::from_yaml(
            &BLOCK.replace("  label: Acme's server\n", "")
        )),
        "open_weights.label"
    );
    assert_eq!(
        refused_key(Settings::from_yaml(&format!("{BLOCK}  token: abc\n"))),
        "open_weights.token"
    );
    assert_eq!(
        refused_key(Settings::from_yaml(&BLOCK.replace(
            "model: meta-models/Muse-Spark-Open-816B-A42B",
            "model: 'two words'"
        ))),
        "open_weights.model"
    );
}

/// The check writes `capabilities` and `verified_at` together; either alone
/// was typed in by hand, and config-set never writes them.
#[test]
fn refuses_hand_written_capabilities_naming_the_key() {
    assert_eq!(
        refused_key(Settings::from_yaml(&format!(
            "{BLOCK}  capabilities: [text]\n"
        ))),
        "open_weights.capabilities"
    );
    assert_eq!(
        refused_key(Settings::from_yaml(&format!(
            "{BLOCK}  verified_at: 2026-10-27T10:02:00Z\n"
        ))),
        "open_weights.verified_at"
    );
    assert_eq!(
        refused_key(Settings::from_yaml(&format!(
            "{BLOCK}  capabilities: [text, video]\n  verified_at: 2026-10-27T10:02:00Z\n"
        ))),
        "open_weights.capabilities"
    );
    assert_eq!(
        refused_key(Settings::from_yaml(&format!(
            "{BLOCK}  capabilities: [text]\n  verified_at: yesterday\n"
        ))),
        "open_weights.verified_at"
    );

    let settings = Settings::default()
        .with_setting("open_weights", block_value())
        .expect("block");
    assert_eq!(
        refused_key(settings.with_setting("open_weights.capabilities", json!(["text"]))),
        "open_weights.capabilities"
    );
    assert_eq!(
        refused_key(
            settings.with_setting("open_weights.verified_at", json!("2026-10-27T10:02:00Z"))
        ),
        "open_weights.verified_at"
    );
    let mut with_caps = block_value();
    with_caps["capabilities"] = json!(["text"]);
    assert_eq!(
        refused_key(settings.with_setting("open_weights", with_caps)),
        "open_weights.capabilities"
    );
    assert_eq!(
        refused_key(Settings::default().with_setting("open_weights.label", json!("x"))),
        "open_weights"
    );
}

#[test]
fn the_check_records_capabilities_and_a_new_endpoint_or_model_clears_them() {
    let configured = Settings::default()
        .with_setting("open_weights", block_value())
        .expect("block");
    let checked = configured
        .with_route_check(
            "https://muse.internal.acme.example/v1/",
            "meta-models/Muse-Spark-Open-816B-A42B",
            &[Capability::Image, Capability::Text],
            "2026-10-27T14:02:00+00:00",
        )
        .expect("record");
    let route = checked.open_weights.clone().expect("block");
    assert_eq!(
        route.capabilities,
        Some(vec![Capability::Text, Capability::Image])
    );
    assert!(route.accepts(Capability::Text) && !route.accepts(Capability::Audio));
    assert_eq!(
        Settings::from_yaml(&checked.to_yaml().unwrap()).unwrap(),
        checked
    );

    let relabelled = checked
        .with_setting("open_weights.label", json!("Acme GPU cluster"))
        .expect("label");
    assert!(
        relabelled
            .open_weights
            .expect("block")
            .accepts(Capability::Text)
    );
    for (key, value) in [
        (
            "open_weights.endpoint",
            json!("https://muse2.internal.acme.example/v1"),
        ),
        ("open_weights.model", json!("another-model")),
    ] {
        let moved = checked
            .with_setting(key, value)
            .expect(key)
            .open_weights
            .expect("block");
        assert_eq!(moved.capabilities, None, "{key}");
        assert_eq!(moved.verified_at, None, "{key}");
    }

    let error = configured
        .with_route_check(
            "https://other.acme.example/v1",
            "meta-models/Muse-Spark-Open-816B-A42B",
            &[Capability::Text],
            "2026-10-27T14:02:00+00:00",
        )
        .expect_err("checked another endpoint");
    assert_eq!(error.code(), "private_route_changed");
    assert_eq!(
        Settings::default()
            .with_route_check(
                "https://x.example/v1",
                "m",
                &[],
                "2026-10-27T14:02:00+00:00"
            )
            .expect_err("no block")
            .code(),
        "private_route_changed"
    );
}

#[test]
fn whole_block_updates_preserve_checks_only_when_endpoint_and_model_are_unchanged() {
    let configured = Settings::default()
        .with_setting("open_weights", block_value())
        .expect("block");
    let checked = configured
        .with_route_check(
            "https://muse.internal.acme.example/v1",
            "meta-models/Muse-Spark-Open-816B-A42B",
            &[Capability::Image, Capability::Text],
            "2026-10-27T14:02:00+00:00",
        )
        .expect("record");

    let mut relabelled_block = block_value();
    relabelled_block["label"] = json!("Acme GPU cluster");
    let relabelled = checked
        .with_setting("open_weights", relabelled_block)
        .expect("whole-block relabel");
    let route = relabelled.open_weights.expect("block");
    assert_eq!(
        route.capabilities,
        Some(vec![Capability::Text, Capability::Image])
    );
    assert_eq!(
        route.verified_at.as_deref(),
        Some("2026-10-27T14:02:00+00:00")
    );

    for (field, value) in [
        ("endpoint", json!("https://muse2.internal.acme.example/v1")),
        ("model", json!("another-model")),
    ] {
        let mut changed_block = block_value();
        changed_block[field] = value;
        let changed = checked
            .with_setting("open_weights", changed_block)
            .expect("whole-block route change")
            .open_weights
            .expect("block");
        assert_eq!(changed.capabilities, None, "{field}");
        assert_eq!(changed.verified_at, None, "{field}");
    }

    let unchecked = configured
        .with_setting("open_weights", block_value())
        .expect("unchanged unchecked block")
        .open_weights
        .expect("block");
    assert_eq!(unchecked.capabilities, None);
    assert_eq!(unchecked.verified_at, None);
}

#[test]
fn video_defaults_and_cost_bounds() {
    let defaults = Settings::default();
    assert_eq!(defaults.packs, ["kt", "meetings"]);
    let encoded = serde_json::to_value(&defaults).unwrap();
    assert_eq!(encoded["video"]["max_frames_per_hour"], 120);
    assert_eq!(encoded["video"]["scene_threshold"], 0.3);
    for (key, value) in [
        ("video.max_frames_per_hour", json!(0)),
        ("video.max_frames_per_hour", json!(601)),
        ("video.scene_threshold", json!(0.049)),
        ("video.scene_threshold", json!(0.951)),
        ("costs.image", json!(-0.1)),
        ("costs.currency", json!("dollars")),
    ] {
        let error = defaults.with_setting(key, value).unwrap_err();
        assert_eq!(error.code(), "invalid_library_setting");
        assert!(error.to_string().contains(key));
    }
    let configured = defaults
        .with_setting("costs.image", json!(0.01))
        .unwrap()
        .with_setting("costs.currency", json!("USD"))
        .unwrap()
        .with_setting("packs", json!(["kt", "meetings"]))
        .unwrap();
    assert_eq!(
        Settings::from_yaml(&configured.to_yaml().unwrap()).unwrap(),
        configured
    );
}
