// Tests for internal HTTP handlers
//
// Uses direct handler calls (same pattern as external_tests.rs) for unit tests,
// and a minimal test router with tower::ServiceExt::oneshot for CORS header tests.

use axum::{
    body::Body,
    extract::State,
    http::{header, Method, Request, StatusCode},
    routing::get,
    Json, Router,
};
use ralphx_lib::application::AppState;
use ralphx_lib::commands::ideation_commands::{
    migrate_proposals_impl, CreateCrossProjectSessionInput, MigrateProposalsInput,
};
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::entities::ideation::VerificationStatus;
use ralphx_lib::domain::entities::{
    project::{GitMode, Project},
    IdeationSession, IdeationSessionId, IdeationSessionStatus, Priority, ProjectId,
    ProposalCategory, TaskProposal, TaskProposalId,
};
use ralphx_lib::error::AppError;
use ralphx_lib::http_server::handlers::*;
use ralphx_lib::http_server::types::HttpServerState;
use std::sync::Arc;
use tower::ServiceExt;
use tower_http::cors::{Any, CorsLayer};

// ============================================================================
// Setup helpers
// ============================================================================

async fn setup_test_state() -> HttpServerState {
    let app_state = Arc::new(AppState::new_test());
    let execution_state = Arc::new(ExecutionState::new());
    HttpServerState {
        app_state,
        execution_state,
        delegation_service: Default::default(),
        external_mcp_supervisor: None,
    }
}

async fn setup_sqlite_apply_state() -> HttpServerState {
    let app_state = Arc::new(AppState::new_sqlite_for_apply_test());
    let execution_state = Arc::new(ExecutionState::new());
    HttpServerState {
        app_state,
        execution_state,
        delegation_service: Default::default(),
        external_mcp_supervisor: None,
    }
}

fn test_execution_state() -> Arc<ExecutionState> {
    Arc::new(ExecutionState::new())
}

fn make_project(id: &str, name: &str, dir: &str) -> Project {
    Project {
        id: ProjectId::from_string(id.to_string()),
        name: name.to_string(),
        working_directory: dir.to_string(),
        git_mode: GitMode::Worktree,
        base_branch: None,
        worktree_parent_directory: None,
        use_feature_branches: true,
        merge_validation_mode: Default::default(),
        merge_strategy: Default::default(),
        detected_analysis: None,
        custom_analysis: None,
        analyzed_at: None,
        github_pr_enabled: false,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        archived_at: None,
    }
}

// ============================================================================
// list_projects_internal
// ============================================================================

#[tokio::test]
async fn test_list_projects_internal_returns_all_projects() {
    let state = setup_test_state().await;

    let p1 = make_project("proj-1", "Alpha", "/tmp/alpha");
    let p2 = make_project("proj-2", "Beta", "/tmp/beta");
    state.app_state.project_repo.create(p1).await.unwrap();
    state.app_state.project_repo.create(p2).await.unwrap();

    let result = list_projects_internal(State(state)).await;
    assert!(result.is_ok());
    let summaries = result.unwrap().0;
    assert_eq!(summaries.len(), 2);

    let names: Vec<&str> = summaries.iter().map(|p| p.name.as_str()).collect();
    assert!(names.contains(&"Alpha"));
    assert!(names.contains(&"Beta"));
}

#[tokio::test]
async fn test_list_projects_internal_empty() {
    let state = setup_test_state().await;

    let result = list_projects_internal(State(state)).await;
    assert!(result.is_ok());
    let summaries = result.unwrap().0;
    assert_eq!(summaries.len(), 0);
}

