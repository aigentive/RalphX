use crate::application::AtlassianApiError;

#[test]
fn rate_limiting_is_classified_by_status_not_message_text() {
    let error = AtlassianApiError::from_status(429, "{\"message\":\"Rate limit exceeded\"}");

    assert_eq!(error.status, Some(429));
    assert!(error.is_rate_limited());
    assert!(!error.is_not_found());
    assert!(!error.is_auth_failure());
}

#[test]
fn not_found_and_auth_failures_carry_their_numeric_status() {
    let not_found = AtlassianApiError::from_status(404, "Issue does not exist");
    assert_eq!(not_found.status, Some(404));
    assert!(not_found.is_not_found());

    for code in [401, 403] {
        let denied = AtlassianApiError::from_status(code, "");
        assert_eq!(denied.status, Some(code));
        assert!(denied.is_auth_failure());
    }
}

#[test]
fn transport_failures_have_no_status_so_they_never_read_as_success() {
    let error = AtlassianApiError::transport("Atlassian request timed out");

    assert_eq!(error.status, None);
    assert!(!error.is_rate_limited());
    assert!(!error.is_not_found());
    assert!(!error.is_auth_failure());
    assert_eq!(error.to_string(), "Atlassian request timed out");
}

#[test]
fn status_errors_render_the_status_and_a_body_excerpt() {
    let error = AtlassianApiError::from_status(400, "  field 'summary' is required  ");
    assert_eq!(
        error.to_string(),
        "Atlassian returned HTTP 400: field 'summary' is required"
    );

    let empty_body = AtlassianApiError::from_status(500, "   ");
    assert_eq!(empty_body.to_string(), "Atlassian returned HTTP 500");
}

#[test]
fn body_excerpts_are_bounded_and_split_on_char_boundaries() {
    let long_body = "é".repeat(1_000);
    let error = AtlassianApiError::from_status(500, &long_body);

    // 512-byte cap; the excerpt must remain valid UTF-8 rather than panicking.
    assert!(error.message.len() <= "Atlassian returned HTTP 500: ".len() + 512);
    assert!(error.message.starts_with("Atlassian returned HTTP 500: é"));
}

#[test]
fn converting_to_string_preserves_the_rendered_message_for_legacy_callers() {
    let message: String = AtlassianApiError::from_status(404, "").into();
    assert_eq!(message, "Atlassian returned HTTP 404");
}
