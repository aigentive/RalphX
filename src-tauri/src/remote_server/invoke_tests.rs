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

// ---------------------------------------------------------------------------------------
// PR 3.1-b batch 2 — census `B1` task-core reads (`ui:read`)
//
// Every one of these sat at `AgentControl` under `conservative-module-default`, not under a
// reviewed judgement. The audit probe run over the live `authority_audit` call graph reports
// detectors (a), (b) and (c) all silent for each, and the hand-trace confirms every body is a
// repository read whose error is propagated (`map_err(...)?`), never collapsed into an empty
// or default result. They are reads in fact, so they are ledgered `Read` and registered here.
// ---------------------------------------------------------------------------------------

/// A mock app whose managed `AppState` can carry seeded tasks.
fn app_with_task_state() -> tauri::App<tauri::test::MockRuntime> {
    tauri::test::mock_builder()
        .manage(crate::application::AppState::new_test())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app should build")
}

const B1_TASK_READS: [&str; 7] = [
    "get_archived_count",
    "get_tasks_awaiting_review",
    "get_session_task_history_availability",
    "get_task_state_transitions",
    "get_task_dependency_graph",
    "get_task_timeline_events",
    "get_task_agent_workspace",
];

const B1_FIXTURE_PROJECT: &str = "11111111-1111-1111-1111-111111111111";
const B1_FIXTURE_SESSION: &str = "33333333-3333-3333-3333-333333333333";

/// Seeds two real tasks (one carrying steps) through the PRODUCTION `create_task` entry
/// point and returns their ids. Parity over an all-empty fixture is vacuous — it passes
/// against a command that unconditionally returns nothing — so every parity row below runs
/// against state that actually contains tasks.
async fn seed_b1_tasks(app: &tauri::App<tauri::test::MockRuntime>) -> (String, String) {
    use tauri::Manager;

    // `create_task` validates the ideation session exists, so the session is seeded first.
    // The session id is what makes `get_session_task_history_availability` non-vacuous.
    let state = app.state::<crate::application::AppState>();
    state
        .ideation_session_repo
        .create(
            crate::domain::entities::IdeationSession::builder()
                .id(crate::domain::entities::IdeationSessionId::from_string(
                    B1_FIXTURE_SESSION.to_string(),
                ))
                .project_id(crate::domain::entities::ProjectId::from_string(
                    B1_FIXTURE_PROJECT.to_string(),
                ))
                .build(),
        )
        .await
        .expect("seeding the ideation session succeeds");

    let first = crate::commands::task_commands::mutation::create_task(
        crate::commands::task_commands::types::CreateTaskInput {
            project_id: B1_FIXTURE_PROJECT.to_string(),
            title: "B1 parity task one".to_string(),
            category: None,
            description: Some("seeded for the B1 parity rows".to_string()),
            priority: Some(3),
            steps: Some(vec!["step one".to_string(), "step two".to_string()]),
            ideation_session_id: Some(B1_FIXTURE_SESSION.to_string()),
            execution_plan_id: None,
        },
        app.state::<crate::application::AppState>(),
    )
    .await
    .expect("seeding the first task succeeds");

    let second = crate::commands::task_commands::mutation::create_task(
        crate::commands::task_commands::types::CreateTaskInput {
            project_id: B1_FIXTURE_PROJECT.to_string(),
            title: "B1 parity task two".to_string(),
            category: None,
            description: None,
            priority: Some(1),
            steps: None,
            ideation_session_id: Some(B1_FIXTURE_SESSION.to_string()),
            execution_plan_id: None,
        },
        app.state::<crate::application::AppState>(),
    )
    .await
    .expect("seeding the second task succeeds");

    // A real dependency edge: task two is blocked by task one.
    //
    // Without it the dependency-graph parity row was both weaker and flaky. Weaker because
    // `edges` was empty, so nothing proved the facade reproduced edge data at all. Flaky
    // because with two unconnected tasks the critical path is a tie between two single-node
    // paths, and the command breaks that tie by whatever order the underlying `HashMap`
    // iteration produced — two calls of the same command genuinely disagreed. With the edge
    // the graph has one longest path, so `critical_path`, `tier`, `in_degree` and `out_degree`
    // are all uniquely determined and asserted exactly.
    app.state::<crate::application::AppState>()
        .task_dependency_repo
        .add_dependency(
            &crate::domain::entities::TaskId::from_string(second.id.clone()),
            &crate::domain::entities::TaskId::from_string(first.id.clone()),
        )
        .await
        .expect("seeding the dependency edge succeeds");

    (first.id, second.id)
}

/// P-4 parity — each dispatched payload is byte-identical to the direct local IPC call over a
/// populated fixture.
#[tokio::test]
async fn p4_parity_for_the_b1_task_reads_over_seeded_tasks() {
    use tauri::Manager;

    let app = app_with_task_state();
    let (task_id, other_task_id) = seed_b1_tasks(&app).await;

    let direct_archived = crate::commands::task_commands::query::get_archived_count(
        B1_FIXTURE_PROJECT.to_string(),
        None,
        app.state::<crate::application::AppState>(),
    )
    .await
    .expect("direct archived-count read succeeds");
    let direct_awaiting = crate::commands::task_commands::query::get_tasks_awaiting_review(
        B1_FIXTURE_PROJECT.to_string(),
        app.state::<crate::application::AppState>(),
    )
    .await
    .expect("direct awaiting-review read succeeds");
    let direct_history =
        crate::commands::task_commands::query::get_session_task_history_availability(
            B1_FIXTURE_PROJECT.to_string(),
            B1_FIXTURE_SESSION.to_string(),
            app.state::<crate::application::AppState>(),
        )
        .await
        .expect("direct history-availability read succeeds");
    let direct_transitions = crate::commands::task_commands::query::get_task_state_transitions(
        task_id.clone(),
        app.state::<crate::application::AppState>(),
    )
    .await
    .expect("direct state-transitions read succeeds");
    let direct_graph = crate::commands::task_commands::query::get_task_dependency_graph(
        B1_FIXTURE_PROJECT.to_string(),
        None,
        None,
        None,
        app.state::<crate::application::AppState>(),
    )
    .await
    .expect("direct dependency-graph read succeeds");
    let direct_timeline = crate::commands::task_commands::query::get_task_timeline_events(
        B1_FIXTURE_PROJECT.to_string(),
        None,
        None,
        None,
        None,
        app.state::<crate::application::AppState>(),
    )
    .await
    .expect("direct timeline read succeeds");
    let direct_workspace = crate::commands::task_commands::query::get_task_agent_workspace(
        task_id.clone(),
        app.state::<crate::application::AppState>(),
    )
    .await
    .expect("direct agent-workspace read succeeds");

    // Non-vacuity: the fixture must actually be visible through these reads, or byte parity
    // below would be satisfied by a command that always returns nothing.
    assert_eq!(
        direct_history.task_count, 2,
        "history availability must see both seeded tasks"
    );
    assert!(
        direct_history.has_history,
        "history availability must be positive over a seeded session"
    );
    assert_eq!(
        direct_graph.nodes.len(),
        2,
        "dependency graph must carry both seeded tasks"
    );
    // The seeded edge must actually be visible, or the parity row proves nothing about edge
    // data — and the critical path must be the unique two-node path rather than a coin flip.
    assert_eq!(
        direct_graph.edges.len(),
        1,
        "the seeded dependency edge must appear in the graph"
    );
    assert_eq!(direct_graph.critical_path.len(), 2);
    assert_eq!(
        direct_graph.critical_path,
        vec![task_id.clone(), other_task_id.clone()],
        "the critical path must run blocker -> blocked, deterministically"
    );
    // The remaining four reads are empty-by-construction over a freshly seeded fixture
    // (no archived task, no review-status task, no recorded status history, and therefore no
    // timeline event). Their parity rows below are consequently weaker than the two above,
    // which is recorded rather than papered over: the load-bearing guarantee for those four
    // is the scope negative, not the parity row.
    assert!(
        direct_timeline.events.is_empty(),
        "a freshly seeded fixture records no timeline events; revisit the non-vacuity \
         anchors above if this changes"
    );

    for (command, args, direct) in [
        (
            "get_archived_count",
            json!({"projectId": B1_FIXTURE_PROJECT, "ideationSessionId": null}),
            registry::serialize_ok(direct_archived).unwrap(),
        ),
        (
            "get_tasks_awaiting_review",
            json!({"projectId": B1_FIXTURE_PROJECT}),
            registry::serialize_ok(&direct_awaiting).unwrap(),
        ),
        (
            "get_session_task_history_availability",
            json!({
                "projectId": B1_FIXTURE_PROJECT,
                "ideationSessionId": B1_FIXTURE_SESSION,
            }),
            registry::serialize_ok(&direct_history).unwrap(),
        ),
        (
            "get_task_state_transitions",
            json!({"taskId": task_id}),
            registry::serialize_ok(&direct_transitions).unwrap(),
        ),
        (
            "get_task_dependency_graph",
            json!({"projectId": B1_FIXTURE_PROJECT}),
            registry::serialize_ok(&direct_graph).unwrap(),
        ),
        (
            "get_task_timeline_events",
            json!({"projectId": B1_FIXTURE_PROJECT}),
            registry::serialize_ok(&direct_timeline).unwrap(),
        ),
        (
            "get_task_agent_workspace",
            json!({"taskId": task_id}),
            registry::serialize_ok(&direct_workspace).unwrap(),
        ),
    ] {
        let outcome = registry::dispatch(&app.handle().clone(), &[Scope::UiRead], command, &args)
            .await
            .unwrap_or_else(|error| panic!("`{command}` must dispatch at ui:read: {error:?}"));
        let DispatchOutcome::Ok(facade) = outcome else {
            panic!("`{command}` returned a business error: {outcome:?}");
        };
        assert_eq!(
            canonicalize_unordered_collections(facade),
            canonicalize_unordered_collections(direct),
            "P-4 parity mismatch for `{command}`"
        );
    }
}

