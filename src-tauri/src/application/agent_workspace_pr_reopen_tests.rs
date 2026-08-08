use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use chrono::Utc;

use crate::application::agent_conversation_workspace::{
    agent_conversation_branch_name, resolve_agent_conversation_workspace_path,
    resolve_linked_plan_branch_agent_worktree_path,
};
use crate::application::agent_workspace_pr_reopen::{
    reopen_agent_workspace_pr_for_state, ReopenAgentWorkspacePrResult,
};
use crate::application::agent_workspace_pr_reopen_restore::ReopenLocalWorkspaceState;
use crate::application::git_service::GitService;
use crate::application::services::PrPollerRegistry;
use crate::application::AppState;
use crate::domain::entities::plan_branch::PrStatus as PlanBranchPrStatus;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentConversationWorkspaceStatus,
    AgentWorkspacePrReviewMonitorStatus, ArtifactId, ChatConversation, ChatConversationId,
    IdeationAnalysisBaseRefKind, IdeationSessionId, PlanBranch, Project,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, ChatConversationRepository,
};
use crate::domain::services::github_service::{GithubServiceTrait, PrStatus};
use crate::infrastructure::sqlite::{
    SqliteAgentConversationWorkspaceRepository, SqliteChatConversationRepository,
};
use crate::testing::SqliteTestDb;
use crate::tests::mock_github_service::MockGithubService;

async fn setup_reopen_state(
    suffix: &str,
    mode: AgentConversationWorkspaceMode,
    pr_number: Option<i64>,
    pr_status: Option<&str>,
) -> (
    tempfile::TempDir,
    AppState,
    ChatConversationId,
    AgentConversationWorkspace,
    Arc<MockGithubService>,
) {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let mut project = Project::new(
        format!("Reopen unit {suffix}"),
        temp.path().join("repo").to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory =
        Some(temp.path().join("worktrees").to_string_lossy().to_string());
    let conversation_id = ChatConversationId::new();
    let branch_name = agent_conversation_branch_name(&project, &conversation_id);
    let worktree_path = resolve_agent_conversation_workspace_path(&project, &conversation_id)
        .expect("reopen workspace path should resolve");
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
    workspace.publication_pr_status = pr_status.map(|status| status.to_string());

    let github = Arc::new(MockGithubService::new());
    let github_trait: Arc<dyn GithubServiceTrait> = github.clone();
    let mut state = AppState::new_test();
    state.github_service = Some(Arc::clone(&github_trait));
    state.pr_poller_registry = Arc::new(PrPollerRegistry::new(
        Some(github_trait),
        Arc::clone(&state.plan_branch_repo),
    ));
    state
        .project_repo
        .create(project.clone())
        .await
        .expect("project should be persisted");
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

fn init_repo(repo: &Path) {
    std::fs::create_dir_all(repo).expect("create reopen repository");
    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "test@example.com"]);
    run_git(repo, &["config", "user.name", "Test User"]);
    run_git(repo, &["checkout", "-b", "main"]);
    std::fs::write(repo.join("README.md"), "base\n").expect("write reopen base");
    run_git(repo, &["add", "."]);
    run_git(repo, &["commit", "-m", "initial"]);
}

/// Init a repository wired to a bare `origin`, which restore needs in order to
/// rebuild a branch that terminal cleanup force-deleted.
fn init_repo_with_origin(repo: &Path, origin: &Path) {
    std::fs::create_dir_all(origin).expect("create reopen origin");
    run_git(
        origin.parent().expect("origin parent"),
        &["init", "--bare", origin.to_str().expect("origin path")],
    );
    init_repo(repo);
    run_git(
        repo,
        &[
            "remote",
            "add",
            "origin",
            origin.to_str().expect("origin path"),
        ],
    );
    run_git(repo, &["push", "origin", "main"]);
}

