use std::path::{Path, PathBuf};
use std::process::Command;

use crate::application::agent_conversation_workspace::{
    prepare_agent_conversation_workspace, resolve_agent_conversation_workspace_path,
    resolve_linked_plan_branch_agent_worktree_path, AgentConversationWorkspaceBaseSelection,
};
use crate::application::agent_conversation_workspace_restart::{
    inspect_linked_plan_branch_owner_for_restart,
    prepare_linked_plan_branch_agent_worktree_for_restart, resolve_restart_workspace_cleanup_proof,
    RestartWorkspaceCleanupProof, RestartWorkspaceOwner, RestartWorkspacePreparationError,
    RestartWorkspacePreparationSource,
};
use crate::application::GitService;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, ArtifactId, ChatConversationId,
    ExecutionPlanId, IdeationAnalysisBaseRefKind, IdeationSessionId, PlanBranch, Project, Task,
};
use crate::domain::state_machine::transition_handler::compute_merge_worktree_path;
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

fn checked_test_worktree_child_path(path: &Path, parent: &Path, context: &str) -> PathBuf {
    let path = validate_absolute_non_root_path(path, context)
        .expect("test worktree child path should be absolute and non-root");
    let parent = validate_absolute_non_root_path(parent, "test worktree parent")
        .expect("test worktree parent should be absolute and non-root");
    assert!(
        path.starts_with(&parent),
        "{context} path {} must stay under {}",
        path.display(),
        parent.display()
    );
    path
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

    let prepared = prepare_linked_plan_branch_agent_worktree_for_restart(
        &project,
        &workspace,
        &plan_branch,
        "main",
        RestartWorkspaceCleanupProof::None,
    )
    .await
    .expect("owned conversation worktree should relocate");

    assert_eq!(prepared.path, linked_worktree_path);
    assert_eq!(
        prepared.source,
        RestartWorkspacePreparationSource::RelocatedConversation
    );
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

    let error = prepare_linked_plan_branch_agent_worktree_for_restart(
        &project,
        &workspace,
        &plan_branch,
        "main",
        RestartWorkspaceCleanupProof::None,
    )
    .await
    .expect_err("branch drift should block restart relocation");

    assert!(
        matches!(
            error,
            RestartWorkspacePreparationError::UnsafeOwnership { .. }
        ),
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

    let prepared = prepare_linked_plan_branch_agent_worktree_for_restart(
        &project,
        &workspace,
        &plan_branch,
        "main",
        RestartWorkspaceCleanupProof::None,
    )
    .await
    .expect("existing linked worktree should be reused");

    assert_eq!(prepared.path, linked_worktree_path);
    assert_eq!(
        prepared.source,
        RestartWorkspacePreparationSource::ExistingLinked
    );
    assert_eq!(
        GitService::get_current_branch(&linked_worktree_path)
            .await
            .expect("linked worktree branch should resolve"),
        plan_branch.branch_name
    );
}

#[tokio::test]
async fn restart_preparation_reattaches_preserved_owned_branch_when_both_paths_are_absent() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_repo(&repo_path);
    let project = project_with_worktrees(&repo_path, &worktree_parent);
    let conversation_id =
        ChatConversationId::from_string("conversation-restart-reattach-preserved".to_string());
    let mut workspace = prepare_ideation_workspace(&project, &conversation_id).await;
    let direct_path = PathBuf::from(&workspace.worktree_path);
    let plan_branch = linked_plan_branch_for_workspace(
        &project,
        &mut workspace,
        IdeationSessionId::from_string("session-restart-reattach-preserved"),
    );
    GitService::delete_worktree(&repo_path, &direct_path)
        .await
        .expect("owned direct worktree should be removed");

    let prepared = prepare_linked_plan_branch_agent_worktree_for_restart(
        &project,
        &workspace,
        &plan_branch,
        "main",
        RestartWorkspaceCleanupProof::None,
    )
    .await
    .expect("preserved branch should be reattached");

    assert_eq!(
        prepared.source,
        RestartWorkspacePreparationSource::ReattachedBranch
    );
    assert!(!direct_path.exists());
    assert!(prepared.path.is_dir());
    assert_eq!(
        GitService::get_current_branch(&prepared.path)
            .await
            .expect("reattached branch should resolve"),
        plan_branch.branch_name
    );
}