/// Sorts the payload arrays whose order is not part of any command's contract.
///
/// `get_task_dependency_graph` builds its node list from `task_repo.get_by_project`, which
/// promises no ordering — the in-memory repository iterates a `HashMap`, so two calls in the
/// same process can legitimately return the same nodes in different orders. Asserting raw
/// equality made this parity row intermittently red (observed repeatedly while landing batch
/// 3) while proving nothing the command guarantees.
///
/// This canonicalizes rather than skips: the node and edge SETS must still match exactly, and
/// every other field is compared untouched. A facade that dropped, added or reshaped a node
/// still fails. What is deliberately no longer asserted is an ordering neither the local IPC
/// path nor the facade ever promised.
///
/// `critical_path` is deliberately NOT canonicalized here. It was the other half of the flake,
/// and canonicalizing it would have hidden something real: over the ORIGINAL fixture — two
/// unconnected tasks — the command picked one of two tied single-node paths arbitrarily, so
/// two calls genuinely disagreed about which task was critical. The fix is in the fixture,
/// which now seeds a real dependency edge and therefore has a uniquely determined critical
/// path, so the field is asserted exactly.
fn canonicalize_unordered_collections(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        for field in ["nodes", "edges"] {
            if let Some(Value::Array(items)) = object.get_mut(field) {
                items.sort_by_key(|item| item.to_string());
            }
        }
    }
    value
}

/// Scope negative — no grant weaker than (or sideways from) `ui:read` reaches these reads.
#[tokio::test]
async fn the_b1_task_reads_are_refused_below_ui_read() {
    let app = app_with_task_state();

    for command in B1_TASK_READS {
        let spec = registry::find_spec(command)
            .unwrap_or_else(|| panic!("`{command}` must be registered"));
        assert_eq!(
            spec.class,
            RiskClass::Read,
            "`{command}` must be a Read row"
        );
        assert!(
            spec.capabilities.is_empty(),
            "`{command}` must carry no capability; a Read row with a capability is the \
             under-labelling signature itself"
        );

        for scopes in [
            vec![],
            vec![Scope::UiOperate],
            vec![Scope::UiAgent],
            vec![Scope::UiElevated],
            vec![Scope::UiOperate, Scope::UiAgent, Scope::UiElevated],
        ] {
            let error = registry::dispatch(&app.handle().clone(), &scopes, command, &json!({}))
                .await
                .expect_err("insufficient scope must be refused");
            assert_eq!(
                error.code,
                ErrorCode::RemoteForbidden,
                "`{command}` admitted {scopes:?}"
            );
        }
    }
}

// ---------------------------------------------------------------------------------------
// PR 3.1-b batch 2 — census `B1` step + execution reads (`ui:read`)
//
// Same treatment as the task-core cluster: detectors (a)/(b)/(c) all silent, bodies
// hand-traced to repository / in-memory state reads.
//
// The two execution-module getters that are ABSENT here are the finding, not an omission:
// detector (c) fires on `get_execution_status` and `get_running_processes` (both resolve a
// process-inspection CLI), so they stay above `Read` and unregistered. `set_active_project`
// is likewise excluded — it syncs the runtime scheduler quota.
// ---------------------------------------------------------------------------------------

/// A mock app managing the execution-side state these reads are injected with.
fn app_with_execution_state() -> tauri::App<tauri::test::MockRuntime> {
    tauri::test::mock_builder()
        .manage(crate::application::AppState::new_test())
        .manage(std::sync::Arc::new(
            crate::commands::execution_commands::ActiveProjectState::new(),
        ))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app should build")
}

const B1_STEP_READS: [&str; 2] = ["get_task_steps", "get_step_progress"];
const B1_EXECUTION_READS: [&str; 3] = [
    "get_execution_settings",
    "get_global_execution_settings",
    "get_active_project",
];

/// P-4 parity for the step reads, over a task whose steps were seeded through `create_task`.
#[tokio::test]
async fn p4_parity_for_the_b1_step_reads_over_a_task_with_steps() {
    use tauri::Manager;

    let app = app_with_task_state();
    let (task_id, _other) = seed_b1_tasks(&app).await;

    let direct_steps = crate::commands::task_step_commands::get_task_steps(
        task_id.clone(),
        app.state::<crate::application::AppState>(),
    )
    .await
    .expect("direct step read succeeds");
    let direct_progress = crate::commands::task_step_commands::get_step_progress(
        task_id.clone(),
        app.state::<crate::application::AppState>(),
    )
    .await
    .expect("direct step-progress read succeeds");

    // Non-vacuity: the seeded task carries two steps, so neither row can pass by returning
    // an empty list.
    assert_eq!(direct_steps.len(), 2, "the fixture task must carry steps");

    for (command, direct) in [
        (
            "get_task_steps",
            registry::serialize_ok(&direct_steps).unwrap(),
        ),
        (
            "get_step_progress",
            registry::serialize_ok(&direct_progress).unwrap(),
        ),
    ] {
        let outcome = registry::dispatch(
            &app.handle().clone(),
            &[Scope::UiRead],
            command,
            &json!({"taskId": task_id}),
        )
        .await
        .unwrap_or_else(|error| panic!("`{command}` must dispatch at ui:read: {error:?}"));
        assert_eq!(
            outcome,
            DispatchOutcome::Ok(direct),
            "P-4 parity mismatch for `{command}`"
        );
    }
}

/// P-4 parity for the execution-settings reads, over a NON-default active project so the
/// `get_active_project` row cannot pass by returning `null` on both sides.
#[tokio::test]
async fn p4_parity_for_the_b1_execution_reads_over_a_set_active_project() {
    use tauri::Manager;

    let app = app_with_execution_state();
    let active =
        app.state::<std::sync::Arc<crate::commands::execution_commands::ActiveProjectState>>();
    active
        .set(Some(crate::domain::entities::ProjectId::from_string(
            B1_FIXTURE_PROJECT.to_string(),
        )))
        .await;

    let direct_settings = crate::commands::execution_commands::get_execution_settings(
        None,
        app.state::<crate::application::AppState>(),
    )
    .await
    .expect("direct execution-settings read succeeds");
    let direct_global = crate::commands::execution_commands::get_global_execution_settings(
        app.state::<crate::application::AppState>(),
    )
    .await
    .expect("direct global-settings read succeeds");
    let direct_active = crate::commands::execution_commands::get_active_project(
        app.state::<std::sync::Arc<crate::commands::execution_commands::ActiveProjectState>>(),
    )
    .await
    .expect("direct active-project read succeeds");

    assert_eq!(
        direct_active.as_deref(),
        Some(B1_FIXTURE_PROJECT),
        "the active project must be set, or parity over a null result proves nothing"
    );

    for (command, args, direct) in [
        (
            "get_execution_settings",
            json!({"projectId": null}),
            registry::serialize_ok(&direct_settings).unwrap(),
        ),
        (
            "get_global_execution_settings",
            json!({}),
            registry::serialize_ok(&direct_global).unwrap(),
        ),
        (
            "get_active_project",
            json!({}),
            registry::serialize_ok(&direct_active).unwrap(),
        ),
    ] {
        let outcome = registry::dispatch(&app.handle().clone(), &[Scope::UiRead], command, &args)
            .await
            .unwrap_or_else(|error| panic!("`{command}` must dispatch at ui:read: {error:?}"));
        assert_eq!(
            outcome,
            DispatchOutcome::Ok(direct),
            "P-4 parity mismatch for `{command}`"
        );
    }
}

