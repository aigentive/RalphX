use axum::{body::to_bytes, http::StatusCode};
use ralphx_remote_protocol::{ErrorCode, Scope};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::invoke::{dispatch_outcome_response, invoke_error_response, status_for_error_code};
use super::registry::{self, DispatchOutcome, RemoteInvokeError};

#[derive(Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct WrappedInput {
    project_id: String,
    enabled: Option<bool>,
}

async fn response_json(response: axum::response::Response) -> Value {
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&body).unwrap()
}

#[test]
fn p4_argument_and_serialization_parity_covers_wire_shapes() {
    let flat = json!({"project_id": "flat"});
    let flat_value: String = registry::extract_arg(&flat, "project_id").unwrap();
    assert_eq!(registry::serialize_ok(&flat_value).unwrap(), json!("flat"));

    let camel = json!({"projectId": "camel"});
    let camel_value: String = registry::extract_arg(&camel, "project_id").unwrap();
    assert_eq!(
        registry::serialize_ok(&camel_value).unwrap(),
        json!("camel")
    );

    let wrapped = json!({"input": {"projectId": "wrapped"}});
    let wrapped_value: WrappedInput = registry::extract_arg(&wrapped, "input").unwrap();
    let direct = WrappedInput {
        project_id: "wrapped".to_string(),
        enabled: None,
    };
    assert_eq!(wrapped_value, direct);
    assert_eq!(
        registry::serialize_ok(&wrapped_value).unwrap(),
        serde_json::to_value(&direct).unwrap()
    );

    let absent_optional: Option<bool> = registry::extract_arg(&json!({}), "enabled").unwrap();
    assert_eq!(absent_optional, None);
    assert_eq!(registry::camel_case("include_archived"), "includeArchived");
}

#[tokio::test]
async fn command_error_stays_a_2xx_business_result() {
    let response = dispatch_outcome_response(DispatchOutcome::Err(json!({"kind": "rejected"})));
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response_json(response).await,
        json!({"ok": false, "error": {"kind": "rejected"}})
    );
}

#[tokio::test]
async fn successful_dispatch_uses_the_ok_result_envelope() {
    let app = crate::testing::create_mock_app();
    let direct = crate::commands::health::health_check();
    let dispatched = registry::dispatch(app.handle(), &[Scope::UiRead], "health_check", &json!({}))
        .await
        .unwrap();
    assert_eq!(
        dispatched,
        DispatchOutcome::Ok(registry::serialize_ok(direct).unwrap())
    );

    let response = dispatch_outcome_response(dispatched);
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response_json(response).await,
        json!({"ok": true, "result": {"status": "ok"}})
    );
}

#[tokio::test]
async fn facade_errors_are_non_2xx_with_typed_code_and_message() {
    let cases = [
        (ErrorCode::RemoteUnauthorized, StatusCode::UNAUTHORIZED),
        (ErrorCode::RemoteForbidden, StatusCode::FORBIDDEN),
        (ErrorCode::RemoteCommandUnavailable, StatusCode::NOT_FOUND),
        (ErrorCode::RemoteTimeoutUnknown, StatusCode::REQUEST_TIMEOUT),
        (ErrorCode::RemoteRequestInProgress, StatusCode::CONFLICT),
        (
            ErrorCode::RemoteRequestIdReused,
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            ErrorCode::RemoteVersionMismatch,
            StatusCode::UPGRADE_REQUIRED,
        ),
        (
            ErrorCode::RemoteUnreachable,
            StatusCode::SERVICE_UNAVAILABLE,
        ),
    ];

    for (code, status) in cases {
        assert_eq!(status_for_error_code(code), status);
        let response = invoke_error_response(RemoteInvokeError {
            code,
            message: "mapped message".to_string(),
        });
        assert_eq!(response.status(), status);
        assert_eq!(
            response_json(response).await,
            json!({"code": code, "message": "mapped message"})
        );
    }
}