#[tokio::test]
async fn restart_preparation_recreates_branch_only_with_owned_merged_cleanup_proof() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_repo(&repo_path);
    let project = project_with_worktrees(&repo_path, &worktree_parent);
    let conversation_id =
        ChatConversationId::from_string("conversation-restart-recreate-cleaned".to_string());
    let mut workspace = prepare_ideation_workspace(&project, &conversation_id).await;
    let direct_path = PathBuf::from(&workspace.worktree_path);
    let plan_branch = linked_plan_branch_for_workspace(
        &project,
        &mut workspace,
        IdeationSessionId::from_string("session-restart-recreate-cleaned"),
    );
    GitService::delete_worktree(&repo_path, &direct_path)
        .await
        .expect("owned direct worktree should be removed");
    git(&repo_path, &["branch", "-D", &plan_branch.branch_name]);

    let prepared = prepare_linked_plan_branch_agent_worktree_for_restart(
        &project,
        &workspace,
        &plan_branch,
        "main",
        RestartWorkspaceCleanupProof::OwnedMergedCleanup,
    )
    .await
    .expect("owned merged cleanup should permit branch recreation");

    assert_eq!(
        prepared.source,
        RestartWorkspacePreparationSource::RecreatedFromCleanup
    );
    assert_eq!(
        GitService::get_current_branch(&prepared.path)
            .await
            .expect("recreated branch should resolve"),
        plan_branch.branch_name
    );
}

#[tokio::test]
async fn restart_preparation_refuses_unproven_missing_branch_without_creating_artifacts() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_repo(&repo_path);
    let project = project_with_worktrees(&repo_path, &worktree_parent);
    let conversation_id =
        ChatConversationId::from_string("conversation-restart-unproven-missing".to_string());
    let mut workspace = prepare_ideation_workspace(&project, &conversation_id).await;
    let direct_path = PathBuf::from(&workspace.worktree_path);
    let plan_branch = linked_plan_branch_for_workspace(
        &project,
        &mut workspace,
        IdeationSessionId::from_string("session-restart-unproven-missing"),
    );
    GitService::delete_worktree(&repo_path, &direct_path)
        .await
        .expect("owned direct worktree should be removed");
    git(&repo_path, &["branch", "-D", &plan_branch.branch_name]);
    let linked_path = resolve_linked_plan_branch_agent_worktree_path(&project, &plan_branch)
        .expect("linked path should resolve");

    let error = prepare_linked_plan_branch_agent_worktree_for_restart(
        &project,
        &workspace,
        &plan_branch,
        "main",
        RestartWorkspaceCleanupProof::None,
    )
    .await
    .expect_err("unproven branch loss must fail closed");

    assert_eq!(
        error,
        RestartWorkspacePreparationError::MissingBranchWithoutCleanupProof
    );
    assert!(!direct_path.exists());
    assert!(!linked_path.exists());
    assert!(
        !GitService::branch_exists(&repo_path, &plan_branch.branch_name)
            .await
            .expect("branch probe should succeed")
    );
}

#[tokio::test]
async fn restart_preparation_refuses_branch_checked_out_in_project_root() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_repo(&repo_path);
    let project = project_with_worktrees(&repo_path, &worktree_parent);
    let conversation_id =
        ChatConversationId::from_string("conversation-restart-project-root-owner".to_string());
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id.clone(),
        project.id.clone(),
        AgentConversationWorkspaceMode::Ideation,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        None,
        "main".to_string(),
        resolve_agent_conversation_workspace_path(&project, &conversation_id)
            .expect("conversation path should resolve")
            .to_string_lossy()
            .to_string(),
    );
    let plan_branch = linked_plan_branch_for_workspace(
        &project,
        &mut workspace,
        IdeationSessionId::from_string("session-restart-project-root-owner"),
    );

    let error = prepare_linked_plan_branch_agent_worktree_for_restart(
        &project,
        &workspace,
        &plan_branch,
        "main",
        RestartWorkspaceCleanupProof::None,
    )
    .await
    .expect_err("project-root branch ownership must be refused");

    assert!(matches!(
        error,
        RestartWorkspacePreparationError::UnsafeOwnership { .. }
    ));
    assert_eq!(git(&repo_path, &["branch", "--show-current"]), "main");
}