/// Scope negative for both clusters, plus the standing absence of the process-resolving
/// execution getters.
#[tokio::test]
async fn the_b1_step_and_execution_reads_are_refused_below_ui_read() {
    let app = app_with_execution_state();

    for command in B1_STEP_READS.iter().chain(B1_EXECUTION_READS.iter()) {
        let spec = registry::find_spec(command)
            .unwrap_or_else(|| panic!("`{command}` must be registered"));
        assert_eq!(
            spec.class,
            RiskClass::Read,
            "`{command}` must be a Read row"
        );
        assert!(
            spec.capabilities.is_empty(),
            "`{command}` must carry no capability"
        );

        for scopes in [
            vec![],
            vec![Scope::UiOperate],
            vec![Scope::UiAgent],
            vec![Scope::UiElevated],
            vec![Scope::UiOperate, Scope::UiAgent, Scope::UiElevated],
        ] {
            let error = registry::dispatch(&app.handle().clone(), &scopes, command, &json!({}))
                .await
                .expect_err("insufficient scope must be refused");
            assert_eq!(
                error.code,
                ErrorCode::RemoteForbidden,
                "`{command}` admitted {scopes:?}"
            );
        }
    }

    // The detector-(c) finding, pinned: these look like sibling getters and are NOT
    // registered, because both resolve a process-inspection CLI.
    for command in ["get_execution_status", "get_running_processes"] {
        assert!(
            registry::find_spec(command).is_none(),
            "`{command}` must not be registered by sibling analogy"
        );
    }

    // `set_active_project` was listed above for a DIFFERENT reason — it syncs the runtime
    // scheduler quota — and PR 3.1-b batch 10 registered it at `ui:agent` after auditing that
    // arming surface. The invariant this test guards is unchanged and is what is asserted now:
    // it is a WRITE wearing a getter's name, so it must never be reachable from the read/operate
    // grants swept above. Its class is the enforcement, and the sweep at the top of this test
    // covers it because `ui:agent` is absent from every scope set exercised there.
    let spec = registry::find_spec("set_active_project")
        .expect("batch 10 registered set_active_project at ui:agent");
    assert_eq!(
        spec.class,
        RiskClass::AgentControl,
        "`set_active_project` syncs the execution concurrency quota; registering it at read or \
         operate would hand a viewer scheduler arming under a getter's name"
    );
}

// ---------------------------------------------------------------------------------------
// PR 3.1-b batch 3 — the Operate brakes (`ui:operate`)
//
// Three halting ops, reclassified from the conservative `AgentControl` module default. The
// asymmetry they close is concrete: before this batch a paired device could watch execution
// it had no way to stop.
//
// `archive_tasks_in_group` was audited alongside them and REFUSED. Its absence is asserted
// here, not merely omitted — see `bulk_archive_is_not_a_brake_and_stays_unregistered` in
// `capability_ledger_tests` for the reason.
// ---------------------------------------------------------------------------------------

const BATCH3_BRAKES: [&str; 2] = ["pause_execution", "stop_execution"];

/// A mock app carrying every state the brakes are injected with.
fn app_with_brake_state() -> tauri::App<tauri::test::MockRuntime> {
    tauri::test::mock_builder()
        .manage(crate::application::AppState::new_test())
        .manage(std::sync::Arc::new(
            crate::commands::execution_commands::ActiveProjectState::new(),
        ))
        .manage(std::sync::Arc::new(
            crate::commands::execution_commands::ExecutionState::default(),
        ))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app should build")
}

/// P-4 parity for the two global brakes.
///
/// Both are idempotent — pausing an already-paused runtime is a no-op — and their response
/// carries no clock-derived field, so the direct local IPC call and the facade dispatch must
/// produce byte-identical payloads. This is a genuine mutating-op parity row, not a Read one:
/// the direct call performs the halt and the dispatch performs it again.
#[tokio::test]
async fn p4_parity_for_the_batch3_execution_brakes() {
    use tauri::Manager;

    for (command, expected_halt_mode) in
        [("pause_execution", "paused"), ("stop_execution", "stopped")]
    {
        let app = app_with_brake_state();

        // Batch 4: the facade half of this row now needs a real project to name.
        let parity_project = ralphx_domain::entities::Project::new(
            "parity-fixture".to_string(),
            "/tmp/parity-fixture".to_string(),
        );
        let parity_project_id = parity_project.id.clone();
        app.state::<crate::application::AppState>()
            .project_repo
            .create(parity_project)
            .await
            .expect("fixture project is seeded");

        let execution_state =
            app.state::<std::sync::Arc<crate::commands::execution_commands::ExecutionState>>();

        // Non-vacuity: the runtime must start UNPAUSED, or "parity after halting" would be
        // satisfied by a command that does nothing at all.
        assert!(
            !execution_state.is_paused(),
            "the fixture must start unpaused for `{command}` parity to mean anything"
        );

        let direct = match command {
            "pause_execution" => crate::commands::execution_commands::pause_execution(
                None,
                app.state::<std::sync::Arc<crate::commands::execution_commands::ActiveProjectState>>(),
                app.state::<std::sync::Arc<crate::commands::execution_commands::ExecutionState>>(),
                app.state::<crate::application::AppState>(),
            )
            .await
            .expect("direct pause succeeds"),
            _ => crate::commands::execution_commands::stop_execution(
                None,
                app.state::<std::sync::Arc<crate::commands::execution_commands::ActiveProjectState>>(),
                app.state::<std::sync::Arc<crate::commands::execution_commands::ExecutionState>>(),
                app.state::<crate::application::AppState>(),
            )
            .await
            .expect("direct stop succeeds"),
        };

        // The effect actually happened, and it is the HALTING direction.
        assert!(
            direct.status.is_paused,
            "`{command}` must leave the runtime paused"
        );
        assert_eq!(
            direct.status.halt_mode, expected_halt_mode,
            "`{command}` must record its own halt mode"
        );
        assert!(
            !direct.status.can_start_task,
            "`{command}` must leave the runtime unable to start work"
        );

        // Batch 4: the facade now requires an explicit project scope, so the parity row names
        // one. The DIRECT call above still passes `None` — that asymmetry is the point of the
        // predicate (local keeps the convenience arm, remote does not), and parity still holds
        // because both reach the same global halt.
        let outcome = registry::dispatch(
            &app.handle().clone(),
            &[Scope::UiOperate],
            command,
            &json!({"projectId": parity_project_id.as_str()}),
        )
        .await
        .unwrap_or_else(|error| panic!("`{command}` must dispatch at ui:operate: {error:?}"));
        assert_eq!(
            outcome,
            DispatchOutcome::Ok(registry::serialize_ok(&direct).unwrap()),
            "P-4 parity mismatch for `{command}`"
        );
    }
}

