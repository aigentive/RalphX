use std::collections::VecDeque;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::sync::Mutex;

use crate::application::agent_conversation_workspace::{
    agent_conversation_branch_name, resolve_agent_conversation_workspace_path,
    resolve_linked_plan_branch_agent_worktree_path,
};
use crate::application::chat_service::MockChatService;
use crate::domain::entities::agent_run::PersonaRunAttribution;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode,
    AgentConversationWorkspacePublicationEvent, AgentConversationWorkspaceStatus, AgentRun,
    AgentRunAttribution, AgentRunId, AgentRunStatus, AgentRunUsage, AgentWorkspacePrDescription,
    AgentWorkspacePrReviewMonitor, ArtifactId, ChatContextType, ChatConversationId,
    IdeationAnalysisBaseRefKind, IdeationSessionId, InterruptedConversation, PlanBranch,
    PlanBranchId, Project, ProjectId,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, AgentRunRepository, AgentWorkspaceLocalCleanupClaim,
    PlanBranchRepository,
};
use crate::error::{AppError, AppResult};
use crate::infrastructure::memory::{
    MemoryAgentConversationWorkspaceRepository, MemoryAgentRunRepository,
    MemoryPlanBranchRepository,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};

use super::agent_workspace_terminal_cleanup::{
    cleanup_terminal_agent_workspace_after_pr, terminal_cleanup_target_path,
    terminalize_agent_workspace_after_pr, TerminalAgentWorkspaceCause, TerminalCleanupClaimState,
    TerminalLocalCleanupResult,
};

fn run_git(repo: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git command should run");
    assert!(
        output.status.success(),
        "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn branch_exists(repo: &Path, branch: &str) -> bool {
    Command::new("git")
        .args(["rev-parse", "--verify", branch])
        .current_dir(repo)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn setup_repo(repo: &Path) {
    std::fs::create_dir_all(repo).expect("create repository path");
    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "test@example.com"]);
    run_git(repo, &["config", "user.name", "Test User"]);
    run_git(repo, &["checkout", "-b", "main"]);
    std::fs::write(repo.join("README.md"), "base\n").expect("write base file");
    run_git(repo, &["add", "."]);
    run_git(repo, &["commit", "-m", "initial"]);
}

fn project_for(repo: &Path, worktree_parent: &Path) -> Project {
    let mut project = Project::new(
        "Terminal Cleanup".to_string(),
        repo.to_string_lossy().to_string(),
    );
    project.id = ProjectId::from_string("project-terminal-cleanup".to_string());
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
    project
}

fn workspace_for(
    project: &Project,
    conversation_id: ChatConversationId,
) -> AgentConversationWorkspace {
    let branch_name = agent_conversation_branch_name(project, &conversation_id);
    let worktree_path = resolve_agent_conversation_workspace_path(project, &conversation_id)
        .expect("workspace path should resolve");
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id,
        project.id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        None,
        branch_name,
        worktree_path.to_string_lossy().to_string(),
    );
    workspace.status = AgentConversationWorkspaceStatus::Archived;
    workspace.publication_pr_status = Some("closed".to_string());
    workspace
}

#[tokio::test]
async fn terminal_cleanup_claim_force_removes_dirty_owned_workspace_and_branch() {
    let repository_dir = tempfile::tempdir().expect("repository tempdir");
    let worktree_parent = tempfile::tempdir().expect("worktree parent tempdir");
    setup_repo(repository_dir.path());
    let project = project_for(repository_dir.path(), worktree_parent.path());
    let conversation_id =
        ChatConversationId::from_string("11111111-1111-1111-1111-111111111111".to_string());
    let workspace = workspace_for(&project, conversation_id.clone());
    let worktree_path = Path::new(&workspace.worktree_path);
    std::fs::create_dir_all(worktree_path.parent().expect("workspace parent"))
        .expect("create workspace parent");
    run_git(
        repository_dir.path(),
        &[
            "worktree",
            "add",
            "-b",
            &workspace.branch_name,
            worktree_path.to_str().expect("utf-8 workspace path"),
            "main",
        ],
    );
    std::fs::write(worktree_path.join("uncommitted.txt"), "local work\n")
        .expect("write uncommitted file");
    std::fs::write(worktree_path.join(".gitignore"), "target/\n").expect("write ignore rule");
    std::fs::create_dir_all(worktree_path.join("target")).expect("create ignored directory");
    std::fs::write(
        worktree_path.join("target/test-artifact.bin"),
        "large artifact\n",
    )
    .expect("write ignored artifact");

    let repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    repo.create_or_update(workspace.clone())
        .await
        .expect("persist workspace");

    let (first, second) = tokio::join!(
        cleanup_terminal_agent_workspace_after_pr(repo.clone(), None, &conversation_id, &project,),
        cleanup_terminal_agent_workspace_after_pr(repo.clone(), None, &conversation_id, &project,),
    );

    let outcomes = [first, second];
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| outcome.cleanup_claim == TerminalCleanupClaimState::Claimed)
            .count(),
        1
    );
    assert!(outcomes.iter().all(|outcome| matches!(
        outcome.local_cleanup,
        TerminalLocalCleanupResult::Cleaned | TerminalLocalCleanupResult::Pending
    )));
    assert!(!worktree_path.exists());
    assert!(!branch_exists(
        repository_dir.path(),
        &workspace.branch_name
    ));
    assert_eq!(
        repo.local_cleanup_status_for_test(&conversation_id)
            .await
            .as_deref(),
        Some("cleaned")
    );
}

