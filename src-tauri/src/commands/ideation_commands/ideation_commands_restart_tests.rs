use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use super::ideation_commands_restart::RestartInFlightGuard;
use super::*;
use crate::application::agent_conversation_workspace::{
    prepare_agent_conversation_workspace, resolve_linked_plan_branch_agent_worktree_path,
    AgentConversationWorkspaceBaseSelection,
};
use crate::application::{AppState, GitService};
use crate::domain::entities::plan_branch::PrStatus;
use crate::domain::entities::{
    AgentConversationWorkspaceMode, ArtifactId, ChatConversation, IdeationAnalysisBaseRefKind,
    IdeationAnalysisState, IdeationAnalysisWorkspaceKind, IdeationSession, IdeationSessionId,
    Priority, Project, ProposalCategory, TaskProposal,
};
use crate::domain::services::github_service::{GithubServiceTrait, PrStatus as RemotePrStatus};
use crate::domain::state_machine::transition_handler::compute_merge_worktree_path;
use crate::tests::mock_github_service::MockGithubService;
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

#[test]
fn restart_in_flight_guard_serializes_by_ideation_session() {
    let session_id = IdeationSessionId::new();
    let first = RestartInFlightGuard::acquire(&session_id)
        .expect("first restart should acquire the session guard");
    let duplicate = RestartInFlightGuard::acquire(&session_id)
        .expect_err("duplicate restart should be rejected");
    assert!(duplicate.to_string().contains("already in progress"));
    drop(first);
    RestartInFlightGuard::acquire(&session_id)
        .expect("guard should release after the first restart exits");
}

#[tokio::test]
async fn restart_core_discards_dirty_current_attempt_merge_worktree() {
    let mut state = setup_apply_state();
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
    session.plan_artifact_id = Some(ArtifactId::from_string("approved-plan-artifact"));
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
    state
        .plan_branch_repo
        .update_pr_info(
            &plan_branch.id,
            739,
            "https://github.com/example/ralphx/pull/739".to_string(),
            PrStatus::Open,
            false,
        )
        .await
        .expect("stale local plan PR should be persisted");
    state
        .agent_conversation_workspace_repo
        .update_publication(
            &conversation.id,
            Some(739),
            Some("https://github.com/example/ralphx/pull/739"),
            Some("open"),
            Some("pushed"),
        )
        .await
        .expect("stale local workspace PR should be persisted");
    let github = Arc::new(MockGithubService::new());
    github.state().check_pr_status_result = Some(Ok(RemotePrStatus::Merged {
        merge_commit_sha: Some("merged-before-retry".to_string()),
        merged_at: Some("2026-07-15T10:00:00Z".to_string()),
    }));
    let github_service: Arc<dyn GithubServiceTrait> = github.clone();
    state.github_service = Some(github_service);
    let old_plan_branch_id = plan_branch.id.clone();
    let old_plan_artifact_id = state
        .ideation_session_repo
        .get_by_id(&session.id)
        .await
        .expect("session lookup should succeed")
        .expect("session should remain after apply")
        .plan_artifact_id
        .expect("accepted implementation should retain a plan artifact");
    let old_proposal = state
        .task_proposal_repo
        .get_by_id(&proposal.id)
        .await
        .expect("proposal lookup should succeed")
        .expect("proposal should remain persisted");

    let old_task_id = crate::domain::entities::TaskId::from_string(
        apply_result
            .created_task_ids
            .first()
            .expect("apply should create a proposal task")
            .clone(),
    );
    let mut old_task = state
        .task_repo
        .get_by_id(&old_task_id)
        .await
        .expect("old task lookup should succeed")
        .expect("old task should exist");
    old_task.task_branch = Some(format!("restart/task-{}", old_task.id.as_str()));
    git_ok(
        repo_dir.path(),
        &["branch", old_task.task_branch.as_deref().unwrap(), "main"],
    );

    GitService::delete_worktree(repo_dir.path(), Path::new(&workspace.worktree_path))
        .await
        .expect("stale conversation worktree should be removed");
    let merge_worktree_path = validate_absolute_non_root_path(
        Path::new(&compute_merge_worktree_path(&project, old_task.id.as_str())),
        "restart command merge worktree",
    )
    .expect("derived merge worktree path should be safe");
    GitService::checkout_existing_branch_worktree_strict(
        repo_dir.path(),
        &merge_worktree_path,
        &plan_branch.branch_name,
    )
    .await
    .expect("current attempt merge worktree should be created");
    old_task.worktree_path = Some(merge_worktree_path.to_string_lossy().into_owned());
    state
        .task_repo
        .update(&old_task)
        .await
        .expect("merge worktree ownership should be persisted on the current task");
    // codeql[rust/path-injection]
    std::fs::write(
        merge_worktree_path.join("discarded-untracked.txt"),
        "discard me\n",
    )
    .expect("dirty untracked file should be written");
    // codeql[rust/path-injection]
    std::fs::write(
        merge_worktree_path.join("discarded-tracked.txt"),
        "discard me\n",
    )
    .expect("dirty tracked file should be written");
    git_ok(&merge_worktree_path, &["add", "discarded-tracked.txt"]);

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
    assert!(
        !merge_worktree_path.exists(),
        "restart should delete the verified current-attempt merge worktree"
    );
    assert_eq!(
        git_stdout(&linked_worktree_path, &["rev-parse", "HEAD"]),
        latest_origin_base
    );
    assert_eq!(
        git_stdout(&linked_worktree_path, &["status", "--porcelain"]),
        ""
    );
    assert!(!linked_worktree_path
        .join("discarded-untracked.txt")
        .exists());
    assert!(!linked_worktree_path.join("discarded-tracked.txt").exists());
    let refreshed_session = state
        .ideation_session_repo
        .get_by_id(&session.id)
        .await
        .expect("session lookup should succeed")
        .expect("session should remain");
    assert_eq!(
        refreshed_session.plan_artifact_id.as_ref(),
        Some(&old_plan_artifact_id)
    );
    let refreshed_proposal = state
        .task_proposal_repo
        .get_by_id(&proposal.id)
        .await
        .expect("proposal lookup should succeed")
        .expect("proposal should remain");
    assert_eq!(refreshed_proposal.id, old_proposal.id);
    assert_eq!(refreshed_proposal.title, old_proposal.title);
    assert_ne!(
        refreshed_proposal.created_task_id, old_proposal.created_task_id,
        "the preserved proposal should link to the replacement task"
    );
    let refreshed_plan_branch = state
        .plan_branch_repo
        .get_by_session_id(&session.id)
        .await
        .expect("plan branch lookup should succeed")
        .expect("plan branch should remain");
    assert_eq!(refreshed_plan_branch.id, old_plan_branch_id);
    assert_eq!(refreshed_plan_branch.branch_name, plan_branch.branch_name);
    assert_eq!(refreshed_plan_branch.pr_number, None);
    assert_eq!(github.state().check_pr_status_calls, 1);
    assert_eq!(github.state().close_pr_calls, 0);
    let archived_old_task = state
        .task_repo
        .get_by_id(&old_task_id)
        .await
        .expect("old task lookup should succeed")
        .expect("old task should remain for history");
    assert!(archived_old_task.archived_at.is_some());
    let stored_workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation.id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should remain");
    assert_eq!(stored_workspace.linked_plan_branch_id, Some(plan_branch.id));
    assert_eq!(stored_workspace.publication_pr_number, None);
    assert_eq!(
        state
            .agent_conversation_workspace_repo
            .get_local_cleanup_status(&conversation.id)
            .await
            .expect("cleanup status should load"),
        None
    );
}