/// Build the workspace branch in its worktree, publish it to origin, then delete
/// both local artifacts exactly the way terminal PR cleanup does.
async fn seed_and_clean_published_branch(repo: &Path, worktree_path: &Path, branch_name: &str) {
    std::fs::create_dir_all(worktree_path.parent().expect("workspace parent"))
        .expect("create workspace parent");
    GitService::create_worktree(repo, worktree_path, branch_name, "main")
        .await
        .expect("create reopen worktree");
    std::fs::write(worktree_path.join("agent.txt"), "agent work\n").expect("write agent work");
    run_git(worktree_path, &["add", "."]);
    run_git(worktree_path, &["commit", "-m", "agent work"]);
    run_git(worktree_path, &["push", "origin", branch_name]);

    GitService::delete_worktree(repo, worktree_path)
        .await
        .expect("terminal cleanup removes the worktree");
    GitService::delete_branch(repo, branch_name, true)
        .await
        .expect("terminal cleanup force-deletes the branch");
    assert!(!worktree_path.exists());
}

async fn create_linked_plan_branch(
    state: &AppState,
    conversation_id: &ChatConversationId,
    workspace: &AgentConversationWorkspace,
    suffix: &str,
    pr_number: i64,
) -> PlanBranch {
    let session_id = IdeationSessionId::from_string(format!("session-{suffix}"));
    let mut plan_branch = PlanBranch::new(
        ArtifactId::from_string(format!("artifact-{suffix}")),
        session_id.clone(),
        workspace.project_id.clone(),
        format!("ralphx/reopen/{suffix}"),
        "main".to_string(),
    );
    plan_branch.pr_number = Some(pr_number);
    plan_branch.pr_url = Some(format!("https://github.com/mock/repo/pull/{pr_number}"));
    plan_branch.pr_status = Some(PlanBranchPrStatus::Closed);
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
async fn stale_latch_with_live_open_clears_without_remote_mutation() {
    let (_temp, state, conversation_id, _workspace, github) = setup_reopen_state(
        "latch-cleared",
        AgentConversationWorkspaceMode::Edit,
        Some(1),
        Some("closed"),
    )
    .await;
    github.will_return_status(PrStatus::Open);

    let outcome = reopen_agent_workspace_pr_for_state(&conversation_id, false, &state)
        .await
        .expect("reopen should succeed");

    assert_eq!(outcome.outcome, ReopenAgentWorkspacePrResult::LatchCleared);
    assert_eq!(github.state().reopen_pr_calls, 0);
    let persisted = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(persisted.publication_pr_status.as_deref(), Some("open"));
}

#[tokio::test]
async fn live_closed_without_confirmation_requires_confirmation() {
    let (_temp, state, conversation_id, _workspace, github) = setup_reopen_state(
        "confirmation-required",
        AgentConversationWorkspaceMode::Edit,
        Some(2),
        Some("closed"),
    )
    .await;
    github.will_return_status(PrStatus::Closed);

    let outcome = reopen_agent_workspace_pr_for_state(&conversation_id, false, &state)
        .await
        .expect("confirmation-required is an Ok outcome");

    assert_eq!(
        outcome.outcome,
        ReopenAgentWorkspacePrResult::ConfirmationRequired
    );
    assert_eq!(github.state().reopen_pr_calls, 0);
    assert_eq!(github.state().check_pr_status_calls, 1);
    let persisted = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(persisted.publication_pr_status.as_deref(), Some("closed"));
}

#[tokio::test]
async fn live_closed_with_confirmation_reopens_once_then_clears() {
    let (_temp, state, conversation_id, _workspace, github) = setup_reopen_state(
        "reopens-once",
        AgentConversationWorkspaceMode::Edit,
        Some(3),
        Some("closed"),
    )
    .await;
    github.with_pr_status_sequence(vec![PrStatus::Closed, PrStatus::Open]);

    let outcome = reopen_agent_workspace_pr_for_state(&conversation_id, true, &state)
        .await
        .expect("reopen should succeed");

    assert_eq!(
        outcome.outcome,
        ReopenAgentWorkspacePrResult::ReopenedOnGithub
    );
    assert_eq!(github.state().reopen_pr_calls, 1);
    assert_eq!(github.state().check_pr_status_calls, 2);
    let persisted = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(persisted.publication_pr_status.as_deref(), Some("open"));
}

#[tokio::test]
async fn reopen_that_does_not_take_leaves_latch_closed() {
    let (_temp, state, conversation_id, _workspace, github) = setup_reopen_state(
        "reopen-does-not-take",
        AgentConversationWorkspaceMode::Edit,
        Some(4),
        Some("closed"),
    )
    .await;
    github.with_pr_status_sequence(vec![PrStatus::Closed, PrStatus::Closed]);

    let error = reopen_agent_workspace_pr_for_state(&conversation_id, true, &state)
        .await
        .expect_err("a reopen that does not take must fail closed");

    assert!(error.contains("did not reopen"));
    assert_eq!(github.state().reopen_pr_calls, 1);
    assert_eq!(github.state().check_pr_status_calls, 2);
    let persisted = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(persisted.publication_pr_status.as_deref(), Some("closed"));
}

#[tokio::test]
async fn merged_pr_is_refused_and_latch_corrected() {
    let (_temp, state, conversation_id, _workspace, github) = setup_reopen_state(
        "already-merged",
        AgentConversationWorkspaceMode::Edit,
        Some(5),
        Some("merged"),
    )
    .await;

    let outcome = reopen_agent_workspace_pr_for_state(&conversation_id, true, &state)
        .await
        .expect("already-merged is an Ok outcome");

    assert_eq!(outcome.outcome, ReopenAgentWorkspacePrResult::AlreadyMerged);
    assert_eq!(github.state().check_pr_status_calls, 0);
    assert_eq!(github.state().reopen_pr_calls, 0);
    let persisted = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(persisted.publication_pr_status.as_deref(), Some("merged"));
}

#[tokio::test]
async fn github_error_fails_closed() {
    let (_temp, state, conversation_id, _workspace, github) = setup_reopen_state(
        "github-error",
        AgentConversationWorkspaceMode::Edit,
        Some(6),
        Some("closed"),
    )
    .await;
    github.state().check_pr_status_result = Some(Err(crate::error::AppError::Infrastructure(
        "gh unavailable".to_string(),
    )));

    let error = reopen_agent_workspace_pr_for_state(&conversation_id, false, &state)
        .await
        .expect_err("a GitHub read failure must fail closed");

    assert!(error.contains("gh unavailable"));
    assert_eq!(github.state().reopen_pr_calls, 0);
    let persisted = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(persisted.publication_pr_status.as_deref(), Some("closed"));
}

#[tokio::test]
async fn missing_github_service_fails_closed() {
    let (_temp, mut state, conversation_id, _workspace, _github) = setup_reopen_state(
        "missing-github-service",
        AgentConversationWorkspaceMode::Edit,
        Some(7),
        Some("closed"),
    )
    .await;
    state.github_service = None;

    let error = reopen_agent_workspace_pr_for_state(&conversation_id, false, &state)
        .await
        .expect_err("a missing GitHub integration must fail closed before any write");

    assert!(error.contains("GitHub integration is unavailable"));
    let persisted = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(persisted.publication_pr_status.as_deref(), Some("closed"));
}

#[tokio::test]
async fn review_pr_mode_rearms_monitor_after_unlatch() {
    let db = SqliteTestDb::new("pr-reopen-review-monitor");
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> = Arc::new(
        SqliteAgentConversationWorkspaceRepository::from_shared(db.shared_conn()),
    );

    let temp = tempfile::tempdir().expect("tempdir should be created");
    let mut project = Project::new(
        "Reopen unit review-pr-monitor".to_string(),
        temp.path().join("repo").to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    let conversation_id = ChatConversationId::new();
    let branch_name = agent_conversation_branch_name(&project, &conversation_id);
    let worktree_path = resolve_agent_conversation_workspace_path(&project, &conversation_id)
        .expect("reopen workspace path should resolve");
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id.clone(),
        project.id.clone(),
        AgentConversationWorkspaceMode::ReviewPr,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        Some("base-sha".to_string()),
        branch_name,
        worktree_path.to_string_lossy().to_string(),
    );
    workspace.publication_pr_number = Some(77);
    workspace.publication_pr_url = Some("https://github.com/mock/repo/pull/77".to_string());

    let github = Arc::new(MockGithubService::new());
    let github_trait: Arc<dyn GithubServiceTrait> = github.clone();
    let mut state = AppState::new_test();
    state.agent_conversation_workspace_repo = Arc::clone(&workspace_repo);
    state.github_service = Some(Arc::clone(&github_trait));
    state.pr_poller_registry = Arc::new(PrPollerRegistry::new(
        Some(github_trait),
        Arc::clone(&state.plan_branch_repo),
    ));
    state
        .project_repo
        .create(project.clone())
        .await
        .expect("project should be persisted");
    db.insert_project(project.clone());
    let sqlite_chat_conversation_repo =
        SqliteChatConversationRepository::from_shared(db.shared_conn());
    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.id = conversation_id.clone();
    sqlite_chat_conversation_repo
        .create(conversation)
        .await
        .expect("chat conversation should be persisted");
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should be persisted");
    workspace_repo
        .settle_pr_review_terminal(&conversation_id, 77, "closed", "Closed via test fixture")
        .await
        .expect("terminal settlement should seed a terminal monitor");

    github.will_return_status(PrStatus::Open);

    let outcome = reopen_agent_workspace_pr_for_state(&conversation_id, false, &state)
        .await
        .expect("reopen should succeed");

    assert_eq!(outcome.outcome, ReopenAgentWorkspacePrResult::LatchCleared);
    let monitor = workspace_repo
        .get_pr_review_monitor(&conversation_id)
        .await
        .expect("monitor lookup should succeed")
        .expect("monitor should exist after terminal settlement");
    assert_eq!(
        monitor.status,
        AgentWorkspacePrReviewMonitorStatus::Watching
    );
    assert!(monitor.monitor_enabled);
}

#[tokio::test]
async fn unrestorable_workspace_becomes_missing_not_active() {
    let (_temp, state, conversation_id, _workspace, github) = setup_reopen_state(
        "cleaned-becomes-missing",
        AgentConversationWorkspaceMode::Edit,
        Some(9),
        Some("closed"),
    )
    .await;
    state
        .agent_conversation_workspace_repo
        .mark_local_cleanup_status(&conversation_id, "cleaned", Utc::now())
        .await
        .expect("local cleanup marker should persist");
    github.will_return_status(PrStatus::Open);

    let outcome = reopen_agent_workspace_pr_for_state(&conversation_id, false, &state)
        .await
        .expect("reopen should succeed");

    // The PR still reopens; only the local checkout is unrecoverable.
    assert_eq!(outcome.outcome, ReopenAgentWorkspacePrResult::LatchCleared);
    assert_eq!(
        outcome.local_workspace,
        Some(ReopenLocalWorkspaceState::RestoreFailed)
    );
    assert!(
        outcome.message.contains("could not be restored"),
        "message should explain the restore failure: {}",
        outcome.message
    );
    let persisted = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(persisted.status, AgentConversationWorkspaceStatus::Missing);
    let cleanup_status = state
        .agent_conversation_workspace_repo
        .get_local_cleanup_status(&conversation_id)
        .await
        .expect("local cleanup status lookup should succeed");
    assert_eq!(cleanup_status, None);
}

#[tokio::test]
async fn existing_worktree_becomes_active() {
    let (temp, state, conversation_id, workspace, github) = setup_reopen_state(
        "existing-worktree-active",
        AgentConversationWorkspaceMode::Edit,
        Some(10),
        Some("closed"),
    )
    .await;
    state
        .agent_conversation_workspace_repo
        .update_status(&conversation_id, AgentConversationWorkspaceStatus::Missing)
        .await
        .expect("workspace status should update to missing");
    let repo_path = temp.path().join("repo");
    init_repo(&repo_path);
    let worktree_path = Path::new(&workspace.worktree_path);
    std::fs::create_dir_all(worktree_path.parent().expect("workspace parent"))
        .expect("create workspace parent");
    GitService::create_worktree(&repo_path, worktree_path, &workspace.branch_name, "main")
        .await
        .expect("create reopen worktree");
    github.will_return_status(PrStatus::Open);

    let outcome = reopen_agent_workspace_pr_for_state(&conversation_id, false, &state)
        .await
        .expect("reopen should succeed");

    assert_eq!(
        outcome.local_workspace,
        Some(ReopenLocalWorkspaceState::AlreadyPresent)
    );
    let persisted = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(persisted.status, AgentConversationWorkspaceStatus::Active);
}

#[tokio::test]
async fn cleaned_workspace_is_restored_from_origin() {
    let (temp, state, conversation_id, workspace, github) = setup_reopen_state(
        "restore-from-origin",
        AgentConversationWorkspaceMode::Edit,
        Some(21),
        Some("closed"),
    )
    .await;
    let repo_path = temp.path().join("repo");
    init_repo_with_origin(&repo_path, &temp.path().join("origin.git"));
    let worktree_path = Path::new(&workspace.worktree_path);
    seed_and_clean_published_branch(&repo_path, worktree_path, &workspace.branch_name).await;

    state
        .agent_conversation_workspace_repo
        .mark_local_cleanup_status(&conversation_id, "cleaned", Utc::now())
        .await
        .expect("local cleanup marker should persist");
    state
        .agent_conversation_workspace_repo
        .update_status(&conversation_id, AgentConversationWorkspaceStatus::Missing)
        .await
        .expect("workspace status should update to missing");
    github.will_return_status(PrStatus::Open);

    let outcome = reopen_agent_workspace_pr_for_state(&conversation_id, false, &state)
        .await
        .expect("reopen should succeed");

    assert_eq!(
        outcome.local_workspace,
        Some(ReopenLocalWorkspaceState::Restored)
    );
    assert!(worktree_path.is_dir(), "worktree should be recreated");
    assert_eq!(
        GitService::get_current_branch(worktree_path)
            .await
            .expect("restored worktree branch should resolve"),
        workspace.branch_name
    );
    assert_eq!(
        std::fs::read_to_string(worktree_path.join("agent.txt"))
            .expect("restored worktree should carry the published work"),
        "agent work\n"
    );
    assert!(
        GitService::branch_exists(&repo_path, &workspace.branch_name)
            .await
            .expect("branch lookup should succeed"),
        "local branch should be restored from origin"
    );
    let persisted = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(persisted.status, AgentConversationWorkspaceStatus::Active);
}

#[tokio::test]
async fn restore_fails_closed_when_remote_branch_is_gone() {
    let (temp, state, conversation_id, workspace, github) = setup_reopen_state(
        "restore-remote-gone",
        AgentConversationWorkspaceMode::Edit,
        Some(22),
        Some("closed"),
    )
    .await;
    let repo_path = temp.path().join("repo");
    let origin_path = temp.path().join("origin.git");
    init_repo_with_origin(&repo_path, &origin_path);
    let worktree_path = Path::new(&workspace.worktree_path);
    seed_and_clean_published_branch(&repo_path, worktree_path, &workspace.branch_name).await;

    // The head branch is deleted on the remote and locally, leaving nothing to restore from.
    run_git(
        &repo_path,
        &["push", "origin", "--delete", &workspace.branch_name],
    );
    run_git(&repo_path, &["fetch", "--prune", "origin"]);
    github.will_return_status(PrStatus::Open);

    let outcome = reopen_agent_workspace_pr_for_state(&conversation_id, false, &state)
        .await
        .expect("reopen should succeed even when the checkout cannot be restored");

    assert_eq!(outcome.outcome, ReopenAgentWorkspacePrResult::LatchCleared);
    assert_eq!(
        outcome.local_workspace,
        Some(ReopenLocalWorkspaceState::RestoreFailed)
    );
    assert!(
        outcome
            .message
            .contains("remote branch may have been deleted"),
        "message should name the missing remote branch: {}",
        outcome.message
    );
    assert!(!worktree_path.exists());
    let persisted = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(persisted.status, AgentConversationWorkspaceStatus::Missing);
    assert_eq!(persisted.publication_pr_status.as_deref(), Some("open"));
}

#[tokio::test]
async fn linked_plan_branch_workspace_is_restored_from_origin() {
    let (temp, state, conversation_id, workspace, github) = setup_reopen_state(
        "restore-linked-plan",
        AgentConversationWorkspaceMode::Ideation,
        Some(23),
        Some("closed"),
    )
    .await;
    let plan_branch = create_linked_plan_branch(
        &state,
        &conversation_id,
        &workspace,
        "restore-linked-plan",
        23,
    )
    .await;
    let project = state
        .project_repo
        .get_by_id(&workspace.project_id)
        .await
        .unwrap()
        .unwrap();
    let repo_path = temp.path().join("repo");
    init_repo_with_origin(&repo_path, &temp.path().join("origin.git"));
    let plan_worktree_path = resolve_linked_plan_branch_agent_worktree_path(&project, &plan_branch)
        .expect("linked plan worktree path should resolve");
    seed_and_clean_published_branch(&repo_path, &plan_worktree_path, &plan_branch.branch_name)
        .await;
    github.will_return_status(PrStatus::Open);

    let outcome = reopen_agent_workspace_pr_for_state(&conversation_id, false, &state)
        .await
        .expect("reopen should succeed");

    assert_eq!(
        outcome.local_workspace,
        Some(ReopenLocalWorkspaceState::Restored)
    );
    assert!(plan_worktree_path.is_dir());
    assert_eq!(
        GitService::get_current_branch(&plan_worktree_path)
            .await
            .expect("restored plan worktree branch should resolve"),
        plan_branch.branch_name
    );
}

#[tokio::test]
async fn linked_plan_branch_pr_status_returns_to_open() {
    let (_temp, state, conversation_id, workspace, github) = setup_reopen_state(
        "linked-plan-branch",
        AgentConversationWorkspaceMode::Ideation,
        Some(55),
        Some("closed"),
    )
    .await;
    let plan_branch = create_linked_plan_branch(
        &state,
        &conversation_id,
        &workspace,
        "linked-plan-branch",
        55,
    )
    .await;
    github.will_return_status(PrStatus::Open);

    let outcome = reopen_agent_workspace_pr_for_state(&conversation_id, false, &state)
        .await
        .expect("reopen should succeed");

    assert_eq!(outcome.outcome, ReopenAgentWorkspacePrResult::LatchCleared);
    let reloaded_plan_branch = state
        .plan_branch_repo
        .get_by_id(&plan_branch.id)
        .await
        .expect("plan branch lookup should succeed")
        .expect("plan branch should still exist");
    assert_eq!(
        reloaded_plan_branch.pr_status,
        Some(PlanBranchPrStatus::Open)
    );
}

#[tokio::test]
async fn publication_event_appended_once() {
    let (_temp, state, conversation_id, _workspace, github) = setup_reopen_state(
        "publication-event-once",
        AgentConversationWorkspaceMode::Edit,
        Some(11),
        Some("closed"),
    )
    .await;
    github.will_return_status(PrStatus::Open);

    reopen_agent_workspace_pr_for_state(&conversation_id, false, &state)
        .await
        .expect("reopen should succeed");

    let events = state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("publication events should list");
    let reopened_events: Vec<_> = events
        .iter()
        .filter(|event| event.step == "pr_reopened")
        .collect();
    assert_eq!(reopened_events.len(), 1);
}

#[tokio::test]
async fn successful_reopen_restarts_poll_task() {
    let (_temp, state, conversation_id, _workspace, github) = setup_reopen_state(
        "restarts-poll-task",
        AgentConversationWorkspaceMode::Edit,
        Some(12),
        Some("closed"),
    )
    .await;
    github.will_return_status(PrStatus::Open);

    reopen_agent_workspace_pr_for_state(&conversation_id, false, &state)
        .await
        .expect("reopen should succeed");

    assert!(state
        .pr_poller_registry
        .is_agent_workspace_polling(&conversation_id));
}

#[tokio::test]
async fn confirmation_required_and_merged_do_not_restart_poller() {
    let (_temp, state, conversation_id, _workspace, github) = setup_reopen_state(
        "confirmation-no-poller",
        AgentConversationWorkspaceMode::Edit,
        Some(13),
        Some("closed"),
    )
    .await;
    github.will_return_status(PrStatus::Closed);

    reopen_agent_workspace_pr_for_state(&conversation_id, false, &state)
        .await
        .expect("confirmation-required is an Ok outcome");

    assert!(!state
        .pr_poller_registry
        .is_agent_workspace_polling(&conversation_id));

    let (_temp2, state2, conversation_id2, _workspace2, _github2) = setup_reopen_state(
        "merged-no-poller",
        AgentConversationWorkspaceMode::Edit,
        Some(14),
        Some("merged"),
    )
    .await;

    reopen_agent_workspace_pr_for_state(&conversation_id2, true, &state2)
        .await
        .expect("already-merged is an Ok outcome");

    assert!(!state2
        .pr_poller_registry
        .is_agent_workspace_polling(&conversation_id2));
}

#[tokio::test]
async fn double_reopen_leaves_one_poll_task() {
    let (_temp, state, conversation_id, _workspace, github) = setup_reopen_state(
        "double-reopen",
        AgentConversationWorkspaceMode::Edit,
        Some(15),
        Some("closed"),
    )
    .await;
    github.will_return_status(PrStatus::Open);

    reopen_agent_workspace_pr_for_state(&conversation_id, false, &state)
        .await
        .expect("first reopen should succeed");
    assert!(state
        .pr_poller_registry
        .is_agent_workspace_polling(&conversation_id));

    // Simulate the PR closing and reopening again while the poller from the first
    // reopen is still alive; the registry must stay idempotent (AlreadyRunning),
    // never holding more than one live handle for this conversation.
    let persisted = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    state
        .agent_conversation_workspace_repo
        .update_publication(
            &conversation_id,
            Some(15),
            persisted.publication_pr_url.as_deref(),
            Some("closed"),
            None,
        )
        .await
        .expect("simulated re-close should persist");

    reopen_agent_workspace_pr_for_state(&conversation_id, false, &state)
        .await
        .expect("second reopen should succeed");

    assert!(state
        .pr_poller_registry
        .is_agent_workspace_polling(&conversation_id));
}

#[tokio::test]
async fn poller_restart_falls_back_to_project_root_when_restore_fails() {
    let (temp, state, conversation_id, workspace, github) = setup_reopen_state(
        "project-root-not-worktree",
        AgentConversationWorkspaceMode::Edit,
        Some(16),
        Some("closed"),
    )
    .await;
    let repo_path = temp.path().join("repo");
    // No `origin` remote, so the deleted branch cannot be restored and the poller must
    // fall back to the project root rather than a path that does not exist.
    init_repo(&repo_path);
    assert!(!Path::new(&workspace.worktree_path).exists());
    github.will_return_status(PrStatus::Open);

    let outcome = reopen_agent_workspace_pr_for_state(&conversation_id, false, &state)
        .await
        .expect("reopen should succeed even with an unrestorable worktree");

    // The poll task is spawned synchronously (the registry inserts the JoinHandle into
    // its map before the task's first `.await`), so this is a deterministic check that
    // starting the poller did not fail or short-circuit because the worktree directory
    // it will eventually be told to poll from does not exist on disk.
    assert!(state
        .pr_poller_registry
        .is_agent_workspace_polling(&conversation_id));
    // Restore failure marks the workspace Missing, so `agent_workspace_pr_polling_is_current`
    // stops the poll loop on its very first tick regardless of which directory the loop was
    // started with. Runtime tick-survival therefore cannot be used as a black-box proxy for
    // the fallback path; that guarantee is a source-level property of `unlatch_and_restart`,
    // which only substitutes `project.working_directory` when restore returned no worktree.
    assert_eq!(
        outcome.local_workspace,
        Some(ReopenLocalWorkspaceState::RestoreFailed)
    );
    assert!(!Path::new(&workspace.worktree_path).exists());
}
