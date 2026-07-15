use std::path::Path;
use std::process::Command;

use super::*;
use crate::application::agent_conversation_workspace::{
    prepare_agent_conversation_workspace, resolve_linked_plan_branch_agent_worktree_path,
    AgentConversationWorkspaceBaseSelection,
};
use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspaceMode, ChatConversation, IdeationAnalysisBaseRefKind,
    IdeationAnalysisState, IdeationAnalysisWorkspaceKind, IdeationSession, Priority, Project,
    ProposalCategory, TaskProposal,
};
use crate::utils::path_safety::validate_absolute_non_root_path;

fn setup_apply_state() -> AppState {
    AppState::new_sqlite_for_apply_test()
}

fn git_ok(repo: &Path, args: &[&str]) {
    let repo = validate_absolute_non_root_path(repo, "restart command test repository")
        .expect("test repository path should be safe");
    let output = Command::new("git")
        .args(args)
        // codeql[rust/path-injection]
        .current_dir(&repo)
        .output()
        .expect("git command should run");
    assert!(
        output.status.success(),
        "git {:?} failed: {}{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_stdout(repo: &Path, args: &[&str]) -> String {
    let repo = validate_absolute_non_root_path(repo, "restart command test repository")
        .expect("test repository path should be safe");
    let output = Command::new("git")
        .args(args)
        // codeql[rust/path-injection]
        .current_dir(&repo)
        .output()
        .expect("git command should run");
    assert!(
        output.status.success(),
        "git {:?} failed: {}{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn setup_git_repo() -> tempfile::TempDir {
    let dir = tempfile::TempDir::new().expect("temp repo should be created");
    let repo = validate_absolute_non_root_path(dir.path(), "restart command test repository")
        .expect("test repository path should be safe");
    git_ok(&repo, &["init", "-b", "main"]);
    git_ok(&repo, &["config", "user.email", "test@example.com"]);
    git_ok(&repo, &["config", "user.name", "Test User"]);
    git_ok(&repo, &["commit", "--allow-empty", "-m", "initial"]);
    dir
}

#[tokio::test]
async fn restart_core_prepares_linked_workspace_and_restores_cleanup_state() {
    let state = setup_apply_state();
    let repo_dir = setup_git_repo();
    let origin_dir = tempfile::TempDir::new().expect("origin should be created");
    git_ok(origin_dir.path(), &["init", "--bare", "-b", "main"]);
    git_ok(
        repo_dir.path(),
        &[
            "remote",
            "add",
            "origin",
            origin_dir.path().to_str().unwrap(),
        ],
    );
    git_ok(repo_dir.path(), &["push", "-u", "origin", "main"]);
    let worktree_parent = tempfile::TempDir::new().expect("worktree parent should be created");
    let mut project = Project::new(
        "Restart linked workspace".to_string(),
        repo_dir.path().to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktree_parent.path().to_string_lossy().to_string());
    let project = state
        .project_repo
        .create(project)
        .await
        .expect("project should be created");

    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project.id.clone()))
        .await
        .expect("conversation should be created");
    let mut workspace = prepare_agent_conversation_workspace(
        &project,
        &conversation.id,
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
    .expect("workspace should be prepared");

    let mut session = IdeationSession::new(project.id.clone());
    session.analysis = IdeationAnalysisState {
        base_ref_kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
        base_ref: Some("main".to_string()),
        base_display_name: Some("Project default (main)".to_string()),
        workspace_kind: IdeationAnalysisWorkspaceKind::IdeationWorktree,
        workspace_path: Some(workspace.worktree_path.clone()),
        base_commit: workspace.base_commit.clone(),
        base_locked_at: Some(chrono::Utc::now()),
    };
    let session = state
        .ideation_session_repo
        .create(session)
        .await
        .expect("session should be created");
    workspace.linked_ideation_session_id = Some(session.id.clone());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("linked workspace should be persisted");

    let proposal = state
        .task_proposal_repo
        .create(TaskProposal::new(
            session.id.clone(),
            "Restart proposal",
            ProposalCategory::Feature,
            Priority::Medium,
        ))
        .await
        .expect("proposal should be created");
    let apply_result = apply_proposals_core(
        &state,
        ApplyProposalsInput {
            session_id: session.id.as_str().to_string(),
            proposal_ids: vec![proposal.id.as_str().to_string()],
            target_column: "auto".to_string(),
            base_branch_override: None,
        },
    )
    .await
    .expect("apply should create the first implementation attempt");
    let old_execution_plan_id = apply_result
        .execution_plan_id
        .expect("accepted session should have an execution plan");
    let plan_branch = state
        .plan_branch_repo
        .get_by_session_id(&session.id)
        .await
        .expect("plan branch lookup should succeed")
        .expect("plan branch should exist");
    let linked_worktree_path =
        resolve_linked_plan_branch_agent_worktree_path(&project, &plan_branch)
            .expect("linked path should resolve");

    git_ok(
        repo_dir.path(),
        &["commit", "--allow-empty", "-m", "advance origin base"],
    );
    git_ok(repo_dir.path(), &["push", "origin", "main"]);
    let latest_origin_base = git_stdout(repo_dir.path(), &["rev-parse", "origin/main"]);

    let result = restart_ideation_implementation_core(&state, session.id.as_str().to_string())
        .await
        .expect("restart should prepare and reset the linked worktree");

    assert_eq!(result.old_execution_plan_id, old_execution_plan_id);
    assert_ne!(result.execution_plan_id, old_execution_plan_id);
    assert_eq!(result.created_task_ids.len(), 1);
    assert!(
        !Path::new(&workspace.worktree_path).is_dir(),
        "restart should relocate the stale conversation worktree"
    );
    assert!(linked_worktree_path.is_dir());
    assert_eq!(
        git_stdout(&linked_worktree_path, &["rev-parse", "HEAD"]),
        latest_origin_base
    );
    let stored_workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation.id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should remain");
    assert_eq!(stored_workspace.linked_plan_branch_id, Some(plan_branch.id));
    assert_eq!(
        state
            .agent_conversation_workspace_repo
            .get_local_cleanup_status(&conversation.id)
            .await
            .expect("cleanup status should load"),
        None
    );
}