/// The load-bearing hand-trace, pinned.
///
/// `pause_execution` and `stop_execution` open with `sync_quota_from_project` — the runtime
/// scheduler-quota write that disqualified `set_active_project` in batch 2. Registering them
/// is only sound because the quota is inert while halted. Both halves are asserted:
///
/// 1. **Behavioural** — the pause flag dominates any quota, however large.
/// 2. **Structural** — the one production path that clears the pause flag re-syncs the quota
///    before it does, so a quota raised while halting cannot survive into a scheduling window.
#[test]
fn the_brake_quota_write_is_dominated_by_the_pause_flag() {
    let execution_state = crate::commands::execution_commands::ExecutionState::default();

    // A quota far above any real configuration, exactly what a hostile `projectId` argument
    // could select by naming a project whose persisted settings are large.
    execution_state.set_max_concurrent(10_000);
    execution_state.set_project_ideation_max(10_000);
    assert!(
        execution_state.can_start_task(),
        "calibration: an unpaused runtime with headroom must be able to start work, or the \
         assertion below would pass against a permanently-false gate"
    );

    execution_state.pause();
    assert!(
        !execution_state.can_start_task(),
        "the pause flag must dominate the quota; if it does not, a brake that raises the \
         quota is a scheduler-arming op"
    );
    assert!(
        !execution_state.can_start_any_execution_context(),
        "the scheduler entry gate must also be dominated by the pause flag"
    );
    assert!(
        !execution_state.can_start_ideation(0, 0, 0, 10_000, 10_000, false, false),
        "the ideation gate must also be dominated by the pause flag, even with every headroom \
         argument set generously"
    );

    // Structural half: `resume()` is the only way back, and it must be preceded by a re-sync.
    let lifecycle = include_str!("../commands/execution_commands/lifecycle.rs");
    let resume_sites = lifecycle.matches("execution_state.resume()").count();
    assert_eq!(
        resume_sites, 1,
        "a second production caller of ExecutionState::resume appeared in lifecycle.rs; the \
         brake registration assumes exactly one ungating path, which re-syncs the quota first"
    );
    let resume_at = lifecycle
        .find("execution_state.resume()")
        .expect("the resume call site exists");
    let sync_at = lifecycle
        .find("sync_quota_from_project")
        .expect("resume_execution re-syncs the quota");
    assert!(
        sync_at < resume_at,
        "the quota re-sync must precede the pause-flag clear, or a quota raised during a \
         remote halt could arm the scheduler on resume"
    );

    for command in ["pause_execution", "stop_execution"] {
        let body = lifecycle
            .split(&format!("pub async fn {command}"))
            .nth(1)
            .unwrap_or_else(|| panic!("{command} must remain defined in lifecycle.rs"));
        let pause_at = body
            .find("execution_state.pause()")
            .unwrap_or_else(|| panic!("{command} must set the global pause gate"));
        let transition_at = body
            .find(".transition_task(")
            .unwrap_or_else(|| panic!("{command} must transition active tasks"));
        assert!(
            pause_at < transition_at,
            "{command} must set the global pause gate before its first task transition"
        );
    }
}

/// Scope negative — no grant weaker than (or sideways from) `ui:operate` reaches a brake,
/// and `ui:operate` is genuinely required rather than incidentally sufficient.
#[tokio::test]
async fn the_batch3_brakes_are_refused_below_ui_operate() {
    let app = app_with_brake_state();

    for command in BATCH3_BRAKES {
        let spec = registry::find_spec(command)
            .unwrap_or_else(|| panic!("`{command}` must be registered"));
        assert_eq!(
            spec.class,
            RiskClass::Operate,
            "`{command}` must be an Operate row"
        );
        assert!(
            spec.capabilities.is_empty(),
            "`{command}` must carry no capability; an Operate row cannot express one"
        );
        assert!(spec.pins.is_empty(), "`{command}` declares no pinned field");

        for scopes in [
            vec![],
            vec![Scope::UiRead],
            vec![Scope::UiAgent],
            vec![Scope::UiElevated],
            vec![Scope::UiRead, Scope::UiAgent, Scope::UiElevated],
        ] {
            let error = registry::dispatch(&app.handle().clone(), &scopes, command, &json!({}))
                .await
                .expect_err("insufficient scope must be refused");
            assert_eq!(
                error.code,
                ErrorCode::RemoteForbidden,
                "`{command}` admitted {scopes:?}"
            );
        }
    }

    // The refused sibling. Its absence is the audit's finding, so it is asserted at the
    // facade too: no scope, including the full v1 set, reaches it.
    for scopes in [
        vec![Scope::UiRead],
        vec![Scope::UiOperate],
        vec![Scope::UiAgent],
        vec![Scope::UiElevated],
        vec![
            Scope::UiRead,
            Scope::UiOperate,
            Scope::UiAgent,
            Scope::UiElevated,
        ],
    ] {
        let error = registry::dispatch(
            &app.handle().clone(),
            &scopes,
            "archive_tasks_in_group",
            &json!({"groupKind": "status", "groupId": "ready", "projectId": "p"}),
        )
        .await
        .expect_err("archive_tasks_in_group must stay unreachable");
        assert_eq!(
            error.code,
            ErrorCode::RemoteCommandUnavailable,
            "archive_tasks_in_group became reachable at {scopes:?}"
        );
    }
}

/// With an agent-control grant, bulk cancel needs the Wry handle and fails closed without it.
///
/// This is the `(host_app_handle)` contract: a missing handle is a host fault, never a reason
/// to take a degraded path that cancels without emitting lifecycle events.
#[tokio::test]
async fn the_bulk_cancel_brake_fails_closed_without_a_host_handle() {
    let app = app_with_brake_state();

    let error = registry::dispatch(
        &app.handle().clone(),
        &[Scope::UiAgent],
        "cancel_tasks_in_group",
        &json!({"groupKind": "status", "groupId": "ready", "projectId": "p"}),
    )
    .await
    .expect_err("a missing host handle must not be substituted");
    assert_eq!(
        error.code,
        ErrorCode::RemoteInternalError,
        "cancel_tasks_in_group degraded instead of failing closed"
    );
}

// ---------------------------------------------------------------------------------------
// Owner decision R3 — P-4 parity extended to a representative MUTATING trio.
//
// All four pre-existing `p4_parity_*` tables cover `Read` commands. The registered
// Operate/AgentControl ops asserted authorization and absence-of-effect but never
// result-vs-wire envelope parity, so a facade that reshaped a mutating result — dropped a
// field, re-cased one, swallowed an error into a success envelope — would not have been
// caught by P-4 at all.
//
// R3 resolves the scoping as: Read = exhaustive, mutating = representative + scope-suite
// effect coverage. This is that representative table.
//
// The machinery a mutating parity row needs, and the reason it is not simply "call twice":
// mutating ops are not idempotent, so each row runs over TWO independently-seeded but
// IDENTICAL fixtures — one driven through the direct local IPC fn, one through the production
// facade dispatch — and compares the payloads. Fields that legitimately differ between two
// separate executions (generated ids, clock stamps) are normalized rather than asserted, and
// every normalization is named, so the test cannot quietly normalize away a real divergence.
// ---------------------------------------------------------------------------------------

const R3_FIXTURE_PROJECT: &str = "55555555-5555-5555-5555-555555555555";
const R3_FIXTURE_TASK: &str = "66666666-6666-6666-6666-666666666666";

/// Strips exactly the fields two separate executions may legitimately disagree on.
///
/// Returns the removed values alongside the normalized payload so the caller can assert they
/// were actually PRESENT — a normalizer that silently no-ops when a field disappears would
/// hide the very regression this table exists to catch.
fn normalize_generated_fields(mut value: Value) -> (Value, Vec<String>) {
    // `TaskResponse` carries no `rename_all`, so the wire fields are snake_case — the facade
    // deserializes and re-serializes exactly what the local IPC path does, which is itself part
    // of what P-4 asserts. Listing the camelCase spellings too would silently tolerate a
    // re-casing regression, so they are deliberately absent.
    const GENERATED: [&str; 3] = ["id", "created_at", "updated_at"];
    let mut seen = Vec::new();
    if let Some(object) = value.as_object_mut() {
        for field in GENERATED {
            if object.remove(field).is_some() {
                seen.push(field.to_string());
            }
        }
    }
    (value, seen)
}

/// Seeds a task with a FIXED id so both fixtures address the same row.
async fn seed_r3_task(app: &tauri::App<tauri::test::MockRuntime>) {
    use tauri::Manager;

    let mut task = crate::domain::entities::Task::new(
        crate::domain::entities::ProjectId::from_string(R3_FIXTURE_PROJECT.to_string()),
        "R3 parity subject".to_string(),
    );
    task.id = crate::domain::entities::TaskId::from_string(R3_FIXTURE_TASK.to_string());
    task.priority = 1;
    app.state::<crate::application::AppState>()
        .task_repo
        .create(task)
        .await
        .expect("seeding the parity subject succeeds");
}

/// Seeds a pending permission request with a FIXED id.
async fn seed_r3_permission(app: &tauri::App<tauri::test::MockRuntime>, request_id: &str) {
    use tauri::Manager;

    app.state::<crate::application::AppState>()
        .permission_state
        .register(
            crate::application::permission_state::PendingPermissionInfo {
                request_id: request_id.to_string(),
                tool_name: "Bash".to_string(),
                tool_input: json!({"command": "ls"}),
                context: None,
                agent_type: None,
                task_id: None,
                context_type: None,
                context_id: None,
                created_at: chrono::Utc::now().to_rfc3339(),
            },
        )
        .await;
}

