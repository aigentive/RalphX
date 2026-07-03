use axum::{body::Body, extract::{Path, State}, http::Request, routing::get, Json};
use ralphx_lib::application::{AppState, TeamService, TeamStateTracker};
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::entities::{
    ApiKey, ApiKeyId, PERMISSION_ADMIN, PERMISSION_READ, PERMISSION_WRITE,
};
use ralphx_lib::domain::services::key_crypto::{generate_raw_key, hash_key, key_prefix};
use ralphx_lib::http_server::handlers::*;
use ralphx_lib::http_server::types::HttpServerState;
use ralphx_lib::testing::SqliteTestDb;
use std::sync::Arc;
use tower::ServiceExt;

/// Build a minimal axum router exposing `GET /validate-key` backed by `validate_api_key`.
fn validate_api_key_app(state: HttpServerState) -> axum::Router {
    axum::Router::new()
        .route("/validate-key", get(validate_api_key))
        .with_state(state)
}

// ============================================================================
// Test helpers
// ============================================================================

async fn setup_test_state() -> HttpServerState {
    let app_state = Arc::new(AppState::new_test());
    let execution_state = Arc::new(ExecutionState::new());
    let tracker = TeamStateTracker::new();
    let team_service = Arc::new(TeamService::new_without_events(Arc::new(tracker.clone())));
    HttpServerState {
        app_state,
        execution_state,
        team_tracker: tracker,
        team_service,
        delegation_service: Default::default(),
    }
}

/// Build a test state where the api_key_repo is backed by a real SQLite in-memory database.
/// Needed for tests that rely on real error semantics (e.g., NotFound from
/// update_api_key_permissions when a key doesn't exist).
fn setup_sqlite_api_key_state() -> (SqliteTestDb, HttpServerState) {
    use ralphx_lib::infrastructure::sqlite::sqlite_api_key_repo::SqliteApiKeyRepository;

    let db = SqliteTestDb::new("http-handler-api-keys");
    let shared_conn = db.shared_conn();
    let sqlite_repo: Arc<dyn ralphx_lib::domain::repositories::ApiKeyRepository> =
        Arc::new(SqliteApiKeyRepository::from_shared(shared_conn));

    // Build a base test state, then override the api_key_repo with the real SQLite one.
    let mut app_state = AppState::new_test();
    app_state.api_key_repo = sqlite_repo;

    let execution_state = Arc::new(ExecutionState::new());
    let tracker = TeamStateTracker::new();
    let team_service = Arc::new(TeamService::new_without_events(Arc::new(tracker.clone())));
    let state = HttpServerState {
        app_state: Arc::new(app_state),
        execution_state,
        team_tracker: tracker,
        team_service,
        delegation_service: Default::default(),
    };
    (db, state)
}

/// Build a minimal router with require_admin_key applied to a GET /test route.
fn test_app(state: HttpServerState) -> axum::Router {
    axum::Router::new()
        .route("/test", get(|| async { "ok" }))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            require_admin_key,
        ))
        .with_state(state)
}

/// Create an active API key in the repo and return (raw_key, key_id).
async fn create_test_key(state: &HttpServerState, permissions: i32) -> (String, ApiKeyId) {
    let raw = generate_raw_key();
    let key = ApiKey {
        id: ApiKeyId::new(),
        name: "test-key".to_string(),
        key_hash: hash_key(&raw),
        key_prefix: key_prefix(&raw),
        permissions,
        created_at: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        revoked_at: None,
        last_used_at: None,
        grace_expires_at: None,
        metadata: None,
    };
    let id = key.id.clone();
    state.app_state.api_key_repo.create(key).await.unwrap();
    (raw, id)
}

// ============================================================================
// Bootstrap mode (no active keys)
// ============================================================================

#[tokio::test]
async fn test_middleware_bootstrap_allows_unauthenticated() {
    // When no active keys exist, management routes are accessible without auth.
    let state = setup_test_state().await;
    let app = test_app(state);

    let response = app
        .oneshot(Request::builder().uri("/test").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), axum::http::StatusCode::OK);
}

// ============================================================================
// Auth required once active keys exist
// ============================================================================