#[tokio::test]
async fn terminal_cleanup_persists_unsafe_failure_for_mismatched_workspace_path() {
    let repository_dir = tempfile::tempdir().expect("repository tempdir");
    let worktree_parent = tempfile::tempdir().expect("worktree parent tempdir");
    setup_repo(repository_dir.path());
    let project = project_for(repository_dir.path(), worktree_parent.path());
    let conversation_id =
        ChatConversationId::from_string("22222222-2222-2222-2222-222222222222".to_string());
    let mut workspace = workspace_for(&project, conversation_id.clone());
    workspace.worktree_path = repository_dir.path().to_string_lossy().to_string();
    let repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    repo.create_or_update(workspace)
        .await
        .expect("persist workspace");

    let outcome =
        cleanup_terminal_agent_workspace_after_pr(repo.clone(), None, &conversation_id, &project)
            .await;

    assert_eq!(outcome.cleanup_claim, TerminalCleanupClaimState::Claimed);
    assert_eq!(
        outcome.local_cleanup,
        TerminalLocalCleanupResult::FailedUnsafe
    );
    assert!(outcome
        .message
        .as_deref()
        .is_some_and(|message| message.contains("workspace path mismatch")));
    assert_eq!(
        repo.local_cleanup_status_for_test(&conversation_id)
            .await
            .as_deref(),
        Some("failed_unsafe")
    );
    assert!(repository_dir.path().join("README.md").exists());
}

#[tokio::test]
async fn terminal_cleanup_resolves_and_force_removes_linked_plan_branch_target() {
    let repository_dir = tempfile::tempdir().expect("repository tempdir");
    let worktree_parent = tempfile::tempdir().expect("worktree parent tempdir");
    setup_repo(repository_dir.path());
    let project = project_for(repository_dir.path(), worktree_parent.path());
    let conversation_id =
        ChatConversationId::from_string("33333333-3333-3333-3333-333333333333".to_string());
    let session_id = IdeationSessionId::from_string("terminal-cleanup-session".to_string());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
    let plan_branch = plan_branch_repo
        .create(PlanBranch::new(
            ArtifactId::from_string("terminal-cleanup-artifact"),
            session_id.clone(),
            project.id.clone(),
            "ralphx/terminal-cleanup/plan-linked".to_string(),
            "main".to_string(),
        ))
        .await
        .expect("persist linked plan branch");
    let linked_path = resolve_linked_plan_branch_agent_worktree_path(&project, &plan_branch)
        .expect("linked worktree path should resolve");
    std::fs::create_dir_all(linked_path.parent().expect("linked workspace parent"))
        .expect("create linked workspace parent");
    run_git(
        repository_dir.path(),
        &[
            "worktree",
            "add",
            "-b",
            &plan_branch.branch_name,
            linked_path.to_str().expect("utf-8 linked workspace path"),
            "main",
        ],
    );
    std::fs::write(linked_path.join("local-plan-change.txt"), "discard me\n")
        .expect("write linked local change");

    let mut workspace = workspace_for(&project, conversation_id.clone());
    workspace.mode = AgentConversationWorkspaceMode::Ideation;
    workspace.branch_name = plan_branch.branch_name.clone();
    workspace.linked_ideation_session_id = Some(session_id);
    workspace.linked_plan_branch_id = Some(plan_branch.id.clone());
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("persist linked workspace");

    let outcome = cleanup_terminal_agent_workspace_after_pr(
        workspace_repo.clone(),
        Some(plan_branch_repo),
        &conversation_id,
        &project,
    )
    .await;

    assert_eq!(outcome.cleanup_claim, TerminalCleanupClaimState::Claimed);
    assert_eq!(outcome.local_cleanup, TerminalLocalCleanupResult::Cleaned);
    assert!(!linked_path.exists());
    assert!(!branch_exists(
        repository_dir.path(),
        &plan_branch.branch_name
    ));
    assert_eq!(
        workspace_repo
            .local_cleanup_status_for_test(&conversation_id)
            .await
            .as_deref(),
        Some("cleaned")
    );
}

#[tokio::test]
async fn terminal_cleanup_blocks_deletion_while_an_active_run_cannot_be_stopped() {
    let repository_dir = tempfile::tempdir().expect("repository tempdir");
    let worktree_parent = tempfile::tempdir().expect("worktree parent tempdir");
    setup_repo(repository_dir.path());
    let project = project_for(repository_dir.path(), worktree_parent.path());
    let conversation_id =
        ChatConversationId::from_string("44444444-4444-4444-4444-444444444444".to_string());
    let workspace = workspace_for(&project, conversation_id.clone());
    let worktree_path = Path::new(&workspace.worktree_path);
    std::fs::create_dir_all(worktree_path.parent().expect("workspace parent"))
        .expect("create workspace parent");
    run_git(
        repository_dir.path(),
        &[
            "worktree",
            "add",
            "-b",
            &workspace.branch_name,
            worktree_path.to_str().expect("utf-8 workspace path"),
            "main",
        ],
    );

    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("persist workspace");
    let run_repo = Arc::new(MemoryAgentRunRepository::new());
    run_repo
        .create(AgentRun::new(conversation_id.clone()))
        .await
        .expect("persist active run");

    let outcome = terminalize_agent_workspace_after_pr(
        workspace_repo.clone(),
        run_repo,
        None,
        None,
        &conversation_id,
        &project,
        TerminalAgentWorkspaceCause::ClosedPr,
    )
    .await;

    assert!(!outcome.runtime_shutdown_succeeded);
    assert_eq!(outcome.cleanup_claim, TerminalCleanupClaimState::NotClaimed);
    assert_eq!(outcome.local_cleanup, TerminalLocalCleanupResult::Pending);
    assert!(worktree_path.exists());
    assert!(branch_exists(repository_dir.path(), &workspace.branch_name));
    assert_eq!(
        workspace_repo
            .local_cleanup_status_for_test(&conversation_id)
            .await,
        None
    );
}

