use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use crate::application::agent_conversation_archive::{
    archive_agent_conversation_for_state, close_agent_workspace_pr_for_state,
};
use crate::application::agent_conversation_workspace::{
    agent_conversation_branch_name, resolve_agent_conversation_workspace_path,
};
use crate::application::agent_workspace_terminal_cleanup::{
    TerminalCleanupClaimState, TerminalLocalCleanupResult,
};
use crate::application::git_service::GitService;
use crate::application::AppState;
use crate::domain::entities::plan_branch::PrStatus;

use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentConversationWorkspaceStatus,
    ArtifactId, ChatContextType, ChatConversation, ChatConversationId, ExecutionPlan,
    ExecutionPlanId, ExecutionPlanStatus, IdeationAnalysisBaseRefKind, IdeationSessionId,
    PlanBranch, PlanBranchStatus, Project, ProjectId, Task,
};
use crate::domain::services::github_service::GithubServiceTrait;
use crate::domain::services::RunningAgentKey;
use crate::error::AppError;
use crate::tests::mock_github_service::MockGithubService;

async fn setup_archive_state(
    suffix: &str,
    mode: AgentConversationWorkspaceMode,
    pr_number: Option<i64>,
) -> (
    tempfile::TempDir,
    AppState,
    ChatConversationId,
    AgentConversationWorkspace,
    Arc<MockGithubService>,
) {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let mut project = Project::new(
        format!("Archive unit {suffix}"),
        temp.path().join("repo").to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory =
        Some(temp.path().join("worktrees").to_string_lossy().to_string());
    let conversation_id = ChatConversationId::new();
    let branch_name = agent_conversation_branch_name(&project, &conversation_id);
    let worktree_path = resolve_agent_conversation_workspace_path(&project, &conversation_id)
        .expect("archive workspace path should resolve");
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id.clone(),
        project.id.clone(),
        mode,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        Some("base-sha".to_string()),
        branch_name,
        worktree_path.to_string_lossy().to_string(),
    );
    workspace.publication_pr_number = pr_number;
    workspace.publication_pr_url =
        pr_number.map(|number| format!("https://github.com/mock/repo/pull/{number}"));
    workspace.publication_pr_status = pr_number.map(|_| "open".to_string());

    let github = Arc::new(MockGithubService::new());
    let github_trait: Arc<dyn GithubServiceTrait> = github.clone();
    let mut state = AppState::new_test();
    state.github_service = Some(github_trait);
    state
        .project_repo
        .create(project.clone())
        .await
        .expect("project should be persisted");
    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.id = conversation_id.clone();
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("conversation should be persisted");
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should be persisted");

    (temp, state, conversation_id, workspace, github)
}

fn run_git(repo: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git command should run");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn initialize_archive_repo(repo: &Path) {
    std::fs::create_dir_all(repo).expect("create archive repository");
    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "test@example.com"]);
    run_git(repo, &["config", "user.name", "Test User"]);
    run_git(repo, &["checkout", "-b", "main"]);
    std::fs::write(repo.join("README.md"), "base\n").expect("write archive base");
    run_git(repo, &["add", "."]);
    run_git(repo, &["commit", "-m", "initial"]);
}

fn branch_exists(repo: &Path, branch: &str) -> bool {
    Command::new("git")
        .args([
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ])
        .current_dir(repo)
        .status()
        .is_ok_and(|status| status.success())
}

async fn create_linked_plan_branch(
    state: &AppState,
    conversation_id: &ChatConversationId,
    workspace: &AgentConversationWorkspace,
    suffix: &str,
    execution_plan: Option<&ExecutionPlan>,
) -> PlanBranch {
    let session_id = execution_plan
        .map(|plan| plan.session_id.clone())
        .unwrap_or_else(|| IdeationSessionId::from_string(format!("session-{suffix}")));
    let mut plan_branch = PlanBranch::new(
        ArtifactId::from_string(format!("artifact-{suffix}")),
        session_id.clone(),
        workspace.project_id.clone(),
        format!("ralphx/archive/{suffix}"),
        "main".to_string(),
    );
    plan_branch.execution_plan_id = execution_plan.map(|plan| plan.id.clone());
    let plan_branch_id = plan_branch.id.clone();
    let created = state
        .plan_branch_repo
        .create(plan_branch)
        .await
        .expect("plan branch should be created");
    state
        .agent_conversation_workspace_repo
        .update_links(conversation_id, Some(&session_id), Some(&plan_branch_id))
        .await
        .expect("workspace links should be updated");
    created
}

