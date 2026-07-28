//! P-20: request idempotency through the PRODUCTION router entry path.
//!
//! Every scenario drives `authenticated_remote_routes` → bearer auth → `invoke_handler` →
//! the real `RemoteDedupState` → the real SQLite store. The dispatcher is the only double,
//! and it exists to COUNT side effects: an idempotency test that cannot count executions
//! proves nothing, because the failure mode is a second execution, not a wrong status code.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use axum::{
    body::to_bytes,
    body::Body,
    http::{header, Method, Request, StatusCode},
    Router,
};
use ralphx_remote_protocol::{ErrorCode, RiskClass, Scope};
use serde_json::{json, Value};
use tower::ServiceExt;

use super::auth::RemoteAuthContext;
use super::auth_tests::{pair_device_with_scopes, TEST_ENVIRONMENT_ID};
use super::dedup::{
    args_hash, class_requires_dedup, DedupDecision, RemoteDedupState,
    REMOTE_INVOKE_BODY_LIMIT_BYTES,
};
use super::endpoints::RemoteRouterState;
use super::invoke::RemoteInvokeDispatcher;
use super::registry::{DispatchOutcome, RemoteInvokeError};
use super::session_registry::RemoteSessionRegistry;
use super::settings::RemoteExposureMode;
use super::{authenticated_remote_routes, INVOKE_PATH};
use crate::domain::entities::{
    RemoteDedupOutcomeKind, RemoteDeviceId, RemoteRequestDedupRecord, RemoteScopeSet,
};
use crate::domain::repositories::{RemoteRequestDedupLookup, RemoteRequestDedupRepository};
use crate::error::{AppError, AppResult};
use crate::infrastructure::sqlite::migrations::run_migrations;
use crate::infrastructure::sqlite::{DbConnection, SqliteRemoteRequestDedupRepository};

/// A registered `Operate`-class command. Dedup applies.
const MUTATING_COMMAND: &str = "create_task";
/// A registered `Read`-class command. Dedup is exempt by spec.
const READ_COMMAND: &str = "list_tasks";

// ---------------------------------------------------------------------------------------
// Doubles: the dispatcher COUNTS, the failing stores INJECT
// ---------------------------------------------------------------------------------------

/// Counts dispatches so every absence claim below is an assertion on executions, not status.
struct CountingDispatcher {
    calls: Arc<AtomicUsize>,
    outcome: DispatchOutcome,
    facade_error: Option<RemoteInvokeError>,
}

impl CountingDispatcher {
    fn ok() -> (Arc<Self>, Arc<AtomicUsize>) {
        Self::build(DispatchOutcome::Ok(json!({"taskId": "task-1"})), None)
    }

    fn command_error() -> (Arc<Self>, Arc<AtomicUsize>) {
        Self::build(
            DispatchOutcome::Err(json!("the task could not be created")),
            None,
        )
    }

    fn facade_error(error: RemoteInvokeError) -> (Arc<Self>, Arc<AtomicUsize>) {
        Self::build(DispatchOutcome::Ok(Value::Null), Some(error))
    }

    fn build(
        outcome: DispatchOutcome,
        facade_error: Option<RemoteInvokeError>,
    ) -> (Arc<Self>, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        (
            Arc::new(Self {
                calls: calls.clone(),
                outcome,
                facade_error,
            }),
            calls,
        )
    }
}

#[async_trait]
impl RemoteInvokeDispatcher for CountingDispatcher {
    async fn dispatch(
        &self,
        _scopes: &[Scope],
        _command: &str,
        _args: &Value,
    ) -> Result<DispatchOutcome, RemoteInvokeError> {
        if let Some(error) = &self.facade_error {
            // A facade rejection happens BEFORE the target command runs, so it must not count
            // as an execution.
            return Err(error.clone());
        }
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.outcome.clone())
    }
}

/// A store whose `lookup` always fails, to prove the read fails CLOSED.
struct FailingLookupStore;