#[tokio::test]
async fn test_list_projects_internal_no_scope_filtering() {
    // Verify that ALL projects are returned regardless of project IDs —
    // the internal endpoint has no ProjectScope header filtering.
    let state = setup_test_state().await;

    let p1 = make_project("scope-proj-a", "ProjA", "/tmp/a");
    let p2 = make_project("scope-proj-b", "ProjB", "/tmp/b");
    let p3 = make_project("scope-proj-c", "ProjC", "/tmp/c");
    state.app_state.project_repo.create(p1).await.unwrap();
    state.app_state.project_repo.create(p2).await.unwrap();
    state.app_state.project_repo.create(p3).await.unwrap();

    let result = list_projects_internal(State(state)).await;
    let summaries = result.unwrap().0;
    // All 3 projects returned — no scope filtering applied
    assert_eq!(summaries.len(), 3);
}

#[tokio::test]
async fn test_list_projects_internal_includes_working_directory() {
    // Verify that internal endpoint returns working_directory matching the project path.
    let state = setup_test_state().await;

    let p1 = make_project("wdtest-1", "RepoA", "/home/user/projects/repo-a");
    let p2 = make_project("wdtest-2", "RepoB", "/srv/repos/repo-b");
    state.app_state.project_repo.create(p1).await.unwrap();
    state.app_state.project_repo.create(p2).await.unwrap();

    let result = list_projects_internal(State(state)).await;
    let summaries = result.unwrap().0;
    assert_eq!(summaries.len(), 2);

    let repo_a = summaries
        .iter()
        .find(|p| p.name == "RepoA")
        .expect("RepoA not found");
    assert_eq!(repo_a.working_directory, "/home/user/projects/repo-a");

    let repo_b = summaries
        .iter()
        .find(|p| p.name == "RepoB")
        .expect("RepoB not found");
    assert_eq!(repo_b.working_directory, "/srv/repos/repo-b");
}

// ============================================================================
// create_cross_project_session_http
// ============================================================================

#[tokio::test]
async fn test_create_cross_project_session_http_without_shell_handle_reaches_session_validation() {
    // AppState::new_test() has no shell handle. The HTTP boundary must still
    // use its shared EventSink and reach the domain validation path.
    let state = setup_test_state().await;
    let target_project = tempfile::tempdir_in(
        std::env::current_dir().expect("current workspace for target project"),
    )
    .expect("target project directory");

    let input = CreateCrossProjectSessionInput {
        target_project_path: target_project.path().to_string_lossy().into_owned(),
        source_session_id: "nonexistent-session-id".to_string(),
        title: None,
    };

    let result = create_cross_project_session_http(State(state), Json(input)).await;
    assert!(result.is_err());

    let err = result.unwrap_err();
    assert_eq!(err.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(err
        .message
        .as_deref()
        .unwrap_or("")
        .contains("Source session not found"));
}

// ============================================================================
// CORS header tests (router-level)
//
// These tests build a minimal Axum router mirroring the production CORS
// structure and verify that:
//   - /api/internal/* routes do NOT return Access-Control-Allow-Origin
//   - public routes DO return Access-Control-Allow-Origin: *
//
// This catches regressions where a permissive CorsLayer is accidentally
// applied to the internal router (the Axum layer-ordering issue described
// in the task spec).
// ============================================================================

/// Builds a test app with the same CORS structure as the production router:
/// - internal_routes: no CORS
/// - public_routes: permissive CorsLayer::allow_origin(Any)
async fn build_cors_test_app() -> Router {
    let state = setup_test_state().await;

    // Internal routes — NO CORS (matches production)
    let internal_routes =
        Router::new().route("/api/internal/projects", get(list_projects_internal));

    // Minimal public routes with permissive CORS (matches production)
    let public_routes = Router::new()
        .route("/health", get(|| async { axum::http::StatusCode::OK }))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        );

    Router::new()
        .merge(internal_routes)
        .merge(public_routes)
        .with_state(state)
}

#[tokio::test]
async fn test_internal_routes_have_no_cors_headers() {
    let app = build_cors_test_app().await;

    // Send a request with an Origin header (simulates a browser cross-origin request)
    let request = Request::builder()
        .method(Method::GET)
        .uri("/api/internal/projects")
        .header(header::ORIGIN, "http://example.com")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    // Internal routes must NOT have Access-Control-Allow-Origin
    assert!(
        response
            .headers()
            .get("access-control-allow-origin")
            .is_none(),
        "Expected no CORS headers on /api/internal/ routes, but found Access-Control-Allow-Origin"
    );
}