#[tokio::test]
async fn archive_closes_workspace_pr_only_when_requested() {
    let (_temp, state, conversation_id, _workspace, github) = setup_archive_state(
        "workspace-pr",
        AgentConversationWorkspaceMode::Edit,
        Some(42),
    )
    .await;
    let conversation_id_str = conversation_id.as_str();
    let running_key =
        RunningAgentKey::new(ChatContextType::Project.to_string(), &conversation_id_str);
    state
        .running_agent_registry
        .register(
            running_key.clone(),
            0,
            conversation_id_str,
            "run-workspace-pr".to_string(),
            None,
            None,
        )
        .await;

    archive_agent_conversation_for_state(&conversation_id, &state, true)
        .await
        .expect("archive should succeed");

    assert!(!state.running_agent_registry.is_running(&running_key).await);
    assert_eq!(github.state().close_pr_calls, 1);
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(workspace.status, AgentConversationWorkspaceStatus::Archived);
    assert_eq!(workspace.publication_pr_status.as_deref(), Some("closed"));
}

#[tokio::test]
async fn archive_without_remote_close_immediately_force_removes_local_workspace() {
    let (temp, state, conversation_id, workspace, github) = setup_archive_state(
        "force-local-cleanup",
        AgentConversationWorkspaceMode::Edit,
        Some(142),
    )
    .await;
    let repo_path = temp.path().join("repo");
    initialize_archive_repo(&repo_path);
    let worktree_path = Path::new(&workspace.worktree_path);
    std::fs::create_dir_all(worktree_path.parent().expect("workspace parent"))
        .expect("create workspace parent");
    GitService::create_worktree(&repo_path, worktree_path, &workspace.branch_name, "main")
        .await
        .expect("create archive worktree");
    std::fs::write(worktree_path.join("uncommitted.txt"), "discard me\n")
        .expect("write archive local change");

    let outcome = archive_agent_conversation_for_state(&conversation_id, &state, false)
        .await
        .expect("archive should remain logically successful");

    assert_eq!(github.state().close_pr_calls, 0);
    assert_eq!(outcome.local_cleanup, TerminalLocalCleanupResult::Cleaned);
    assert!(!worktree_path.exists());
    assert!(!branch_exists(&repo_path, &workspace.branch_name));
}

#[tokio::test]
async fn explicit_close_immediately_force_removes_local_workspace() {
    let (temp, state, conversation_id, workspace, github) = setup_archive_state(
        "explicit-close-force-local",
        AgentConversationWorkspaceMode::Edit,
        Some(143),
    )
    .await;
    let repo_path = temp.path().join("repo");
    initialize_archive_repo(&repo_path);
    let worktree_path = Path::new(&workspace.worktree_path);
    std::fs::create_dir_all(worktree_path.parent().expect("workspace parent"))
        .expect("create workspace parent");
    GitService::create_worktree(&repo_path, worktree_path, &workspace.branch_name, "main")
        .await
        .expect("create explicit-close worktree");
    std::fs::write(worktree_path.join("uncommitted.txt"), "discard me\n")
        .expect("write explicit-close local change");

    close_agent_workspace_pr_for_state(&conversation_id, &state)
        .await
        .expect("explicit close should succeed");

    assert_eq!(github.state().close_pr_calls, 1);
    assert!(!worktree_path.exists());
    assert!(!branch_exists(&repo_path, &workspace.branch_name));
    let persisted = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup")
        .expect("workspace retained for history");
    assert_eq!(persisted.publication_pr_status.as_deref(), Some("closed"));
}

#[tokio::test]
async fn explicit_close_remote_failure_preserves_open_local_workspace() {
    let (temp, state, conversation_id, workspace, github) = setup_archive_state(
        "explicit-close-remote-failure",
        AgentConversationWorkspaceMode::Edit,
        Some(144),
    )
    .await;
    let repo_path = temp.path().join("repo");
    initialize_archive_repo(&repo_path);
    let worktree_path = Path::new(&workspace.worktree_path);
    std::fs::create_dir_all(worktree_path.parent().expect("workspace parent"))
        .expect("create workspace parent");
    GitService::create_worktree(&repo_path, worktree_path, &workspace.branch_name, "main")
        .await
        .expect("create explicit-close worktree");
    github.state().close_pr_result = Some(Err(AppError::Infrastructure(
        "remote close unavailable".to_string(),
    )));

    let error = close_agent_workspace_pr_for_state(&conversation_id, &state)
        .await
        .expect_err("remote close failure must block local terminalization");

    assert!(error.contains("remote close unavailable"));
    assert_eq!(github.state().close_pr_calls, 1);
    assert!(worktree_path.exists());
    assert!(branch_exists(&repo_path, &workspace.branch_name));
    let persisted = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup")
        .expect("workspace retained");
    assert_eq!(persisted.publication_pr_status.as_deref(), Some("open"));
    assert!(state
        .agent_conversation_workspace_repo
        .get_local_cleanup_status(&conversation_id)
        .await
        .expect("cleanup status lookup")
        .is_none());
}