#[async_trait]
impl RemoteRequestDedupRepository for FailingLookupStore {
    async fn lookup(
        &self,
        _device_id: &RemoteDeviceId,
        _request_id: &str,
        _now: &str,
    ) -> AppResult<RemoteRequestDedupLookup> {
        Err(AppError::Database("dedup store is down".to_string()))
    }

    async fn record(&self, _record: RemoteRequestDedupRecord) -> AppResult<()> {
        Ok(())
    }

    async fn purge_expired(&self, _now: &str) -> AppResult<usize> {
        Ok(0)
    }
}

/// A store whose `record` always fails, to pin the documented (d3) residual.
struct FailingRecordStore {
    inner: Arc<SqliteRemoteRequestDedupRepository>,
}

#[async_trait]
impl RemoteRequestDedupRepository for FailingRecordStore {
    async fn lookup(
        &self,
        device_id: &RemoteDeviceId,
        request_id: &str,
        now: &str,
    ) -> AppResult<RemoteRequestDedupLookup> {
        self.inner.lookup(device_id, request_id, now).await
    }

    async fn record(&self, _record: RemoteRequestDedupRecord) -> AppResult<()> {
        Err(AppError::Database("dedup record write failed".to_string()))
    }

    async fn purge_expired(&self, now: &str) -> AppResult<usize> {
        self.inner.purge_expired(now).await
    }
}

// ---------------------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------------------

/// Auth context and dedup store over ONE migrated connection, mirroring production wiring.
fn shared_context() -> (RemoteAuthContext, Arc<SqliteRemoteRequestDedupRepository>) {
    let conn = rusqlite::Connection::open_in_memory().expect("in-memory database should open");
    run_migrations(&conn).expect("migrations should apply");
    let shared = Arc::new(tokio::sync::Mutex::new(conn));
    let auth = RemoteAuthContext::from_db(
        DbConnection::from_shared(shared.clone()),
        RemoteSessionRegistry::new(),
        RemoteExposureMode::Serve,
    );
    let store = Arc::new(SqliteRemoteRequestDedupRepository::from_db(
        DbConnection::from_shared(shared),
    ));
    (auth, store)
}

fn router(
    auth: &RemoteAuthContext,
    dispatcher: Arc<dyn RemoteInvokeDispatcher>,
    store: Arc<dyn RemoteRequestDedupRepository>,
) -> Router {
    authenticated_remote_routes(
        RemoteRouterState::new_with_invoke_dispatcher(
            TEST_ENVIRONMENT_ID,
            auth.clone(),
            dispatcher,
        )
        .with_dedup(Arc::new(RemoteDedupState::new(store))),
    )
}

/// A router with NO dedup state, to prove mutating commands are refused rather than dispatched.
fn router_without_dedup(
    auth: &RemoteAuthContext,
    dispatcher: Arc<dyn RemoteInvokeDispatcher>,
) -> Router {
    authenticated_remote_routes(RemoteRouterState::new_with_invoke_dispatcher(
        TEST_ENVIRONMENT_ID,
        auth.clone(),
        dispatcher,
    ))
}

fn invoke(token: &str, request_id: &str, cmd: &str, args: Value) -> Request<Body> {
    Request::builder()
        .method(Method::POST)
        .uri(INVOKE_PATH)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::from(
            json!({"requestId": request_id, "cmd": cmd, "args": args}).to_string(),
        ))
        .expect("request should build")
}

async fn body_json(response: axum::response::Response) -> Value {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body should read");
    serde_json::from_slice(&bytes).expect("body should be JSON")
}

async fn operate_device(auth: &RemoteAuthContext) -> (String, RemoteDeviceId) {
    pair_device_with_scopes(
        auth,
        "phone",
        RemoteScopeSet::from_scopes([Scope::UiRead, Scope::UiOperate]),
    )
    .await
}

// ---------------------------------------------------------------------------------------
// Hashing and class policy
// ---------------------------------------------------------------------------------------

