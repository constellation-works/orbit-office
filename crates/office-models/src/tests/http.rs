//! Host parsing and the allowlist check for IPv6 endpoints.

use crate::http::{check_url, host_of};

#[test]
fn host_of_strips_brackets_and_port_from_an_ipv6_literal() {
    assert_eq!(host_of("http://[::1]:8080/v1").as_deref(), Some("::1"));
    assert_eq!(host_of("http://[::1]/v1").as_deref(), Some("::1"));
    assert_eq!(
        host_of("https://user@[::1]:8443/v1").as_deref(),
        Some("::1")
    );
}

#[test]
fn host_of_refuses_an_unterminated_ipv6_literal() {
    assert_eq!(host_of("http://[::1:8080/v1"), None);
}

#[test]
fn check_url_accepts_an_ipv6_loopback_endpoint_over_http() {
    assert_eq!(check_url("http://[::1]:8080/v1").expect("loopback"), "::1");
}

#[test]
fn check_url_refuses_a_non_loopback_ipv6_host() {
    let error = check_url("http://[2001:db8::1]:8080/v1").expect_err("not allowlisted");
    assert_eq!(error.code(), "model_host_not_allowed");
}
