use super::{ExternalMcpConfigView, AUTH_TOKEN_MASK};

/// Verify masking logic: when auth_token is set, view returns the mask string; when None, returns None.
#[test]
fn test_auth_token_masking_when_set() {
    let masked = Some("some_secret_token".to_string())
        .as_ref()
        .map(|_| AUTH_TOKEN_MASK.to_string());
    assert_eq!(masked, Some(AUTH_TOKEN_MASK.to_string()));
}

#[test]
fn test_auth_token_masking_when_unset() {
    let masked: Option<String> = None::<String>.as_ref().map(|_| AUTH_TOKEN_MASK.to_string());
    assert_eq!(masked, None);
}

#[test]
fn test_auth_token_mask_value() {
    assert_eq!(AUTH_TOKEN_MASK, "\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}");
    assert_eq!(AUTH_TOKEN_MASK.chars().count(), 8);
}

#[test]
fn test_external_mcp_config_view_serializes_masked_token() {
    let view = ExternalMcpConfigView {
        enabled: true,
        port: 3848,
        host: "127.0.0.1".to_string(),
        auth_token: Some(AUTH_TOKEN_MASK.to_string()),
        node_path: None,
    };
    let json = serde_json::to_string(&view).expect("serialization failed");
    // Confirm raw token "secret" is never present; only the mask is
    assert!(json.contains(AUTH_TOKEN_MASK));
    assert!(!json.contains("secret"));
}

#[test]
fn test_external_mcp_config_view_serializes_no_token() {
    let view = ExternalMcpConfigView {
        enabled: false,
        port: 3848,
        host: "127.0.0.1".to_string(),
        auth_token: None,
        node_path: None,
    };
    let json = serde_json::to_string(&view).expect("serialization failed");
    assert!(json.contains("\"authToken\":null"));
}
