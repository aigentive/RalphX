use std::path::{Path, PathBuf};
use std::process::Command;

use crate::application::agent_conversation_workspace::{
    prepare_agent_conversation_workspace, relocate_linked_plan_branch_agent_worktree_for_restart,
    resolve_agent_conversation_workspace_path, resolve_linked_plan_branch_agent_worktree_path,
    AgentConversationWorkspaceBaseSelection,
};
use crate::application::GitService;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, ArtifactId, ChatConversationId,
    IdeationAnalysisBaseRefKind, IdeationSessionId, PlanBranch, Project,
};
use crate::utils::path_safety::validate_absolute_non_root_path;

fn git(repo: &Path, args: &[&str]) -> String {
    let repo = validate_absolute_non_root_path(repo, "test git repository")
        .expect("test git repository path should be absolute and non-root");
    let output = Command::new("git")
        .args(args)
        // codeql[rust/path-injection]
        .current_dir(&repo)
        .output()
        .expect("git command should spawn");
    assert!(
        output.status.success(),
        "git {:?} failed: {}{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn setup_repo(root: &Path) {
    let root = validate_absolute_non_root_path(root, "test git repository")
        .expect("test git repository path should be absolute and non-root");
    // codeql[rust/path-injection]
    std::fs::create_dir_all(&root).expect("repo root should be created");
    git(&root, &["init", "-b", "main"]);
    git(&root, &["config", "user.email", "test@example.com"]);
    git(&root, &["config", "user.name", "Test User"]);
    let readme_path = root.join("README.md");
    // codeql[rust/path-injection]
    std::fs::write(readme_path, "hello\n").expect("fixture file should be written");
    git(&root, &["add", "README.md"]);
    git(&root, &["commit", "-m", "initial"]);
}

fn project_with_worktrees(repo_path: &Path, worktree_parent: &Path) -> Project {
    let mut project = Project::new(
        "Restart relocation".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
    project
}

async fn prepare_ideation_workspace(
    project: &Project,
    conversation_id: &ChatConversationId,
) -> AgentConversationWorkspace {
    prepare_agent_conversation_workspace(
        project,
        conversation_id,
        AgentConversationWorkspaceMode::Ideation,
        AgentConversationWorkspaceBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
            branch_mode: None,
            base_ref: Some("main".to_string()),
            display_name: Some("Project default (main)".to_string()),
            source_pull_request: None,
        },
    )
    .await
    .expect("ideation workspace should be prepared")
}

fn linked_plan_branch_for_workspace(
    project: &Project,
    workspace: &mut AgentConversationWorkspace,
    session_id: IdeationSessionId,
) -> PlanBranch {
    let plan_branch = PlanBranch::new(
        ArtifactId::new(),
        session_id.clone(),
        project.id.clone(),
        workspace.branch_name.clone(),
        "main".to_string(),
    );
    workspace.linked_ideation_session_id = Some(session_id);
    workspace.linked_plan_branch_id = Some(plan_branch.id.clone());
    plan_branch
}

#[tokio::test]
async fn restart_relocation_moves_owned_conversation_worktree_to_linked_plan_path() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_repo(&repo_path);
    let project = project_with_worktrees(&repo_path, &worktree_parent);
    let conversation_id =
        ChatConversationId::from_string("conversation-restart-relocate-owned".to_string());
    let mut workspace = prepare_ideation_workspace(&project, &conversation_id).await;
    let old_worktree_path = PathBuf::from(&workspace.worktree_path);
    let plan_branch = linked_plan_branch_for_workspace(
        &project,
        &mut workspace,
        IdeationSessionId::from_string("session-restart-relocate-owned"),
    );
    let linked_worktree_path =
        resolve_linked_plan_branch_agent_worktree_path(&project, &plan_branch)
            .expect("linked path should resolve");

    let relocated =
        relocate_linked_plan_branch_agent_worktree_for_restart(&project, &workspace, &plan_branch)
            .await
            .expect("owned conversation worktree should relocate");

    assert_eq!(relocated, linked_worktree_path);
    assert!(
        !old_worktree_path.exists(),
        "the owned conversation worktree path should be vacated"
    );
    assert!(linked_worktree_path.is_dir());
    assert_eq!(
        GitService::get_current_branch(&linked_worktree_path)
            .await
            .expect("relocated worktree branch should resolve"),
        plan_branch.branch_name
    );
}

#[tokio::test]
async fn restart_relocation_rejects_owned_workspace_branch_drift_before_move() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_repo(&repo_path);
    let project = project_with_worktrees(&repo_path, &worktree_parent);
    let conversation_id =
        ChatConversationId::from_string("conversation-restart-relocate-drift".to_string());
    let mut workspace = prepare_ideation_workspace(&project, &conversation_id).await;
    let old_worktree_path = PathBuf::from(&workspace.worktree_path);
    let plan_branch = linked_plan_branch_for_workspace(
        &project,
        &mut workspace,
        IdeationSessionId::from_string("session-restart-relocate-drift"),
    );
    git(
        &old_worktree_path,
        &["checkout", "-b", "feature/restart-drift"],
    );

    let error =
        relocate_linked_plan_branch_agent_worktree_for_restart(&project, &workspace, &plan_branch)
            .await
            .expect_err("branch drift should block restart relocation");

    assert!(
        error
            .to_string()
            .contains("Owned agent conversation worktree"),
        "unexpected relocation error: {error}"
    );
    assert!(old_worktree_path.is_dir());
    let linked_worktree_path =
        resolve_linked_plan_branch_agent_worktree_path(&project, &plan_branch)
            .expect("linked path should resolve");
    assert!(!linked_worktree_path.exists());
}

#[tokio::test]
async fn restart_relocation_reuses_existing_linked_plan_worktree() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_repo(&repo_path);
    let project = project_with_worktrees(&repo_path, &worktree_parent);
    let conversation_id =
        ChatConversationId::from_string("conversation-restart-relocate-existing".to_string());
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id.clone(),
        project.id.clone(),
        AgentConversationWorkspaceMode::Ideation,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        None,
        "feature/existing-linked-plan".to_string(),
        resolve_agent_conversation_workspace_path(&project, &conversation_id)
            .expect("conversation path should resolve")
            .to_string_lossy()
            .to_string(),
    );
    git(&repo_path, &["branch", workspace.branch_name.as_str()]);
    let plan_branch = linked_plan_branch_for_workspace(
        &project,
        &mut workspace,
        IdeationSessionId::from_string("session-restart-relocate-existing"),
    );
    let linked_worktree_path =
        resolve_linked_plan_branch_agent_worktree_path(&project, &plan_branch)
            .expect("linked path should resolve");
    GitService::checkout_existing_branch_worktree(
        &repo_path,
        &linked_worktree_path,
        &plan_branch.branch_name,
    )
    .await
    .expect("linked plan worktree should be created");

    let relocated =
        relocate_linked_plan_branch_agent_worktree_for_restart(&project, &workspace, &plan_branch)
            .await
            .expect("existing linked worktree should be reused");

    assert_eq!(relocated, linked_worktree_path);
    assert_eq!(
        GitService::get_current_branch(&linked_worktree_path)
            .await
            .expect("linked worktree branch should resolve"),
        plan_branch.branch_name
    );
}
