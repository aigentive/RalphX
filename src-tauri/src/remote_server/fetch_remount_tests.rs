//! P-1 + R-8: the curated fetch remount, driven through the PRODUCTION router entry path.
//!
//! Every routing scenario builds `authenticated_remote_routes` — the same function the listener
//! calls — so bearer auth, trust-header stripping and the per-route scope gate are all real. The
//! absence assertions are the point: a remount bug does not look like a wrong status code on a
//! mounted route, it looks like an UNLISTED route answering, or a scope-less device being served.

use std::sync::Arc;

use axum::{
    body::{to_bytes, Body},
    http::{header, Method, Request, StatusCode},
    Router,
};
use ralphx_remote_protocol::{RiskClass, Scope};
use serde_json::{json, Value};
use tower::ServiceExt;

use super::auth::RemoteAuthContext;
use super::auth_tests::{in_memory_auth_context, pair_device_with_scopes, TEST_ENVIRONMENT_ID};
use super::authenticated_remote_routes;
use super::endpoints::RemoteRouterState;
use super::fetch_remount::{
    remount_router, RemountMethod, SharedHttpAppState, REMOTE_FETCH_BODY_LIMIT_BYTES,
    REMOUNT_ALLOWLIST, REMOUNT_DENIED_SINKS,
};
use super::invoke::RemoteInvokeDispatcher;
use super::registry::{scope_for_class, DispatchOutcome, RemoteInvokeError};
use crate::application::AppState;
use crate::commands::ExecutionState;
use crate::domain::entities::RemoteScopeSet;

// ---------------------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------------------

/// The remount never dispatches through the facade; this double exists only to satisfy the
/// router state, and it PANICS so a test that accidentally routes into `/invoke` is loud.
struct UnusedDispatcher;

#[async_trait::async_trait]
impl RemoteInvokeDispatcher for UnusedDispatcher {
    async fn dispatch(
        &self,
        _scopes: &[Scope],
        command: &str,
        _args: &Value,
    ) -> Result<DispatchOutcome, RemoteInvokeError> {
        panic!("the fetch remount must never reach the invoke facade (command `{command}`)");
    }
}

fn shared_state() -> Arc<SharedHttpAppState> {
    Arc::new(SharedHttpAppState::new(
        Arc::new(AppState::new_sqlite_test()),
        Arc::new(ExecutionState::new()),
    ))
}

/// The production router WITH the remount wired.
fn router_with_remount(auth: &RemoteAuthContext, shared: Arc<SharedHttpAppState>) -> Router {
    authenticated_remote_routes(
        RemoteRouterState::new_with_invoke_dispatcher(
            TEST_ENVIRONMENT_ID,
            auth.clone(),
            Arc::new(UnusedDispatcher),
        )
        .with_remount(shared),
    )
}

/// The production router with NO shared state — the fail-closed shape.
fn router_without_remount(auth: &RemoteAuthContext) -> Router {
    authenticated_remote_routes(RemoteRouterState::new_with_invoke_dispatcher(
        TEST_ENVIRONMENT_ID,
        auth.clone(),
        Arc::new(UnusedDispatcher),
    ))
}