/// A retry that re-serializes its args from a hash map must not read as a different request.
#[test]
fn the_args_hash_is_stable_under_key_reordering_but_not_under_value_changes() {
    let a = args_hash("create_task", &json!({"title": "x", "projectId": "p"}));
    let b = args_hash("create_task", &json!({"projectId": "p", "title": "x"}));
    assert_eq!(a, b, "key order is not part of the request's identity");

    assert_ne!(
        a,
        args_hash("create_task", &json!({"title": "y", "projectId": "p"})),
        "a changed value must change the hash"
    );
    assert_ne!(
        a,
        args_hash("update_task", &json!({"title": "x", "projectId": "p"})),
        "the command name is part of the binding"
    );
    // Array order IS semantic and must remain part of the identity.
    assert_ne!(
        args_hash("create_task", &json!({"tags": ["a", "b"]})),
        args_hash("create_task", &json!({"tags": ["b", "a"]}))
    );
}

/// Length-prefixing the command name stops a boundary shift from colliding two requests.
#[test]
fn command_and_args_boundaries_cannot_be_shifted_into_a_collision() {
    assert_ne!(
        args_hash("ab", &json!("c")),
        args_hash("a", &json!("bc")),
        "cmd/args boundary must not be ambiguous"
    );
}

#[test]
fn only_read_class_is_exempt_from_dedup() {
    assert!(!class_requires_dedup(RiskClass::Read));
    for class in [
        RiskClass::Operate,
        RiskClass::PathScoped,
        RiskClass::AgentControl,
        RiskClass::Elevated,
        RiskClass::Denied,
    ] {
        assert!(
            class_requires_dedup(class),
            "{class:?} must participate in dedup"
        );
    }
}

// ---------------------------------------------------------------------------------------
// (a) Replay returns the cached result without re-executing
// ---------------------------------------------------------------------------------------

#[tokio::test]
async fn a_replayed_mutating_request_returns_the_cached_result_and_does_not_re_execute() {
    let (auth, store) = shared_context();
    let (token, _) = operate_device(&auth).await;
    let (dispatcher, calls) = CountingDispatcher::ok();

    let first = router(&auth, dispatcher.clone(), store.clone())
        .oneshot(invoke(
            &token,
            "req-1",
            MUTATING_COMMAND,
            json!({"title": "a"}),
        ))
        .await
        .expect("first request should complete");
    assert_eq!(first.status(), StatusCode::OK);
    let first_body = body_json(first).await;

    let second = router(&auth, dispatcher, store)
        .oneshot(invoke(
            &token,
            "req-1",
            MUTATING_COMMAND,
            json!({"title": "a"}),
        ))
        .await
        .expect("replay should complete");
    assert_eq!(second.status(), StatusCode::OK);
    assert_eq!(
        body_json(second).await,
        first_body,
        "the replay must be byte-identical to the original envelope"
    );

    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "ABSENCE: the command must have executed exactly once"
    );
}

/// A command-level `Err` is a COMPLETED outcome: the command ran and decided.
#[tokio::test]
async fn a_replayed_command_level_error_is_cached_and_does_not_re_execute() {
    let (auth, store) = shared_context();
    let (token, _) = operate_device(&auth).await;
    let (dispatcher, calls) = CountingDispatcher::command_error();

    let first = router(&auth, dispatcher.clone(), store.clone())
        .oneshot(invoke(&token, "req-err", MUTATING_COMMAND, json!({})))
        .await
        .expect("first request should complete");
    assert_eq!(first.status(), StatusCode::OK);
    let first_body = body_json(first).await;
    assert_eq!(first_body["ok"], json!(false));

    let second = router(&auth, dispatcher, store)
        .oneshot(invoke(&token, "req-err", MUTATING_COMMAND, json!({})))
        .await
        .expect("replay should complete");
    assert_eq!(body_json(second).await, first_body);

    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "ABSENCE: a cached business error must not re-run the command"
    );
}

// ---------------------------------------------------------------------------------------
// (b) Simultaneous duplicates execute exactly once
// ---------------------------------------------------------------------------------------

