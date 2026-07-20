use axum::http::{HeaderMap, HeaderValue, StatusCode};

use super::team_artifacts::artifact_author;

#[test]
fn artifact_author_uses_validated_canonical_transport_identity() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-ralphx-agent-type",
        HeaderValue::from_static("ralphx-ideation-specialist-backend"),
    );

    assert_eq!(
        artifact_author(&headers).expect("canonical caller should be accepted"),
        "ralphx-ideation-specialist-backend"
    );
}

#[test]
fn artifact_author_falls_back_to_system_without_transport_identity() {
    assert_eq!(
        artifact_author(&HeaderMap::new()).expect("non-MCP callers use system attribution"),
        "system"
    );
}

#[test]
fn artifact_author_rejects_unknown_transport_identity() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-ralphx-agent-type",
        HeaderValue::from_static("unknown-agent"),
    );

    let (status, message) = artifact_author(&headers).expect_err("unknown caller must fail closed");
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(message.contains("Unknown canonical caller agent"));
}
