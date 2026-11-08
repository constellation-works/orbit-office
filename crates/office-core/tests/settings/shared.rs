use super::refused_key;
use office_core::domain::settings::Settings;
use serde_json::json;

#[test]
fn shared_block_is_closed_and_validated_on_yaml_reads_and_deserialization() {
    let block = json!({"remote":"ssh://git@example.invalid/team.git","branch":"main","member":"member-one"});
    let settings: Settings = serde_json::from_value(json!({"shared":block})).unwrap();
    assert_eq!(
        Settings::from_yaml(&settings.to_yaml().unwrap()).unwrap(),
        settings
    );
    for (key, value) in [
        (
            "remote",
            json!("ssh://git:fixture-secret@example.invalid/team.git"),
        ),
        ("remote", json!("https://@example.invalid/team.git")),
        ("remote", json!("file:///srv/team.git")),
        ("remote", json!("relative/team.git")),
        ("remote", json!("ext::fixture-secret")),
        ("member", json!("a".repeat(33))),
        ("member", json!("a--b")),
        ("branch", json!("bad..branch")),
        ("unknown", json!("fixture-secret")),
    ] {
        let mut invalid = block.clone();
        invalid[key] = value.clone();
        let result = Settings::from_yaml(&json!({"shared":invalid}).to_string());
        let error = result.unwrap_err();
        assert_eq!(error.code(), "invalid_library_setting");
        assert!(error.to_string().contains(&format!("shared.{key}")));
        assert!(!error.to_string().contains("fixture-secret"));
        assert!(serde_json::from_value::<Settings>(json!({"shared":invalid})).is_err());
    }
}

#[test]
fn shared_remotes_accept_local_spaces_and_scp_absolute_paths() {
    for remote in [
        "/srv/shared library.git",
        "git@example.invalid:/srv/library.git",
    ] {
        let settings = Settings::default()
            .with_setting(
                "shared",
                json!({"remote":remote,"branch":"main","member":"member"}),
            )
            .unwrap();
        assert_eq!(settings.shared.unwrap().remote, remote);
    }
}

#[test]
fn a_legacy_scribe_block_loads_is_ignored_and_is_not_written_back() {
    // A settings file as an earlier build wrote it (pretty JSON), with a
    // full scribe block, including values that build would have refused.
    let file = json!({
        "packs": ["kt"],
        "scribe": {
            "cadence": {"minutes": 30, "turns": 4},
            "tags": [{"name": "lesson", "scope": "project", "instruction": "Record this.",
                      "sink": "scr_only", "mode": "apply"}],
            "precedents": {"half_life_days": 7}
        }
    });
    let settings = Settings::from_yaml(&serde_json::to_string_pretty(&file).expect("json"))
        .expect("a legacy scribe block loads");
    let expected = Settings::default()
        .with_setting("packs", json!(["kt"]))
        .expect("packs");
    assert_eq!(settings, expected);
    // The retired block's half-life does not carry over to precedents.
    assert_eq!(settings.precedents.half_life_days, 90);
    let written = settings.to_yaml().expect("encode");
    assert!(!written.contains("scribe"), "{written}");
    // A hand-edited YAML block and a scalar scribe value load the same way.
    Settings::from_yaml("scribe:\n  cadence: 3\n").expect("yaml mapping");
    Settings::from_yaml("scribe: 3\n").expect("scalar");
}

#[test]
fn scribe_keys_are_unknown_settings_for_config_set() {
    for key in [
        "scribe",
        "scribe.cadence",
        "scribe.tags",
        "scribe.tags.lesson",
        "scribe.precedents.half_life_days",
    ] {
        let error = Settings::default()
            .with_setting(key, json!(null))
            .expect_err("scribe keys are retired");
        assert_eq!(error.code(), "unknown_library_setting", "{key}: {error}");
    }
}

#[test]
fn precedents_half_life_days_accepts_bounds_and_null_restores_the_default() {
    let defaults = Settings::default();
    assert_eq!(defaults.precedents.half_life_days, 90);
    assert!(!defaults.to_yaml().expect("encode").contains("precedents"));
    for days in [1, 3650] {
        let set = Settings::default()
            .with_setting("precedents.half_life_days", json!(days))
            .expect("in range");
        assert_eq!(set.precedents.half_life_days, days);
        let reloaded = Settings::from_yaml(&set.to_yaml().expect("encode")).expect("reload");
        assert_eq!(reloaded, set);
    }
    let restored = Settings::default()
        .with_setting("precedents.half_life_days", json!(30))
        .and_then(|set| set.with_setting("precedents.half_life_days", json!(null)))
        .expect("null restores");
    assert_eq!(restored, Settings::default());
    for value in [json!(0), json!(3651), json!("90"), json!(1.5)] {
        assert_eq!(
            refused_key(Settings::default().with_setting("precedents.half_life_days", value)),
            "precedents.half_life_days"
        );
    }
    assert_eq!(
        refused_key(Settings::from_yaml("precedents:\n  window: 3\n")),
        "precedents.window"
    );
}

#[test]
fn retired_note_kind_is_ignored_on_read_and_refused_on_write_while_decisions_is_a_kind() {
    let settings = Settings::from_yaml("retention:\n  note: 30\nsensitivity:\n  note: private\n")
        .expect("a config written before note was retired still loads");
    assert_eq!(settings, Settings::default());
    assert!(!settings.retention.contains_key("note"));
    assert!(!settings.sensitivity.contains_key("note"));
    assert!(!Settings::default_yaml().contains("note"));
    for key in ["retention.note", "sensitivity.note"] {
        let error = settings
            .with_setting(key, serde_json::json!(null))
            .unwrap_err();
        assert_eq!(error.code(), "unknown_library_setting", "{key}");
    }
    assert_eq!(settings.retention["decisions"], None);
    assert_eq!(settings.sensitivity["decisions"].as_str(), "normal");
    let updated = settings
        .with_setting("sensitivity.decisions", serde_json::json!("private"))
        .expect("decisions is a kind");
    assert_eq!(updated.sensitivity["decisions"].as_str(), "private");
}