#[tokio::test]
async fn test_public_routes_have_permissive_cors_headers() {
    let app = build_cors_test_app().await;

    let request = Request::builder()
        .method(Method::GET)
        .uri("/health")
        .header(header::ORIGIN, "http://example.com")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    // Public routes MUST have Access-Control-Allow-Origin: *
    let cors_header = response.headers().get("access-control-allow-origin");
    assert!(
        cors_header.is_some(),
        "Expected Access-Control-Allow-Origin header on public routes"
    );
    assert_eq!(cors_header.unwrap(), "*");
}

#[tokio::test]
async fn test_internal_route_options_preflight_has_no_cors() {
    // A CORS preflight (OPTIONS) to an internal route should NOT return
    // Access-Control-Allow-Origin. This confirms that browser clients cannot
    // successfully preflight cross-origin requests to internal routes.
    let app = build_cors_test_app().await;

    let request = Request::builder()
        .method(Method::OPTIONS)
        .uri("/api/internal/projects")
        .header(header::ORIGIN, "http://evil.example.com")
        .header("access-control-request-method", "GET")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    // The preflight should either 404 or succeed without CORS headers
    // (no CorsLayer means no ACAO header regardless of response status)
    assert!(
        response
            .headers()
            .get("access-control-allow-origin")
            .is_none(),
        "CORS preflight to internal route must not return Access-Control-Allow-Origin"
    );
}

// ============================================================================
// migrate_proposals_http
// ============================================================================

fn make_session(project_id_str: &str) -> IdeationSession {
    IdeationSession::new(ProjectId::from_string(project_id_str.to_string()))
}

fn make_proposal(session_id: &IdeationSessionId, title: &str) -> TaskProposal {
    TaskProposal::new(
        session_id.clone(),
        title,
        ProposalCategory::Feature,
        Priority::Medium,
    )
}

fn make_proposal_with_target(
    session_id: &IdeationSessionId,
    title: &str,
    target_project: &str,
) -> TaskProposal {
    let mut p = make_proposal(session_id, title);
    p.target_project = Some(target_project.to_string());
    p
}

#[tokio::test]
async fn test_migrate_proposals_source_not_found() {
    let state = setup_test_state().await;

    let input = MigrateProposalsInput {
        source_session_id: "nonexistent-source".to_string(),
        target_session_id: "nonexistent-target".to_string(),
        proposal_ids: None,
        target_project_filter: None,
    };

    let result = migrate_proposals_impl(&state.app_state, input).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Source session not found"));
}

#[tokio::test]
async fn test_migrate_proposals_target_not_found() {
    let state = setup_test_state().await;

    let session = make_session("proj-1");
    let source_id = session.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    let input = MigrateProposalsInput {
        source_session_id: source_id,
        target_session_id: "nonexistent-target".to_string(),
        proposal_ids: None,
        target_project_filter: None,
    };

    let result = migrate_proposals_impl(&state.app_state, input).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Target session not found"));
}

#[tokio::test]
async fn test_migrate_proposals_empty_source_returns_empty() {
    let state = setup_test_state().await;

    let src = make_session("proj-1");
    let dst = make_session("proj-2");
    let source_id = src.id.as_str().to_string();
    let target_id = dst.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(src)
        .await
        .unwrap();
    state
        .app_state
        .ideation_session_repo
        .create(dst)
        .await
        .unwrap();

    let input = MigrateProposalsInput {
        source_session_id: source_id,
        target_session_id: target_id,
        proposal_ids: None,
        target_project_filter: None,
    };

    let result = migrate_proposals_impl(&state.app_state, input)
        .await
        .unwrap();
    assert!(result.migrated.is_empty());
    assert!(result.dropped_dependencies.is_empty());
}