#[tokio::test]
async fn a_duplicate_arriving_while_the_first_is_in_flight_is_refused_not_executed() {
    let (auth, store) = shared_context();
    let (token, device) = operate_device(&auth).await;
    let (dispatcher, calls) = CountingDispatcher::ok();
    let dedup = Arc::new(RemoteDedupState::new(store.clone()));
    let app = authenticated_remote_routes(
        RemoteRouterState::new_with_invoke_dispatcher(
            TEST_ENVIRONMENT_ID,
            auth.clone(),
            dispatcher.clone(),
        )
        .with_dedup(dedup.clone()),
    );

    // Claim the slot exactly as the handler would, then send the duplicate: this is the lost
    // race, deterministically reproduced.
    let holder = dedup
        .begin(&device, "race-1", MUTATING_COMMAND, &json!({"title": "a"}))
        .await
        .expect("the first claim should succeed");
    assert!(matches!(holder, DedupDecision::Proceed));

    let response = app
        .oneshot(invoke(
            &token,
            "race-1",
            MUTATING_COMMAND,
            json!({"title": "a"}),
        ))
        .await
        .expect("duplicate should complete");

    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(
        body_json(response).await["code"],
        json!("REMOTE_REQUEST_IN_PROGRESS")
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "ABSENCE: the loser of the race must not dispatch"
    );
}