#[tokio::test]
async fn archive_requested_remote_close_failure_preserves_active_state() {
    let (_temp, state, conversation_id, _workspace, github) = setup_archive_state(
        "archive-close-remote-failure",
        AgentConversationWorkspaceMode::Edit,
        Some(145),
    )
    .await;
    github.state().close_pr_result = Some(Err(AppError::Infrastructure(
        "remote close unavailable".to_string(),
    )));

    let error = archive_agent_conversation_for_state(&conversation_id, &state, true)
        .await
        .expect_err("requested remote close failure must block archive authority");

    assert!(error.contains("remote close unavailable"));
    assert_eq!(github.state().close_pr_calls, 1);
    assert!(state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .expect("conversation lookup")
        .expect("conversation retained")
        .archived_at
        .is_none());
    let persisted = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup")
        .expect("workspace retained");
    assert_eq!(persisted.status, AgentConversationWorkspaceStatus::Active);
    assert_eq!(persisted.publication_pr_status.as_deref(), Some("open"));
    assert!(state
        .agent_conversation_workspace_repo
        .get_local_cleanup_status(&conversation_id)
        .await
        .expect("cleanup status lookup")
        .is_none());
}

#[tokio::test]
async fn archive_without_workspace_stops_stored_context_and_returns_cleaned_outcome() {
    let state = AppState::new_test();
    let conversation_id = ChatConversationId::new();
    let mut conversation =
        ChatConversation::new_project(ProjectId::from_string("archive-no-workspace".to_string()));
    conversation.id = conversation_id.clone();
    conversation.context_type = ChatContextType::Standalone;
    conversation.context_id = conversation_id.as_str();
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("conversation should persist");
    let running_key = RunningAgentKey::new(
        ChatContextType::Standalone.to_string(),
        conversation_id.as_str(),
    );
    state
        .running_agent_registry
        .register(
            running_key.clone(),
            0,
            conversation_id.as_str(),
            "run-standalone-archive".to_string(),
            None,
            None,
        )
        .await;

    let outcome = archive_agent_conversation_for_state(&conversation_id, &state, false)
        .await
        .expect("archive without workspace should succeed");

    assert!(!state.running_agent_registry.is_running(&running_key).await);
    assert!(outcome.runtime_shutdown_succeeded);
    assert_eq!(outcome.cleanup_claim, TerminalCleanupClaimState::NotClaimed);
    assert_eq!(outcome.local_cleanup, TerminalLocalCleanupResult::Cleaned);
    assert_eq!(outcome.message, None);
    let archived = state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .expect("conversation lookup")
        .expect("conversation retained");
    assert!(archived.archived_at.is_some());
    assert_eq!(archived.context_type, ChatContextType::Standalone);

    state
        .chat_conversation_repo
        .restore(&conversation_id)
        .await
        .expect("standalone conversation should restore");
    let restored = state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .expect("restored conversation lookup")
        .expect("restored conversation retained");
    assert!(restored.archived_at.is_none());
    assert_eq!(restored.context_type, ChatContextType::Standalone);
}

#[tokio::test]
async fn archive_workspace_with_missing_project_preserves_active_state() {
    let state = AppState::new_test();
    let missing_project_id = ProjectId::from_string("archive-missing-project".to_string());
    let conversation_id = ChatConversationId::new();
    let mut conversation = ChatConversation::new_project(missing_project_id.clone());
    conversation.id = conversation_id.clone();
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("conversation should persist");
    let workspace = AgentConversationWorkspace::new(
        conversation_id.clone(),
        missing_project_id,
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        Some("base-sha".to_string()),
        "ralphx/archive/missing-project".to_string(),
        "/tmp/ralphx-missing-project-worktree".to_string(),
    );
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");

    let error = archive_agent_conversation_for_state(&conversation_id, &state, false)
        .await
        .expect_err("missing project must block archive authority");

    assert!(error.contains("Project not found"));
    assert!(state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .expect("conversation lookup")
        .expect("conversation retained")
        .archived_at
        .is_none());
    assert_eq!(
        state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup")
            .expect("workspace retained")
            .status,
        AgentConversationWorkspaceStatus::Active
    );
}