#[tokio::test]
async fn terminal_cleanup_rejects_missing_terminal_authority_without_deletion() {
    let repository_dir = tempfile::tempdir().expect("repository tempdir");
    let worktree_parent = tempfile::tempdir().expect("worktree parent tempdir");
    setup_repo(repository_dir.path());
    let project = project_for(repository_dir.path(), worktree_parent.path());
    let conversation_id =
        ChatConversationId::from_string("55555555-5555-5555-5555-555555555555".to_string());
    let mut workspace = workspace_for(&project, conversation_id.clone());
    workspace.status = AgentConversationWorkspaceStatus::Active;
    workspace.publication_pr_status = Some("open".to_string());
    let worktree_path = Path::new(&workspace.worktree_path);
    std::fs::create_dir_all(worktree_path.parent().expect("workspace parent"))
        .expect("create workspace parent");
    run_git(
        repository_dir.path(),
        &[
            "worktree",
            "add",
            "-b",
            &workspace.branch_name,
            worktree_path.to_str().expect("utf-8 workspace path"),
            "main",
        ],
    );
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("persist active workspace");

    let outcome = cleanup_terminal_agent_workspace_after_pr(
        workspace_repo.clone(),
        None,
        &conversation_id,
        &project,
    )
    .await;

    assert_eq!(outcome.cleanup_claim, TerminalCleanupClaimState::NotClaimed);
    assert_eq!(
        outcome.local_cleanup,
        TerminalLocalCleanupResult::FailedUnsafe
    );
    assert!(worktree_path.exists());
    assert!(branch_exists(repository_dir.path(), &workspace.branch_name));
    assert_eq!(
        workspace_repo
            .local_cleanup_status_for_test(&conversation_id)
            .await,
        None
    );
}

#[tokio::test]
async fn terminalize_stops_active_run_and_records_archive_reason_before_cleanup() {
    let repository_dir = tempfile::tempdir().expect("repository tempdir");
    let worktree_parent = tempfile::tempdir().expect("worktree parent tempdir");
    setup_repo(repository_dir.path());
    let project = project_for(repository_dir.path(), worktree_parent.path());
    let conversation_id =
        ChatConversationId::from_string("66666666-6666-6666-6666-666666666666".to_string());
    let workspace = workspace_for(&project, conversation_id.clone());
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("persist workspace");
    let run_repo = Arc::new(MemoryAgentRunRepository::new());
    let run = run_repo
        .create(AgentRun::new(conversation_id.clone()))
        .await
        .expect("persist active run");
    let chat_service = Arc::new(MockChatService::new());

    let outcome = terminalize_agent_workspace_after_pr(
        workspace_repo.clone(),
        run_repo.clone(),
        None,
        Some(chat_service.clone()),
        &conversation_id,
        &project,
        TerminalAgentWorkspaceCause::ArchivedConversation,
    )
    .await;

    assert!(outcome.runtime_shutdown_succeeded);
    assert_eq!(outcome.cleanup_claim, TerminalCleanupClaimState::Claimed);
    assert_eq!(outcome.local_cleanup, TerminalLocalCleanupResult::Cleaned);
    assert_eq!(
        chat_service.get_stop_agent_calls().await,
        vec![(
            ChatContextType::Project,
            conversation_id.as_str().to_string()
        )]
    );
    let stored_run = run_repo
        .get_by_id(&run.id)
        .await
        .expect("load failed run")
        .expect("run remains stored");
    assert!(!stored_run.is_active());
    assert!(stored_run
        .error_message
        .as_deref()
        .is_some_and(|message| message.contains("workspace conversation was archived")));
    assert_eq!(
        workspace_repo
            .local_cleanup_status_for_test(&conversation_id)
            .await
            .as_deref(),
        Some("cleaned")
    );
}

#[tokio::test]
async fn terminal_cleanup_returns_cleaned_when_marker_already_final() {
    let repository_dir = tempfile::tempdir().expect("repository tempdir");
    let worktree_parent = tempfile::tempdir().expect("worktree parent tempdir");
    setup_repo(repository_dir.path());
    let project = project_for(repository_dir.path(), worktree_parent.path());
    let conversation_id =
        ChatConversationId::from_string("77777777-7777-7777-7777-777777777777".to_string());
    let workspace = workspace_for(&project, conversation_id.clone());
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("persist workspace");
    workspace_repo
        .mark_local_cleanup_status(&conversation_id, "cleaned", chrono::Utc::now())
        .await
        .expect("mark final cleanup");

    let outcome =
        cleanup_terminal_agent_workspace_after_pr(workspace_repo, None, &conversation_id, &project)
            .await;

    assert_eq!(
        outcome.cleanup_claim,
        TerminalCleanupClaimState::AlreadyCleaned
    );
    assert_eq!(outcome.local_cleanup, TerminalLocalCleanupResult::Cleaned);
    assert_eq!(outcome.message, None);
}

#[tokio::test]
async fn terminal_cleanup_fails_closed_when_linked_plan_repo_is_unavailable() {
    let repository_dir = tempfile::tempdir().expect("repository tempdir");
    let worktree_parent = tempfile::tempdir().expect("worktree parent tempdir");
    setup_repo(repository_dir.path());
    let project = project_for(repository_dir.path(), worktree_parent.path());
    let conversation_id =
        ChatConversationId::from_string("88888888-8888-8888-8888-888888888888".to_string());
    let mut workspace = workspace_for(&project, conversation_id.clone());
    workspace.mode = AgentConversationWorkspaceMode::Ideation;
    workspace.linked_plan_branch_id = Some(PlanBranchId::from_string("missing-plan-branch"));
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("persist linked workspace");

    let outcome = cleanup_terminal_agent_workspace_after_pr(
        workspace_repo.clone(),
        None,
        &conversation_id,
        &project,
    )
    .await;

    assert_eq!(outcome.cleanup_claim, TerminalCleanupClaimState::Claimed);
    assert_eq!(
        outcome.local_cleanup,
        TerminalLocalCleanupResult::FailedUnsafe
    );
    assert!(outcome
        .message
        .as_deref()
        .is_some_and(|message| message.contains("repository is unavailable")));
    assert_eq!(
        workspace_repo
            .local_cleanup_status_for_test(&conversation_id)
            .await
            .as_deref(),
        Some("failed_unsafe")
    );
}

