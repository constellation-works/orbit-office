//! The private-route token is bound to the origin it was saved for.

use crate::domain::endpoint::{BoundToken, TokenBinding};

fn record(origin: &str, token: &str) -> String {
    serde_json::json!({ "origin": origin, "token": token }).to_string()
}

#[test]
fn a_bound_token_is_offered_only_to_its_own_origin() {
    let bound = BoundToken::parse(&record("https://a.example", "tok-a")).expect("a record");

    assert_eq!(bound.token_for("https://a.example/v1"), Some("tok-a"));
    assert_eq!(
        bound.token_for("https://A.example:443/other"),
        Some("tok-a")
    );
    assert_eq!(bound.token_for("https://b.example/v1"), None);
    assert_eq!(bound.token_for("http://a.example/v1"), None);
    assert_eq!(bound.token_for("https://a.example:8443/v1"), None);
    assert_eq!(bound.token_for("https://a.example.b.example/v1"), None);
    assert_eq!(bound.token_for("not a url"), None);
}

#[test]
fn loopback_ports_are_distinct_origins() {
    let bound = BoundToken::parse(&record("http://127.0.0.1:4100", "tok")).expect("a record");

    assert_eq!(bound.token_for("http://127.0.0.1:4100/v1"), Some("tok"));
    assert_eq!(bound.token_for("http://127.0.0.1:4101/v1"), None);
}

#[test]
fn anything_but_the_exact_record_has_no_binding() {
    for stored in [
        "bare-token".to_string(),
        String::new(),
        "{}".to_string(),
        r#"{"origin":"https://a.example"}"#.to_string(),
        record("https://a.example", ""),
        record("a.example", "tok"),
        r#"{"origin":"https://a.example","token":"tok","extra":1}"#.to_string(),
        r#"{"origin":"https://a.example","token":7}"#.to_string(),
        r#"["https://a.example","tok"]"#.to_string(),
    ] {
        assert_eq!(BoundToken::parse(&stored), None, "{stored}");
    }
}

#[test]
fn debug_never_shows_the_token() {
    let bound = BoundToken::parse(&record("https://a.example", "do-not-echo")).expect("a record");

    let shown = format!("{bound:?}");
    assert!(!shown.contains("do-not-echo"), "{shown}");
}

#[test]
fn status_binding_names_where_the_token_may_go() {
    let stored = record("https://a.example", "tok");
    let cases = [
        (None, Some("https://a.example/v1"), TokenBinding::Unset),
        (Some(""), Some("https://a.example/v1"), TokenBinding::Unset),
        (
            Some("bare-token"),
            Some("https://a.example/v1"),
            TokenBinding::Unbound,
        ),
        (
            Some(stored.as_str()),
            Some("https://a.example/v1"),
            TokenBinding::ThisEndpoint,
        ),
        (
            Some(stored.as_str()),
            Some("https://b.example/v1"),
            TokenBinding::OtherEndpoint,
        ),
        (Some(stored.as_str()), None, TokenBinding::OtherEndpoint),
    ];
    for (value, endpoint, expected) in cases {
        assert_eq!(
            TokenBinding::of(value, endpoint),
            expected,
            "{value:?} {endpoint:?}"
        );
    }
    assert_eq!(TokenBinding::ThisEndpoint.as_str(), "this_endpoint");
}