/// Substitutes a concrete value for every `:param` segment so the request actually matches the
/// mounted path pattern instead of 404-ing on shape.
fn concrete_path(pattern: &str) -> String {
    pattern
        .split('/')
        .map(|segment| {
            if segment.starts_with(':') {
                "00000000-0000-4000-8000-000000000000"
            } else {
                segment
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn request_for(path: &str, method: RemountMethod, token: Option<&str>) -> Request<Body> {
    let builder = Request::builder()
        .uri(concrete_path(path))
        .method(match method {
            RemountMethod::Get => Method::GET,
            RemountMethod::Post => Method::POST,
        });
    let builder = match token {
        Some(token) => builder.header(header::AUTHORIZATION, format!("Bearer {token}")),
        None => builder,
    };
    match method {
        RemountMethod::Get => builder.body(Body::empty()),
        RemountMethod::Post => builder
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(json!({}).to_string())),
    }
    .expect("request should build")
}

async fn body_json(response: axum::response::Response) -> Value {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body should read");
    serde_json::from_slice(&bytes).unwrap_or(Value::Null)
}

async fn read_device(auth: &RemoteAuthContext) -> String {
    pair_device_with_scopes(auth, "reader", RemoteScopeSet::from_scopes([Scope::UiRead]))
        .await
        .0
}

/// A device with real grants that deliberately EXCLUDE `ui:read`.
async fn operate_only_device(auth: &RemoteAuthContext) -> String {
    pair_device_with_scopes(
        auth,
        "operator",
        RemoteScopeSet::from_scopes([Scope::UiOperate, Scope::UiAgent]),
    )
    .await
    .0
}

/// Describes the response as either the listener's unlisted-route refusal or what it was
/// instead, so a failure names the actual status rather than just "not 404".
async fn unavailability(response: axum::response::Response) -> Result<(), String> {
    let status = response.status();
    let body = body_json(response).await;
    if status == StatusCode::NOT_FOUND && body["code"] == json!("REMOTE_COMMAND_UNAVAILABLE") {
        return Ok(());
    }
    Err(format!("status {status}, body {body}"))
}

async fn is_unavailable(response: axum::response::Response) -> bool {
    unavailability(response).await.is_ok()
}

// ---------------------------------------------------------------------------------------
// The checked-in artifact
// ---------------------------------------------------------------------------------------

#[test]
fn the_allowlist_lists_no_route_twice() {
    for (index, route) in REMOUNT_ALLOWLIST.iter().enumerate() {
        let duplicate = REMOUNT_ALLOWLIST
            .iter()
            .skip(index + 1)
            .any(|other| other.path == route.path && other.method == route.method);
        assert!(
            !duplicate,
            "`{}` appears twice in REMOUNT_ALLOWLIST",
            route.path
        );
    }
}

#[test]
fn every_allowlist_row_carries_a_classification_and_a_reason() {
    for route in REMOUNT_ALLOWLIST {
        assert!(
            scope_for_class(route.class).is_some(),
            "`{}` is classified `{:?}`, which maps to no scope; a Denied row must not be mounted",
            route.path,
            route.class
        );
        assert!(
            !route.reason.trim().is_empty(),
            "`{}` has no recorded classification reason",
            route.path
        );
        assert!(
            route.path.starts_with("/api/"),
            "`{}` is not a :3847 fetch path",
            route.path
        );
    }
}

/// v1 is deliberately read-only: a proxied fetch carries no `requestId`, so a mounted mutating
/// route would be at-least-once on retry. If this ever fails, the dedup story must move first.
#[test]
fn v1_mounts_read_class_routes_only() {
    for route in REMOUNT_ALLOWLIST {
        assert_eq!(
            route.class,
            RiskClass::Read,
            "`{}` is not Read-class; mutating fetch routes have no idempotency substrate",
            route.path
        );
    }
}

#[test]
fn the_named_denied_sinks_are_recorded_with_their_reason() {
    for expected in ["POST /api/permission/resolve", "POST /api/question/resolve"] {
        let row = REMOUNT_DENIED_SINKS
            .iter()
            .find(|(name, _)| *name == expected)
            .unwrap_or_else(|| panic!("`{expected}` must be named in REMOUNT_DENIED_SINKS"));
        assert_eq!(
            row.1, "unvalidated-dual-decision-resolve",
            "`{expected}` must record the dual-decision reason"
        );
    }
}

/// The named sinks are additionally proven absent from the positive allowlist, so the two
/// artifacts can never contradict each other.
#[test]
fn no_denied_sink_appears_in_the_allowlist() {
    for (name, _) in REMOUNT_DENIED_SINKS {
        let path = name.split_whitespace().nth(1).expect("`METHOD /path` form");
        assert!(
            !REMOUNT_ALLOWLIST.iter().any(|route| route.path == path),
            "`{path}` is both denied and allowlisted"
        );
    }
}

// ---------------------------------------------------------------------------------------
// P-1: the mounted set equals the allowlist
// ---------------------------------------------------------------------------------------

#[test]
fn the_builder_mounts_exactly_the_allowlist() {
    let shared = shared_state();
    let build = remount_router(&shared).expect("the allowlist should build");
    let expected: Vec<_> = REMOUNT_ALLOWLIST
        .iter()
        .map(|route| (route.path, route.method))
        .collect();
    assert_eq!(
        build.mounted, expected,
        "the builder must mount the allowlist, in full and in order"
    );
}

#[tokio::test]
async fn every_allowlisted_route_is_reachable_with_the_required_scope() {
    let auth = in_memory_auth_context();
    let token = read_device(&auth).await;
    let shared = shared_state();

    for route in REMOUNT_ALLOWLIST {
        let response = router_with_remount(&auth, Arc::clone(&shared))
            .oneshot(request_for(route.path, route.method, Some(&token)))
            .await
            .expect("request should complete");
        assert!(
            !is_unavailable(response).await,
            "`{}` is allowlisted but answered with the unlisted-route refusal",
            route.path
        );
    }
}

/// The negative half of route-set equality: real :3847 paths that this slice deliberately does
/// NOT mount must stay unreachable, including both named denied sinks.
#[tokio::test]
async fn unlisted_backend_paths_stay_unavailable() {
    let auth = in_memory_auth_context();
    let token = read_device(&auth).await;
    let shared = shared_state();

    // Every one of these is a live route on :3847 today.
    let unmounted = [
        ("/api/permission/resolve", RemountMethod::Post),
        ("/api/question/resolve", RemountMethod::Post),
        ("/api/approve_task", RemountMethod::Post),
        ("/api/request_task_changes", RemountMethod::Post),
        ("/api/update_task", RemountMethod::Post),
        ("/api/add_task_note", RemountMethod::Post),
        ("/api/get_automation", RemountMethod::Post),
        ("/api/update_automation", RemountMethod::Post),
        ("/api/finalize_automation", RemountMethod::Post),
        ("/api/update_plan_artifact", RemountMethod::Post),
        ("/api/approve_plan_artifact", RemountMethod::Post),
        ("/api/verification/confirm", RemountMethod::Post),
        ("/api/agent_tasks/create", RemountMethod::Post),
        ("/api/agent_tasks/update", RemountMethod::Post),
        ("/api/agent_tasks/claim", RemountMethod::Post),
        ("/api/agent_tasks/complete", RemountMethod::Post),
        ("/api/agent_workflows/runs/start", RemountMethod::Post),
        ("/api/agent_workflows/runs/cancel", RemountMethod::Post),
        ("/api/agent_workflows/scripts/approve", RemountMethod::Post),
        ("/api/conversations/:id/active-state", RemountMethod::Get),
        (
            "/api/agent-workspaces/:id/pr-review-context",
            RemountMethod::Get,
        ),
        (
            "/api/agent-workspaces/:id/file-content-range",
            RemountMethod::Get,
        ),
        (
            "/api/ideation/sessions/:id/child-status",
            RemountMethod::Get,
        ),
        ("/api/internal/projects", RemountMethod::Get),
        ("/api/auth/keys", RemountMethod::Get),
    ];

    // The per-device bucket refills at 10 req/s, so the sweep rotates devices every few paths.
    // Without this the tail of the list would 429 and prove nothing about mounting.
    let mut token = token;
    for (index, (path, method)) in unmounted.into_iter().enumerate() {
        if index != 0 && index % REQUESTS_PER_DEVICE == 0 {
            token = pair_device_with_scopes(
                &auth,
                &format!("reader-{index}"),
                RemoteScopeSet::from_scopes([Scope::UiRead]),
            )
            .await
            .0;
        }
        let response = router_with_remount(&auth, Arc::clone(&shared))
            .oneshot(request_for(path, method, Some(&token)))
            .await
            .expect("request should complete");
        if let Err(actual) = unavailability(response).await {
            panic!(
                "`{path}` is not allowlisted and must answer with the unlisted-route \
                 refusal; got {actual}"
            );
        }
    }
}

/// Comfortably under the 10 req/s per-device token bucket.
const REQUESTS_PER_DEVICE: usize = 6;

// ---------------------------------------------------------------------------------------
// Scope enforcement — identical policy to /invoke
// ---------------------------------------------------------------------------------------

/// A device with genuine, HIGHER grants but no `ui:read` is refused on every mounted route:
/// scope is per-class, not "any grant will do".
#[tokio::test]
async fn a_device_without_the_required_scope_is_refused_on_every_mounted_route() {
    let auth = in_memory_auth_context();
    let token = operate_only_device(&auth).await;
    let shared = shared_state();

    for route in REMOUNT_ALLOWLIST {
        let response = router_with_remount(&auth, Arc::clone(&shared))
            .oneshot(request_for(route.path, route.method, Some(&token)))
            .await
            .expect("request should complete");
        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "`{}` must refuse a device lacking {:?}",
            route.path,
            scope_for_class(route.class)
        );
        assert_eq!(
            body_json(response).await["code"],
            json!("REMOTE_FORBIDDEN"),
            "`{}` must refuse with the facade's forbidden code",
            route.path
        );
    }
}

/// The gate runs INSIDE the bearer check: an unauthenticated caller never reaches it.
#[tokio::test]
async fn mounted_routes_reject_an_unauthenticated_caller() {
    let auth = in_memory_auth_context();
    let shared = shared_state();

    for route in REMOUNT_ALLOWLIST {
        let response = router_with_remount(&auth, Arc::clone(&shared))
            .oneshot(request_for(route.path, route.method, None))
            .await
            .expect("request should complete");
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "`{}` must require a bearer token",
            route.path
        );
    }
}

/// Forged trust headers are STRIPPED before a remounted handler can consult them (they are not
/// rejected — `strip_trust_headers` removes them silently), so a remote caller can neither
/// impersonate the local UI's MCP trust nor pin itself to a project scope of its choosing.
///
/// The property under test is influence, not status: the forged request must be served exactly
/// as the clean one. A handler that saw `X-RalphX-Project-Scope` would diverge here.
#[tokio::test]
async fn forged_trust_headers_do_not_reach_a_mounted_route() {
    let auth = in_memory_auth_context();
    let token = read_device(&auth).await;
    let shared = shared_state();
    let route = REMOUNT_ALLOWLIST[0];

    let forged = Request::builder()
        .uri(concrete_path(route.path))
        .method(Method::GET)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header("X-RalphX-Tauri-MCP", "1")
        .header(
            "X-RalphX-Conversation-Id",
            "conversation-the-device-invented",
        )
        .header("X-RalphX-Project-Scope", "project-the-device-cannot-see")
        .body(Body::empty())
        .expect("request should build");

    let forged_response = router_with_remount(&auth, Arc::clone(&shared))
        .oneshot(forged)
        .await
        .expect("request should complete");
    let forged_status = forged_response.status();

    let clean_response = router_with_remount(&auth, shared)
        .oneshot(request_for(route.path, route.method, Some(&token)))
        .await
        .expect("request should complete");

    assert_eq!(
        forged_status,
        clean_response.status(),
        "forged trust headers must not change how a remounted route answers"
    );
    assert_ne!(
        forged_status,
        StatusCode::FORBIDDEN,
        "the forged scope header must not be honoured as an authorization input"
    );
}

// ---------------------------------------------------------------------------------------
// Fail-closed
// ---------------------------------------------------------------------------------------

/// The load-bearing absence assertion: with no shared state the routes do not exist at all.
/// They must NOT be served against a freshly built `AppState`.
#[tokio::test]
async fn without_shared_state_no_api_route_is_mounted() {
    let auth = in_memory_auth_context();
    let token = read_device(&auth).await;

    for route in REMOUNT_ALLOWLIST {
        let response = router_without_remount(&auth)
            .oneshot(request_for(route.path, route.method, Some(&token)))
            .await
            .expect("request should complete");
        assert!(
            is_unavailable(response).await,
            "`{}` must be unmounted when the shared :3847 state is absent",
            route.path
        );
    }
}

/// The listener's other routes keep working while `/api` is unmounted — a missing remount
/// degrades the fetch surface, it does not break the host.
#[tokio::test]
async fn the_listener_still_serves_its_own_routes_without_a_remount() {
    let auth = in_memory_auth_context();
    let token = read_device(&auth).await;

    let request = Request::builder()
        .uri(super::HEALTH_PATH)
        .method(Method::GET)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .expect("request should build");

    let response = router_without_remount(&auth)
        .oneshot(request)
        .await
        .expect("request should complete");
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn a_mounted_post_route_enforces_the_fetch_body_limit() {
    let auth = in_memory_auth_context();
    let token = read_device(&auth).await;
    let shared = shared_state();
    let post_route = REMOUNT_ALLOWLIST
        .iter()
        .find(|route| route.method == RemountMethod::Post)
        .expect("the allowlist should contain a POST route");

    let oversized = json!({ "padding": "x".repeat(REMOTE_FETCH_BODY_LIMIT_BYTES + 1) });
    let request = Request::builder()
        .uri(concrete_path(post_route.path))
        .method(Method::POST)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(oversized.to_string()))
        .expect("request should build");

    let response = router_with_remount(&auth, shared)
        .oneshot(request)
        .await
        .expect("request should complete");
    assert_eq!(
        response.status(),
        StatusCode::PAYLOAD_TOO_LARGE,
        "an oversized body must be refused before the handler deserializes it"
    );
}

// ---------------------------------------------------------------------------------------
// R-8: Arc identity
// ---------------------------------------------------------------------------------------

/// The whole point of the newtype: the remount reads the SAME memory :3847 does.
#[test]
fn the_shared_state_hands_out_the_same_arcs_it_was_built_from() {
    let app_state = Arc::new(AppState::new_sqlite_test());
    let execution_state = Arc::new(ExecutionState::new());
    let shared = SharedHttpAppState::new(Arc::clone(&app_state), Arc::clone(&execution_state));

    let http_state = shared.http_server_state();
    assert!(
        Arc::ptr_eq(&http_state.app_state, &app_state),
        "the remount must reuse the :3847 AppState, never a rebuild"
    );
    assert!(
        Arc::ptr_eq(&http_state.execution_state, &execution_state),
        "the remount must reuse the :3847 ExecutionState"
    );
    assert!(Arc::ptr_eq(shared.app_state(), &app_state));
    assert!(Arc::ptr_eq(shared.execution_state(), &execution_state));
}

/// Repeated resolution must not drift: two requests served by the same listener see one graph.
#[test]
fn every_resolution_of_the_shared_state_yields_the_same_arcs() {
    let shared = shared_state();
    let first = shared.http_server_state();
    let second = shared.http_server_state();

    assert!(Arc::ptr_eq(&first.app_state, &second.app_state));
    assert!(Arc::ptr_eq(&first.execution_state, &second.execution_state));
    assert!(
        Arc::ptr_eq(&first.delegation_service, &second.delegation_service),
        "the delegation service is built once, not per request"
    );
}

/// Field-level identity across the `AppState` graph the remount serves from.
///
/// `build_http_app_state` is the function that decides which fields the :3847 `AppState` shares
/// with the Tauri-managed one. This test pins the enumeration against the live source, so a
/// field silently dropped from that list fails here instead of surfacing as a remote client
/// reading different authority state than the local UI.
#[test]
fn the_r8_shared_field_enumeration_matches_build_http_app_state() {
    let source = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/application/runtime_wiring.rs"
    ))
    .expect("runtime_wiring.rs must be readable; a missing source fails CLOSED");

    let start = source
        .find("pub fn build_http_app_state")
        .expect("build_http_app_state must exist");
    let body = &source[start..];

    for field in R8_SHARED_FIELDS {
        assert!(
            body.contains(field),
            "`{field}` is documented as shared with :3847 but no longer appears in \
             build_http_app_state; update docs/architecture/remote-r8-appstate.md"
        );
    }
    assert!(
        !R8_SHARED_FIELDS.is_empty(),
        "the shared-field enumeration must not be empty"
    );
}