#[tokio::test]
async fn test_migrate_proposals_basic_export() {
    let state = setup_test_state().await;

    let src = make_session("proj-1");
    let dst = make_session("proj-2");
    let source_sid = IdeationSessionId::from_string(src.id.as_str().to_string());
    let source_id = src.id.as_str().to_string();
    let target_id = dst.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(src)
        .await
        .unwrap();
    state
        .app_state
        .ideation_session_repo
        .create(dst)
        .await
        .unwrap();

    let p1 = make_proposal(&source_sid, "Proposal A");
    let p2 = make_proposal(&source_sid, "Proposal B");
    let p1 = state.app_state.task_proposal_repo.create(p1).await.unwrap();
    let p2 = state.app_state.task_proposal_repo.create(p2).await.unwrap();

    let input = MigrateProposalsInput {
        source_session_id: source_id,
        target_session_id: target_id.clone(),
        proposal_ids: None,
        target_project_filter: None,
    };

    let result = migrate_proposals_impl(&state.app_state, input)
        .await
        .unwrap();

    assert_eq!(result.migrated.len(), 2, "Should migrate both proposals");
    assert!(result.dropped_dependencies.is_empty());

    // Verify source IDs match
    let source_ids: std::collections::HashSet<_> = result
        .migrated
        .iter()
        .map(|m| m.source_id.as_str())
        .collect();
    assert!(source_ids.contains(p1.id.as_str()));
    assert!(source_ids.contains(p2.id.as_str()));

    // Verify target proposals were created in target session
    let target_session_id = IdeationSessionId::from_string(target_id);
    let target_proposals = state
        .app_state
        .task_proposal_repo
        .get_by_session(&target_session_id)
        .await
        .unwrap();
    assert_eq!(target_proposals.len(), 2);

    // Verify traceability fields
    for p in &target_proposals {
        assert!(p.migrated_from_session_id.is_some());
        assert!(p.migrated_from_proposal_id.is_some());
    }
}

#[tokio::test]
async fn test_migrate_proposals_partial_subset() {
    let state = setup_test_state().await;

    let src = make_session("proj-1");
    let dst = make_session("proj-2");
    let source_sid = IdeationSessionId::from_string(src.id.as_str().to_string());
    let source_id = src.id.as_str().to_string();
    let target_id = dst.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(src)
        .await
        .unwrap();
    state
        .app_state
        .ideation_session_repo
        .create(dst)
        .await
        .unwrap();

    let p1 = make_proposal(&source_sid, "Proposal A");
    let p2 = make_proposal(&source_sid, "Proposal B");
    let p3 = make_proposal(&source_sid, "Proposal C");
    let p1 = state.app_state.task_proposal_repo.create(p1).await.unwrap();
    let _p2 = state.app_state.task_proposal_repo.create(p2).await.unwrap();
    let p3 = state.app_state.task_proposal_repo.create(p3).await.unwrap();

    // Migrate only p1 and p3
    let input = MigrateProposalsInput {
        source_session_id: source_id,
        target_session_id: target_id.clone(),
        proposal_ids: Some(vec![p1.id.as_str().to_string(), p3.id.as_str().to_string()]),
        target_project_filter: None,
    };

    let result = migrate_proposals_impl(&state.app_state, input)
        .await
        .unwrap();

    assert_eq!(
        result.migrated.len(),
        2,
        "Should migrate only the 2 specified proposals"
    );

    let target_session_id = IdeationSessionId::from_string(target_id);
    let target_proposals = state
        .app_state
        .task_proposal_repo
        .get_by_session(&target_session_id)
        .await
        .unwrap();
    assert_eq!(target_proposals.len(), 2);
}