#[tokio::test]
async fn terminal_cleanup_fails_closed_when_linked_plan_branch_is_missing() {
    let repository_dir = tempfile::tempdir().expect("repository tempdir");
    let worktree_parent = tempfile::tempdir().expect("worktree parent tempdir");
    setup_repo(repository_dir.path());
    let project = project_for(repository_dir.path(), worktree_parent.path());
    let conversation_id =
        ChatConversationId::from_string("99999999-9999-9999-9999-999999999999".to_string());
    let mut workspace = workspace_for(&project, conversation_id.clone());
    workspace.mode = AgentConversationWorkspaceMode::Ideation;
    workspace.linked_plan_branch_id = Some(PlanBranchId::from_string("missing-plan-branch"));
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("persist linked workspace");

    let outcome = cleanup_terminal_agent_workspace_after_pr(
        workspace_repo.clone(),
        Some(Arc::new(MemoryPlanBranchRepository::new())),
        &conversation_id,
        &project,
    )
    .await;

    assert_eq!(outcome.cleanup_claim, TerminalCleanupClaimState::Claimed);
    assert_eq!(
        outcome.local_cleanup,
        TerminalLocalCleanupResult::FailedUnsafe
    );
    assert!(outcome
        .message
        .as_deref()
        .is_some_and(|message| message.contains("was not found")));
}

#[tokio::test]
async fn terminal_cleanup_fails_closed_when_project_checkout_path_is_invalid() {
    let repository_dir = tempfile::tempdir().expect("repository tempdir");
    let invalid_repo_path = repository_dir.path().join("not-a-directory");
    std::fs::write(&invalid_repo_path, "not a git checkout\n").expect("write invalid repo path");
    let worktree_parent = tempfile::tempdir().expect("worktree parent tempdir");
    let project = project_for(&invalid_repo_path, worktree_parent.path());
    let conversation_id =
        ChatConversationId::from_string("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa".to_string());
    let workspace = workspace_for(&project, conversation_id.clone());
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("persist workspace");

    let outcome = cleanup_terminal_agent_workspace_after_pr(
        workspace_repo.clone(),
        None,
        &conversation_id,
        &project,
    )
    .await;

    assert_eq!(outcome.cleanup_claim, TerminalCleanupClaimState::Claimed);
    assert_eq!(
        outcome.local_cleanup,
        TerminalLocalCleanupResult::FailedUnsafe
    );
    assert!(outcome.message.is_some());
    assert_eq!(
        workspace_repo
            .local_cleanup_status_for_test(&conversation_id)
            .await
            .as_deref(),
        Some("failed_unsafe")
    );
}

#[tokio::test]
async fn terminal_cleanup_target_path_returns_direct_workspace_path() {
    let repository_dir = tempfile::tempdir().expect("repository tempdir");
    let worktree_parent = tempfile::tempdir().expect("worktree parent tempdir");
    setup_repo(repository_dir.path());
    let project = project_for(repository_dir.path(), worktree_parent.path());
    let conversation_id =
        ChatConversationId::from_string("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb".to_string());
    let workspace = workspace_for(&project, conversation_id);
    let plan_branch_repo = MemoryPlanBranchRepository::new();

    let path = terminal_cleanup_target_path(&workspace, &project, &plan_branch_repo)
        .await
        .expect("direct workspace path should resolve");

    assert_eq!(path, Path::new(&workspace.worktree_path));
}

#[cfg(unix)]
#[tokio::test]
async fn terminal_cleanup_target_path_rejects_symlink_before_process_cleanup() {
    let repository_dir = tempfile::tempdir().expect("repository tempdir");
    let worktree_parent = tempfile::tempdir().expect("worktree parent tempdir");
    let outside = tempfile::tempdir().expect("outside target tempdir");
    setup_repo(repository_dir.path());
    std::fs::write(outside.path().join("keep.txt"), "keep\n").expect("write outside sentinel");
    let project = project_for(repository_dir.path(), worktree_parent.path());
    let conversation_id =
        ChatConversationId::from_string("bcbcbcbc-bcbc-bcbc-bcbc-bcbcbcbcbcbc".to_string());
    let workspace = workspace_for(&project, conversation_id);
    let workspace_path = Path::new(&workspace.worktree_path);
    std::fs::create_dir_all(workspace_path.parent().expect("workspace parent"))
        .expect("create workspace parent");
    std::os::unix::fs::symlink(outside.path(), workspace_path).expect("create workspace symlink");

    let error =
        terminal_cleanup_target_path(&workspace, &project, &MemoryPlanBranchRepository::new())
            .await
            .expect_err("symlinked target must fail before process cleanup");

    assert!(error.contains("symlink"));
    assert!(outside.path().join("keep.txt").exists());
}

#[tokio::test]
async fn terminal_cleanup_target_path_rejects_mismatched_linked_plan_identity() {
    let repository_dir = tempfile::tempdir().expect("repository tempdir");
    let worktree_parent = tempfile::tempdir().expect("worktree parent tempdir");
    setup_repo(repository_dir.path());
    let project = project_for(repository_dir.path(), worktree_parent.path());
    let conversation_id =
        ChatConversationId::from_string("bdbdbdbd-bdbd-bdbd-bdbd-bdbdbdbdbdbd".to_string());
    let mut workspace = workspace_for(&project, conversation_id);
    workspace.mode = AgentConversationWorkspaceMode::Ideation;
    workspace.linked_ideation_session_id =
        Some(IdeationSessionId::from_string("workspace-session"));
    let mut plan_branch = PlanBranch::new(
        ArtifactId::from_string("artifact-linked-mismatch"),
        IdeationSessionId::from_string("different-session"),
        project.id.clone(),
        "ralphx/project/plan-linked-mismatch".to_string(),
        "main".to_string(),
    );
    plan_branch.id = PlanBranchId::from_string("linked-mismatch");
    workspace.linked_plan_branch_id = Some(plan_branch.id.clone());
    workspace.branch_name = plan_branch.branch_name.clone();
    let plan_branch_repo = MemoryPlanBranchRepository::new();
    plan_branch_repo
        .create(plan_branch)
        .await
        .expect("persist plan branch");

    let error = terminal_cleanup_target_path(&workspace, &project, &plan_branch_repo)
        .await
        .expect_err("mismatched plan identity must fail before process cleanup");

    assert!(error.contains("ideation session"));
}

