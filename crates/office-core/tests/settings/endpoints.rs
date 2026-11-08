use super::{BLOCK, block_value, refused_key};
use office_core::{Error, domain::settings::Settings};
use serde_json::json;

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