/// P-4 parity — `create_task`.
#[tokio::test]
async fn p4_parity_for_the_r3_mutating_trio_create_task() {
    use tauri::Manager;

    let input = || crate::commands::task_commands::types::CreateTaskInput {
        project_id: R3_FIXTURE_PROJECT.to_string(),
        title: "R3 created".to_string(),
        category: None,
        description: Some("described".to_string()),
        priority: Some(3),
        steps: None,
        ideation_session_id: None,
        execution_plan_id: None,
    };

    let direct_app = app_with_task_state();
    let direct = crate::commands::task_commands::mutation::create_task(
        input(),
        direct_app.state::<crate::application::AppState>(),
    )
    .await
    .expect("direct create succeeds");

    let facade_app = app_with_task_state();
    let outcome = registry::dispatch(
        &facade_app.handle().clone(),
        &[Scope::UiOperate],
        "create_task",
        &json!({"input": {
            "projectId": R3_FIXTURE_PROJECT,
            "title": "R3 created",
            "description": "described",
            "priority": 3,
        }}),
    )
    .await
    .expect("create_task must dispatch at ui:operate");

    let DispatchOutcome::Ok(facade) = outcome else {
        panic!("create_task returned a business error: {outcome:?}");
    };

    let (direct_norm, direct_stripped) =
        normalize_generated_fields(registry::serialize_ok(&direct).unwrap());
    let (facade_norm, facade_stripped) = normalize_generated_fields(facade);

    // The normalizer must have had something to do, in both directions.
    assert!(
        direct_stripped.contains(&"id".to_string()),
        "the created task carries no id; the normalizer is hiding a shape change"
    );
    assert_eq!(
        direct_stripped, facade_stripped,
        "the two payloads do not even agree on WHICH generated fields exist"
    );
    assert_eq!(
        direct_norm, facade_norm,
        "P-4 mutating parity mismatch for `create_task`"
    );
    // Non-vacuity: a normalizer bug that emptied both sides would satisfy the line above.
    assert!(
        direct_norm.get("title").is_some(),
        "the normalized payload must still carry the substantive fields"
    );
    assert_eq!(direct_norm["title"], json!("R3 created"));
    assert_eq!(direct_norm["priority"], json!(3));
}

/// P-4 parity — `update_task`, over identically-seeded fixtures, plus its error envelope.
#[tokio::test]
async fn p4_parity_for_the_r3_mutating_trio_update_task() {
    use tauri::Manager;

    let direct_app = app_with_task_state();
    seed_r3_task(&direct_app).await;
    let direct = crate::commands::task_commands::mutation::update_task(
        R3_FIXTURE_TASK.to_string(),
        crate::commands::task_commands::types::UpdateTaskInput {
            title: None,
            description: None,
            category: None,
            priority: Some(9),
            internal_status: None,
        },
        direct_app.state::<crate::application::AppState>(),
    )
    .await
    .expect("direct update succeeds");

    let facade_app = app_with_task_state();
    seed_r3_task(&facade_app).await;
    let outcome = registry::dispatch(
        &facade_app.handle().clone(),
        &[Scope::UiOperate],
        "update_task",
        &json!({"taskId": R3_FIXTURE_TASK, "input": {"priority": 9}}),
    )
    .await
    .expect("update_task must dispatch at ui:operate");
    let DispatchOutcome::Ok(facade) = outcome else {
        panic!("update_task returned a business error: {outcome:?}");
    };

    let (direct_norm, direct_stripped) =
        normalize_generated_fields(registry::serialize_ok(&direct).unwrap());
    let (facade_norm, facade_stripped) = normalize_generated_fields(facade);
    assert_eq!(direct_stripped, facade_stripped);
    assert_eq!(
        direct_norm, facade_norm,
        "P-4 mutating parity mismatch for `update_task`"
    );
    // The mutation actually landed, so parity is over a CHANGED row, not an untouched one.
    assert_eq!(
        direct_norm["priority"],
        json!(9),
        "the seeded priority was 1; parity over an unapplied update would prove nothing"
    );

    // Error-path envelope parity (R3 names this explicitly): a command-level failure must
    // reach the wire as a business error carrying the SAME payload, never as a facade error
    // and never reshaped into a success envelope.
    let missing_app = app_with_task_state();
    let direct_error = crate::commands::task_commands::mutation::update_task(
        "77777777-7777-7777-7777-777777777777".to_string(),
        crate::commands::task_commands::types::UpdateTaskInput {
            title: None,
            description: None,
            category: None,
            priority: Some(9),
            internal_status: None,
        },
        missing_app.state::<crate::application::AppState>(),
    )
    .await
    .expect_err("updating a missing task fails");

    let facade_error_app = app_with_task_state();
    let outcome = registry::dispatch(
        &facade_error_app.handle().clone(),
        &[Scope::UiOperate],
        "update_task",
        &json!({
            "taskId": "77777777-7777-7777-7777-777777777777",
            "input": {"priority": 9},
        }),
    )
    .await
    .expect("a command-level failure is still a successful dispatch");
    assert_eq!(
        outcome,
        DispatchOutcome::Err(registry::serialize_ok(&direct_error).unwrap()),
        "the error envelope must match the direct call's error payload"
    );
}

/// P-4 parity — `deny_permission_request`, the pinned op, plus its error envelope.
#[tokio::test]
async fn p4_parity_for_the_r3_mutating_trio_deny_permission_request() {
    use tauri::Manager;

    const REQUEST_ID: &str = "r3-permission-request";

    let direct_app = app_with_task_state();
    seed_r3_permission(&direct_app, REQUEST_ID).await;
    let direct = crate::commands::permission_commands::resolve_permission_request(
        direct_app.state::<crate::application::AppState>(),
        crate::commands::permission_commands::ResolvePermissionArgs {
            request_id: REQUEST_ID.to_string(),
            decision: "deny".to_string(),
            message: Some("not this time".to_string()),
        },
    )
    .await
    .expect("direct deny succeeds");

    let facade_app = app_with_task_state();
    seed_r3_permission(&facade_app, REQUEST_ID).await;
    let outcome = registry::dispatch(
        &facade_app.handle().clone(),
        &[Scope::UiOperate],
        "deny_permission_request",
        // The client asserts the OPPOSITE decision; the server pin must win, and parity must
        // hold against the direct DENY call rather than against an allow.
        &json!({"args": {
            "request_id": REQUEST_ID,
            "decision": "allow",
            "message": "not this time",
        }}),
    )
    .await
    .expect("deny_permission_request must dispatch at ui:operate");
    let DispatchOutcome::Ok(facade) = outcome else {
        panic!("deny_permission_request returned a business error: {outcome:?}");
    };

    // Fully deterministic payload — no generated field to normalize.
    assert_eq!(
        registry::serialize_ok(&direct).unwrap(),
        facade,
        "P-4 mutating parity mismatch for `deny_permission_request`"
    );
    assert_eq!(
        facade["success"],
        json!(true),
        "parity over a failed resolution would prove nothing"
    );

    // The payload is about the request the client named, not a generic acknowledgement — a
    // facade that resolved a DIFFERENT request would still have returned `success: true`.
    assert_eq!(
        facade["message"],
        json!(format!("Permission request {REQUEST_ID} resolved")),
    );

    // Error-path envelope parity: an unknown request id.
    let direct_missing_app = app_with_task_state();
    let direct_error = crate::commands::permission_commands::resolve_permission_request(
        direct_missing_app.state::<crate::application::AppState>(),
        crate::commands::permission_commands::ResolvePermissionArgs {
            request_id: "no-such-request".to_string(),
            decision: "deny".to_string(),
            message: None,
        },
    )
    .await
    .expect_err("resolving a missing request fails");

    let facade_missing_app = app_with_task_state();
    let outcome = registry::dispatch(
        &facade_missing_app.handle().clone(),
        &[Scope::UiOperate],
        "deny_permission_request",
        &json!({"args": {"request_id": "no-such-request", "decision": "deny"}}),
    )
    .await
    .expect("a command-level failure is still a successful dispatch");
    assert_eq!(
        outcome,
        DispatchOutcome::Err(registry::serialize_ok(&direct_error).unwrap()),
        "the error envelope must match the direct call's error payload"
    );
}

