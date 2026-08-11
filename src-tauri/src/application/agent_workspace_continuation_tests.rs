use super::agent_workspace_continuation::*;
use crate::application::agent_conversation_workspace::{
    resolve_agent_conversation_workspace_path, resolve_linked_plan_branch_agent_worktree_path,
};
use crate::application::GitService;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentConversationWorkspaceStatus,
    ArtifactId, ChatConversationId, IdeationAnalysisBaseRefKind, IdeationSessionId, PlanBranch,
    Project,
};
use crate::domain::repositories::PlanBranchRepository;
use crate::infrastructure::memory::MemoryPlanBranchRepository;
use crate::utils::path_safety::validate_absolute_non_root_path;
use std::fs;
use std::path::Path;
use std::process::Command;

fn test_project(parent: &tempfile::TempDir) -> Project {
    let project_root = parent.path().join("project-root");
    fs::create_dir_all(&project_root).expect("project root should be created");
    let mut project = Project::new(
        "Continuation Guard".to_string(),
        project_root.to_string_lossy().to_string(),
    );
    project.worktree_parent_directory = Some(
        parent
            .path()
            .join("worktrees")
            .to_string_lossy()
            .to_string(),
    );
    project
}

fn git(repo: &Path, args: &[&str]) {
    let repo = validate_absolute_non_root_path(repo, "continuation test repository")
        .expect("test repository path should be safe");
    let output = Command::new("git")
        .args(args)
        // codeql[rust/path-injection]
        .current_dir(repo)
        .output()
        .expect("git command should run");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn setup_git_project(parent: &tempfile::TempDir) -> Project {
    let project = test_project(parent);
    let project_root = validate_absolute_non_root_path(
        Path::new(&project.working_directory),
        "continuation test project",
    )
    .expect("project root should be safe");
    git(&project_root, &["init", "-b", "main"]);
    git(&project_root, &["config", "user.email", "test@example.com"]);
    git(&project_root, &["config", "user.name", "Test User"]);
    let readme = project_root.join("README.md");
    // codeql[rust/path-injection]
    fs::write(readme, "continuation\n").expect("fixture should be written");
    git(&project_root, &["add", "README.md"]);
    git(&project_root, &["commit", "-m", "initial"]);
    project
}

fn test_workspace(
    project: &Project,
    conversation_id: ChatConversationId,
) -> AgentConversationWorkspace {
    let expected_path = resolve_agent_conversation_workspace_path(project, &conversation_id)
        .expect("expected workspace path should resolve");
    AgentConversationWorkspace::new(
        conversation_id,
        project.id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        Some("base-sha".to_string()),
        "ralphx/continuation/agent-test".to_string(),
        expected_path.to_string_lossy().to_string(),
    )
}

fn create_git_worktree(project: &Project, workspace: &AgentConversationWorkspace) {
    let path = resolve_agent_conversation_workspace_path(project, &workspace.conversation_id)
        .expect("expected workspace path should resolve");
    fs::create_dir_all(&path).expect("workspace path should be created");
    fs::write(path.join(".git"), "gitdir: ../.git/worktrees/agent-test")
        .expect("git marker should be created");
}

#[test]
fn classify_allows_active_non_terminal_workspace_with_valid_worktree() {
    let parent = tempfile::tempdir().expect("temp dir should be created");
    let project = test_project(&parent);
    let workspace = test_workspace(&project, ChatConversationId::new());
    create_git_worktree(&project, &workspace);

    let availability = classify_agent_workspace_continuation(&project, &workspace);

    assert!(availability.is_available());
    assert_eq!(availability.blocked_reason(), None);
}

#[test]
fn classify_blocks_terminal_workspace_even_when_worktree_exists() {
    let parent = tempfile::tempdir().expect("temp dir should be created");
    let project = test_project(&parent);
    let mut workspace = test_workspace(&project, ChatConversationId::new());
    workspace.publication_pr_status = Some("closed".to_string());
    create_git_worktree(&project, &workspace);

    let reason = classify_agent_workspace_continuation(&project, &workspace)
        .blocked_reason()
        .cloned();

    assert_eq!(
        reason,
        Some(AgentWorkspaceContinuationBlock::TerminalWorkspace)
    );
    assert_eq!(reason.unwrap().code(), "terminal_workspace");
}

#[test]
fn classify_blocks_cleaned_workspace_after_terminal_pr() {
    let parent = tempfile::tempdir().expect("temp dir should be created");
    let project = test_project(&parent);
    let mut workspace = test_workspace(&project, ChatConversationId::new());
    workspace.publication_pr_status = Some("merged".to_string());

    let reason = classify_agent_workspace_continuation(&project, &workspace)
        .blocked_reason()
        .cloned();

    assert_eq!(
        reason,
        Some(AgentWorkspaceContinuationBlock::CleanedAfterTerminal)
    );
    assert_eq!(reason.unwrap().code(), "cleaned_after_terminal");
}

#[test]
fn classify_blocks_missing_non_terminal_workspace() {
    let parent = tempfile::tempdir().expect("temp dir should be created");
    let project = test_project(&parent);
    let workspace = test_workspace(&project, ChatConversationId::new());

    let reason = classify_agent_workspace_continuation(&project, &workspace)
        .blocked_reason()
        .cloned();

    assert_eq!(
        reason,
        Some(AgentWorkspaceContinuationBlock::LocalWorkspaceMissing)
    );
    assert_eq!(reason.unwrap().code(), "local_workspace_missing");
}

#[test]
fn classify_blocks_archived_and_recorded_missing_status_before_path_check() {
    let parent = tempfile::tempdir().expect("temp dir should be created");
    let project = test_project(&parent);
    let mut archived = test_workspace(&project, ChatConversationId::new());
    archived.status = AgentConversationWorkspaceStatus::Archived;
    let mut missing = test_workspace(&project, ChatConversationId::new());
    missing.status = AgentConversationWorkspaceStatus::Missing;

    assert_eq!(
        classify_agent_workspace_continuation(&project, &archived)
            .blocked_reason()
            .cloned(),
        Some(AgentWorkspaceContinuationBlock::ArchivedWorkspace)
    );
    assert_eq!(
        classify_agent_workspace_continuation(&project, &missing)
            .blocked_reason()
            .cloned(),
        Some(AgentWorkspaceContinuationBlock::LocalWorkspaceMissing)
    );
}

#[test]
fn classify_unknown_manual_check_for_non_missing_path_error() {
    let parent = tempfile::tempdir().expect("temp dir should be created");
    let project = test_project(&parent);
    let mut workspace = test_workspace(&project, ChatConversationId::new());
    workspace.worktree_path = parent
        .path()
        .join("unexpected-worktree-path")
        .to_string_lossy()
        .to_string();

    let reason = classify_agent_workspace_continuation(&project, &workspace)
        .blocked_reason()
        .cloned();

    assert_eq!(
        reason.as_ref().map(AgentWorkspaceContinuationBlock::code),
        Some("unknown_requires_manual_check")
    );
    assert!(reason.unwrap().user_message().contains("checked manually"));
}

#[test]
fn continuation_block_codes_and_messages_cover_all_variants() {
    let cases = [
        (
            AgentWorkspaceContinuationBlock::ArchivedWorkspace,
            "archived_workspace",
            "archived",
        ),
        (
            AgentWorkspaceContinuationBlock::TerminalWorkspace,
            "terminal_workspace",
            "terminal PR state",
        ),
        (
            AgentWorkspaceContinuationBlock::CleanedAfterTerminal,
            "cleaned_after_terminal",
            "cleaned",
        ),
        (
            AgentWorkspaceContinuationBlock::LocalWorkspaceMissing,
            "local_workspace_missing",
            "missing locally",
        ),
        (
            AgentWorkspaceContinuationBlock::UnknownRequiresManualCheck(
                "repository unavailable".to_string(),
            ),
            "unknown_requires_manual_check",
            "repository unavailable",
        ),
    ];

    for (reason, code, message_part) in cases {
        assert_eq!(reason.code(), code);
        assert!(
            reason.user_message().contains(message_part),
            "message for {code} should include {message_part:?}"
        );
    }
}

#[tokio::test]
async fn classify_uses_linked_plan_worktree_when_direct_provenance_path_is_missing() {
    let parent = tempfile::tempdir().expect("temp dir should be created");
    let project = setup_git_project(&parent);
    let conversation_id = ChatConversationId::new();
    let mut workspace = test_workspace(&project, conversation_id);
    workspace.mode = AgentConversationWorkspaceMode::Ideation;
    workspace.branch_name = "ralphx/continuation/linked-plan".to_string();
    let session_id = IdeationSessionId::new();
    let plan_branch = PlanBranch::new(
        ArtifactId::new(),
        session_id.clone(),
        project.id.clone(),
        workspace.branch_name.clone(),
        "main".to_string(),
    );
    workspace.linked_ideation_session_id = Some(session_id);
    workspace.linked_plan_branch_id = Some(plan_branch.id.clone());
    let project_root = Path::new(&project.working_directory);
    git(project_root, &["branch", &plan_branch.branch_name]);
    let linked_path = resolve_linked_plan_branch_agent_worktree_path(&project, &plan_branch)
        .expect("linked path should resolve");
    GitService::checkout_existing_branch_worktree(
        project_root,
        &linked_path,
        &plan_branch.branch_name,
    )
    .await
    .expect("linked worktree should be created");
    let plan_branch_repo = MemoryPlanBranchRepository::new();
    plan_branch_repo
        .create(plan_branch)
        .await
        .expect("plan branch should be stored");

    let availability = classify_agent_workspace_continuation_with_plan_branch(
        &project,
        &workspace,
        Some(&plan_branch_repo),
    )
    .await;

    assert_eq!(
        availability,
        AgentWorkspaceContinuationAvailability::Available
    );
}
