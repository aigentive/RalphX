// Integration tests for register_project_external handler.
//
// Uses real SQLite (in-memory, migrated) so that:
// - project_repo, api_key_repo, and db all share the same connection
// - run_transaction rollback semantics are correctly exercised
// - auto-scope-add and duplicate checks see consistent state

use axum::extract::{Json, Path, State};
use axum::http::StatusCode;
use ralphx_lib::application::{AppState, TeamService, TeamStateTracker};
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::entities::{
    ApiKey, ApiKeyId, Project, PERMISSION_CREATE_PROJECT, PERMISSION_MAX, PERMISSION_READ,
};
use ralphx_lib::domain::services::key_crypto::{generate_raw_key, hash_key, key_prefix};
use ralphx_lib::error::AppError;
use ralphx_lib::http_server::handlers::*;
use ralphx_lib::http_server::types::{
    CreateApiKeyRequest, HttpServerState, RegisterProjectExternalRequest, UpdatePermissionsRequest,
};
use ralphx_lib::infrastructure::sqlite::{
    sqlite_api_key_repo::SqliteApiKeyRepository,
    sqlite_project_repo::SqliteProjectRepository,
    DbConnection,
};
use std::sync::Arc;

// ============================================================================
// Test helpers
// ============================================================================

/// Build a fully SQLite-backed HttpServerState where project_repo, api_key_repo,
/// and db all share the same in-memory connection with applied migrations.
/// This is required for register_project_external integration tests to see
/// consistent state across repo lookups and direct db.run_transaction() inserts.
fn setup_sqlite_register_state() -> (ralphx_lib::testing::SqliteTestDb, HttpServerState) {
    let db = ralphx_lib::testing::SqliteTestDb::new("http-handler-projects");
    // Disable FK enforcement: tests insert partial data (no FK targets) for speed
    db.with_connection(|conn| {
        conn.execute("PRAGMA foreign_keys = OFF", [])
            .expect("disable FK");
    });
    let shared_conn = db.shared_conn();

    let mut app_state = AppState::new_test();
    app_state.api_key_repo =
        Arc::new(SqliteApiKeyRepository::from_shared(Arc::clone(&shared_conn)));
    app_state.project_repo =
        Arc::new(SqliteProjectRepository::from_shared(Arc::clone(&shared_conn)));
    app_state.db = DbConnection::from_shared(Arc::clone(&shared_conn));

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

/// Insert an API key with the given permissions and return its id.
async fn insert_key(state: &HttpServerState, permissions: i32) -> ApiKeyId {
    let raw = generate_raw_key();
    let key = ApiKey {
        id: ApiKeyId::new(),
        name: "test-key".to_string(),
        key_hash: hash_key(&raw),
        key_prefix: key_prefix(&raw),
        permissions,
        created_at: chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string(),
        revoked_at: None,
        last_used_at: None,
        grace_expires_at: None,
        metadata: None,
    };
    let id = key.id.clone();
    state.app_state.api_key_repo.create(key).await.unwrap();
    id
}

/// Create a ValidatedExternalKey for use in handler calls.
fn make_validated_key(key_id: &ApiKeyId) -> ValidatedExternalKey {
    ValidatedExternalKey {
        key_id: key_id.as_str().to_string(),
        permissions: PERMISSION_CREATE_PROJECT,
    }
}

/// Create a temp directory under $HOME so the handler's home-directory
/// allowlist check passes. Temp dir is cleaned up when the returned
/// TempDir is dropped.
fn temp_dir_under_home() -> tempfile::TempDir {
    let workspace = std::env::current_dir()
        .expect("current_dir must be available for register_project_external tests");
    tempfile::Builder::new()
        .prefix("ralphx-integ-test-")
        .tempdir_in(workspace)
        .expect("Failed to create temp dir under the workspace")
}

// ============================================================================
// Permission enforcement: PERMISSION_CREATE_PROJECT required (403)
// ============================================================================

/// Verify that keys without CREATE_PROJECT cannot pass the extractor check.
/// We test the permission bitmask logic directly (the extractor runs the same check).
#[tokio::test]
async fn test_create_project_permission_bit_required() {
    let (_db, state) = setup_sqlite_register_state();
    let key_id = insert_key(&state, PERMISSION_READ).await;

    let key = state
        .app_state
        .api_key_repo
        .get_by_id(&key_id)
        .await
        .unwrap()
        .unwrap();

    // Key without CREATE_PROJECT must fail the permission check
    assert!(
        !key.has_permission(PERMISSION_CREATE_PROJECT),
        "READ-only key must not have CREATE_PROJECT"
    );
    // Key WITH CREATE_PROJECT must pass
    let cp_key_id = insert_key(&state, PERMISSION_CREATE_PROJECT).await;
    let cp_key = state
        .app_state
        .api_key_repo
        .get_by_id(&cp_key_id)
        .await
        .unwrap()
        .unwrap();
    assert!(
        cp_key.has_permission(PERMISSION_CREATE_PROJECT),
        "CREATE_PROJECT key must have the bit set"
    );
}

// ============================================================================
// Path restrictions: /etc, /tmp, /private rejected (422)
// ============================================================================

#[tokio::test]
async fn test_register_project_etc_rejected() {
    let (_db, state) = setup_sqlite_register_state();
    let key_id = insert_key(&state, PERMISSION_CREATE_PROJECT).await;
    let validated_key = make_validated_key(&key_id);

    // /etc/passwd exists on macOS and Linux — canonicalize succeeds, then home check → 422
    let result = register_project_external(
        State(state),
        validated_key,
        Json(RegisterProjectExternalRequest {
            working_directory: "/etc/passwd".to_string(),
            name: None,
        }),
    )
    .await;

    let err = result.expect_err("/etc/passwd must be rejected");
    assert_eq!(err.status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn test_register_project_tmp_rejected() {
    let (_db, state) = setup_sqlite_register_state();
    let key_id = insert_key(&state, PERMISSION_CREATE_PROJECT).await;
    let validated_key = make_validated_key(&key_id);

    // /tmp is outside HOME → 422
    let result = register_project_external(
        State(state),
        validated_key,
        Json(RegisterProjectExternalRequest {
            working_directory: "/tmp/some-project".to_string(),
            name: None,
        }),
    )
    .await;

    let err = result.expect_err("/tmp must be rejected");
    assert_eq!(err.status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn test_register_project_usr_rejected() {
    let (_db, state) = setup_sqlite_register_state();
    let key_id = insert_key(&state, PERMISSION_CREATE_PROJECT).await;
    let validated_key = make_validated_key(&key_id);

    let result = register_project_external(
        State(state),
        validated_key,
        Json(RegisterProjectExternalRequest {
            working_directory: "/usr/local/bin".to_string(),
            name: None,
        }),
    )
    .await;

    let err = result.expect_err("/usr must be rejected");
    assert_eq!(err.status, StatusCode::UNPROCESSABLE_ENTITY);
}

/// Path under HOME is accepted (422 is NOT returned).
/// The test registers a real temp directory and verifies the response succeeds.
#[tokio::test]
async fn test_register_project_home_subdir_accepted() {
    let (_db, state) = setup_sqlite_register_state();
    let key_id = insert_key(&state, PERMISSION_CREATE_PROJECT).await;
    let validated_key = make_validated_key(&key_id);
    let tmp = temp_dir_under_home();
    let path = tmp.path().to_str().unwrap().to_string();

    let result = register_project_external(
        State(state),
        validated_key,
        Json(RegisterProjectExternalRequest {
            working_directory: path,
            name: Some("IntegTest".to_string()),
        }),
    )
    .await;

    // Must succeed (not a system or outside-HOME path)
    assert!(result.is_ok(), "Path under HOME must be accepted: {:?}", result.err());
}

#[tokio::test]
async fn test_register_project_persists_merge_validation_mode_off() {
    let (_db, state) = setup_sqlite_register_state();
    let key_id = insert_key(&state, PERMISSION_CREATE_PROJECT).await;
    let validated_key = make_validated_key(&key_id);
    let tmp = temp_dir_under_home();
    let path = tmp.path().to_str().unwrap().to_string();

    let response = register_project_external(
        State(state.clone()),
        validated_key,
        Json(RegisterProjectExternalRequest {
            working_directory: path,
            name: Some("DefaultMode".to_string()),
        }),
    )
    .await
    .expect("registration must succeed");

    let project_id = response.0.id;
    let merge_validation_mode = state
        .app_state
        .db
        .run(move |conn| {
            conn.query_row(
                "SELECT merge_validation_mode FROM projects WHERE id = ?1",
                [project_id],
                |row| row.get::<_, String>(0),
            )
            .map_err(AppError::from)
        })
        .await
        .unwrap();

    assert_eq!(merge_validation_mode, "off");
}

// ============================================================================
// Duplicate working_directory → 409 Conflict
// ============================================================================

#[tokio::test]
async fn test_register_project_duplicate_path_returns_409() {
    let (_db, state) = setup_sqlite_register_state();
    let key_id = insert_key(&state, PERMISSION_CREATE_PROJECT).await;
    let tmp = temp_dir_under_home();
    let path = tmp.path().to_str().unwrap().to_string();

    // First registration must succeed
    let validated_key1 = make_validated_key(&key_id);
    let first = register_project_external(
        State(state.clone()),
        validated_key1,
        Json(RegisterProjectExternalRequest {
            working_directory: path.clone(),
            name: Some("FirstReg".to_string()),
        }),
    )
    .await;
    assert!(first.is_ok(), "First registration must succeed");

    // Second registration to the same path must return 409
    let validated_key2 = make_validated_key(&key_id);
    let second = register_project_external(
        State(state),
        validated_key2,
        Json(RegisterProjectExternalRequest {
            working_directory: path,
            name: Some("SecondReg".to_string()),
        }),
    )
    .await;

    let err = second.expect_err("Duplicate path must return 409");
    assert_eq!(
        err.status,
        StatusCode::CONFLICT,
        "Duplicate working_directory must return 409 Conflict"
    );
}

// ============================================================================
// Auto-scope-add: creating key gets scope, other key does NOT
// ============================================================================

#[tokio::test]
async fn test_register_project_creating_key_gets_scope() {
    let (_db, state) = setup_sqlite_register_state();
    let creating_key_id = insert_key(&state, PERMISSION_CREATE_PROJECT).await;
    let other_key_id = insert_key(&state, PERMISSION_CREATE_PROJECT).await;
    let tmp = temp_dir_under_home();
    let path = tmp.path().to_str().unwrap().to_string();

    let validated_key = make_validated_key(&creating_key_id);
    let response = register_project_external(
        State(state.clone()),
        validated_key,
        Json(RegisterProjectExternalRequest {
            working_directory: path,
            name: Some("ScopeTest".to_string()),
        }),
    )
    .await
    .expect("Registration must succeed");

    let project_id = response.0.id.clone();
    let creating_str = creating_key_id.as_str().to_string();
    let other_str = other_key_id.as_str().to_string();
    let project_id_for_other = project_id.clone();

    // Creating key must have scope
    let creating_has_scope = state
        .app_state
        .db
        .run(move |conn| {
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM api_key_projects \
                 WHERE api_key_id = ?1 AND project_id = ?2",
                rusqlite::params![creating_str, project_id],
                |row| row.get(0),
            )?;
            Ok(count)
        })
        .await
        .unwrap();
    assert_eq!(creating_has_scope, 1, "Creating key must have scope on new project");

    // Other key must NOT have scope (least privilege)
    let other_has_scope = state
        .app_state
        .db
        .run(move |conn| {
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM api_key_projects \
                 WHERE api_key_id = ?1 AND project_id = ?2",
                rusqlite::params![other_str, project_id_for_other],
                |row| row.get(0),
            )?;
            Ok(count)
        })
        .await
        .unwrap();
    assert_eq!(other_has_scope, 0, "Other key must NOT have scope on new project");
}

// ============================================================================
// Atomic rollback: both INSERTs rolled back when closure returns Err
// ============================================================================

#[tokio::test]
async fn test_run_transaction_rolls_back_both_inserts_on_error() {
    let (_db, state) = setup_sqlite_register_state();
    let key_id = insert_key(&state, PERMISSION_CREATE_PROJECT).await;
    let key_id_str = key_id.as_str().to_string();

    // Build a project domain object (same as handler would)
    let project = Project::new(
        "RollbackTest".to_string(),
        "/tmp/rollback-test".to_string(),
    );
    let project_id = project.id.as_str().to_string();
    let project_id_for_scope_check = project_id.clone();

    // Run a transaction that inserts the project row, then deliberately fails.
    // Both the project INSERT and any subsequent scope INSERT must be rolled back.
    let result = state
        .app_state
        .db
        .run_transaction(move |conn| {
            conn.execute(
                "INSERT INTO projects (id, name, working_directory, git_mode, base_branch, \
                 worktree_parent_directory, use_feature_branches, merge_validation_mode, \
                 merge_strategy, detected_analysis, custom_analysis, analyzed_at, created_at, \
                 updated_at, github_pr_enabled) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                rusqlite::params![
                    project.id.as_str(),
                    project.name,
                    project.working_directory,
                    project.git_mode.to_string(),
                    project.base_branch,
                    project.worktree_parent_directory,
                    project.use_feature_branches as i64,
                    project.merge_validation_mode.to_string(),
                    project.merge_strategy.to_string(),
                    project.detected_analysis,
                    project.custom_analysis,
                    project.analyzed_at,
                    project.created_at.to_rfc3339(),
                    project.updated_at.to_rfc3339(),
                    project.github_pr_enabled as i64,
                ],
            )
            .map_err(|e| AppError::Database(format!("Insert project: {e}")))?;

            // Deliberately inject failure AFTER the first INSERT to test rollback
            Err::<(), _>(AppError::Database(
                "Injected failure to test rollback atomicity".to_string(),
            ))
        })
        .await;

    assert!(result.is_err(), "Transaction must fail when closure returns Err");

    // Verify project row was rolled back
    let project_row_count = state
        .app_state
        .db
        .run(move |conn| {
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM projects WHERE id = ?1",
                rusqlite::params![project_id],
                |row| row.get(0),
            )?;
            Ok(count)
        })
        .await
        .unwrap();
    assert_eq!(project_row_count, 0, "Project row must not exist after rollback");

    // Verify no orphaned scope row was created
    let scope_row_count = state
        .app_state
        .db
        .run(move |conn| {
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM api_key_projects \
                 WHERE api_key_id = ?1 AND project_id = ?2",
                rusqlite::params![key_id_str, project_id_for_scope_check],
                |row| row.get(0),
            )?;
            Ok(count)
        })
        .await
        .unwrap();
    assert_eq!(scope_row_count, 0, "Scope row must not exist after rollback");
}

