use std::sync::Arc;

use async_trait::async_trait;
use axum::{
    body::to_bytes,
    body::Body,
    http::{header, Method, Request, StatusCode},
    Router,
};
use ralphx_remote_protocol::{ErrorCode, RiskClass, Scope};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tower::ServiceExt;

use super::auth::RemoteAuthContext;
use super::auth_tests::{in_memory_auth_context, pair_device_with_scopes, TEST_ENVIRONMENT_ID};
use super::endpoints::RemoteRouterState;
use super::invoke::{
    dispatch_outcome_response, invoke_error_response, status_for_error_code, RemoteInvokeDispatcher,
};
use super::registry::{self, enforce_scope, DispatchOutcome, RemoteCommandSpec, RemoteInvokeError};
use super::{authenticated_remote_routes, INVOKE_PATH};
use crate::domain::entities::RemoteScopeSet;

/// Drives the PRODUCTION `registry::dispatch` — and therefore the production
/// `enforce_scope` — from the router.
///
/// `auth_tests` installs `UnavailableInvokeDispatcher`, so before this existed no
/// router-level test executed the authorization gate at all: regressing `enforce_scope`
/// to a no-op passed the entire branch.
struct RegistryInvokeDispatcher {
    app: tauri::AppHandle<tauri::test::MockRuntime>,
}

#[async_trait]
impl RemoteInvokeDispatcher for RegistryInvokeDispatcher {
    async fn dispatch(
        &self,
        scopes: &[Scope],
        command: &str,
        args: &Value,
    ) -> Result<DispatchOutcome, RemoteInvokeError> {
        registry::dispatch(&self.app, scopes, command, args).await
    }
}

fn router_with_real_registry(
    context: &RemoteAuthContext,
    app: tauri::AppHandle<tauri::test::MockRuntime>,
) -> Router {
    authenticated_remote_routes(RemoteRouterState::new_with_invoke_dispatcher(
        TEST_ENVIRONMENT_ID,
        context.clone(),
        Arc::new(RegistryInvokeDispatcher { app }),
    ))
}

fn invoke_request(token: &str, cmd: &str, args: Value) -> Request<Body> {
    Request::builder()
        .method(Method::POST)
        .uri(INVOKE_PATH)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::from(
            json!({"requestId": uuid::Uuid::new_v4().to_string(), "cmd": cmd, "args": args})
                .to_string(),
        ))
        .expect("request should build")
}

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
        (ErrorCode::RemoteInvalidArguments, StatusCode::BAD_REQUEST),
        (
            ErrorCode::RemoteInternalError,
            StatusCode::INTERNAL_SERVER_ERROR,
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

// ---------------------------------------------------------------------------------------
// Error taxonomy: malformed arguments are NOT "command unavailable"
// ---------------------------------------------------------------------------------------

#[test]
fn malformed_arguments_do_not_masquerade_as_an_unsupported_command() {
    // `RemoteCommandUnavailable` is what the client reads as "this host does not support the
    // command", a terminal signal about to gate remote affordances. An argument error must
    // never produce it.
    let error = registry::extract_arg::<String>(&json!({"project_id": 17}), "project_id")
        .expect_err("a type-mismatched argument must fail");
    assert_eq!(error.code, ErrorCode::RemoteInvalidArguments);
    assert_eq!(
        status_for_error_code(error.code),
        StatusCode::BAD_REQUEST,
        "argument errors are a 4xx distinct from the 404 unavailable envelope"
    );

    let missing = registry::extract_arg::<String>(&json!({}), "project_id")
        .expect_err("a missing required argument must fail");
    assert_eq!(missing.code, ErrorCode::RemoteInvalidArguments);

    // Only a `find_spec` miss keeps the 404 code.
    let unavailable = RemoteInvokeError::unavailable("no_such_command");
    assert_eq!(unavailable.code, ErrorCode::RemoteCommandUnavailable);
    assert_eq!(
        status_for_error_code(unavailable.code),
        StatusCode::NOT_FOUND
    );
}

#[test]
fn a_response_that_will_not_serialize_is_a_host_fault_not_an_unavailable_command() {
    #[derive(Debug)]
    struct Unserializable;
    impl serde::Serialize for Unserializable {
        fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
            Err(serde::ser::Error::custom("nope"))
        }
    }

    let error = registry::serialize_ok(Unserializable).expect_err("serialization must fail");
    assert_eq!(error.code, ErrorCode::RemoteInternalError);
    assert_eq!(
        status_for_error_code(error.code),
        StatusCode::INTERNAL_SERVER_ERROR
    );
}