#[tokio::test]
async fn restart_preparation_refuses_branch_checked_out_in_another_worktree() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_repo(&repo_path);
    let project = project_with_worktrees(&repo_path, &worktree_parent);
    let conversation_id =
        ChatConversationId::from_string("conversation-restart-other-owner".to_string());
    let mut workspace = prepare_ideation_workspace(&project, &conversation_id).await;
    let direct_path = PathBuf::from(&workspace.worktree_path);
    let plan_branch = linked_plan_branch_for_workspace(
        &project,
        &mut workspace,
        IdeationSessionId::from_string("session-restart-other-owner"),
    );
    GitService::delete_worktree(&repo_path, &direct_path)
        .await
        .expect("owned direct worktree should be removed");
    let other_path = worktree_parent.join("unknown-owner");
    GitService::checkout_existing_branch_worktree_strict(
        &repo_path,
        &other_path,
        &plan_branch.branch_name,
    )
    .await
    .expect("test should check the preserved branch out elsewhere");
    let linked_path = resolve_linked_plan_branch_agent_worktree_path(&project, &plan_branch)
        .expect("linked path should resolve");

    let error = prepare_linked_plan_branch_agent_worktree_for_restart(
        &project,
        &workspace,
        &plan_branch,
        "main",
        RestartWorkspaceCleanupProof::None,
    )
    .await
    .expect_err("another worktree owner must be refused");

    assert!(matches!(
        error,
        RestartWorkspacePreparationError::UnsafeOwnership { .. }
    ));
    assert!(other_path.is_dir());
    assert!(!linked_path.exists());
    assert_eq!(
        GitService::get_current_branch(&other_path)
            .await
            .expect("other worktree branch should remain intact"),
        plan_branch.branch_name
    );
}

#[tokio::test]
async fn restart_owner_inspection_accepts_only_the_exact_current_attempt_merge_worktree() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_repo(&repo_path);
    let project = project_with_worktrees(&repo_path, &worktree_parent);
    let conversation_id =
        ChatConversationId::from_string("conversation-restart-current-merge".to_string());
    let mut workspace = prepare_ideation_workspace(&project, &conversation_id).await;
    let direct_path = PathBuf::from(&workspace.worktree_path);
    let session_id = IdeationSessionId::from_string("session-restart-current-merge");
    let execution_plan_id = ExecutionPlanId::from_string("execution-restart-current-merge");
    let mut plan_branch =
        linked_plan_branch_for_workspace(&project, &mut workspace, session_id.clone());
    plan_branch.execution_plan_id = Some(execution_plan_id.clone());
    GitService::delete_worktree(&repo_path, &direct_path)
        .await
        .expect("owned direct worktree should be removed");

    let mut task = Task::new(project.id.clone(), "Current merge owner".to_string());
    task.execution_plan_id = Some(execution_plan_id.clone());
    let merge_path = checked_test_worktree_child_path(
        Path::new(&compute_merge_worktree_path(&project, task.id.as_str())),
        &worktree_parent,
        "restart current-attempt merge worktree",
    );
    task.worktree_path = Some(merge_path.to_string_lossy().into_owned());
    GitService::checkout_existing_branch_worktree_strict(
        &repo_path,
        &merge_path,
        &plan_branch.branch_name,
    )
    .await
    .expect("current-attempt merge worktree should be created");

    let owner = inspect_linked_plan_branch_owner_for_restart(
        &project,
        &workspace,
        &plan_branch,
        &session_id,
        &execution_plan_id,
        &[task.clone()],
        RestartWorkspaceCleanupProof::None,
    )
    .await
    .expect("exact current-attempt merge owner should be accepted");
    assert_eq!(
        owner,
        RestartWorkspaceOwner::CurrentAttemptMerge {
            task_id: task.id.clone(),
            path: merge_path.clone(),
        }
    );

    task.execution_plan_id = Some(ExecutionPlanId::from_string("stale-execution-plan"));
    let error = inspect_linked_plan_branch_owner_for_restart(
        &project,
        &workspace,
        &plan_branch,
        &session_id,
        &execution_plan_id,
        &[task],
        RestartWorkspaceCleanupProof::None,
    )
    .await
    .expect_err("stale-attempt ownership must fail closed");
    assert!(matches!(
        error,
        RestartWorkspacePreparationError::UnsafeOwnership { .. }
    ));

    // codeql[rust/path-injection]
    assert!(merge_path.is_dir(), "inspection must not mutate the owner");
}