// ============================================================================
// Upper-bound validation: permissions > 15 rejected on update (422)
// ============================================================================

#[tokio::test]
async fn test_update_key_permissions_above_max_rejected() {
    let (_db, state) = setup_sqlite_register_state();
    let key_id = insert_key(&state, PERMISSION_READ).await;

    let result = update_key_permissions(
        State(state),
        Path(key_id.as_str().to_string()),
        Json(UpdatePermissionsRequest {
            permissions: (PERMISSION_MAX as i64) + 1, // 16 — above max
        }),
    )
    .await;

    let err = result.expect_err("Permissions above max must be rejected");
    assert_eq!(
        err,
        StatusCode::UNPROCESSABLE_ENTITY,
        "Permissions > PERMISSION_MAX must return 422"
    );
}

// ============================================================================
// Upper-bound validation: permissions > 15 rejected on create (422)
// ============================================================================

#[tokio::test]
async fn test_create_api_key_above_max_permissions_rejected() {
    let (_db, state) = setup_sqlite_register_state();

    let result = create_api_key(
        State(state),
        Json(CreateApiKeyRequest {
            name: "test".to_string(),
            permissions: Some((PERMISSION_MAX) + 1), // 16 — above max
            project_ids: None,
        }),
    )
    .await;

    let err = result.expect_err("Permissions above max must be rejected on create");
    assert_eq!(
        err,
        StatusCode::UNPROCESSABLE_ENTITY,
        "create_api_key with permissions > PERMISSION_MAX must return 422"
    );
}

#[tokio::test]
async fn test_create_api_key_negative_permissions_rejected() {
    let (_db, state) = setup_sqlite_register_state();

    let result = create_api_key(
        State(state),
        Json(CreateApiKeyRequest {
            name: "test".to_string(),
            permissions: Some(-1),
            project_ids: None,
        }),
    )
    .await;

    let err = result.expect_err("Negative permissions must be rejected on create");
    assert_eq!(err, StatusCode::UNPROCESSABLE_ENTITY);
}