// ---------------------------------------------------------------------------------------
// enforce_scope — negative coverage (the facade's only authorization gate)
// ---------------------------------------------------------------------------------------

fn spec_with(class: RiskClass, authz: Option<registry::AuthzPredicate>) -> RemoteCommandSpec {
    RemoteCommandSpec {
        name: "fixture",
        target: "fixture_target",
        class,
        capabilities: &[],
        authz,
        validate: None,
        pins: &[],
    }
}

#[test]
fn enforce_scope_refuses_every_insufficient_grant() {
    // Class scope missing entirely.
    for (class, sufficient) in [
        (RiskClass::Read, Scope::UiRead),
        (RiskClass::Operate, Scope::UiOperate),
        (RiskClass::PathScoped, Scope::UiOperate),
        (RiskClass::AgentControl, Scope::UiAgent),
        (RiskClass::Elevated, Scope::UiElevated),
    ] {
        let spec = spec_with(class, None);
        let refused =
            enforce_scope(&spec, &[], &json!({})).expect_err("an empty grant authorizes nothing");
        assert_eq!(refused.code, ErrorCode::RemoteForbidden, "{class:?}");

        // Every OTHER scope is also insufficient — holding one scope never implies another.
        for wrong in [
            Scope::UiRead,
            Scope::UiOperate,
            Scope::UiAgent,
            Scope::UiElevated,
        ] {
            if wrong == sufficient {
                continue;
            }
            assert!(
                enforce_scope(&spec, &[wrong], &json!({})).is_err(),
                "{class:?} must not be authorized by {wrong:?}"
            );
        }
        enforce_scope(&spec, &[sufficient], &json!({})).expect("the exact class scope authorizes");
    }

    // Denied is unauthorizable by every grant, including the full set.
    let denied = spec_with(RiskClass::Denied, None);
    assert!(enforce_scope(
        &denied,
        &[
            Scope::UiRead,
            Scope::UiOperate,
            Scope::UiAgent,
            Scope::UiElevated
        ],
        &json!({})
    )
    .is_err());
}

#[test]
fn enforce_scope_applies_the_argument_sensitive_predicate_above_the_class_scope() {
    let spec = spec_with(RiskClass::Operate, Some(registry::update_task_authz));

    // Content-free update: the class scope is enough.
    enforce_scope(
        &spec,
        &[Scope::UiOperate],
        &json!({"input": {"priority": 3}}),
    )
    .expect("a non-content update needs only ui:operate");

    // Title/description are deferred spawn authority and demand ui:agent even though the
    // command's class scope was already satisfied.
    for field in ["title", "description"] {
        let args = json!({"input": {field: "poisoned"}});
        let refused = enforce_scope(&spec, &[Scope::UiOperate], &args)
            .expect_err("content writes must escalate past the class scope");
        assert_eq!(refused.code, ErrorCode::RemoteForbidden);
        enforce_scope(&spec, &[Scope::UiOperate, Scope::UiAgent], &args)
            .expect("ui:agent authorizes the content write");
    }
}

#[tokio::test]
async fn the_router_refuses_a_registered_command_the_device_lacks_scope_for() {
    let context = in_memory_auth_context();
    let (token, _) = pair_device_with_scopes(
        &context,
        "operate-only",
        RemoteScopeSet::from_scopes([Scope::UiOperate]),
    )
    .await;
    let app = crate::testing::create_mock_app();

    let response = router_with_real_registry(&context, app.handle().clone())
        .oneshot(invoke_request(
            &token,
            "list_tasks",
            json!({"projectId": "p1"}),
        ))
        .await
        .expect("invoke request should complete");

    assert_eq!(
        response.status(),
        StatusCode::FORBIDDEN,
        "a ui:operate device must not reach a ui:read command"
    );
    assert_eq!(
        response_json(response).await["code"],
        json!(ErrorCode::RemoteForbidden)
    );
}