#[tokio::test]
async fn test_migrate_proposals_target_project_filter() {
    let state = setup_test_state().await;

    let src = make_session("proj-1");
    let dst = make_session("proj-2");
    let source_sid = IdeationSessionId::from_string(src.id.as_str().to_string());
    let source_id = src.id.as_str().to_string();
    let target_id = dst.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(src)
        .await
        .unwrap();
    state
        .app_state
        .ideation_session_repo
        .create(dst)
        .await
        .unwrap();

    let p1 = make_proposal_with_target(&source_sid, "Proposal A", "project-alpha");
    let p2 = make_proposal_with_target(&source_sid, "Proposal B", "project-beta");
    let p3 = make_proposal(&source_sid, "Proposal C"); // no target_project

    state.app_state.task_proposal_repo.create(p1).await.unwrap();
    state.app_state.task_proposal_repo.create(p2).await.unwrap();
    state.app_state.task_proposal_repo.create(p3).await.unwrap();

    // Migrate only proposals with target_project = "project-alpha"
    let input = MigrateProposalsInput {
        source_session_id: source_id,
        target_session_id: target_id.clone(),
        proposal_ids: None,
        target_project_filter: Some("project-alpha".to_string()),
    };

    let result = migrate_proposals_impl(&state.app_state, input)
        .await
        .unwrap();

    assert_eq!(
        result.migrated.len(),
        1,
        "Should migrate only the alpha proposal"
    );

    let target_session_id = IdeationSessionId::from_string(target_id);
    let target_proposals = state
        .app_state
        .task_proposal_repo
        .get_by_session(&target_session_id)
        .await
        .unwrap();
    assert_eq!(target_proposals.len(), 1);
    assert_eq!(
        target_proposals[0].target_project, None,
        "target_project must be cleared on migrated proposals"
    );
}

#[tokio::test]
async fn test_migrate_proposals_dependency_remapping() {
    let state = setup_test_state().await;

    let src = make_session("proj-1");
    let dst = make_session("proj-2");
    let source_sid = IdeationSessionId::from_string(src.id.as_str().to_string());
    let source_id = src.id.as_str().to_string();
    let target_id = dst.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(src)
        .await
        .unwrap();
    state
        .app_state
        .ideation_session_repo
        .create(dst)
        .await
        .unwrap();

    let p1 = make_proposal(&source_sid, "Proposal A");
    let p2 = make_proposal(&source_sid, "Proposal B");
    let p1 = state.app_state.task_proposal_repo.create(p1).await.unwrap();
    let p2 = state.app_state.task_proposal_repo.create(p2).await.unwrap();

    // p2 depends on p1
    state
        .app_state
        .proposal_dependency_repo
        .add_dependency(&p2.id, &p1.id, None, Some("test"))
        .await
        .unwrap();

    let input = MigrateProposalsInput {
        source_session_id: source_id,
        target_session_id: target_id.clone(),
        proposal_ids: None,
        target_project_filter: None,
    };

    let result = migrate_proposals_impl(&state.app_state, input)
        .await
        .unwrap();

    assert_eq!(result.migrated.len(), 2);
    assert!(
        result.dropped_dependencies.is_empty(),
        "Internal dependency should be remapped, not dropped"
    );

    // Find the target IDs for p1 and p2
    let new_p1_id = result
        .migrated
        .iter()
        .find(|m| m.source_id == p1.id.as_str())
        .map(|m| &m.target_id)
        .unwrap();
    let new_p2_id = result
        .migrated
        .iter()
        .find(|m| m.source_id == p2.id.as_str())
        .map(|m| &m.target_id)
        .unwrap();

    // Verify dependency was remapped
    let new_p2 = TaskProposalId::from_string(new_p2_id.clone());
    let deps = state
        .app_state
        .proposal_dependency_repo
        .get_dependencies(&new_p2)
        .await
        .unwrap();
    assert_eq!(deps.len(), 1);
    assert_eq!(deps[0].as_str(), new_p1_id.as_str());
}