/// The fields `build_http_app_state` Arc-shares with the Tauri-managed `AppState`, mirrored in
/// `docs/architecture/remote-r8-appstate.md`. Every repository not listed here is a fresh struct
/// over the SHARED `db` connection, so its reads are still identical.
const R8_SHARED_FIELDS: &[&str] = &[
    "question_state",
    "permission_state",
    "message_queue",
    "queued_message_repo",
    "interactive_process_registry",
    "github_service",
    "pr_poller_registry",
    "streaming_state_cache",
    "webhook_publisher",
    "session_merge_locks",
    "window_focus_state",
    "notification_service_cache",
    "agent_capability_gate",
    "share_startup_coordinator",
    "share_plan_verification_runtime",
];

/// `DelegationService` is the one deliberately FRESH field. It backs `/api/internal/*` only,
/// which this module never mounts — proven by scanning the mounted handlers' own sources.
#[test]
fn no_mounted_route_touches_delegation_service() {
    for (relative, function) in MOUNTED_HANDLERS {
        let body = handler_body(relative, function);
        assert!(
            !body.contains("delegation_service"),
            "`{function}` reads delegation_service, which the remount supplies FRESH; \
             either share it or drop the route"
        );
    }
}

/// Reads one handler's body out of its source file.
///
/// Scoped to the function rather than the file on purpose: `agent_workflows.rs` also holds the
/// workflow RUNNER, which legitimately drives `delegation_service`. A file-wide scan would
/// conflate the runner with the two read handlers this slice actually mounts.
fn handler_body(relative: &str, function: &str) -> String {
    let path = format!(
        "{}/src/http_server/handlers/{relative}",
        env!("CARGO_MANIFEST_DIR")
    );
    let source = std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!("{path} must be readable; a missing source fails CLOSED: {error}")
    });
    let signature = format!("pub async fn {function}(");
    let start = source
        .find(&signature)
        .unwrap_or_else(|| panic!("`{function}` must exist in {relative}"));
    let tail = &source[start..];
    // Handler bodies end at the first column-0 closing brace.
    let end = tail
        .find("\n}\n")
        .unwrap_or_else(|| panic!("`{function}` must have a terminating brace"));
    tail[..end].to_string()
}

/// Every handler behind `REMOUNT_ALLOWLIST`, as (source file, function name).
const MOUNTED_HANDLERS: &[(&str, &str)] = &[
    ("artifacts/query.rs", "get_session_plan"),
    ("plan_complexity.rs", "get_plan_complexity_assessment"),
    ("ideation/verification/query.rs", "get_plan_verification"),
    ("agent_tasks.rs", "list_agent_tasks"),
    ("agent_tasks.rs", "list_agent_task_lists"),
    ("agent_tasks.rs", "list_agent_tasks_for_list"),
    ("agent_workflows.rs", "get_agent_workflow_run"),
    (
        "agent_workflows.rs",
        "get_latest_agent_workflow_run_for_script",
    ),
];

/// The allowlist and the handler inventory must describe the same surface.
#[test]
fn the_handler_inventory_covers_every_allowlist_row() {
    assert_eq!(
        MOUNTED_HANDLERS.len(),
        REMOUNT_ALLOWLIST.len(),
        "every mounted route needs an inventory entry for the fresh-field audit"
    );
}