#[tokio::test]
async fn the_router_answers_an_unregistered_command_with_the_unavailable_envelope() {
    let context = in_memory_auth_context();
    let (token, _) = pair_device_with_scopes(
        &context,
        "full",
        RemoteScopeSet::from_scopes([Scope::UiRead, Scope::UiOperate, Scope::UiAgent]),
    )
    .await;
    let app = crate::testing::create_mock_app();

    // `list_projects` is ledgered Elevated and deliberately unregistered.
    let response = router_with_real_registry(&context, app.handle().clone())
        .oneshot(invoke_request(&token, "list_projects", json!({})))
        .await
        .expect("invoke request should complete");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        response_json(response).await["code"],
        json!(ErrorCode::RemoteCommandUnavailable)
    );
}

#[tokio::test]
async fn the_router_admits_a_registered_command_the_device_does_hold_scope_for() {
    let context = in_memory_auth_context();
    let (token, _) = pair_device_with_scopes(
        &context,
        "reader",
        RemoteScopeSet::from_scopes([Scope::UiRead]),
    )
    .await;
    let app = crate::testing::create_mock_app();

    let response = router_with_real_registry(&context, app.handle().clone())
        .oneshot(invoke_request(&token, "health_check", json!({})))
        .await
        .expect("invoke request should complete");

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "the positive path must still work, or the negatives above prove nothing"
    );
    assert_eq!(
        response_json(response).await,
        json!({"ok": true, "result": {"status": "ok"}})
    );
}

// ---------------------------------------------------------------------------------------
// PR 3.1-b batch 1 — the 2.7 pending-gate reads (`ui:read`)
//
// `list_pending_permission_gates` / `list_pending_question_gates` are the reconnect
// rehydration reads the 2.7 lane added. Without them registered, a remote client that
// reconnects mid-gate has no way to learn a gate is open: `pending-gate-reconcile.ts`
// treats `REMOTE_COMMAND_UNAVAILABLE` as "cannot reconcile". Registering them at `Read` is
// the flag-on precondition for remote P-21.
// ---------------------------------------------------------------------------------------

/// A mock app whose managed `AppState` carries the live permission/question gate state.
///
/// `create_mock_app` manages nothing, so a dispatch through it would fail for the wrong
/// reason — the parity rows below would then prove nothing about the gate state.
fn app_with_gate_state() -> tauri::App<tauri::test::MockRuntime> {
    tauri::test::mock_builder()
        .manage(crate::application::AppState::new_test())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app should build")
}

const GATE_READS: [&str; 2] = [
    "list_pending_permission_gates",
    "list_pending_question_gates",
];

/// P-4 parity — the dispatched payload is byte-identical to the direct local IPC call, with
/// a NON-EMPTY gate registered so an all-empty response cannot fake the equality.
#[tokio::test]
async fn p4_parity_for_the_pending_gate_reads_over_a_populated_state() {
    use tauri::Manager;

    let app = app_with_gate_state();
    let state = app.state::<crate::application::AppState>();

    state
        .permission_state
        .register(crate::application::PendingPermissionInfo {
            request_id: "gate-permission-1".to_string(),
            tool_name: "mcp__ralphx__get_task_context".to_string(),
            tool_input: json!({"task_id": "task-1"}),
            context: Some("Needs task context".to_string()),
            agent_type: Some("worker".to_string()),
            task_id: Some("task-1".to_string()),
            context_type: Some("task".to_string()),
            context_id: Some("task-1".to_string()),
            created_at: chrono::Utc::now().to_rfc3339(),
        })
        .await;
    state
        .question_state
        .register(
            "gate-question-1".to_string(),
            "22222222-2222-2222-2222-222222222222".to_string(),
            "Continue?".to_string(),
            None,
            vec![],
            false,
        )
        .await;

    let direct_permissions = crate::commands::permission_commands::list_pending_permission_gates(
        app.state::<crate::application::AppState>(),
    )
    .await
    .expect("the direct permission-gate read succeeds");
    let direct_questions = crate::commands::question_commands::list_pending_question_gates(
        app.state::<crate::application::AppState>(),
    )
    .await
    .expect("the direct question-gate read succeeds");

    assert_eq!(
        direct_permissions.len(),
        1,
        "the fixture must be non-empty, or parity over an empty list proves nothing"
    );
    assert_eq!(
        direct_questions.len(),
        1,
        "question fixture must be non-empty"
    );

    for (command, direct) in [
        (
            "list_pending_permission_gates",
            registry::serialize_ok(&direct_permissions).unwrap(),
        ),
        (
            "list_pending_question_gates",
            registry::serialize_ok(&direct_questions).unwrap(),
        ),
    ] {
        let dispatched = registry::dispatch(app.handle(), &[Scope::UiRead], command, &json!({}))
            .await
            .unwrap_or_else(|error| panic!("`{command}` must dispatch: {error:?}"));
        assert_eq!(
            dispatched,
            DispatchOutcome::Ok(direct),
            "`{command}` remote dispatch must be byte-identical to the local IPC call"
        );
    }
}