#[tokio::test]
async fn test_migrate_proposals_cross_session_dep_dropped_with_warning() {
    let state = setup_test_state().await;

    let src = make_session("proj-1");
    let dst = make_session("proj-2");
    let source_sid = IdeationSessionId::from_string(src.id.as_str().to_string());
    let source_id = src.id.as_str().to_string();
    let target_id = dst.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(src)
        .await
        .unwrap();
    state
        .app_state
        .ideation_session_repo
        .create(dst)
        .await
        .unwrap();

    // p1 and p2 are in source session; only p1 will be migrated
    let p1 = make_proposal(&source_sid, "Proposal A");
    let p2 = make_proposal(&source_sid, "Proposal B");
    let p1 = state.app_state.task_proposal_repo.create(p1).await.unwrap();
    let p2 = state.app_state.task_proposal_repo.create(p2).await.unwrap();

    // p1 depends on p2 — but we only migrate p1, so the dep should be dropped
    state
        .app_state
        .proposal_dependency_repo
        .add_dependency(&p1.id, &p2.id, None, Some("test"))
        .await
        .unwrap();

    // Migrate only p1
    let input = MigrateProposalsInput {
        source_session_id: source_id,
        target_session_id: target_id,
        proposal_ids: Some(vec![p1.id.as_str().to_string()]),
        target_project_filter: None,
    };

    let result = migrate_proposals_impl(&state.app_state, input)
        .await
        .unwrap();

    assert_eq!(result.migrated.len(), 1);
    assert_eq!(
        result.dropped_dependencies.len(),
        1,
        "Cross-session dep should be dropped with warning"
    );
    assert!(result.dropped_dependencies[0]
        .reason
        .contains("not included in the migration set"));
}

#[tokio::test]
async fn test_migrate_proposals_traceability_fields_set() {
    let state = setup_test_state().await;

    let src = make_session("proj-1");
    let dst = make_session("proj-2");
    let source_sid = IdeationSessionId::from_string(src.id.as_str().to_string());
    let source_id = src.id.as_str().to_string();
    let target_id = dst.id.as_str().to_string();
    state
        .app_state
        .ideation_session_repo
        .create(src)
        .await
        .unwrap();
    state
        .app_state
        .ideation_session_repo
        .create(dst)
        .await
        .unwrap();

    let p1 = make_proposal(&source_sid, "Proposal A");
    let p1 = state.app_state.task_proposal_repo.create(p1).await.unwrap();
    let p1_id = p1.id.as_str().to_string();

    let input = MigrateProposalsInput {
        source_session_id: source_id.clone(),
        target_session_id: target_id.clone(),
        proposal_ids: None,
        target_project_filter: None,
    };

    let result = migrate_proposals_impl(&state.app_state, input)
        .await
        .unwrap();
    assert_eq!(result.migrated.len(), 1);

    let new_id = &result.migrated[0].target_id;
    let target_session_id = IdeationSessionId::from_string(target_id);
    let target_proposals = state
        .app_state
        .task_proposal_repo
        .get_by_session(&target_session_id)
        .await
        .unwrap();

    let migrated = target_proposals
        .iter()
        .find(|p| p.id.as_str() == new_id)
        .unwrap();
    assert_eq!(
        migrated.migrated_from_session_id.as_deref(),
        Some(source_id.as_str())
    );
    assert_eq!(
        migrated.migrated_from_proposal_id.as_deref(),
        Some(p1_id.as_str())
    );
    assert!(
        migrated.created_task_id.is_none(),
        "created_task_id should be reset on migration"
    );
}

// ============================================================================
// finalize_proposals_impl — all-foreign short-circuit
// ============================================================================

