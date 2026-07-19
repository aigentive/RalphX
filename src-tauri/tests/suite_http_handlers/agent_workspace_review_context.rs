use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use ralphx_lib::application::{AppState, TeamService, TeamStateTracker};
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentRun,
    AgentWorkspaceReviewGateStatus, AgentWorkspaceReviewMonitorStatus, AgentWorkspaceReviewOutcome,
    AgentWorkspaceReviewTargetScope, ArtifactId, ChatConversation, ChatConversationId,
    IdeationAnalysisBaseRefKind, Project,
};
use ralphx_lib::http_server::handlers::agent_workspaces::{
    get_agent_workspace_review_context, AgentWorkspaceReviewContextQuery,
};
use ralphx_lib::http_server::types::HttpServerState;
use std::path::Path as StdPath;
use std::process::Command;
use std::sync::Arc;

fn git(repo: impl AsRef<StdPath>, args: &[&str]) -> String {
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

fn test_state() -> HttpServerState {
    let app_state = Arc::new(AppState::new_test());
    let team_tracker = TeamStateTracker::new();
    let team_service = Arc::new(TeamService::new_without_events(Arc::new(
        team_tracker.clone(),
    )));
    HttpServerState {
        app_state,
        execution_state: Arc::new(ExecutionState::new()),
        team_tracker,
        team_service,
        delegation_service: Default::default(),
    }
}

#[tokio::test]
async fn outdated_artifact_does_not_revoke_exact_active_reviewer_authority() {
    let repo = tempfile::TempDir::new().expect("repo tempdir");
    let worktrees = tempfile::TempDir::new().expect("worktree tempdir");
    git(repo.path(), &["init", "-b", "main"]);
    git(repo.path(), &["config", "user.email", "test@example.com"]);
    git(repo.path(), &["config", "user.name", "RalphX Test"]);
    std::fs::write(repo.path().join("README.md"), "base\n").expect("write base file");
    git(repo.path(), &["add", "README.md"]);
    git(repo.path(), &["commit", "-m", "base"]);
    let base_sha = git(repo.path(), &["rev-parse", "HEAD"]);

    let state = test_state();
    let conversation_id = ChatConversationId::new();
    let mut project = Project::new(
        "Review Runtime Authority".to_string(),
        repo.path().to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    state
        .app_state
        .project_repo
        .create(project.clone())
        .await
        .expect("seed project");
    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.id = conversation_id;
    state
        .app_state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("seed conversation");

    let workspace_path = worktrees.path().join("workspace");
    let branch_name = "ralphx/test/review-runtime-authority";
    git(
        repo.path(),
        &[
            "worktree",
            "add",
            "-b",
            branch_name,
            workspace_path.to_str().expect("workspace path"),
            "main",
        ],
    );
    std::fs::write(
        workspace_path.join("implementation.txt"),
        "current change\n",
    )
    .expect("write workspace change");
    let workspace = AgentConversationWorkspace::new(
        conversation_id,
        project.id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        Some(base_sha),
        branch_name.to_string(),
        workspace_path.to_string_lossy().to_string(),
    );
    state
        .app_state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("seed workspace");

    let axum::Json(initial) = get_agent_workspace_review_context(
        State(state.clone()),
        Path(conversation_id.to_string()),
        HeaderMap::new(),
        Query(AgentWorkspaceReviewContextQuery::default()),
    )
    .await
    .expect("load initial context");
    let target = initial.target.expect("review target");
    let review_conversation_id = ChatConversationId::new();
    let run = AgentRun::new(review_conversation_id);
    let run_id = run.id;
    let mut monitor = state
        .app_state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&conversation_id)
        .await
        .expect("load monitor")
        .expect("monitor exists");
    let target_scope: AgentWorkspaceReviewTargetScope =
        target.scope.parse().expect("valid target scope");
    monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    monitor.review_outcome = AgentWorkspaceReviewOutcome::None;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Reviewing;
    monitor.current_target_scope = Some(target_scope);
    monitor.current_diff_fingerprint = Some(target.diff_fingerprint);
    monitor.reviewed_target_scope = Some(target_scope);
    monitor.reviewed_diff_fingerprint = Some("historical-fingerprint".to_string());
    monitor.review_artifact_id = Some(ArtifactId::from_string("historical-review"));
    monitor.review_artifact_version = Some(1);
    monitor.review_conversation_id = Some(review_conversation_id);
    monitor.last_run_id = Some(run_id.to_string());
    state
        .app_state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("bind active review");
    state
        .app_state
        .agent_run_repo
        .create(run)
        .await
        .expect("seed active run");

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
    let axum::Json(context) = get_agent_workspace_review_context(
        State(state),
        Path(conversation_id.to_string()),
        headers,
        Query(AgentWorkspaceReviewContextQuery::default()),
    )
    .await
    .expect("load authorized context");

    assert!(context.review_artifact_is_outdated);
    assert!(!context.review_artifact_is_current);
    assert!(context.can_mutate_review_state);
    assert_eq!(context.review_runtime_state, "active_owned");
}