/// A duplicate id already in flight for DIFFERENT args is a reuse, not a coalescible duplicate.
#[tokio::test]
async fn an_in_flight_id_reused_for_different_args_is_rejected_as_reuse() {
    let (auth, store) = shared_context();
    let (token, device) = operate_device(&auth).await;
    let (dispatcher, calls) = CountingDispatcher::ok();
    let dedup = Arc::new(RemoteDedupState::new(store.clone()));
    let app = authenticated_remote_routes(
        RemoteRouterState::new_with_invoke_dispatcher(
            TEST_ENVIRONMENT_ID,
            auth.clone(),
            dispatcher,
        )
        .with_dedup(dedup.clone()),
    );

    dedup
        .begin(&device, "race-2", MUTATING_COMMAND, &json!({"title": "a"}))
        .await
        .expect("the first claim should succeed");

    let response = app
        .oneshot(invoke(
            &token,
            "race-2",
            MUTATING_COMMAND,
            json!({"title": "DIFFERENT"}),
        ))
        .await
        .expect("request should complete");

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

// ---------------------------------------------------------------------------------------
// (c) Hash binding
// ---------------------------------------------------------------------------------------

#[tokio::test]
async fn the_same_request_id_with_different_args_is_rejected_and_leaves_the_cache_intact() {
    let (auth, store) = shared_context();
    let (token, _) = operate_device(&auth).await;
    let (dispatcher, calls) = CountingDispatcher::ok();

    let first = router(&auth, dispatcher.clone(), store.clone())
        .oneshot(invoke(
            &token,
            "req-2",
            MUTATING_COMMAND,
            json!({"title": "a"}),
        ))
        .await
        .expect("first request should complete");
    let first_body = body_json(first).await;

    let reused = router(&auth, dispatcher.clone(), store.clone())
        .oneshot(invoke(
            &token,
            "req-2",
            MUTATING_COMMAND,
            json!({"title": "DIFFERENT"}),
        ))
        .await
        .expect("reuse should complete");
    assert_eq!(reused.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        body_json(reused).await["code"],
        json!("REMOTE_REQUEST_ID_REUSED")
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "ABSENCE: a reused id must not dispatch"
    );

    // The original outcome survives the rejected reuse.
    let replay = router(&auth, dispatcher, store)
        .oneshot(invoke(
            &token,
            "req-2",
            MUTATING_COMMAND,
            json!({"title": "a"}),
        ))
        .await
        .expect("replay should complete");
    assert_eq!(body_json(replay).await, first_body);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

/// Key order changing between the original and the retry must NOT read as reuse.
#[tokio::test]
async fn a_retry_that_reorders_its_argument_keys_still_replays() {
    let (auth, store) = shared_context();
    let (token, _) = operate_device(&auth).await;
    let (dispatcher, calls) = CountingDispatcher::ok();

    let first = router(&auth, dispatcher.clone(), store.clone())
        .oneshot(invoke(
            &token,
            "req-order",
            MUTATING_COMMAND,
            json!({"title": "a", "projectId": "p"}),
        ))
        .await
        .expect("first request should complete");
    let first_body = body_json(first).await;

    let replay = router(&auth, dispatcher, store)
        .oneshot(invoke(
            &token,
            "req-order",
            MUTATING_COMMAND,
            json!({"projectId": "p", "title": "a"}),
        ))
        .await
        .expect("replay should complete");
    assert_eq!(replay.status(), StatusCode::OK);
    assert_eq!(body_json(replay).await, first_body);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

// ---------------------------------------------------------------------------------------
// (d) Restart matrix
// ---------------------------------------------------------------------------------------

/// (d1) A claim with no durable row does not survive a restart — and must not, or a crash
/// mid-dispatch would wedge that id into a permanent 409.
#[tokio::test]
async fn d1_a_reservation_without_a_durable_row_does_not_survive_a_restart() {
    let (auth, store) = shared_context();
    let (token, device) = operate_device(&auth).await;
    let (dispatcher, calls) = CountingDispatcher::ok();

    // "Before the restart": a state that claimed the slot and never completed.
    let dead = RemoteDedupState::new(store.clone());
    dead.begin(&device, "req-d1", MUTATING_COMMAND, &json!({}))
        .await
        .expect("claim should succeed");
    drop(dead);

    // A FRESH dedup state over the SAME durable store — the restart.
    let response = router(&auth, dispatcher, store)
        .oneshot(invoke(&token, "req-d1", MUTATING_COMMAND, json!({})))
        .await
        .expect("post-restart request should complete");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "a lost in-flight claim must not block execution after a restart"
    );
}

/// (d2) A durable row DOES survive: the replay is served without a second effect.
#[tokio::test]
async fn d2_a_durable_row_replays_across_a_restart_without_re_executing() {
    let (auth, store) = shared_context();
    let (token, _) = operate_device(&auth).await;
    let (dispatcher, calls) = CountingDispatcher::ok();

    let first = router(&auth, dispatcher.clone(), store.clone())
        .oneshot(invoke(&token, "req-d2", MUTATING_COMMAND, json!({})))
        .await
        .expect("first request should complete");
    let first_body = body_json(first).await;
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    // A brand new router + dedup state, same durable store: the restart.
    let replay = router(&auth, dispatcher, store)
        .oneshot(invoke(&token, "req-d2", MUTATING_COMMAND, json!({})))
        .await
        .expect("replay should complete");
    assert_eq!(body_json(replay).await, first_body);
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "ABSENCE: the durable row must prevent the second execution"
    );
}

/// (d3) The DOCUMENTED residual: effect committed, durable write failed → a replay
/// re-executes. The response still succeeds; the system never reports a stuck state.
#[tokio::test]
async fn d3_a_failed_record_after_a_committed_effect_is_the_documented_re_execution_residual() {
    let (auth, store) = shared_context();
    let (token, _) = operate_device(&auth).await;
    let (dispatcher, calls) = CountingDispatcher::ok();
    let failing: Arc<dyn RemoteRequestDedupRepository> = Arc::new(FailingRecordStore {
        inner: store.clone(),
    });

    let first = router(&auth, dispatcher.clone(), failing.clone())
        .oneshot(invoke(&token, "req-d3", MUTATING_COMMAND, json!({})))
        .await
        .expect("first request should complete");
    assert_eq!(
        first.status(),
        StatusCode::OK,
        "the effect happened; the client must not be told it failed"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    let replay = router(&auth, dispatcher, failing)
        .oneshot(invoke(&token, "req-d3", MUTATING_COMMAND, json!({})))
        .await
        .expect("replay should complete");
    assert_eq!(replay.status(), StatusCode::OK);
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "documented residual: without a durable row the replay re-executes"
    );
}

/// (d4) An expired row is a NEW request.
#[tokio::test]
async fn d4_an_expired_row_is_treated_as_a_new_request() {
    let (auth, store) = shared_context();
    let (token, device) = operate_device(&auth).await;
    let (dispatcher, calls) = CountingDispatcher::ok();

    // Seed a row that is already past its TTL.
    RemoteRequestDedupRepository::record(
        store.as_ref(),
        RemoteRequestDedupRecord {
            device_id: device,
            request_id: "req-d4".to_string(),
            args_hash: args_hash(MUTATING_COMMAND, &json!({})),
            outcome: RemoteDedupOutcomeKind::Ok,
            response: r#"{"ok":true,"result":"stale"}"#.to_string(),
            created_at: "2000-01-01T00:00:00.000Z".to_string(),
            expires_at: "2000-01-01T00:10:00.000Z".to_string(),
        },
    )
    .await
    .expect("expired row should seed");

    let response = router(&auth, dispatcher, store)
        .oneshot(invoke(&token, "req-d4", MUTATING_COMMAND, json!({})))
        .await
        .expect("request should complete");
    let body = body_json(response).await;
    assert_ne!(
        body["result"],
        json!("stale"),
        "an expired row must not be replayed"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

// ---------------------------------------------------------------------------------------
// Fail-closed
// ---------------------------------------------------------------------------------------

#[tokio::test]
async fn a_dedup_store_read_failure_refuses_the_request_and_dispatches_nothing() {
    let (auth, _store) = shared_context();
    let (token, _) = operate_device(&auth).await;
    let (dispatcher, calls) = CountingDispatcher::ok();

    let response = router(&auth, dispatcher, Arc::new(FailingLookupStore))
        .oneshot(invoke(&token, "req-fail", MUTATING_COMMAND, json!({})))
        .await
        .expect("request should complete");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        body_json(response).await["code"],
        json!("REMOTE_INTERNAL_ERROR")
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "ABSENCE: an unreadable dedup store must never be read as 'no record → execute'"
    );
}

#[tokio::test]
async fn a_router_without_a_dedup_store_refuses_mutating_commands_instead_of_dispatching() {
    let (auth, _store) = shared_context();
    let (token, _) = operate_device(&auth).await;
    let (dispatcher, calls) = CountingDispatcher::ok();

    let response = router_without_dedup(&auth, dispatcher)
        .oneshot(invoke(&token, "req-nostate", MUTATING_COMMAND, json!({})))
        .await
        .expect("request should complete");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "ABSENCE: a missing dedup store must not silently restore at-least-once semantics"
    );
}

#[tokio::test]
async fn an_empty_request_id_is_rejected_before_dispatch() {
    let (auth, store) = shared_context();
    let (token, _) = operate_device(&auth).await;
    let (dispatcher, calls) = CountingDispatcher::ok();

    let response = router(&auth, dispatcher, store)
        .oneshot(invoke(&token, "", MUTATING_COMMAND, json!({})))
        .await
        .expect("request should complete");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

// ---------------------------------------------------------------------------------------
// Facade errors release without a record
// ---------------------------------------------------------------------------------------

/// A facade rejection executed nothing, so a corrected retry with the same id must run.
#[tokio::test]
async fn a_facade_error_releases_the_reservation_without_caching_it() {
    let (auth, store) = shared_context();
    let (token, _) = operate_device(&auth).await;
    let (rejecting, rejected_calls) =
        CountingDispatcher::facade_error(RemoteInvokeError::forbidden("nope"));

    let denied = router(&auth, rejecting, store.clone())
        .oneshot(invoke(&token, "req-retry", MUTATING_COMMAND, json!({})))
        .await
        .expect("request should complete");
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    assert_eq!(rejected_calls.load(Ordering::SeqCst), 0);

    // Nothing was cached, so the same id is free — including for different args.
    let (dispatcher, calls) = CountingDispatcher::ok();
    let retry = router(&auth, dispatcher, store)
        .oneshot(invoke(
            &token,
            "req-retry",
            MUTATING_COMMAND,
            json!({"title": "corrected"}),
        ))
        .await
        .expect("retry should complete");
    assert_eq!(
        retry.status(),
        StatusCode::OK,
        "a corrected retry after a facade rejection must be allowed"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

/// An unknown command must not burn the id either — dispatch 404s on its own.
#[tokio::test]
async fn an_unknown_command_does_not_consume_the_request_id() {
    let (auth, store) = shared_context();
    let (token, _) = operate_device(&auth).await;
    let (rejecting, _) =
        CountingDispatcher::facade_error(RemoteInvokeError::unavailable("no_such_command"));

    let unknown = router(&auth, rejecting, store.clone())
        .oneshot(invoke(&token, "req-unknown", "no_such_command", json!({})))
        .await
        .expect("request should complete");
    assert_eq!(unknown.status(), StatusCode::NOT_FOUND);

    let (dispatcher, calls) = CountingDispatcher::ok();
    let corrected = router(&auth, dispatcher, store)
        .oneshot(invoke(&token, "req-unknown", MUTATING_COMMAND, json!({})))
        .await
        .expect("corrected request should complete");
    assert_eq!(corrected.status(), StatusCode::OK);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

// ---------------------------------------------------------------------------------------
// Read exemption
// ---------------------------------------------------------------------------------------

#[tokio::test]
async fn a_replayed_read_dispatches_again_and_writes_no_dedup_row() {
    let (auth, store) = shared_context();
    let (token, device) = operate_device(&auth).await;
    let (dispatcher, calls) = CountingDispatcher::ok();

    for _ in 0..2 {
        let response = router(&auth, dispatcher.clone(), store.clone())
            .oneshot(invoke(&token, "req-read", READ_COMMAND, json!({})))
            .await
            .expect("read should complete");
        assert_eq!(response.status(), StatusCode::OK);
    }

    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "reads are exempt: a replay must dispatch again"
    );
    assert_eq!(
        RemoteRequestDedupRepository::lookup(
            store.as_ref(),
            &device,
            "req-read",
            "2026-07-28T10:00:00.000Z"
        )
        .await
        .expect("lookup should succeed"),
        RemoteRequestDedupLookup::Absent,
        "ABSENCE: a read must write no dedup row"
    );
}

// ---------------------------------------------------------------------------------------
// C-16 body budget
// ---------------------------------------------------------------------------------------

#[tokio::test]
async fn an_over_budget_invoke_body_is_rejected_before_any_dispatch() {
    let (auth, store) = shared_context();
    let (token, _) = operate_device(&auth).await;
    let (dispatcher, calls) = CountingDispatcher::ok();

    let oversized = "x".repeat(REMOTE_INVOKE_BODY_LIMIT_BYTES + 1_024);
    let response = router(&auth, dispatcher, store)
        .oneshot(invoke(
            &token,
            "req-big",
            MUTATING_COMMAND,
            json!({ "title": oversized }),
        ))
        .await
        .expect("request should complete");

    assert_eq!(
        response.status(),
        StatusCode::PAYLOAD_TOO_LARGE,
        "an over-budget body must be refused, not parsed"
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "ABSENCE: an over-budget body must not dispatch"
    );
}

#[tokio::test]
async fn a_body_inside_the_budget_is_accepted() {
    let (auth, store) = shared_context();
    let (token, _) = operate_device(&auth).await;
    let (dispatcher, calls) = CountingDispatcher::ok();

    // Comfortably inside the limit once JSON framing is added.
    let payload = "x".repeat(REMOTE_INVOKE_BODY_LIMIT_BYTES / 2);
    let response = router(&auth, dispatcher, store)
        .oneshot(invoke(
            &token,
            "req-ok-size",
            MUTATING_COMMAND,
            json!({ "title": payload }),
        ))
        .await
        .expect("request should complete");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

/// The status mapping the codes above rely on must stay total and unchanged.
#[test]
fn the_dedup_error_codes_keep_their_documented_statuses() {
    use super::invoke::status_for_error_code;
    assert_eq!(
        status_for_error_code(ErrorCode::RemoteRequestInProgress),
        StatusCode::CONFLICT
    );
    assert_eq!(
        status_for_error_code(ErrorCode::RemoteRequestIdReused),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        status_for_error_code(ErrorCode::RemoteInternalError),
        StatusCode::INTERNAL_SERVER_ERROR
    );
}