#[tokio::test]
async fn restart_owner_inspection_classifies_owned_conversation_and_linked_worktrees() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_repo(&repo_path);
    let project = project_with_worktrees(&repo_path, &worktree_parent);
    let conversation_id =
        ChatConversationId::from_string("conversation-restart-owner-classification".to_string());
    let mut workspace = prepare_ideation_workspace(&project, &conversation_id).await;
    let session_id = IdeationSessionId::from_string("session-restart-owner-classification");
    let execution_plan_id = ExecutionPlanId::from_string("execution-owner-classification");
    let mut plan_branch =
        linked_plan_branch_for_workspace(&project, &mut workspace, session_id.clone());
    plan_branch.execution_plan_id = Some(execution_plan_id.clone());

    let owner = inspect_linked_plan_branch_owner_for_restart(
        &project,
        &workspace,
        &plan_branch,
        &session_id,
        &execution_plan_id,
        &[],
        RestartWorkspaceCleanupProof::None,
    )
    .await
    .expect("conversation-owned worktree should be classified");
    assert_eq!(owner, RestartWorkspaceOwner::OwnedConversation);

    let conversation_path = PathBuf::from(&workspace.worktree_path);
    let linked_path = resolve_linked_plan_branch_agent_worktree_path(&project, &plan_branch)
        .expect("linked path should resolve");
    GitService::delete_worktree(&repo_path, &conversation_path)
        .await
        .expect("conversation worktree should be removed");
    GitService::checkout_existing_branch_worktree_strict(
        &repo_path,
        &linked_path,
        &plan_branch.branch_name,
    )
    .await
    .expect("linked plan worktree should be created");

    let owner = inspect_linked_plan_branch_owner_for_restart(
        &project,
        &workspace,
        &plan_branch,
        &session_id,
        &execution_plan_id,
        &[],
        RestartWorkspaceCleanupProof::None,
    )
    .await
    .expect("linked worktree should be classified");
    assert_eq!(owner, RestartWorkspaceOwner::ExistingLinked);
}

#[tokio::test]
async fn restart_owner_inspection_classifies_unowned_and_recreatable_branches() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_repo(&repo_path);
    let project = project_with_worktrees(&repo_path, &worktree_parent);
    let conversation_id =
        ChatConversationId::from_string("conversation-restart-unowned-branch".to_string());
    let mut workspace = prepare_ideation_workspace(&project, &conversation_id).await;
    let session_id = IdeationSessionId::from_string("session-restart-unowned-branch");
    let execution_plan_id = ExecutionPlanId::from_string("execution-unowned-branch");
    let mut plan_branch =
        linked_plan_branch_for_workspace(&project, &mut workspace, session_id.clone());
    plan_branch.execution_plan_id = Some(execution_plan_id.clone());
    let conversation_path = PathBuf::from(&workspace.worktree_path);
    GitService::delete_worktree(&repo_path, &conversation_path)
        .await
        .expect("conversation worktree should be removed");

    let owner = inspect_linked_plan_branch_owner_for_restart(
        &project,
        &workspace,
        &plan_branch,
        &session_id,
        &execution_plan_id,
        &[],
        RestartWorkspaceCleanupProof::None,
    )
    .await
    .expect("unowned preserved branch should be classified");
    assert_eq!(owner, RestartWorkspaceOwner::UnownedPreservedBranch);

    git(&repo_path, &["branch", "-D", &plan_branch.branch_name]);
    let owner = inspect_linked_plan_branch_owner_for_restart(
        &project,
        &workspace,
        &plan_branch,
        &session_id,
        &execution_plan_id,
        &[],
        RestartWorkspaceCleanupProof::OwnedMergedCleanup,
    )
    .await
    .expect("owned cleanup proof should permit recreation");
    assert_eq!(owner, RestartWorkspaceOwner::RecreatableFromCleanup);
}

