use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
};
use tower::ServiceExt;

use super::transport_spike::{cors_probe_router, DebugCorsProbeOrdering, DEBUG_CORS_PROBE_ORIGIN};

fn preflight_request(origin: &'static str) -> Request<Body> {
    Request::builder()
        .method(Method::OPTIONS)
        .uri("/remote/v1/invoke")
        .header(header::ORIGIN, origin)
        .header(header::ACCESS_CONTROL_REQUEST_METHOD, Method::POST.as_str())
        .header(
            header::ACCESS_CONTROL_REQUEST_HEADERS,
            "authorization,content-type",
        )
        .body(Body::empty())
        .expect("preflight request should be valid")
}

#[tokio::test]
async fn auth_before_options_rejects_preflight_without_cors_headers() {
    let response = cors_probe_router(DebugCorsProbeOrdering::AuthBeforeOptions)
        .oneshot(preflight_request(DEBUG_CORS_PROBE_ORIGIN))
        .await
        .expect("probe router should respond");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(response
        .headers()
        .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
        .is_none());
}

#[tokio::test]
async fn options_before_auth_returns_a_restrictive_preflight_response() {
    let response = cors_probe_router(DebugCorsProbeOrdering::OptionsBeforeAuth)
        .oneshot(preflight_request(DEBUG_CORS_PROBE_ORIGIN))
        .await
        .expect("probe router should respond");

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .expect("preflight should name the fixed development origin"),
        DEBUG_CORS_PROBE_ORIGIN
    );
    assert_eq!(
        response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_METHODS)
            .expect("preflight should permit the probe method"),
        "POST"
    );
}

#[tokio::test]
async fn options_before_auth_never_reflects_an_unlisted_origin() {
    let unlisted_origin = "https://unlisted.example";
    let response = cors_probe_router(DebugCorsProbeOrdering::OptionsBeforeAuth)
        .oneshot(preflight_request(unlisted_origin))
        .await
        .expect("probe router should respond");

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_ne!(
        response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .expect("preflight should retain the fixed allowlist"),
        unlisted_origin
    );
}