#[tokio::test]
async fn terminalize_blocks_cleanup_when_active_run_lookup_fails() {
    let repository_dir = tempfile::tempdir().expect("repository tempdir");
    let worktree_parent = tempfile::tempdir().expect("worktree parent tempdir");
    setup_repo(repository_dir.path());
    let project = project_for(repository_dir.path(), worktree_parent.path());
    let conversation_id =
        ChatConversationId::from_string("cccccccc-cccc-cccc-cccc-cccccccccccc".to_string());
    let workspace = workspace_for(&project, conversation_id.clone());
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("persist workspace");
    let run_repo = Arc::new(ControlledAgentRunRepo::with_active_results([Err(
        AppError::Database("lookup unavailable".to_string()),
    )]));

    let outcome = terminalize_agent_workspace_after_pr(
        workspace_repo.clone(),
        run_repo,
        None,
        None,
        &conversation_id,
        &project,
        TerminalAgentWorkspaceCause::ClosedPr,
    )
    .await;

    assert!(!outcome.runtime_shutdown_succeeded);
    assert_eq!(outcome.cleanup_claim, TerminalCleanupClaimState::NotClaimed);
    assert_eq!(outcome.local_cleanup, TerminalLocalCleanupResult::Pending);
    assert!(outcome
        .message
        .as_deref()
        .is_some_and(|message| message.contains("Failed to inspect")));
    assert!(workspace_repo
        .local_cleanup_status_for_test(&conversation_id)
        .await
        .is_none());
}

#[tokio::test]
async fn terminalize_blocks_cleanup_when_runtime_stop_fails() {
    let repository_dir = tempfile::tempdir().expect("repository tempdir");
    let worktree_parent = tempfile::tempdir().expect("worktree parent tempdir");
    setup_repo(repository_dir.path());
    let project = project_for(repository_dir.path(), worktree_parent.path());
    let conversation_id =
        ChatConversationId::from_string("dddddddd-dddd-dddd-dddd-dddddddddddd".to_string());
    let workspace = workspace_for(&project, conversation_id.clone());
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("persist workspace");
    let run_repo = Arc::new(MemoryAgentRunRepository::new());
    run_repo
        .create(AgentRun::new(conversation_id.clone()))
        .await
        .expect("persist run");
    let chat_service = Arc::new(MockChatService::new());
    chat_service.fail_next_stop_agent_calls(1).await;

    let outcome = terminalize_agent_workspace_after_pr(
        workspace_repo.clone(),
        run_repo,
        None,
        Some(chat_service.clone()),
        &conversation_id,
        &project,
        TerminalAgentWorkspaceCause::ClosedPr,
    )
    .await;

    assert!(!outcome.runtime_shutdown_succeeded);
    assert_eq!(outcome.cleanup_claim, TerminalCleanupClaimState::NotClaimed);
    assert_eq!(outcome.local_cleanup, TerminalLocalCleanupResult::Pending);
    assert_eq!(
        chat_service.get_stop_agent_calls().await.len(),
        1,
        "runtime stop should be attempted before blocking cleanup"
    );
    assert!(outcome
        .message
        .as_deref()
        .is_some_and(|message| message.contains("Failed to stop")));
    assert!(workspace_repo
        .local_cleanup_status_for_test(&conversation_id)
        .await
        .is_none());
}

#[tokio::test]
async fn terminalize_blocks_cleanup_when_failed_run_cannot_be_persisted() {
    let repository_dir = tempfile::tempdir().expect("repository tempdir");
    let worktree_parent = tempfile::tempdir().expect("worktree parent tempdir");
    setup_repo(repository_dir.path());
    let project = project_for(repository_dir.path(), worktree_parent.path());
    let conversation_id =
        ChatConversationId::from_string("eeeeeeee-eeee-eeee-eeee-eeeeeeeeeeee".to_string());
    let workspace = workspace_for(&project, conversation_id.clone());
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("persist workspace");
    let active_run = AgentRun::new(conversation_id.clone());
    let run_repo = Arc::new(
        ControlledAgentRunRepo::with_active_results([Ok(Some(active_run.clone())), Ok(None)])
            .with_fail_result(Err(AppError::Database("write unavailable".to_string()))),
    );
    let chat_service = Arc::new(MockChatService::new());

    let outcome = terminalize_agent_workspace_after_pr(
        workspace_repo.clone(),
        run_repo,
        None,
        Some(chat_service),
        &conversation_id,
        &project,
        TerminalAgentWorkspaceCause::MergedPr,
    )
    .await;

    assert!(!outcome.runtime_shutdown_succeeded);
    assert_eq!(outcome.cleanup_claim, TerminalCleanupClaimState::NotClaimed);
    assert_eq!(outcome.local_cleanup, TerminalLocalCleanupResult::Pending);
    assert!(outcome
        .message
        .as_deref()
        .is_some_and(|message| message.contains("Failed to persist")));
    assert!(workspace_repo
        .local_cleanup_status_for_test(&conversation_id)
        .await
        .is_none());
}

