use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use ralphx_lib::application::agent_workspace_review::load_agent_workspace_review_context;
use ralphx_lib::application::AppState;
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentRun,
    AgentWorkspaceReviewGateStatus, AgentWorkspaceReviewMonitorStatus, AgentWorkspaceReviewOutcome,
    ChatConversation, ChatConversationId, IdeationAnalysisBaseRefKind, Project,
};
use ralphx_lib::http_server::handlers::agent_workspaces::{
    get_agent_workspace_review_diff_page, list_agent_workspace_review_files,
    GetAgentWorkspaceReviewDiffPageQuery, ListAgentWorkspaceReviewFilesQuery,
};
use ralphx_lib::http_server::types::HttpServerState;
use std::path::Path as StdPath;
use std::process::Command;
use std::sync::Arc;

fn git(repo: &StdPath, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git command should spawn");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn http_state(app_state: Arc<AppState>) -> HttpServerState {
    HttpServerState {
        app_state,
        execution_state: Arc::new(ExecutionState::new()),
        delegation_service: Default::default(),
        external_mcp_supervisor: None,
    }
}

async fn active_review_fixture() -> (
    tempfile::TempDir,
    HttpServerState,
    ChatConversationId,
    HeaderMap,
) {
    let repo = tempfile::TempDir::new().expect("repo tempdir");
    git(repo.path(), &["init", "-b", "main"]);
    git(repo.path(), &["config", "user.email", "test@example.com"]);
    git(repo.path(), &["config", "user.name", "RalphX Test"]);
    std::fs::write(repo.path().join("README.md"), "base\n").expect("write base file");
    git(repo.path(), &["add", "README.md"]);
    git(repo.path(), &["commit", "-m", "base"]);
    let base_sha = git(repo.path(), &["rev-parse", "HEAD"]);
    std::fs::write(repo.path().join("review-me.txt"), "new evidence\n")
        .expect("write review change");

    let app_state = Arc::new(AppState::new_test());
    let mut project = Project::new(
        "Review diff HTTP".to_string(),
        repo.path().to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .expect("seed project");
    let conversation_id = ChatConversationId::new();
    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.id = conversation_id;
    app_state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("seed conversation");
    let workspace = AgentConversationWorkspace::new(
        conversation_id,
        project.id,
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        Some(base_sha),
        "ralphx/test/review-diff-http".to_string(),
        repo.path().to_string_lossy().to_string(),
    );
    app_state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("seed workspace");
    let context = load_agent_workspace_review_context(app_state.as_ref(), &workspace)
        .await
        .expect("load review context");
    let target = context.target.expect("review target");
    let review_conversation_id = ChatConversationId::new();
    let run = AgentRun::new(review_conversation_id);
    let run_id = run.id;
    app_state
        .agent_run_repo
        .create(run)
        .await
        .expect("seed reviewer run");
    let mut monitor = context.monitor;
    monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    monitor.review_outcome = AgentWorkspaceReviewOutcome::None;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Reviewing;
    monitor.current_target_scope = Some(target.scope);
    monitor.current_diff_fingerprint = Some(target.diff_fingerprint);
    monitor.review_conversation_id = Some(review_conversation_id);
    monitor.last_run_id = Some(run_id.to_string());
    app_state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("bind active reviewer");

    let mut headers = HeaderMap::new();
    headers.insert(
        "x-ralphx-agent-run-id",
        run_id.to_string().parse().expect("run header"),
    );
    headers.insert(
        "x-ralphx-conversation-id",
        review_conversation_id
            .as_str()
            .parse()
            .expect("conversation header"),
    );
    (repo, http_state(app_state), conversation_id, headers)
}

#[tokio::test]
async fn review_file_pages_require_active_transport_identity_and_do_not_mutate_events() {
    let (_repo, state, conversation_id, headers) = active_review_fixture().await;
    let before = state
        .app_state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("load events before");

    let axum::Json(response) = list_agent_workspace_review_files(
        State(state.clone()),
        Path(conversation_id.to_string()),
        headers.clone(),
        Query(ListAgentWorkspaceReviewFilesQuery {
            cursor: None,
            limit: Some(10),
        }),
    )
    .await
    .expect("authorized file page");
    assert!(response.success);
    assert_eq!(response.page.total_count, 1);
    assert_eq!(response.page.files[0].path, "review-me.txt");
    assert_eq!(response.page.files[0].sources, vec!["unstaged"]);

    let axum::Json(diff_response) = get_agent_workspace_review_diff_page(
        State(state.clone()),
        Path(conversation_id.to_string()),
        headers.clone(),
        Query(GetAgentWorkspaceReviewDiffPageQuery {
            cursor: None,
            path: Some("review-me.txt".to_string()),
            source: Some("unstaged".to_string()),
            limit: Some(10),
        }),
    )
    .await
    .expect("authorized diff page");
    assert!(diff_response.success);
    assert_eq!(diff_response.diff.page.file_path, "review-me.txt");
    assert!(!diff_response.diff.page.rows.is_empty());

    let malformed_cursor = list_agent_workspace_review_files(
        State(state.clone()),
        Path(conversation_id.to_string()),
        headers.clone(),
        Query(ListAgentWorkspaceReviewFilesQuery {
            cursor: Some("not-a-valid-cursor!".to_string()),
            limit: None,
        }),
    )
    .await
    .expect_err("malformed cursor must be a typed bad request");
    assert_eq!(malformed_cursor.0, StatusCode::BAD_REQUEST);

    let denied = list_agent_workspace_review_files(
        State(state.clone()),
        Path(conversation_id.to_string()),
        HeaderMap::new(),
        Query(ListAgentWorkspaceReviewFilesQuery::default()),
    )
    .await
    .expect_err("missing runtime identity must fail closed");
    assert_eq!(denied.0, StatusCode::CONFLICT);

    let after = state
        .app_state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("load events after");
    assert_eq!(after.len(), before.len());
}