/// Both take no arguments — an argument-bearing call must not become an unavailable command,
/// and extra client-supplied keys must be ignored rather than smuggled in.
#[tokio::test]
async fn the_pending_gate_reads_ignore_client_supplied_arguments() {
    let app = app_with_gate_state();

    for command in GATE_READS {
        let dispatched = registry::dispatch(
            app.handle(),
            &[Scope::UiRead],
            command,
            &json!({"projectId": "smuggled", "limit": 9000}),
        )
        .await
        .unwrap_or_else(|error| panic!("`{command}` must ignore surplus args: {error:?}"));
        assert_eq!(dispatched, DispatchOutcome::Ok(json!([])));
    }
}

/// Scope negative — the gate reads sit at `Read`, so every OTHER scope is insufficient and
/// an empty grant authorizes neither.
#[tokio::test]
async fn the_pending_gate_reads_are_refused_below_ui_read() {
    let app = app_with_gate_state();

    for command in GATE_READS {
        let spec = registry::find_spec(command)
            .unwrap_or_else(|| panic!("`{command}` must be registered on the facade"));
        assert_eq!(
            spec.class,
            RiskClass::Read,
            "`{command}` is a pure in-memory/repository gate read"
        );
        assert!(
            spec.capabilities.is_empty(),
            "`{command}` carries no capability; a Read row with capabilities is a mislabel"
        );

        for insufficient in [
            vec![],
            vec![Scope::UiOperate],
            vec![Scope::UiAgent],
            vec![Scope::UiElevated],
            vec![Scope::UiOperate, Scope::UiAgent, Scope::UiElevated],
        ] {
            let refused = registry::dispatch(app.handle(), &insufficient, command, &json!({}))
                .await
                .expect_err("a grant without ui:read must not reach the gate read");
            assert_eq!(
                refused.code,
                ErrorCode::RemoteForbidden,
                "`{command}` under {insufficient:?} must be FORBIDDEN, not unavailable"
            );
        }
    }
}

/// The router-level admission both 2.7 clients actually take.
#[tokio::test]
async fn a_ui_read_device_reaches_the_pending_gate_reads_over_the_router() {
    let context = in_memory_auth_context();
    let (token, _) = pair_device_with_scopes(
        &context,
        "gate-reader",
        RemoteScopeSet::from_scopes([Scope::UiRead]),
    )
    .await;

    for command in GATE_READS {
        let app = app_with_gate_state();
        let response = router_with_real_registry(&context, app.handle().clone())
            .oneshot(invoke_request(&token, command, json!({})))
            .await
            .expect("invoke request should complete");

        assert_eq!(
            response.status(),
            StatusCode::OK,
            "`{command}` must be reachable for a ui:read device"
        );
        assert_eq!(
            response_json(response).await,
            json!({"ok": true, "result": []}),
            "`{command}` returns the empty gate list on a fresh host"
        );
    }
}