#[tokio::test]
async fn terminalize_blocks_cleanup_when_active_run_remains_after_stop() {
    let repository_dir = tempfile::tempdir().expect("repository tempdir");
    let worktree_parent = tempfile::tempdir().expect("worktree parent tempdir");
    setup_repo(repository_dir.path());
    let project = project_for(repository_dir.path(), worktree_parent.path());
    let conversation_id =
        ChatConversationId::from_string("ffffffff-ffff-ffff-ffff-ffffffffffff".to_string());
    let workspace = workspace_for(&project, conversation_id.clone());
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("persist workspace");
    let active_run = AgentRun::new(conversation_id.clone());
    let run_repo = Arc::new(ControlledAgentRunRepo::with_active_results([
        Ok(Some(active_run.clone())),
        Ok(Some(active_run)),
    ]));
    let chat_service = Arc::new(MockChatService::new());

    let outcome = terminalize_agent_workspace_after_pr(
        workspace_repo.clone(),
        run_repo,
        None,
        Some(chat_service),
        &conversation_id,
        &project,
        TerminalAgentWorkspaceCause::ClosedPr,
    )
    .await;

    assert!(!outcome.runtime_shutdown_succeeded);
    assert_eq!(outcome.cleanup_claim, TerminalCleanupClaimState::NotClaimed);
    assert_eq!(outcome.local_cleanup, TerminalLocalCleanupResult::Pending);
    assert!(outcome
        .message
        .as_deref()
        .is_some_and(|message| message.contains("remained active")));
    assert!(workspace_repo
        .local_cleanup_status_for_test(&conversation_id)
        .await
        .is_none());
}

#[tokio::test]
async fn terminalize_blocks_cleanup_when_post_stop_run_lookup_fails() {
    let repository_dir = tempfile::tempdir().expect("repository tempdir");
    let worktree_parent = tempfile::tempdir().expect("worktree parent tempdir");
    setup_repo(repository_dir.path());
    let project = project_for(repository_dir.path(), worktree_parent.path());
    let conversation_id =
        ChatConversationId::from_string("10101010-1010-1010-1010-101010101010".to_string());
    let workspace = workspace_for(&project, conversation_id.clone());
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("persist workspace");
    let active_run = AgentRun::new(conversation_id.clone());
    let run_repo = Arc::new(ControlledAgentRunRepo::with_active_results([
        Ok(Some(active_run)),
        Err(AppError::Database("verify unavailable".to_string())),
    ]));
    let chat_service = Arc::new(MockChatService::new());

    let outcome = terminalize_agent_workspace_after_pr(
        workspace_repo.clone(),
        run_repo,
        None,
        Some(chat_service),
        &conversation_id,
        &project,
        TerminalAgentWorkspaceCause::ClosedPr,
    )
    .await;

    assert!(!outcome.runtime_shutdown_succeeded);
    assert_eq!(outcome.cleanup_claim, TerminalCleanupClaimState::NotClaimed);
    assert_eq!(outcome.local_cleanup, TerminalLocalCleanupResult::Pending);
    assert!(outcome
        .message
        .as_deref()
        .is_some_and(|message| message.contains("Failed to verify")));
    assert!(workspace_repo
        .local_cleanup_status_for_test(&conversation_id)
        .await
        .is_none());
}

#[tokio::test]
async fn terminal_cleanup_reports_claim_repository_failure_without_deletion() {
    let repository_dir = tempfile::tempdir().expect("repository tempdir");
    let worktree_parent = tempfile::tempdir().expect("worktree parent tempdir");
    setup_repo(repository_dir.path());
    let project = project_for(repository_dir.path(), worktree_parent.path());
    let conversation_id =
        ChatConversationId::from_string("11111111-2222-3333-4444-555555555555".to_string());
    let workspace = workspace_for(&project, conversation_id.clone());
    let repo = ControlledWorkspaceRepo::new();
    repo.create_or_update(workspace)
        .await
        .expect("persist workspace");
    repo.fail_next_claim("claim unavailable");
    let repo = Arc::new(repo);

    let outcome =
        cleanup_terminal_agent_workspace_after_pr(repo, None, &conversation_id, &project).await;

    assert_eq!(outcome.cleanup_claim, TerminalCleanupClaimState::NotClaimed);
    assert_eq!(
        outcome.local_cleanup,
        TerminalLocalCleanupResult::FailedOperational
    );
    assert!(outcome
        .message
        .as_deref()
        .is_some_and(|message| message.contains("Failed to claim")));
}

#[tokio::test]
async fn terminal_cleanup_reports_finalize_repository_failure_after_local_cleanup() {
    let repository_dir = tempfile::tempdir().expect("repository tempdir");
    let worktree_parent = tempfile::tempdir().expect("worktree parent tempdir");
    setup_repo(repository_dir.path());
    let project = project_for(repository_dir.path(), worktree_parent.path());
    let conversation_id =
        ChatConversationId::from_string("12121212-3434-5656-7878-909090909090".to_string());
    let workspace = workspace_for(&project, conversation_id.clone());
    let repo = ControlledWorkspaceRepo::new();
    repo.create_or_update(workspace)
        .await
        .expect("persist workspace");
    repo.fail_next_finalize("finalize unavailable");
    let repo = Arc::new(repo);

    let outcome =
        cleanup_terminal_agent_workspace_after_pr(repo, None, &conversation_id, &project).await;

    assert_eq!(outcome.cleanup_claim, TerminalCleanupClaimState::Claimed);
    assert_eq!(
        outcome.local_cleanup,
        TerminalLocalCleanupResult::FailedOperational
    );
    assert!(outcome
        .message
        .as_deref()
        .is_some_and(|message| message.contains("Failed to persist")));
}

#[tokio::test]
async fn terminal_cleanup_reports_already_cleaned_when_claim_was_settled_elsewhere() {
    let repository_dir = tempfile::tempdir().expect("repository tempdir");
    let worktree_parent = tempfile::tempdir().expect("worktree parent tempdir");
    setup_repo(repository_dir.path());
    let project = project_for(repository_dir.path(), worktree_parent.path());
    let conversation_id =
        ChatConversationId::from_string("13131313-2424-3535-4646-575757575757".to_string());
    let workspace = workspace_for(&project, conversation_id.clone());
    let repo = ControlledWorkspaceRepo::new();
    repo.create_or_update(workspace)
        .await
        .expect("persist workspace");
    repo.set_next_finalize(false);
    repo.set_next_status(Some("cleaned".to_string()));
    let repo = Arc::new(repo);

    let outcome =
        cleanup_terminal_agent_workspace_after_pr(repo, None, &conversation_id, &project).await;

    assert_eq!(
        outcome.cleanup_claim,
        TerminalCleanupClaimState::AlreadyCleaned
    );
    assert_eq!(outcome.local_cleanup, TerminalLocalCleanupResult::Cleaned);
    assert_eq!(outcome.message, None);
}