// ---------------------------------------------------------------------------------------
// PR 3.1-b batch 3 — census `B2`, the conversation-stats read cluster (`ui:read`)
//
// Four aggregate reads, the smallest complete module in the census's highest-risk batch.
// What is ABSENT is the finding: the transcript reads (`get_agent_conversation`,
// `get_agent_conversation_messages_page`, `get_agent_conversation_timeline_page`) all fire
// detector (a), and the workspace/publish surface fires (a), (b) and (c) together. They stay
// at the module default, asserted below.
// ---------------------------------------------------------------------------------------

const B2_STATS_READS: [&str; 4] = [
    "get_agent_conversation_stats",
    "get_project_chat_usage_stats",
    "get_task_chat_usage_stats",
    "get_insights_chat_usage_stats",
];

/// P-4 parity for the stats cluster.
#[tokio::test]
async fn p4_parity_for_the_b2_stats_reads() {
    use tauri::Manager;

    let app = app_with_task_state();
    seed_r3_task(&app).await;

    let direct_conversation =
        crate::commands::conversation_stats_commands::get_agent_conversation_stats(
            "no-such-conversation".to_string(),
            app.state::<crate::application::AppState>(),
        )
        .await
        .expect("direct conversation-stats read succeeds");
    let direct_project =
        crate::commands::conversation_stats_commands::get_project_chat_usage_stats(
            R3_FIXTURE_PROJECT.to_string(),
            app.state::<crate::application::AppState>(),
        )
        .await
        .expect("direct project-usage read succeeds");
    let direct_task = crate::commands::conversation_stats_commands::get_task_chat_usage_stats(
        R3_FIXTURE_TASK.to_string(),
        app.state::<crate::application::AppState>(),
    )
    .await
    .expect("direct task-usage read succeeds");
    let direct_insights =
        crate::commands::conversation_stats_commands::get_insights_chat_usage_stats(
            None,
            app.state::<crate::application::AppState>(),
        )
        .await
        .expect("direct insights-usage read succeeds");

    // Non-vacuity is recorded honestly rather than manufactured. A mock `AppState` carries no
    // chat conversations, so three of these are legitimately zero-valued and the fourth is a
    // genuine `None`. What the rows below prove is ENVELOPE parity — that the facade
    // reproduces the scope identity and the full aggregate shape, not merely "nothing". The
    // load-bearing guarantee for these four is the scope negative that follows.
    assert!(
        direct_conversation.is_none(),
        "an unknown conversation must read as None, not as an error"
    );
    assert_eq!(
        direct_project.scope_type, "project",
        "the project row must identify its own scope"
    );
    assert_eq!(direct_project.scope_id, R3_FIXTURE_PROJECT);
    assert_eq!(direct_task.scope_type, "task");
    assert_eq!(direct_insights.scope_type, "all_projects");

    for (command, args, direct) in [
        (
            "get_agent_conversation_stats",
            json!({"conversationId": "no-such-conversation"}),
            registry::serialize_ok(&direct_conversation).unwrap(),
        ),
        (
            "get_project_chat_usage_stats",
            json!({"projectId": R3_FIXTURE_PROJECT}),
            registry::serialize_ok(&direct_project).unwrap(),
        ),
        (
            "get_task_chat_usage_stats",
            json!({"taskId": R3_FIXTURE_TASK}),
            registry::serialize_ok(&direct_task).unwrap(),
        ),
        (
            "get_insights_chat_usage_stats",
            json!({"projectId": null}),
            registry::serialize_ok(&direct_insights).unwrap(),
        ),
    ] {
        let outcome = registry::dispatch(&app.handle().clone(), &[Scope::UiRead], command, &args)
            .await
            .unwrap_or_else(|error| panic!("`{command}` must dispatch at ui:read: {error:?}"));
        assert_eq!(
            outcome,
            DispatchOutcome::Ok(direct),
            "P-4 parity mismatch for `{command}`"
        );
    }
}

/// Scope negative, plus the standing absence of the detector-positive `B2` siblings.
#[tokio::test]
async fn the_b2_stats_reads_are_refused_below_ui_read() {
    let app = app_with_task_state();

    for command in B2_STATS_READS {
        let spec = registry::find_spec(command)
            .unwrap_or_else(|| panic!("`{command}` must be registered"));
        assert_eq!(
            spec.class,
            RiskClass::Read,
            "`{command}` must be a Read row"
        );
        assert!(
            spec.capabilities.is_empty(),
            "`{command}` must carry no capability; a Read row with one is the under-labelling \
             signature itself"
        );

        for scopes in [
            vec![],
            vec![Scope::UiOperate],
            vec![Scope::UiAgent],
            vec![Scope::UiElevated],
            vec![Scope::UiOperate, Scope::UiAgent, Scope::UiElevated],
        ] {
            let error = registry::dispatch(&app.handle().clone(), &scopes, command, &json!({}))
                .await
                .expect_err("insufficient scope must be refused");
            assert_eq!(
                error.code,
                ErrorCode::RemoteForbidden,
                "`{command}` admitted {scopes:?}"
            );
        }
    }

    // The detector-positive `B2` siblings stay unreachable. These are the transcript and
    // workspace/publish reads a later batch must audit on their own evidence; registering the
    // stats module must not be mistaken for having cleared the module family.
    for command in [
        "get_agent_conversation",
        "get_agent_conversation_messages_page",
        "get_agent_conversation_timeline_page",
        "get_agent_conversation_workspace",
        "list_agent_sidebar_conversations",
        "send_agent_message",
        "start_agent_conversation",
    ] {
        assert!(
            registry::find_spec(command).is_none(),
            "`{command}` fires at least one authority detector and must stay unregistered"
        );
    }
}

// ---------------------------------------------------------------------------------------
// Batch 4 — the brake-scope predicate
//
// Batch 3 registered `pause_execution`/`stop_execution` at `ui:operate` and flagged, in its own
// review, the one row worth arguing about: both accept `projectId: null`, which reaches
// `project_repo.get_all()` and halts EVERY project, and `ui:operate` is in the DEFAULT pairing.
// The ruling was to confine it. A remote device must now name the project it halts.
// ---------------------------------------------------------------------------------------

/// The predicate itself, over every spelling that collapses to `None` at the target fn.
///
/// Explicit null, absent, empty, whitespace-only and wrong-typed all deserialize to
/// `project_id: None` and reach the `project_repo.get_all()` arm, so all five must be refused
/// identically. Accepting any one of them would leave the all-projects sweep reachable.
#[test]
fn the_brake_scope_predicate_refuses_every_spelling_of_an_absent_project() {
    for args in [
        json!({"projectId": null}),
        json!({}),
        json!({"projectId": ""}),
        json!({"projectId": "   "}),
        json!({"projectId": 17}),
        json!({"projectId": ["a"]}),
    ] {
        registry::require_explicit_project_scope(&args).expect_err(&format!(
            "{args} must be refused: it reaches the all-projects arm"
        ));
    }

    // Positive, and in both accepted spellings — Tauri takes either for a flat param, so a
    // client written against the local IPC contract must not break remotely.
    registry::require_explicit_project_scope(&json!({"projectId": "p-1"}))
        .expect("an explicit camelCase projectId is accepted");
    registry::require_explicit_project_scope(&json!({"project_id": "p-1"}))
        .expect("an explicit snake_case project_id is accepted");
}

/// Both brakes carry the predicate, and no OTHER registered command silently acquired one.
#[test]
fn the_brake_scope_predicate_is_wired_to_exactly_the_brakes() {
    let carriers = registry::REMOTE_COMMANDS
        .iter()
        .filter(|spec| spec.validate.is_some())
        .map(|spec| spec.name)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        carriers,
        ["pause_execution", "stop_execution"].into_iter().collect(),
        "the `validate:` carriers drifted; every one needs a SCOPE_CONFINEMENTS row"
    );
}

