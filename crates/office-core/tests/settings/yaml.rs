use super::{BLOCK, refused_key};
use office_core::domain::endpoint::{Capability, EndpointKind};
use office_core::{Error, domain::settings::Settings};

#[test]
fn pre_batch_default_config_loads() {
    // Exact default emitted by office.init before the private-route change.
    // Its retired `note` keys are ignored on read.
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
    let settings = Settings::from_yaml("retention:\n  meeting: null\nsensitivity:\n  decisions: private\npacks:\n  - meetings\n  - receipts\nthresholds:\n  repetition_count: 4\nopen_weights:\n  endpoint: https://muse.internal.acme.example/v1   # OpenAI-compatible base URL\n  model: meta-models/Muse-Spark-Open-816B-A42B       # as the server names it\n  kind: company            # company | provider | own_hardware\n  label: Acme's server     # shown in the app\n  capabilities: [text, image, audio]   # written by the check below, never typed in\n  verified_at: 2026-10-27T10:02:00-04:00\n").expect("settings");
    assert_eq!(settings.retention["meeting"], None);
    assert_eq!(settings.retention["decisions"], None);
    assert_eq!(settings.sensitivity["decisions"].as_str(), "private");
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