#[tokio::test]
async fn terminal_cleanup_reports_lost_claim_when_finalize_does_not_settle() {
    let repository_dir = tempfile::tempdir().expect("repository tempdir");
    let worktree_parent = tempfile::tempdir().expect("worktree parent tempdir");
    setup_repo(repository_dir.path());
    let project = project_for(repository_dir.path(), worktree_parent.path());
    let conversation_id =
        ChatConversationId::from_string("14141414-2525-3636-4747-585858585858".to_string());
    let workspace = workspace_for(&project, conversation_id.clone());
    let repo = ControlledWorkspaceRepo::new();
    repo.create_or_update(workspace)
        .await
        .expect("persist workspace");
    repo.set_next_finalize(false);
    repo.set_next_status(Some("cleaning".to_string()));
    let repo = Arc::new(repo);

    let outcome =
        cleanup_terminal_agent_workspace_after_pr(repo, None, &conversation_id, &project).await;

    assert_eq!(outcome.cleanup_claim, TerminalCleanupClaimState::Claimed);
    assert_eq!(
        outcome.local_cleanup,
        TerminalLocalCleanupResult::FailedOperational
    );
    assert!(outcome
        .message
        .as_deref()
        .is_some_and(|message| message.contains("claim was no longer current")));
}

struct ControlledAgentRunRepo {
    active_results: Mutex<VecDeque<AppResult<Option<AgentRun>>>>,
    fail_result: Mutex<Option<AppResult<()>>>,
}

impl ControlledAgentRunRepo {
    fn with_active_results(results: impl IntoIterator<Item = AppResult<Option<AgentRun>>>) -> Self {
        Self {
            active_results: Mutex::new(results.into_iter().collect()),
            fail_result: Mutex::new(None),
        }
    }

    fn with_fail_result(self, result: AppResult<()>) -> Self {
        *self.fail_result.lock().expect("fail result lock") = Some(result);
        self
    }
}

#[async_trait]
impl AgentRunRepository for ControlledAgentRunRepo {
    async fn create(&self, run: AgentRun) -> AppResult<AgentRun> {
        Ok(run)
    }

    async fn get_by_id(&self, _id: &AgentRunId) -> AppResult<Option<AgentRun>> {
        Ok(None)
    }