#[tokio::test]
async fn test_middleware_rejects_missing_auth_header() {
    let state = setup_test_state().await;
    // Create an active admin key so bootstrap mode is inactive.
    create_test_key(&state, PERMISSION_ADMIN).await;
    let app = test_app(state);

    let response = app
        .oneshot(Request::builder().uri("/test").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), axum::http::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_middleware_rejects_invalid_key() {
    let state = setup_test_state().await;
    create_test_key(&state, PERMISSION_ADMIN).await;
    let app = test_app(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/test")
                .header("Authorization", "Bearer not-a-real-key")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), axum::http::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_middleware_rejects_non_admin_key() {
    let state = setup_test_state().await;
    // Create an admin key so bootstrap mode is inactive, then create a read-only key.
    create_test_key(&state, PERMISSION_ADMIN).await;
    let (read_only_raw, _) = create_test_key(&state, PERMISSION_READ | PERMISSION_WRITE).await;
    let app = test_app(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/test")
                .header("Authorization", format!("Bearer {}", read_only_raw))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), axum::http::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_middleware_allows_valid_admin_key_bearer() {
    let state = setup_test_state().await;
    let (raw, _) = create_test_key(&state, PERMISSION_ADMIN).await;
    let app = test_app(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/test")
                .header("Authorization", format!("Bearer {}", raw))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), axum::http::StatusCode::OK);
}

#[tokio::test]
async fn test_middleware_allows_valid_admin_key_x_ralphx_header() {
    let state = setup_test_state().await;
    let (raw, _) = create_test_key(&state, PERMISSION_ADMIN).await;
    let app = test_app(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/test")
                .header("X-RalphX-Key", raw)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), axum::http::StatusCode::OK);
}

#[tokio::test]
async fn test_middleware_rejects_revoked_key() {
    let state = setup_test_state().await;
    let (raw, id) = create_test_key(&state, PERMISSION_ADMIN).await;
    // Revoke the key.
    state.app_state.api_key_repo.revoke(&id).await.unwrap();
    // Create another active key so bootstrap mode stays inactive.
    create_test_key(&state, PERMISSION_ADMIN).await;
    let app = test_app(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/test")
                .header("Authorization", format!("Bearer {}", raw))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), axum::http::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_middleware_bootstrap_exits_once_key_exists() {
    // Verify bootstrap mode is inactive after the first active key is created.
    let state = setup_test_state().await;

    // Bootstrap: no auth needed yet.
    let app = test_app(state.clone());
    let r = app
        .oneshot(Request::builder().uri("/test").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(r.status(), axum::http::StatusCode::OK);

    // Create a key — now auth is required.
    create_test_key(&state, PERMISSION_ADMIN).await;
    let app = test_app(state);
    let r = app
        .oneshot(Request::builder().uri("/test").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(r.status(), axum::http::StatusCode::UNAUTHORIZED);
}

// ============================================================================
// get_audit_log handler
// ============================================================================

#[tokio::test]
async fn test_get_audit_log_handler() {
    let state = setup_test_state().await;
    let (_, key_id) = create_test_key(&state, PERMISSION_ADMIN).await;

    let result = get_audit_log(
        State(state.clone()),
        Path(key_id.as_str().to_string()),
    )
    .await;

    let response = result.expect("get_audit_log should succeed");
    // Memory repo returns an empty list — verify the entries field exists (is a Vec)
    assert!(response.0.entries.is_empty(), "memory repo returns no entries");
}

// ============================================================================
// update_key_permissions handler
// ============================================================================

#[tokio::test]
async fn test_update_key_permissions_handler_success() {
    let (_db, state) = setup_sqlite_api_key_state();
    let (_, key_id) = create_test_key(&state, PERMISSION_READ).await;

    let result = update_key_permissions(
        State(state.clone()),
        Path(key_id.as_str().to_string()),
        Json(UpdatePermissionsRequest { permissions: (PERMISSION_READ | PERMISSION_WRITE) as i64 }),
    )
    .await;

    let response = result.expect("update should succeed");
    assert!(response.0.success, "success must be true");
}

#[tokio::test]
async fn test_update_key_permissions_handler_not_found() {
    let (_db, state) = setup_sqlite_api_key_state();

    let result = update_key_permissions(
        State(state.clone()),
        Path("nonexistent-key-id".to_string()),
        Json(UpdatePermissionsRequest { permissions: PERMISSION_READ as i64 }),
    )
    .await;

    let err = result.expect_err("should fail for missing key");
    assert_eq!(err, axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_update_key_permissions_handler_invalid_permissions() {
    let state = setup_test_state().await;
    let (_, key_id) = create_test_key(&state, PERMISSION_ADMIN).await;

    let result = update_key_permissions(
        State(state.clone()),
        Path(key_id.as_str().to_string()),
        Json(UpdatePermissionsRequest { permissions: -1 }),
    )
    .await;

    let err = result.expect_err("negative permissions should be rejected");
    assert_eq!(err, axum::http::StatusCode::UNPROCESSABLE_ENTITY);
}

// ============================================================================
// validate_api_key handler — HTTP 401 for invalid/revoked keys
// ============================================================================

#[tokio::test]
async fn test_validate_api_key_returns_401_for_invalid_key() {
    // An unknown key (not in the repo) must produce HTTP 401.
    let state = setup_test_state().await;
    let app = validate_api_key_app(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/validate-key")
                .header("Authorization", "Bearer rxk_live_notarealkey")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), axum::http::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_validate_api_key_returns_401_for_revoked_key() {
    // A revoked key must produce HTTP 401 (not 200 with valid=false).
    let state = setup_test_state().await;
    let (raw, id) = create_test_key(&state, PERMISSION_READ).await;
    state.app_state.api_key_repo.revoke(&id).await.unwrap();
    let app = validate_api_key_app(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/validate-key")
                .header("Authorization", format!("Bearer {}", raw))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), axum::http::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_validate_api_key_returns_200_for_valid_key() {
    // A valid active key must produce HTTP 200.
    let state = setup_test_state().await;
    let (raw, _) = create_test_key(&state, PERMISSION_READ).await;
    let app = validate_api_key_app(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/validate-key")
                .header("Authorization", format!("Bearer {}", raw))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), axum::http::StatusCode::OK);
}

#[tokio::test]
async fn test_validate_api_key_returns_401_for_missing_key() {
    // No Authorization header must produce HTTP 401.
    let state = setup_test_state().await;
    let app = validate_api_key_app(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/validate-key")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), axum::http::StatusCode::UNAUTHORIZED);
}
