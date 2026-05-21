use std::collections::HashMap;

use super::*;

#[test]
fn normalizes_site_url_to_https() {
    assert_eq!(
        normalize_site_url("example.atlassian.net/").unwrap(),
        "https://example.atlassian.net"
    );
    assert!(normalize_site_url("http://example.atlassian.net").is_err());
}

#[test]
fn normalizes_loopback_oauth_redirect_uri() {
    assert_eq!(
        normalize_oauth_redirect_uri("http://LOCALHOST:8765/atlassian/oauth/callback/").unwrap(),
        "http://localhost:8765/atlassian/oauth/callback"
    );
    assert_eq!(
        normalize_oauth_redirect_uri("http://127.12.0.1:8765/callback").unwrap(),
        "http://127.12.0.1:8765/callback"
    );
}

#[test]
fn rejects_non_loopback_oauth_redirect_uri() {
    assert!(normalize_oauth_redirect_uri("https://127.0.0.1:8765/callback").is_err());
    assert!(normalize_oauth_redirect_uri("http://example.com:8765/callback").is_err());
    assert!(normalize_oauth_redirect_uri("http://127.0.0.1/callback").is_err());
}

#[test]
fn oauth_callback_result_requires_matching_state() {
    let mut params = HashMap::new();
    params.insert("state".to_string(), "expected".to_string());
    params.insert("code".to_string(), "auth-code".to_string());

    assert_eq!(
        oauth_callback_result(&params, "expected").unwrap(),
        "auth-code"
    );
    assert!(oauth_callback_result(&params, "other").is_err());
}