#[tokio::test]
async fn explicit_close_without_github_service_preserves_open_workspace() {
    let (_temp, mut state, conversation_id, _workspace, _github) = setup_archive_state(
        "explicit-close-no-github",
        AgentConversationWorkspaceMode::Edit,
        Some(146),
    )
    .await;
    state.github_service = None;

    let error = close_agent_workspace_pr_for_state(&conversation_id, &state)
        .await
        .expect_err("missing GitHub integration must block close");

    assert!(error.contains("GitHub integration is unavailable"));
    let persisted = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup")
        .expect("workspace retained");
    assert_eq!(persisted.publication_pr_status.as_deref(), Some("open"));
    assert!(state
        .agent_conversation_workspace_repo
        .get_local_cleanup_status(&conversation_id)
        .await
        .expect("cleanup status lookup")
        .is_none());
}

#[tokio::test]
async fn archive_skips_remote_close_for_terminal_workspace_pr() {
    let (_temp, state, conversation_id, _workspace, github) =
        setup_archive_state("closed-pr", AgentConversationWorkspaceMode::Edit, Some(43)).await;
    state
        .agent_conversation_workspace_repo
        .update_publication(&conversation_id, Some(43), None, Some("closed"), None)
        .await
        .expect("publication update should succeed");

    archive_agent_conversation_for_state(&conversation_id, &state, true)
        .await
        .expect("archive should succeed");

    assert_eq!(github.state().close_pr_calls, 0);
}

