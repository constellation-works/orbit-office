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
