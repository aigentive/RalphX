use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use crate::application::agent_conversation_workspace::{
    agent_conversation_branch_name, resolve_agent_conversation_workspace_path,
    resolve_linked_plan_branch_agent_worktree_path,
};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentConversationWorkspaceStatus,
    AgentRun, ArtifactId, ChatConversationId, IdeationAnalysisBaseRefKind, IdeationSessionId,
    PlanBranch, Project, ProjectId,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, AgentRunRepository, PlanBranchRepository,
};
use crate::infrastructure::memory::{
    MemoryAgentConversationWorkspaceRepository, MemoryAgentRunRepository,
    MemoryPlanBranchRepository,
};

use super::agent_workspace_terminal_cleanup::{
    cleanup_terminal_agent_workspace_after_pr, terminalize_agent_workspace_after_pr,
    TerminalAgentWorkspaceCause, TerminalCleanupClaimState, TerminalLocalCleanupResult,
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
        .is_some_and(|message| message.contains("workspace_path_mismatch")));
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
