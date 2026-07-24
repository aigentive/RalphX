use std::process::Command;
use std::sync::Arc;

use crate::common::{MockGithubService, SubmittingPlanPrAgentClient};
use axum::{
    extract::{Path, State},
    Json,
};
use ralphx_lib::application::agent_conversation_workspace::resolve_agent_conversation_workspace_path;
use ralphx_lib::application::agent_workspace_review::{
    apply_review_artifact_to_monitor, load_agent_workspace_review_context,
};
use ralphx_lib::application::AppState;
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode,
    AgentConversationWorkspacePublicationEvent, AgentWorkspaceReviewGateStatus,
    AgentWorkspaceReviewMonitorStatus, ArtifactId, ChatContextType, ChatConversation,
    ChatConversationId, IdeationAnalysisBaseRefKind, Project,
};
use ralphx_lib::domain::services::github_service::{GithubServiceTrait, PrDetail, PrStatus};
use ralphx_lib::http_server::handlers::agent_workspaces::{
    complete_agent_workspace_review_run, CompleteAgentWorkspaceReviewRunRequest,
};
use ralphx_lib::http_server::types::HttpServerState;

fn git(repo: impl AsRef<std::path::Path>, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git command should spawn");
    assert!(
        output.status.success(),
        "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn make_http_state(app_state: Arc<AppState>) -> HttpServerState {
    HttpServerState {
        app_state,
        execution_state: Arc::new(ExecutionState::new()),
        delegation_service: Default::default(),
    }
}

#[tokio::test]
async fn passed_workspace_review_resumes_pr_fix_publish_after_stale_recovery_block() {
    let repo = tempfile::TempDir::new().expect("repo tempdir");
    let worktrees = tempfile::TempDir::new().expect("worktree tempdir");
    git(repo.path(), &["init", "-b", "main"]);
    git(repo.path(), &["config", "user.email", "test@example.com"]);
    git(repo.path(), &["config", "user.name", "RalphX Test"]);
    std::fs::write(repo.path().join("README.md"), "base\n").expect("write base file");
    git(repo.path(), &["add", "README.md"]);
    git(repo.path(), &["commit", "-m", "base"]);
    let base_sha = git(repo.path(), &["rev-parse", "HEAD"]);

    let github = Arc::new(MockGithubService::new());
    let conversation_id = ChatConversationId::new();
    let mut state = AppState::new_test();
    state.github_service = Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>);
    let workspace_repo = Arc::clone(&state.agent_conversation_workspace_repo);
    let state = state.with_agent_client(Arc::new(SubmittingPlanPrAgentClient::new(workspace_repo)));
    let app_state = Arc::new(state);
    let mut project = Project::new(
        "Stale Recovered PR Fix Review Resume".to_string(),
        repo.path().to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktrees.path().to_string_lossy().to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .expect("seed project");

    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.id = conversation_id;
    conversation.context_type = ChatContextType::Project;
    conversation.context_id = project.id.as_str().to_string();
    conversation.title = Some("Fix stale recovered review publish".to_string());
    app_state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("seed conversation");

    let workspace_path = resolve_agent_conversation_workspace_path(&project, &conversation_id)
        .expect("workspace path");
    let branch_name = "ralphx/test/stale-recovered-pr-fix-review";
    git(
        repo.path(),
        &[
            "worktree",
            "add",
            "-b",
            branch_name,
            workspace_path.to_str().unwrap(),
            "main",
        ],
    );
    std::fs::write(workspace_path.join("fix.txt"), "ci fix\n").expect("write workspace change");
    git(&workspace_path, &["add", "fix.txt"]);
    git(&workspace_path, &["commit", "-m", "fix CI"]);
    let pr_detail = PrDetail {
        number: 681,
        title: "Existing PR title".to_string(),
        body: Some("Existing PR body".to_string()),
        author: Some("maintainer".to_string()),
        created_at: None,
        url: Some("https://github.com/owner/repo/pull/681".to_string()),
        state: PrStatus::Open,
        is_draft: false,
        head_ref_name: branch_name.to_string(),
        base_ref_name: "main".to_string(),
    };
    github.will_return_pr_detail(pr_detail.clone());
    github.will_return_pr_detail(pr_detail);

    let mut workspace = AgentConversationWorkspace::new(
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
    workspace.publication_pr_number = Some(681);
    workspace.publication_pr_url = Some("https://github.com/owner/repo/pull/681".to_string());
    workspace.publication_pr_status = Some("open".to_string());
    workspace.publication_push_status = Some("failed".to_string());
    workspace.auto_publish_enabled = true;
    workspace.pr_autofix_enabled = true;
    workspace.pr_auto_merge_desired = true;
    workspace.pr_supervision_status = Some("blocked".to_string());
    workspace.pr_supervision_summary =
        Some("Recovered stale PR autofix state; no active fixer run is running.".to_string());
    app_state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("seed workspace");
    app_state
        .agent_conversation_workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id,
            "pr_autofix_workspace_review",
            "reviewing",
            "PR fix completed; Workspace Review started before publishing resumes.",
            Some("workspace_review_started".to_string()),
        ))
        .await
        .expect("seed pending review event");

    let review_context = load_agent_workspace_review_context(app_state.as_ref(), &workspace)
        .await
        .expect("review context should load");
    let target = review_context.target.expect("review target should exist");
    let mut monitor = review_context.monitor;
    apply_review_artifact_to_monitor(
        &mut monitor,
        target.scope,
        target.head_sha,
        target.diff_fingerprint,
        Some("review-run".to_string()),
        ArtifactId::from_string("review-artifact-stale-recovered-resume"),
        1,
        chrono::Utc::now(),
        None,
    );
    monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Reviewing;
    app_state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("review monitor should persist");

    let Json(response) = complete_agent_workspace_review_run(
        State(make_http_state(Arc::clone(&app_state))),
        Path(conversation_id.to_string()),
        Json(CompleteAgentWorkspaceReviewRunRequest {
            outcome: Some("passed".to_string()),
            summary: "Review passed".to_string(),
            blocker: None,
            created_by_run_id: Some("review-run".to_string()),
        }),
    )
    .await
    .expect("passed workspace review should complete");

    assert_eq!(response.monitor.review_gate_status, "passed");
    assert_eq!(
        github.push_calls(),
        1,
        "passed review should resume existing PR publish even after stale recovery blocked supervision"
    );
    assert_eq!(
        github.fetch_pr_detail_calls(),
        2,
        "existing PR publication should revalidate the target after pushing"
    );
    let updated = app_state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load workspace")
        .expect("workspace exists");
    assert_eq!(updated.pr_supervision_status.as_deref(), Some("monitoring"));
    assert_eq!(updated.publication_push_status.as_deref(), Some("pushed"));
    let events = app_state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("list publication events");
    assert!(events.iter().any(|event| {
        event.step == "pr_autofix_workspace_review_passed"
            && event.status == "publishing"
            && event.classification.as_deref() == Some("workspace_review_passed")
    }));
    assert!(events
        .iter()
        .any(|event| event.step == "published" && event.status == "succeeded"));
}
