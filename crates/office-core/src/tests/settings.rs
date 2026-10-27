//! Library settings configuration through the public parser.

use office_common::Sensitivity;

use crate::domain::settings::Settings;

const LIBRARY_EXAMPLE: &str = include_str!("../../../../examples/library/.office/config.yaml");

#[test]
fn library_example_config_parses_with_its_documented_settings() {
    let settings = Settings::from_yaml(LIBRARY_EXAMPLE).expect("example Library config parses");

    assert_eq!(settings.retention["meeting"], Some(90));
    assert_eq!(settings.sensitivity["email"], Sensitivity::Private);
    assert_eq!(settings.sensitivity["receipt"], Sensitivity::Private);
    assert_eq!(settings.packs, ["meetings"]);
    assert_eq!(settings.thresholds.repetition_count, 3);
    assert_eq!(settings.thresholds.window_days, 30);
    assert_eq!(settings.open_weights, None);
}

#[test]
fn invalid_open_weights_values_point_to_their_documented_limits() {
    let invalid_model = Settings::from_yaml(
        "open_weights:\n  endpoint: https://example.com/v1\n  model: 'my model'\n  kind: provider\n  label: Local\n",
    )
    .expect_err("model names must not contain whitespace");
    let invalid_label = Settings::from_yaml(&format!(
        "open_weights:\n  endpoint: https://example.com/v1\n  model: model\n  kind: provider\n  label: {}\n",
        "x".repeat(81)
    ))
    .expect_err("labels must not exceed their documented length");

    for error in [invalid_model, invalid_label] {
        assert!(
            error
                .to_string()
                .contains("docs/design/office/specs/plugin-surface.md#private-route-field-limits")
        );
    }
}