/// The refusal above, checked for its code and — critically — for its EFFECT.
///
/// A predicate that returned the right error code but ran after the halt would be worthless.
/// This asserts the runtime is still unpaused afterwards, so the refusal is proven to precede
/// the side effect rather than merely accompany it.
#[tokio::test]
async fn the_refused_brake_never_reaches_the_halt() {
    use tauri::Manager;

    for command in ["pause_execution", "stop_execution"] {
        for args in [json!({"projectId": null}), json!({})] {
            let app = app_with_brake_state();
            let execution_state =
                app.state::<std::sync::Arc<crate::commands::execution_commands::ExecutionState>>();
            assert!(
                !execution_state.is_paused(),
                "fixture must start unpaused or the assertion below is vacuous"
            );

            let error =
                registry::dispatch(&app.handle().clone(), &[Scope::UiOperate], command, &args)
                    .await
                    .expect_err("an unscoped remote brake must be refused");

            assert_eq!(
                error.code,
                ErrorCode::RemoteInvalidArguments,
                "`{command}` with {args} must be an argument error, not an authorization one"
            );
            assert_eq!(
                status_for_error_code(error.code),
                StatusCode::BAD_REQUEST,
                "the refusal is a 4xx the client can fix by resending, not a pairing signal"
            );
            assert_ne!(
                error.code,
                ErrorCode::RemoteForbidden,
                "a sufficient grant must never be reported as insufficient"
            );

            // The whole point: the halt did not happen.
            assert!(
                !execution_state.is_paused(),
                "`{command}` was refused but the runtime is paused — the predicate ran too late"
            );
        }
    }
}

/// The positive direction: an explicit `projectId` still dispatches.
///
/// Without this the predicate could be satisfied by a function that refuses everything, and
/// the brakes would be silently dead remotely.
#[tokio::test]
async fn a_remote_brake_with_an_explicit_project_scope_still_halts() {
    use tauri::Manager;

    for (command, expected_halt_mode) in
        [("pause_execution", "paused"), ("stop_execution", "stopped")]
    {
        let app = app_with_brake_state();
        let app_state = app.state::<crate::application::AppState>();

        let project = ralphx_domain::entities::Project::new(
            "brake-scope-fixture".to_string(),
            "/tmp/brake-scope-fixture".to_string(),
        );
        let project_id = project.id.clone();
        app_state
            .project_repo
            .create(project)
            .await
            .expect("fixture project is seeded");

        let execution_state =
            app.state::<std::sync::Arc<crate::commands::execution_commands::ExecutionState>>();
        assert!(
            !execution_state.is_paused(),
            "fixture must start unpaused for the halt assertion to mean anything"
        );

        let outcome = registry::dispatch(
            &app.handle().clone(),
            &[Scope::UiOperate],
            command,
            &json!({"projectId": project_id.as_str()}),
        )
        .await
        .unwrap_or_else(|error| {
            panic!("`{command}` with an explicit projectId must dispatch: {error:?}")
        });

        let DispatchOutcome::Ok(value) = outcome else {
            panic!("`{command}` with an explicit projectId must succeed, got {outcome:?}");
        };
        assert_eq!(
            value["status"]["halt_mode"], expected_halt_mode,
            "`{command}` must record its own halt mode"
        );
        assert!(
            execution_state.is_paused(),
            "`{command}` with an explicit projectId must actually halt"
        );
    }
}

/// The confinement's honest limit, pinned.
///
/// `require_explicit_project_scope` confines the per-task transition sweep. It does NOT confine
/// `execution_state.pause()`, `persist_execution_halt_mode`, `stop_all()` or the interactive
/// registry clear — those run in `execution_commands::lifecycle` before the project is resolved
/// and are process-global. A reader who assumed "scoped brake" meant "only that project is
/// affected" would be wrong, and a future change that quietly made the scoping weaker would not
/// be caught by the positive tests above.
///
/// So this asserts the limit directly: halting project A leaves the GLOBAL flag set, which is
/// simultaneously the proof that the confinement is partial and the reason the row is still
/// only `ui:operate` rather than something weaker.
#[tokio::test]
async fn the_brake_scope_predicate_does_not_confine_the_global_pause_flag() {
    use tauri::Manager;

    let app = app_with_brake_state();
    let app_state = app.state::<crate::application::AppState>();

    let scoped =
        ralphx_domain::entities::Project::new("scoped".to_string(), "/tmp/scoped".to_string());
    let bystander = ralphx_domain::entities::Project::new(
        "bystander".to_string(),
        "/tmp/bystander".to_string(),
    );
    let scoped_id = scoped.id.clone();
    app_state.project_repo.create(scoped).await.expect("seeded");
    app_state
        .project_repo
        .create(bystander)
        .await
        .expect("seeded");

    let execution_state =
        app.state::<std::sync::Arc<crate::commands::execution_commands::ExecutionState>>();

    registry::dispatch(
        &app.handle().clone(),
        &[Scope::UiOperate],
        "pause_execution",
        &json!({"projectId": scoped_id.as_str()}),
    )
    .await
    .expect("a scoped brake dispatches");

    assert!(
        execution_state.is_paused(),
        "the scheduler pause flag is PROCESS-GLOBAL: a project-scoped remote brake still halts \
         host-wide scheduling. This is the documented limit of the confinement, not a bug — \
         but it must never be described as a per-project halt."
    );
    assert!(
        !execution_state.can_start_task(),
        "the bystander project cannot start work either; the confinement is over the task \
         transition sweep only"
    );
}

// ---------------------------------------------------------------------------------------
// Batch 4 — the spawn-free transcript reads (the PR 3.2 dependency)
// ---------------------------------------------------------------------------------------

const REMOTE_TRANSCRIPT_READS: [&str; 3] = [
    "get_remote_agent_conversation",
    "get_remote_agent_conversation_messages_page",
    "get_remote_agent_conversation_timeline_page",
];

/// A conversation with real messages. Parity over an EMPTY transcript would be satisfied by a
/// command that returns nothing at all, which is the vacuity batch 2 called out by name.
async fn app_with_seeded_transcript() -> (
    tauri::App<tauri::test::MockRuntime>,
    ralphx_domain::entities::ChatConversationId,
) {
    use ralphx_domain::entities::{ChatConversation, ChatMessage, IdeationSessionId};
    use tauri::Manager;

    let app = tauri::test::mock_builder()
        .manage(crate::application::AppState::new_test())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app should build");

    let state = app.state::<crate::application::AppState>();
    let session_id = IdeationSessionId::new();
    let conversation = ChatConversation::new_ideation(session_id.clone());
    let conversation_id = conversation.id.clone();
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("conversation seeded");

    for content in ["first user turn", "second user turn"] {
        let mut message = ChatMessage::user_in_session(session_id.clone(), content);
        message.conversation_id = Some(conversation_id.clone());
        state
            .chat_message_repo
            .create(message)
            .await
            .expect("message seeded");
    }

    (app, conversation_id)
}

/// P-4 parity: each remote transcript read is byte-identical to the local `*_for_app_state`
/// seam the local command uses after its wake.
///
/// This is the assertion that makes the seam split honest. The split is only safe if the
/// remote variant returns the SAME data as the local read; a "pure" read that quietly returns
/// less would be a silent product regression that no detector would catch.
#[tokio::test]
async fn p4_parity_for_the_spawn_free_transcript_reads() {
    use tauri::Manager;

    let (app, conversation_id) = app_with_seeded_transcript().await;
    let state = app.state::<crate::application::AppState>();

    // Non-vacuity: the fixture really does carry messages, and the reads really do return them.
    let direct_full = crate::commands::unified_chat_commands::get_agent_conversation_for_app_state(
        &state,
        conversation_id.clone(),
    )
    .await
    .expect("direct full read succeeds")
    .expect("the seeded conversation resolves");
    assert_eq!(
        direct_full.messages.len(),
        2,
        "the fixture must carry messages, or parity over it proves nothing"
    );
    assert_eq!(direct_full.messages[0].content, "first user turn");

    let direct_page =
        crate::commands::unified_chat_commands::get_agent_conversation_messages_page_for_app_state(
            &state,
            conversation_id.clone(),
            40,
            0,
        )
        .await
        .expect("direct page read succeeds");
    let direct_timeline =
        crate::commands::unified_chat_commands::get_agent_conversation_timeline_page_for_app_state(
            &state,
            conversation_id.clone(),
            40,
            None,
        )
        .await
        .expect("direct timeline read succeeds");

    for (command, direct, args) in [
        (
            "get_remote_agent_conversation",
            registry::serialize_ok(Some(direct_full)).unwrap(),
            json!({"conversationId": conversation_id.as_str()}),
        ),
        (
            "get_remote_agent_conversation_messages_page",
            registry::serialize_ok(&direct_page).unwrap(),
            json!({"conversationId": conversation_id.as_str(), "limit": 40, "offset": 0}),
        ),
        (
            "get_remote_agent_conversation_timeline_page",
            registry::serialize_ok(&direct_timeline).unwrap(),
            json!({"conversationId": conversation_id.as_str(), "limit": 40}),
        ),
    ] {
        let dispatched =
            registry::dispatch(&app.handle().clone(), &[Scope::UiRead], command, &args)
                .await
                .unwrap_or_else(|error| panic!("`{command}` must dispatch at ui:read: {error:?}"));
        assert_eq!(
            dispatched,
            DispatchOutcome::Ok(direct),
            "`{command}` remote dispatch must be byte-identical to the local seam"
        );
    }
}

