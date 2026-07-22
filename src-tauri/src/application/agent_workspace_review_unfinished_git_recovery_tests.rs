use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use crate::application::agent_workspace_pr_supervision_recovery::{
    recover_agent_workspace_pr_supervision, AgentWorkspacePrSupervisionRecoveryDeps,
    AgentWorkspacePrSupervisionRecoveryTrigger,
};
use crate::application::agent_workspace_publish_recovery::recover_stale_agent_workspace_publish_repairs_for_state;
use crate::application::agent_workspace_review::WORKSPACE_REVIEW_UNFINISHED_GIT_OPERATION_ERROR;
use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode,
    AgentConversationWorkspacePublicationEvent, ChatConversationId, IdeationAnalysisBaseRefKind,
    Project,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, AgentRunRepository, PlanBranchRepository,
};
use crate::domain::services::GithubServiceTrait;
use crate::error::AppError;
use crate::infrastructure::memory::{
    MemoryAgentConversationWorkspaceRepository, MemoryAgentRunRepository,
    MemoryPlanBranchRepository, MemoryProjectRepository, MemoryTaskOutcomeRepository,
};
use crate::tests::mock_github_service::MockGithubService;

fn git(repo: &Path, args: &[&str]) -> String {
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

fn unfinished_recovery_fixture(
    root: &Path,
    conversation_id: ChatConversationId,
) -> (Project, AgentConversationWorkspace) {
    let repo = root.join("repo");
    std::fs::create_dir_all(&repo).expect("repo directory");
    git(&repo, &["init", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test User"]);
    std::fs::write(repo.join("README.md"), "base\n").expect("base file");
    git(&repo, &["add", "README.md"]);
    git(&repo, &["commit", "-m", "base"]);
    let base_sha = git(&repo, &["rev-parse", "HEAD"]);
    std::fs::write(repo.join("fix.txt"), "pending fix\n").expect("pending fix");
    std::fs::write(repo.join(".git").join("MERGE_HEAD"), "unfinished\n").expect("merge metadata");

    let mut project = Project::new(
        "Unfinished Review Recovery".to_string(),
        repo.to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    project.github_pr_enabled = true;
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id,
        project.id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        Some(base_sha),
        "ralphx/test/unfinished-recovery".to_string(),
        repo.to_string_lossy().to_string(),
    );
    workspace.publication_pr_number = Some(42);
    workspace.publication_pr_status = Some("open".to_string());
    workspace.publication_push_status = Some("needs_agent".to_string());
    workspace.pr_supervision_status = Some("fixing".to_string());
    workspace.pr_autofix_enabled = true;
    workspace.auto_publish_enabled = true;
    (project, workspace)
}

async fn seed_pending_handoff(
    repo: &dyn AgentConversationWorkspaceRepository,
    conversation_id: ChatConversationId,
) {
    repo.append_publication_event(AgentConversationWorkspacePublicationEvent::new(
        conversation_id,
        "pr_autofix_workspace_review",
        "reviewing",
        "PR fix completed; Workspace Review started before publishing resumes.",
        Some("workspace_review_started".to_string()),
    ))
    .await
    .expect("seed pending handoff");
}

fn assert_unfinished_error(error: AppError) {
    assert_eq!(
        error.to_string(),
        AppError::Conflict(WORKSPACE_REVIEW_UNFINISHED_GIT_OPERATION_ERROR.to_string()).to_string()
    );
}

#[tokio::test]
async fn workspace_review_unfinished_git_recovery_keeps_stale_publish_handoff_indeterminate() {
    let root = tempfile::tempdir().expect("fixture root");
    let conversation_id = ChatConversationId::new();
    let (project, workspace) = unfinished_recovery_fixture(root.path(), conversation_id);
    let state = AppState::new_test();
    state
        .project_repo
        .create(project)
        .await
        .expect("seed project");
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("seed workspace");
    seed_pending_handoff(
        state.agent_conversation_workspace_repo.as_ref(),
        conversation_id,
    )
    .await;
    let before_events = state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("list events");

    let error = recover_stale_agent_workspace_publish_repairs_for_state(&state)
        .await
        .expect_err("unsettled target must remain indeterminate");

    assert_unfinished_error(error);
    let after = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load workspace")
        .expect("workspace exists");
    assert_eq!(
        after.publication_push_status.as_deref(),
        Some("needs_agent")
    );
    assert_eq!(after.pr_supervision_status.as_deref(), Some("fixing"));
    let after_events = state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("list events");
    assert_eq!(after_events.len(), before_events.len());
}

#[tokio::test]
async fn workspace_review_unfinished_git_recovery_stops_pr_supervision_before_side_effects() {
    let root = tempfile::tempdir().expect("fixture root");
    let conversation_id = ChatConversationId::new();
    let (project, workspace) = unfinished_recovery_fixture(root.path(), conversation_id);
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("seed workspace");
    seed_pending_handoff(workspace_repo.as_ref(), conversation_id).await;
    let before_events = workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("list events");
    let github = Arc::new(MockGithubService::new());

    let error = recover_agent_workspace_pr_supervision(
        AgentWorkspacePrSupervisionRecoveryDeps {
            workspace_repo: Arc::clone(&workspace_repo)
                as Arc<dyn AgentConversationWorkspaceRepository>,
            project_repo: Arc::new(MemoryProjectRepository::with_projects(vec![project])),
            plan_branch_repo: Arc::new(MemoryPlanBranchRepository::new())
                as Arc<dyn PlanBranchRepository>,
            github: Arc::clone(&github) as Arc<dyn GithubServiceTrait>,
            pr_poller_registry: None,
            transition_service: None,
            chat_service: None,
            agent_run_repo: Arc::new(MemoryAgentRunRepository::new())
                as Arc<dyn AgentRunRepository>,
            task_outcome_repo: Arc::new(MemoryTaskOutcomeRepository::new()),
            app_handle: None,
            pr_fix_review_publish_resumer: None,
        },
        conversation_id,
        AgentWorkspacePrSupervisionRecoveryTrigger::Startup,
    )
    .await
    .expect_err("unsettled target must stop PR supervision recovery");

    assert_unfinished_error(error);
    let after = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load workspace")
        .expect("workspace exists");
    assert_eq!(
        after.publication_push_status.as_deref(),
        Some("needs_agent")
    );
    assert_eq!(after.pr_supervision_status.as_deref(), Some("fixing"));
    let after_events = workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("list events");
    assert_eq!(after_events.len(), before_events.len());
    assert_eq!(github.state().check_pr_sync_state_calls, 0);
    assert_eq!(github.state().fetch_pr_health_calls, 0);
}