/// When every proposal in a session has `target_project` pointing to a
/// different directory than the session's own project, `finalize_proposals_impl`
/// must:
///   - return `session_status = "accepted"`
///   - create no local tasks (`tasks_created == 0`)
///   - report the number of skipped foreign proposals
///   - persist `Accepted` status on the session in the repo
#[tokio::test]
async fn test_all_foreign_finalize_transitions_session_to_accepted() {
    let state = setup_sqlite_apply_state().await;

    // Create a project with a known working directory
    let project = make_project("foreign-proj-1", "SourceProject", "/tmp/source-project");
    let project_id = project.id.as_str().to_string();
    state.app_state.project_repo.create(project).await.unwrap();

    // Create an Active ideation session for that project
    let session = IdeationSession::new(ProjectId::from_string(project_id));
    let session_id = session.id.as_str().to_string();
    let session_id_typed = IdeationSessionId::from_string(session_id.clone());
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    // Create proposals that all point to a DIFFERENT project directory
    let p1 = make_proposal_with_target(&session_id_typed, "Foreign Task 1", "/tmp/other-project");
    let p2 = make_proposal_with_target(&session_id_typed, "Foreign Task 2", "/tmp/other-project");
    let p3 = make_proposal_with_target(&session_id_typed, "Foreign Task 3", "/tmp/other-project");
    state.app_state.task_proposal_repo.create(p1).await.unwrap();
    state.app_state.task_proposal_repo.create(p2).await.unwrap();
    state.app_state.task_proposal_repo.create(p3).await.unwrap();

    // Call finalize_proposals_impl — accessible via the handlers::* glob re-export
    let result = finalize_proposals_impl(
        &state.app_state,
        &test_execution_state(),
        &session_id,
        false,
    )
    .await;

    assert!(
        result.is_ok(),
        "finalize_proposals_impl should succeed: {:?}",
        result.err()
    );
    let response = result.unwrap();

    // Session status must be "accepted" (all proposals were foreign)
    assert_eq!(
        response.session_status, "accepted",
        "Expected session_status 'accepted', got '{}'",
        response.session_status
    );

    // No local proposals → no tasks created
    assert_eq!(
        response.tasks_created, 0,
        "Expected tasks_created == 0 (no local proposals)"
    );

    // All 3 foreign proposals should be reported as skipped
    assert!(
        response.skipped_foreign_count > 0,
        "Expected skipped_foreign_count > 0, got {}",
        response.skipped_foreign_count
    );
    assert_eq!(
        response.skipped_foreign_count, 3,
        "Expected skipped_foreign_count == 3 (one per foreign proposal)"
    );

    // Verify the session was persisted as Accepted in the repo
    let updated_session = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session_id_typed)
        .await
        .unwrap()
        .expect("Session must still exist after finalize");

    assert_eq!(
        updated_session.status,
        IdeationSessionStatus::Accepted,
        "Session in repo must be Accepted after all-foreign finalize"
    );
}

#[tokio::test]
async fn test_all_foreign_finalize_ignores_retired_state_when_accept_gate_disabled() {
    let state = setup_sqlite_apply_state().await;

    let project = make_project(
        "foreign-proj-active-verification",
        "SourceProject",
        "/tmp/source-project",
    );
    let project_id = project.id.as_str().to_string();
    state.app_state.project_repo.create(project).await.unwrap();

    let session = IdeationSession::new(ProjectId::from_string(project_id));
    let session_id = session.id.as_str().to_string();
    let session_id_typed = IdeationSessionId::from_string(session_id.clone());
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();
    state
        .app_state
        .ideation_session_repo
        .update_verification_state(&session_id_typed, VerificationStatus::Reviewing, true)
        .await
        .unwrap();

    let p1 = make_proposal_with_target(&session_id_typed, "Foreign Task 1", "/tmp/other-project");
    let p2 = make_proposal_with_target(&session_id_typed, "Foreign Task 2", "/tmp/other-project");
    state.app_state.task_proposal_repo.create(p1).await.unwrap();
    state.app_state.task_proposal_repo.create(p2).await.unwrap();

    let result = finalize_proposals_impl(
        &state.app_state,
        &test_execution_state(),
        &session_id,
        false,
    )
    .await
    .expect("retired verifier summary state must not block acceptance");
    assert_eq!(result.session_status, "accepted");

    let updated_session = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session_id_typed)
        .await
        .unwrap()
        .expect("Session must still exist after blocked finalize");

    assert_eq!(
        updated_session.status,
        IdeationSessionStatus::Accepted,
        "Session should be accepted when exact proof is advisory"
    );
}