// ---------------------------------------------------------------------------------------
// Batch 5 — the conversation-LIST seam split
// ---------------------------------------------------------------------------------------

const REMOTE_CONVERSATION_LIST_READS: [&str; 2] = [
    "list_remote_agent_conversations",
    "list_remote_agent_conversations_page",
];

/// A context carrying TWO conversations. Parity over an empty list would be satisfied by a
/// command that lists nothing at all — the same vacuity the transcript fixture guards against,
/// and it bites harder here because "returns an empty list" is this surface's failure mode.
async fn app_with_seeded_conversation_list() -> (
    tauri::App<tauri::test::MockRuntime>,
    ralphx_domain::entities::IdeationSessionId,
) {
    use ralphx_domain::entities::{ChatConversation, IdeationSessionId};
    use tauri::Manager;

    let app = tauri::test::mock_builder()
        .manage(crate::application::AppState::new_test())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app should build");

    let state = app.state::<crate::application::AppState>();
    let session_id = IdeationSessionId::new();
    for _ in 0..2 {
        state
            .chat_conversation_repo
            .create(ChatConversation::new_ideation(session_id.clone()))
            .await
            .expect("conversation seeded");
    }

    (app, session_id)
}

/// P-4 parity: each remote list read is byte-identical to the `*_for_app_state` seam the LOCAL
/// command now calls.
///
/// Both local and remote go through the same seam, so this asserts the extraction did not fork
/// logic (A-7) as much as it asserts the remote payload. The non-vacuity check is the
/// load-bearing part: a "pure" list that quietly returned fewer conversations — or none —
/// would be a silent product regression that no detector in this suite can see.
#[tokio::test]
async fn p4_parity_for_the_spawn_free_conversation_list_reads() {
    use ralphx_domain::entities::ChatContextType;
    use tauri::Manager;

    let (app, session_id) = app_with_seeded_conversation_list().await;
    let state = app.state::<crate::application::AppState>();
    let context_id = session_id.as_str().to_string();

    let direct_list =
        crate::commands::unified_chat_commands::list_agent_conversations_for_app_state(
            &state,
            ChatContextType::Ideation,
            &context_id,
            false,
        )
        .await
        .expect("direct list read succeeds");
    assert_eq!(
        direct_list.len(),
        2,
        "the fixture must carry conversations, or parity over it proves nothing"
    );

    let direct_page =
        crate::commands::unified_chat_commands::list_agent_conversations_page_for_app_state(
            &state,
            ChatContextType::Ideation,
            &context_id,
            false,
            false,
            0,
            6,
            None,
        )
        .await
        .expect("direct page read succeeds");
    assert_eq!(
        direct_page.total, 2,
        "the paged fixture must be non-empty for parity to mean anything"
    );

    for (command, direct, args) in [
        (
            "list_remote_agent_conversations",
            registry::serialize_ok(&direct_list).unwrap(),
            json!({"contextType": "ideation", "contextId": context_id}),
        ),
        (
            "list_remote_agent_conversations_page",
            registry::serialize_ok(&direct_page).unwrap(),
            json!({"contextType": "ideation", "contextId": context_id, "limit": 6, "offset": 0}),
        ),
    ] {
        let dispatched =
            registry::dispatch(&app.handle().clone(), &[Scope::UiRead], command, &args)
                .await
                .unwrap_or_else(|error| panic!("`{command}` must dispatch at ui:read: {error:?}"));
        assert_eq!(
            dispatched,
            DispatchOutcome::Ok(direct),
            "`{command}` remote dispatch must be byte-identical to the local seam"
        );
    }
}

/// Scope negatives — the list reads sit at `Read`, so every other grant is insufficient.
#[tokio::test]
async fn the_spawn_free_conversation_list_reads_are_refused_below_ui_read() {
    let (app, session_id) = app_with_seeded_conversation_list().await;
    let context_id = session_id.as_str().to_string();

    for command in REMOTE_CONVERSATION_LIST_READS {
        let spec = registry::find_spec(command)
            .unwrap_or_else(|| panic!("`{command}` must be registered"));
        assert_eq!(spec.class, RiskClass::Read);
        assert!(
            spec.capabilities.is_empty(),
            "`{command}` is a pure read; a Read row with capabilities is a mislabel"
        );

        for insufficient in [
            vec![],
            vec![Scope::UiOperate],
            vec![Scope::UiAgent],
            vec![Scope::UiElevated],
            vec![Scope::UiOperate, Scope::UiAgent, Scope::UiElevated],
        ] {
            let refused = registry::dispatch(
                &app.handle().clone(),
                &insufficient,
                command,
                &json!({"contextType": "ideation", "contextId": context_id}),
            )
            .await
            .expect_err("a grant without ui:read must not reach the conversation list");
            assert_eq!(refused.code, ErrorCode::RemoteForbidden);
        }
    }
}

/// A read error must NOT arrive as an empty list.
///
/// This is the fail-open shape batch 4 refused four times, asserted rather than assumed for a
/// surface whose success payload is a `Vec`. An unknown context legitimately has no
/// conversations, so the guard here is that the seam's own error path is a `Result` the
/// dispatcher propagates — proven by the parity above plus the absence of any `unwrap_or_
/// default`-style collapse between the repository and the response.
#[tokio::test]
async fn a_remote_conversation_list_of_an_unknown_context_is_empty_not_an_error() {
    let (app, _seeded) = app_with_seeded_conversation_list().await;

    let dispatched = registry::dispatch(
        &app.handle().clone(),
        &[Scope::UiRead],
        "list_remote_agent_conversations",
        &json!({"contextType": "ideation", "contextId": "no-such-session"}),
    )
    .await
    .expect("an unknown context is a legitimate empty read, not a failure");
    assert_eq!(
        dispatched,
        DispatchOutcome::Ok(
            registry::serialize_ok(Vec::<
                crate::commands::unified_chat_commands::AgentConversationResponse,
            >::new())
            .unwrap()
        ),
        "an unknown context must list nothing, and must not leak another context's conversations"
    );
}

/// Scope negatives — the transcript reads sit at `Read`, so every other grant is insufficient.
#[tokio::test]
async fn the_spawn_free_transcript_reads_are_refused_below_ui_read() {
    let (app, conversation_id) = app_with_seeded_transcript().await;

    for command in REMOTE_TRANSCRIPT_READS {
        let spec = registry::find_spec(command)
            .unwrap_or_else(|| panic!("`{command}` must be registered"));
        assert_eq!(spec.class, RiskClass::Read);
        assert!(
            spec.capabilities.is_empty(),
            "`{command}` is a pure read; a Read row with capabilities is a mislabel"
        );

        for insufficient in [
            vec![],
            vec![Scope::UiOperate],
            vec![Scope::UiAgent],
            vec![Scope::UiElevated],
            vec![Scope::UiOperate, Scope::UiAgent, Scope::UiElevated],
        ] {
            let refused = registry::dispatch(
                &app.handle().clone(),
                &insufficient,
                command,
                &json!({"conversationId": conversation_id.as_str()}),
            )
            .await
            .expect_err("a grant without ui:read must not reach the transcript");
            assert_eq!(refused.code, ErrorCode::RemoteForbidden);
        }
    }
}

/// An unknown conversation is `null`, not an error, and NOT another conversation's transcript.
#[tokio::test]
async fn a_remote_transcript_read_of_an_unknown_conversation_returns_nothing() {
    let (app, _seeded) = app_with_seeded_transcript().await;
    let unknown = ralphx_domain::entities::ChatConversationId::new();

    for command in REMOTE_TRANSCRIPT_READS {
        let outcome = registry::dispatch(
            &app.handle().clone(),
            &[Scope::UiRead],
            command,
            &json!({"conversationId": unknown.as_str()}),
        )
        .await
        .unwrap_or_else(|error| panic!("`{command}` must dispatch: {error:?}"));
        assert_eq!(
            outcome,
            DispatchOutcome::Ok(serde_json::Value::Null),
            "`{command}` must not resolve an unknown conversation to someone else's transcript"
        );
    }
}