#[tokio::test]
async fn archive_ideation_workspace_cleans_current_execution_only() {
    let (_temp, state, conversation_id, workspace, _github) = setup_archive_state(
        "current-execution",
        AgentConversationWorkspaceMode::Ideation,
        None,
    )
    .await;
    let session_id = IdeationSessionId::from_string("session-current-execution".to_string());
    let current_plan = state
        .execution_plan_repo
        .create(ExecutionPlan::new(session_id.clone()))
        .await
        .expect("current plan should be created");
    let mut stale_plan = ExecutionPlan::new(session_id.clone());
    stale_plan.status = ExecutionPlanStatus::Superseded;
    let stale_plan = state
        .execution_plan_repo
        .create(stale_plan)
        .await
        .expect("stale plan should be created");
    let plan_branch = create_linked_plan_branch(
        &state,
        &conversation_id,
        &workspace,
        "current-execution",
        Some(&current_plan),
    )
    .await;

    let mut current_task = Task::new(workspace.project_id.clone(), "current task".to_string());
    current_task.ideation_session_id = Some(session_id.clone());
    current_task.execution_plan_id = Some(current_plan.id.clone());
    let current_task_id = current_task.id.clone();
    state
        .task_repo
        .create(current_task)
        .await
        .expect("current task should be created");

    let mut stale_task = Task::new(workspace.project_id.clone(), "stale task".to_string());
    stale_task.ideation_session_id = Some(session_id);
    stale_task.execution_plan_id = Some(stale_plan.id.clone());
    let stale_task_id = stale_task.id.clone();
    state
        .task_repo
        .create(stale_task)
        .await
        .expect("stale task should be created");

    archive_agent_conversation_for_state(&conversation_id, &state, false)
        .await
        .expect("archive should succeed");

    assert!(state
        .task_repo
        .get_by_id(&current_task_id)
        .await
        .unwrap()
        .unwrap()
        .archived_at
        .is_some());
    assert!(state
        .task_repo
        .get_by_id(&stale_task_id)
        .await
        .unwrap()
        .unwrap()
        .archived_at
        .is_none());
    assert_eq!(
        state
            .execution_plan_repo
            .get_by_id(&current_plan.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        ExecutionPlanStatus::Superseded
    );
    assert_eq!(
        state
            .plan_branch_repo
            .get_by_id(&plan_branch.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        PlanBranchStatus::Abandoned
    );
}

#[tokio::test]
async fn archive_closes_plan_branch_pr_without_workspace_publication() {
    let (_temp, state, conversation_id, workspace, github) = setup_archive_state(
        "plan-branch-pr",
        AgentConversationWorkspaceMode::Ideation,
        None,
    )
    .await;
    let plan_branch =
        create_linked_plan_branch(&state, &conversation_id, &workspace, "plan-branch-pr", None)
            .await;
    state
        .plan_branch_repo
        .update_pr_info(
            &plan_branch.id,
            55,
            "https://github.com/mock/repo/pull/55".to_string(),
            PrStatus::Open,
            false,
        )
        .await
        .expect("plan branch pr update should succeed");

    archive_agent_conversation_for_state(&conversation_id, &state, true)
        .await
        .expect("archive should succeed");

    assert_eq!(github.state().close_pr_calls, 1);
    assert_eq!(
        state
            .plan_branch_repo
            .get_by_id(&plan_branch.id)
            .await
            .unwrap()
            .unwrap()
            .pr_status,
        Some(PrStatus::Closed)
    );
    assert_eq!(
        state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .unwrap()
            .unwrap()
            .publication_pr_number,
        Some(55)
    );
}

#[tokio::test]
async fn explicit_close_uses_plan_branch_pr_before_workspace_publication() {
    let (_temp, state, conversation_id, workspace, github) = setup_archive_state(
        "explicit-close",
        AgentConversationWorkspaceMode::Ideation,
        Some(99),
    )
    .await;
    let plan_branch =
        create_linked_plan_branch(&state, &conversation_id, &workspace, "explicit-close", None)
            .await;
    state
        .plan_branch_repo
        .update_pr_info(
            &plan_branch.id,
            77,
            "https://github.com/mock/repo/pull/77".to_string(),
            PrStatus::Open,
            false,
        )
        .await
        .expect("plan branch pr update should succeed");

    close_agent_workspace_pr_for_state(&conversation_id, &state)
        .await
        .expect("close should succeed");

    assert_eq!(github.state().close_pr_calls, 1);
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(workspace.publication_pr_number, Some(77));
    assert_eq!(workspace.publication_pr_status.as_deref(), Some("closed"));
}

#[tokio::test]
async fn archive_missing_linked_execution_plan_fails_before_archiving() {
    let (_temp, state, conversation_id, workspace, _github) = setup_archive_state(
        "missing-plan",
        AgentConversationWorkspaceMode::Ideation,
        None,
    )
    .await;
    let session_id = IdeationSessionId::from_string("session-missing-plan".to_string());
    let mut plan_branch = PlanBranch::new(
        ArtifactId::from_string("artifact-missing-plan"),
        session_id.clone(),
        workspace.project_id.clone(),
        "plan/missing-plan".to_string(),
        "main".to_string(),
    );
    plan_branch.execution_plan_id = Some(ExecutionPlanId::from_string(
        "missing-execution-plan".to_string(),
    ));
    let plan_branch_id = plan_branch.id.clone();
    state
        .plan_branch_repo
        .create(plan_branch)
        .await
        .expect("plan branch should be created");
    state
        .agent_conversation_workspace_repo
        .update_links(&conversation_id, Some(&session_id), Some(&plan_branch_id))
        .await
        .expect("workspace links should be updated");

    let error = archive_agent_conversation_for_state(&conversation_id, &state, false)
        .await
        .expect_err("archive should fail closed");

    assert!(error.contains("Linked execution plan not found"));
    assert!(state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .unwrap()
        .unwrap()
        .archived_at
        .is_none());
    assert_eq!(
        state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        AgentConversationWorkspaceStatus::Active
    );
}

#[tokio::test]
async fn archive_without_pr_close_request_preserves_open_workspace_pr() {
    let (_temp, state, conversation_id, _workspace, github) = setup_archive_state(
        "skip-workspace-pr",
        AgentConversationWorkspaceMode::Edit,
        Some(88),
    )
    .await;

    archive_agent_conversation_for_state(&conversation_id, &state, false)
        .await
        .expect("archive should succeed");

    assert_eq!(github.state().close_pr_calls, 0);
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(workspace.status, AgentConversationWorkspaceStatus::Archived);
    assert_eq!(workspace.publication_pr_status.as_deref(), Some("open"));
}

#[tokio::test]
async fn archive_review_pr_never_closes_pr_even_when_requested() {
    let (_temp, state, conversation_id, _workspace, github) = setup_archive_state(
        "review-pr",
        AgentConversationWorkspaceMode::ReviewPr,
        Some(89),
    )
    .await;

    archive_agent_conversation_for_state(&conversation_id, &state, true)
        .await
        .expect("archive should succeed without closing reviewed PR");

    assert_eq!(github.state().close_pr_calls, 0);
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(workspace.status, AgentConversationWorkspaceStatus::Archived);
    assert_eq!(workspace.publication_pr_status.as_deref(), Some("open"));
}

#[tokio::test]
async fn explicit_close_rejects_review_pr_without_closing_or_marking_it_closed() {
    let (_temp, state, conversation_id, _workspace, github) = setup_archive_state(
        "review-pr-explicit-close",
        AgentConversationWorkspaceMode::ReviewPr,
        Some(90),
    )
    .await;

    let error = close_agent_workspace_pr_for_state(&conversation_id, &state)
        .await
        .expect_err("Review PR closure should be rejected");

    assert!(error.contains("Review PR"));
    assert_eq!(github.state().close_pr_calls, 0);
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(workspace.publication_pr_status.as_deref(), Some("open"));
}