    async fn get_latest_for_conversation(
        &self,
        _conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentRun>> {
        Ok(None)
    }

    async fn get_active_for_conversation(
        &self,
        _conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentRun>> {
        self.active_results
            .lock()
            .expect("active results lock")
            .pop_front()
            .unwrap_or(Ok(None))
    }

    async fn get_by_conversation(
        &self,
        _conversation_id: &ChatConversationId,
    ) -> AppResult<Vec<AgentRun>> {
        Ok(Vec::new())
    }

    async fn update_status(&self, _id: &AgentRunId, _status: AgentRunStatus) -> AppResult<()> {
        Ok(())
    }

    async fn update_usage(&self, _id: &AgentRunId, _usage: &AgentRunUsage) -> AppResult<()> {
        Ok(())
    }

    async fn update_attribution(
        &self,
        _id: &AgentRunId,
        _attribution: &AgentRunAttribution,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn set_persona_attribution(
        &self,
        _id: &AgentRunId,
        _attribution: PersonaRunAttribution,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn complete(&self, _id: &AgentRunId) -> AppResult<()> {
        Ok(())
    }

    async fn complete_if_prune_cancelled(&self, _id: &AgentRunId) -> AppResult<bool> {
        Ok(false)
    }

    async fn fail(&self, _id: &AgentRunId, _error_message: &str) -> AppResult<()> {
        self.fail_result
            .lock()
            .expect("fail result lock")
            .take()
            .unwrap_or(Ok(()))
    }

    async fn cancel(&self, _id: &AgentRunId) -> AppResult<()> {
        Ok(())
    }

    async fn cancel_with_reason(&self, _id: &AgentRunId, _reason: &str) -> AppResult<()> {
        Ok(())
    }

    async fn delete(&self, _id: &AgentRunId) -> AppResult<()> {
        Ok(())
    }

    async fn delete_by_conversation(&self, _conversation_id: &ChatConversationId) -> AppResult<()> {
        Ok(())
    }

    async fn count_by_status(
        &self,
        _conversation_id: &ChatConversationId,
        _status: AgentRunStatus,
    ) -> AppResult<u32> {
        Ok(0)
    }

    async fn cancel_all_running(&self) -> AppResult<u32> {
        Ok(0)
    }

    async fn cancel_running_started_before(&self, _cutoff: DateTime<Utc>) -> AppResult<u32> {
        Ok(0)
    }

    async fn get_interrupted_conversations(&self) -> AppResult<Vec<InterruptedConversation>> {
        Ok(Vec::new())
    }
}

struct ControlledWorkspaceRepo {
    inner: MemoryAgentConversationWorkspaceRepository,
    claim_result: Mutex<Option<AppResult<AgentWorkspaceLocalCleanupClaim>>>,
    finalize_result: Mutex<Option<AppResult<bool>>>,
    status_result: Mutex<Option<AppResult<Option<String>>>>,
}

impl ControlledWorkspaceRepo {
    fn new() -> Self {
        Self {
            inner: MemoryAgentConversationWorkspaceRepository::new(),
            claim_result: Mutex::new(None),
            finalize_result: Mutex::new(None),
            status_result: Mutex::new(None),
        }
    }

    fn fail_next_claim(&self, message: &str) {
        *self.claim_result.lock().expect("claim result lock") =
            Some(Err(AppError::Database(message.to_string())));
    }

    fn fail_next_finalize(&self, message: &str) {
        *self.finalize_result.lock().expect("finalize result lock") =
            Some(Err(AppError::Database(message.to_string())));
    }

    fn set_next_finalize(&self, finalized: bool) {
        *self.finalize_result.lock().expect("finalize result lock") = Some(Ok(finalized));
    }

    fn set_next_status(&self, status: Option<String>) {
        *self.status_result.lock().expect("status result lock") = Some(Ok(status));
    }
}

#[async_trait]
impl AgentConversationWorkspaceRepository for ControlledWorkspaceRepo {
    async fn set_last_blocked_pr_health_fingerprint(
        &self,
        _conversation_id: &ChatConversationId,
        _fingerprint: Option<&str>,
    ) -> AppResult<()> {
        Ok(())
    }
    async fn create_or_update(
        &self,
        workspace: AgentConversationWorkspace,
    ) -> AppResult<AgentConversationWorkspace> {
        self.inner.create_or_update(workspace).await
    }

    async fn get_by_conversation_id(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentConversationWorkspace>> {
        self.inner.get_by_conversation_id(conversation_id).await
    }

    async fn get_by_project_id(
        &self,
        project_id: &ProjectId,
    ) -> AppResult<Vec<AgentConversationWorkspace>> {
        self.inner.get_by_project_id(project_id).await
    }

    async fn claim_local_cleanup(
        &self,
        conversation_id: &ChatConversationId,
        claimed_at: DateTime<Utc>,
        stale_before: DateTime<Utc>,
    ) -> AppResult<AgentWorkspaceLocalCleanupClaim> {
        if let Some(result) = self.claim_result.lock().expect("claim result lock").take() {
            return result;
        }
        self.inner
            .claim_local_cleanup(conversation_id, claimed_at, stale_before)
            .await
    }

    async fn finalize_local_cleanup(
        &self,
        conversation_id: &ChatConversationId,
        claimed_at: DateTime<Utc>,
        status: &str,
        checked_at: DateTime<Utc>,
    ) -> AppResult<bool> {
        if let Some(result) = self
            .finalize_result
            .lock()
            .expect("finalize result lock")
            .take()
        {
            return result;
        }
        self.inner
            .finalize_local_cleanup(conversation_id, claimed_at, status, checked_at)
            .await
    }

    async fn get_local_cleanup_status(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<String>> {
        if let Some(result) = self
            .status_result
            .lock()
            .expect("status result lock")
            .take()
        {
            return result;
        }
        self.inner.get_local_cleanup_status(conversation_id).await
    }

    async fn list_active_direct_published_workspaces(
        &self,
    ) -> AppResult<Vec<AgentConversationWorkspace>> {
        self.inner.list_active_direct_published_workspaces().await
    }

    async fn list_active_needs_agent_workspaces(
        &self,
    ) -> AppResult<Vec<AgentConversationWorkspace>> {
        self.inner.list_active_needs_agent_workspaces().await
    }

    async fn update_links(
        &self,
        conversation_id: &ChatConversationId,
        ideation_session_id: Option<&IdeationSessionId>,
        plan_branch_id: Option<&PlanBranchId>,
    ) -> AppResult<()> {
        self.inner
            .update_links(conversation_id, ideation_session_id, plan_branch_id)
            .await
    }

    async fn update_publication(
        &self,
        conversation_id: &ChatConversationId,
        pr_number: Option<i64>,
        pr_url: Option<&str>,
        pr_status: Option<&str>,
        push_status: Option<&str>,
    ) -> AppResult<()> {
        self.inner
            .update_publication(conversation_id, pr_number, pr_url, pr_status, push_status)
            .await
    }

    async fn update_pr_supervision_preferences(
        &self,
        conversation_id: &ChatConversationId,
        autofix_enabled: bool,
        auto_merge_desired: bool,
        auto_merge_method: &str,
    ) -> AppResult<()> {
        self.inner
            .update_pr_supervision_preferences(
                conversation_id,
                autofix_enabled,
                auto_merge_desired,
                auto_merge_method,
            )
            .await
    }

    async fn update_status(
        &self,
        conversation_id: &ChatConversationId,
        status: AgentConversationWorkspaceStatus,
    ) -> AppResult<()> {
        self.inner.update_status(conversation_id, status).await
    }

    async fn save_pr_description(
        &self,
        conversation_id: &ChatConversationId,
        description: AgentWorkspacePrDescription,
    ) -> AppResult<()> {
        self.inner
            .save_pr_description(conversation_id, description)
            .await
    }

    async fn get_pr_description(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentWorkspacePrDescription>> {
        self.inner.get_pr_description(conversation_id).await
    }

    async fn clear_pr_description(&self, conversation_id: &ChatConversationId) -> AppResult<()> {
        self.inner.clear_pr_description(conversation_id).await
    }

    async fn append_publication_event(
        &self,
        event: AgentConversationWorkspacePublicationEvent,
    ) -> AppResult<()> {
        self.inner.append_publication_event(event).await
    }

    async fn list_publication_events(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Vec<AgentConversationWorkspacePublicationEvent>> {
        self.inner.list_publication_events(conversation_id).await
    }

    async fn set_pr_review_auto_approve_enabled(
        &self,
        conversation_id: &ChatConversationId,
        enabled: bool,
    ) -> AppResult<AgentWorkspacePrReviewMonitor> {
        self.inner
            .set_pr_review_auto_approve_enabled(conversation_id, enabled)
            .await
    }

    async fn mark_pr_review_first_action_resolved(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<AgentWorkspacePrReviewMonitor> {
        self.inner
            .mark_pr_review_first_action_resolved(conversation_id)
            .await
    }

    async fn claim_pending_pr_review_action(&self, action_id: &str) -> AppResult<bool> {
        self.inner.claim_pending_pr_review_action(action_id).await
    }

    async fn delete(&self, conversation_id: &ChatConversationId) -> AppResult<()> {
        self.inner.delete(conversation_id).await
    }
}