#[tokio::test]
async fn test_all_foreign_finalize_respects_required_exact_verification() {
    let state = setup_sqlite_apply_state().await;
    let project = make_project(
        "foreign-proj-required-verification",
        "SourceProject",
        "/tmp/source-project",
    );
    let project_id = project.id.clone();
    state.app_state.project_repo.create(project).await.unwrap();

    let session = IdeationSession::builder()
        .project_id(project_id)
        .plan_artifact_id(ralphx_lib::domain::entities::ArtifactId::from_string(
            "unverified-plan".to_string(),
        ))
        .build();
    let session_id = session.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();
    let mut proposal = make_proposal_with_target(&session_id, "Foreign Task", "/tmp/other-project");
    proposal.affected_paths = Some("[\"README.md\"]".to_string());
    state
        .app_state
        .task_proposal_repo
        .create(proposal)
        .await
        .unwrap();

    let mut settings = state
        .app_state
        .ideation_settings_repo
        .get_settings()
        .await
        .unwrap();
    settings.require_verification_for_accept = true;
    settings.auto_verify_plans = false;
    state
        .app_state
        .ideation_settings_repo
        .update_settings(&settings)
        .await
        .unwrap();

    let result = finalize_proposals_impl(
        &state.app_state,
        &test_execution_state(),
        session_id.as_str(),
        false,
    )
    .await;
    assert!(
        matches!(result, Err(AppError::Validation(_))),
        "all-foreign terminal acceptance must require exact proof: {result:?}"
    );
    let persisted = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session_id)
        .await
        .unwrap()
        .expect("session should exist");
    assert_eq!(persisted.status, IdeationSessionStatus::Active);
}

#[tokio::test]
async fn test_migrate_then_finalize_target_session_accepted() {
    // Use SQLite-backed state so apply_proposals_core's db.run_transaction can see
    // all rows inserted by the repo trait methods in the same test.
    let app_state = AppState::new_sqlite_for_apply_test();

    // Create source and target projects; project IDs must match the sessions.
    let src_project = make_project("proj-src", "Source", "/tmp/src");
    let dst_project = make_project("proj-dst", "Destination", "/tmp/dst");
    app_state.project_repo.create(src_project).await.unwrap();
    app_state.project_repo.create(dst_project).await.unwrap();

    let src = make_session("proj-src");
    let dst = make_session("proj-dst");
    let source_id = src.id.as_str().to_string();
    let target_id = dst.id.as_str().to_string();
    let source_sid = IdeationSessionId::from_string(source_id.clone());
    app_state.ideation_session_repo.create(src).await.unwrap();
    app_state.ideation_session_repo.create(dst).await.unwrap();

    // Proposal targeting proj-dst — before fix this would stay foreign after migration
    let mut p = make_proposal_with_target(&source_sid, "Cross-project Feature", "proj-dst");
    p.affected_paths = Some(r#"["src-tauri/src/http_server"]"#.to_string());
    app_state.task_proposal_repo.create(p).await.unwrap();

    // Migrate to target session
    let migrate_input = MigrateProposalsInput {
        source_session_id: source_id,
        target_session_id: target_id.clone(),
        proposal_ids: None,
        target_project_filter: Some("proj-dst".to_string()),
    };
    let result = migrate_proposals_impl(&app_state, migrate_input)
        .await
        .unwrap();
    assert_eq!(result.migrated.len(), 1);

    // After fix: target_project is cleared so the proposal is local to the target session
    let target_sid = IdeationSessionId::from_string(target_id.clone());
    let target_proposals = app_state
        .task_proposal_repo
        .get_by_session(&target_sid)
        .await
        .unwrap();
    assert_eq!(target_proposals[0].target_project, None);

    // finalize_proposals_impl sees all proposals as local → calls apply_proposals_core
    // → session converts to Accepted
    let response = finalize_proposals_impl(&app_state, &test_execution_state(), &target_id, false)
        .await
        .unwrap();
    assert_eq!(
        response.session_status, "accepted",
        "All proposals local (target_project cleared) — session should transition to Accepted"
    );
}