#[tokio::test]
async fn restart_owner_inspection_refuses_unregistered_derived_linked_path() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_repo(&repo_path);
    let project = project_with_worktrees(&repo_path, &worktree_parent);
    let conversation_id =
        ChatConversationId::from_string("conversation-restart-unregistered-linked".to_string());
    let mut workspace = prepare_ideation_workspace(&project, &conversation_id).await;
    let direct_path = PathBuf::from(&workspace.worktree_path);
    let session_id = IdeationSessionId::from_string("session-restart-unregistered-linked");
    let execution_plan_id = ExecutionPlanId::from_string("execution-unregistered-linked");
    let mut plan_branch =
        linked_plan_branch_for_workspace(&project, &mut workspace, session_id.clone());
    plan_branch.execution_plan_id = Some(execution_plan_id.clone());
    GitService::delete_worktree(&repo_path, &direct_path)
        .await
        .expect("owned direct worktree should be removed");
    let linked_path = resolve_linked_plan_branch_agent_worktree_path(&project, &plan_branch)
        .expect("linked path should resolve");
    // codeql[rust/path-injection]
    std::fs::create_dir_all(&linked_path).expect("unregistered linked directory should exist");

    let error = inspect_linked_plan_branch_owner_for_restart(
        &project,
        &workspace,
        &plan_branch,
        &session_id,
        &execution_plan_id,
        &[],
        RestartWorkspaceCleanupProof::None,
    )
    .await
    .expect_err("an unregistered physical linked path must fail closed");

    assert!(matches!(
        error,
        RestartWorkspacePreparationError::UnsafeOwnership { .. }
    ));
    assert!(
        linked_path.is_dir(),
        "inspection must not mutate unknown data"
    );
}

#[test]
fn restart_cleanup_proof_requires_merged_terminal_state_with_cleaned_marker() {
    use crate::domain::entities::plan_branch::PrStatus;

    let project = Project::new("Cleanup proof".to_string(), "/owned/project".to_string());
    let conversation_id = ChatConversationId::new();
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id,
        project.id.clone(),
        AgentConversationWorkspaceMode::Ideation,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        None,
        "ralphx/test/plan".to_string(),
        "/owned/worktree".to_string(),
    );
    let mut plan_branch = PlanBranch::new(
        ArtifactId::new(),
        IdeationSessionId::new(),
        project.id,
        workspace.branch_name.clone(),
        "main".to_string(),
    );

    workspace.publication_pr_status = Some("closed".to_string());
    assert_eq!(
        resolve_restart_workspace_cleanup_proof(&workspace, Some("cleaned"), &plan_branch, None,),
        RestartWorkspaceCleanupProof::None,
        "closed cleanup may preserve the branch"
    );

    workspace.publication_pr_status = Some("merged".to_string());
    assert_eq!(
        resolve_restart_workspace_cleanup_proof(&workspace, Some("cleaned"), &plan_branch, None,),
        RestartWorkspaceCleanupProof::OwnedMergedCleanup
    );

    workspace.publication_pr_status = None;
    plan_branch.status = crate::domain::entities::PlanBranchStatus::Merged;
    assert_eq!(
        resolve_restart_workspace_cleanup_proof(&workspace, None, &plan_branch, Some("cleaned"),),
        RestartWorkspaceCleanupProof::OwnedMergedCleanup
    );

    plan_branch.status = crate::domain::entities::PlanBranchStatus::Active;
    plan_branch.pr_status = Some(PrStatus::Merged);
    assert_eq!(
        resolve_restart_workspace_cleanup_proof(&workspace, None, &plan_branch, Some("cleaned"),),
        RestartWorkspaceCleanupProof::OwnedMergedCleanup
    );
}

#[test]
fn restart_preparation_errors_map_to_user_safe_validation_messages() {
    let unsafe_error = RestartWorkspacePreparationError::UnsafeOwnership {
        detail: "/tmp/private/worktree".to_string(),
    };
    let operation_error = RestartWorkspacePreparationError::Operation {
        detail: "git failed".to_string(),
    };
    let missing_error = RestartWorkspacePreparationError::MissingBranchWithoutCleanupProof;

    assert_eq!(
        unsafe_error.to_string(),
        "unsafe ownership: /tmp/private/worktree"
    );
    assert!(unsafe_error
        .into_app_error()
        .to_string()
        .contains("could not safely restore"));
    assert_eq!(operation_error.to_string(), "operation failed: git failed");
    assert!(operation_error
        .into_app_error()
        .to_string()
        .contains("Check Git access"));
    assert_eq!(
        missing_error.to_string(),
        "missing branch has no owned cleanup proof"
    );
    assert!(missing_error
        .into_app_error()
        .to_string()
        .contains("ownership of the missing branch could not be verified"));
}
