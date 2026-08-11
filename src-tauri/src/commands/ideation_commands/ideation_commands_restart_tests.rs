use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use super::ideation_commands_restart::{
    archive_execution_plan_tasks, cleanup_branch_update_worktrees_for_restart,
    preflight_branch_updates_for_restart, stop_branch_updates_for_restart, RestartInFlightGuard,
};
use super::*;
use crate::application::agent_conversation_workspace::{
    prepare_agent_conversation_workspace, resolve_linked_plan_branch_agent_worktree_path,
    AgentConversationWorkspaceBaseSelection,
};
use crate::application::{AppState, GitService};
use crate::commands::ExecutionState;
use crate::domain::entities::plan_branch::PrStatus;
use crate::domain::entities::{
    AgentConversationWorkspaceMode, ArtifactId, BranchUpdateCapacityOwnership,
    BranchUpdateContinuation, BranchUpdateDirection, BranchUpdateOperation,
    BranchUpdateWorkspaceOwnership, ChatContextType, ChatConversation, GitTargetLeaseOwner,
    IdeationAnalysisBaseRefKind, IdeationAnalysisState, IdeationAnalysisWorkspaceKind,
    IdeationSession, IdeationSessionId, InternalStatus, Priority, Project, ProposalCategory,
    TaskProposal,
};
use crate::domain::repositories::{BranchUpdateActivation, BranchUpdateActivationOutcome};
use crate::domain::services::github_service::{GithubServiceTrait, PrStatus as RemotePrStatus};
use crate::domain::services::{QueueKey, QueuedMessage, RunningAgentKey};
use crate::domain::state_machine::transition_handler::{
    compute_merge_worktree_path, compute_plan_update_worktree_path,
    compute_source_update_worktree_path,
};
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

#[test]
fn restart_task_archive_rejects_changed_attempt_membership() {
    let connection = rusqlite::Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch(
            "CREATE TABLE tasks (
                id TEXT PRIMARY KEY,
                execution_plan_id TEXT NOT NULL,
                archived_at TEXT,
                updated_at TEXT NOT NULL
            );
            INSERT INTO tasks (id, execution_plan_id, archived_at, updated_at)
            VALUES ('task-known', 'attempt-current', NULL, 'before'),
                   ('task-raced', 'attempt-current', NULL, 'before');",
        )
        .expect("fixture should be created");
    let execution_plan_id =
        crate::domain::entities::ExecutionPlanId::from_string("attempt-current");
    let expected = [crate::domain::entities::TaskId::from_string(
        "task-known".to_string(),
    )];

    let error = archive_execution_plan_tasks(
        &connection,
        &execution_plan_id,
        &expected,
        "2026-07-15T18:00:00Z",
    )
    .expect_err("a concurrently inserted task must reject replacement");

    assert!(error.to_string().contains("tasks changed"));
    let archived: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM tasks WHERE archived_at IS NOT NULL",
            [],
            |row| row.get(0),
        )
        .expect("archive count should load");
    assert_eq!(archived, 0, "membership failure must not archive any task");
}

#[tokio::test]
async fn restart_branch_update_preflight_rejects_missing_durable_authority() {
    let state = setup_apply_state();
    let repo_dir = setup_git_repo();
    let project = state
        .project_repo
        .create(Project::new(
            "Missing branch update authority".to_string(),
            repo_dir.path().to_string_lossy().into_owned(),
        ))
        .await
        .expect("project should be created");
    let mut task = crate::domain::entities::Task::new(
        project.id.clone(),
        "Updating without operation".to_string(),
    );
    task.internal_status = crate::domain::entities::InternalStatus::UpdatingTaskBranch;
    let task = state
        .task_repo
        .create(task)
        .await
        .expect("task should be created");

    let error = preflight_branch_updates_for_restart(&state, &project, &[task])
        .await
        .expect_err("updating status without an active operation must fail closed");

    assert!(error
        .to_string()
        .contains("without active durable authority"));
}

#[tokio::test]
async fn restart_branch_update_preflight_rejects_stale_operation_state() {
    let state = setup_apply_state();
    let repo_dir = setup_git_repo();
    let worktree_parent = tempfile::TempDir::new().expect("worktree parent should be created");
    let mut project = Project::new(
        "Stale branch update operation".to_string(),
        repo_dir.path().to_string_lossy().into_owned(),
    );
    project.worktree_parent_directory = Some(worktree_parent.path().to_string_lossy().into_owned());
    let project = state
        .project_repo
        .create(project)
        .await
        .expect("project should be created");
    let mut task = crate::domain::entities::Task::new(
        project.id.clone(),
        "Task with stale branch update".to_string(),
    );
    task.task_branch = Some(format!("task/stale-update-{}", task.id.as_str()));
    let task_branch = task.task_branch.clone().expect("task branch should be set");
    git_ok(repo_dir.path(), &["branch", &task_branch, "main"]);
    let task = state
        .task_repo
        .create(task)
        .await
        .expect("task should be created");
    let target_identity = GitService::canonical_target_identity(repo_dir.path(), &task_branch)
        .await
        .expect("target identity should resolve");
    let mut operation = BranchUpdateOperation::new(
        task.id.clone(),
        BranchUpdateDirection::TaskBranch,
        BranchUpdateContinuation::ResumeExecution,
        "restart-stale-history",
        "main",
        task_branch,
        BranchUpdateWorkspaceOwnership::OperationWorktree,
        BranchUpdateCapacityOwnership::Inherited,
        target_identity,
        chrono::Utc::now(),
    );
    operation.workspace_path = Some(
        validate_absolute_non_root_path(
            Path::new(&compute_source_update_worktree_path(
                &project,
                task.id.as_str(),
            )),
            "restart stale branch-update workspace",
        )
        .expect("derived workspace path should be safe"),
    );
    let activation = state
        .branch_update_repo
        .activate(BranchUpdateActivation {
            operation,
            expected_status: InternalStatus::Backlog,
            update_status: InternalStatus::UpdatingPlanBranch,
            trigger: "restart_stale_branch_update_test".to_string(),
        })
        .await
        .expect("branch update should activate");
    assert!(matches!(
        activation,
        BranchUpdateActivationOutcome::Applied { .. }
    ));

    let error = preflight_branch_updates_for_restart(&state, &project, &[task])
        .await
        .expect_err("status drift must fail closed");

    assert!(error
        .to_string()
        .contains("does not match its active operation"));
}

#[tokio::test]
async fn restart_branch_update_preflight_rejects_released_target_lease() {
    let state = setup_apply_state();
    let repo_dir = setup_git_repo();
    let worktree_parent = tempfile::TempDir::new().expect("worktree parent should be created");
    let mut project = Project::new(
        "Missing target lease".to_string(),
        repo_dir.path().to_string_lossy().into_owned(),
    );
    project.worktree_parent_directory = Some(worktree_parent.path().to_string_lossy().into_owned());
    let project = state
        .project_repo
        .create(project)
        .await
        .expect("project should be created");
    let mut task = crate::domain::entities::Task::new(
        project.id.clone(),
        "Task with released lease".to_string(),
    );
    task.task_branch = Some(format!("task/released-lease-{}", task.id.as_str()));
    let task_branch = task.task_branch.clone().expect("task branch should be set");
    git_ok(repo_dir.path(), &["branch", &task_branch, "main"]);
    let task = state
        .task_repo
        .create(task)
        .await
        .expect("task should be created");
    let target_identity = GitService::canonical_target_identity(repo_dir.path(), &task_branch)
        .await
        .expect("target identity should resolve");
    let mut operation = BranchUpdateOperation::new(
        task.id.clone(),
        BranchUpdateDirection::TaskBranch,
        BranchUpdateContinuation::ResumeExecution,
        "restart-missing-lease-history",
        "main",
        task_branch,
        BranchUpdateWorkspaceOwnership::OperationWorktree,
        BranchUpdateCapacityOwnership::Inherited,
        target_identity.clone(),
        chrono::Utc::now(),
    );
    operation.workspace_path = Some(
        validate_absolute_non_root_path(
            Path::new(&compute_source_update_worktree_path(
                &project,
                task.id.as_str(),
            )),
            "restart missing-lease branch-update workspace",
        )
        .expect("derived workspace path should be safe"),
    );
    let activation = state
        .branch_update_repo
        .activate(BranchUpdateActivation {
            operation,
            expected_status: InternalStatus::Backlog,
            update_status: InternalStatus::UpdatingTaskBranch,
            trigger: "restart_missing_lease_branch_update_test".to_string(),
        })
        .await
        .expect("branch update should activate");
    let (operation_id, fencing_epoch) = match activation {
        BranchUpdateActivationOutcome::Applied {
            operation_id,
            fencing_epoch,
            ..
        } => (operation_id, fencing_epoch),
        other => panic!("unexpected activation outcome: {other:?}"),
    };
    let owner = GitTargetLeaseOwner::branch_update(task.id.as_str(), operation_id.as_str());
    state
        .branch_update_repo
        .release_target_lease(&target_identity, &owner, fencing_epoch)
        .await
        .expect("lease release should complete");

    let error = preflight_branch_updates_for_restart(&state, &project, &[task])
        .await
        .expect_err("released lease must fail closed");

    assert!(error.to_string().contains("authority is busy or stale"));
}

#[tokio::test]
async fn restart_branch_update_preflight_rejects_unregistered_existing_workspace() {
    let state = setup_apply_state();
    let repo_dir = setup_git_repo();
    let worktree_parent = tempfile::TempDir::new().expect("worktree parent should be created");
    let mut project = Project::new(
        "Unregistered branch update workspace".to_string(),
        repo_dir.path().to_string_lossy().into_owned(),
    );
    project.worktree_parent_directory = Some(worktree_parent.path().to_string_lossy().into_owned());
    let project = state
        .project_repo
        .create(project)
        .await
        .expect("project should be created");
    let mut task = crate::domain::entities::Task::new(
        project.id.clone(),
        "Task with unregistered workspace".to_string(),
    );
    task.task_branch = Some(format!("task/unregistered-update-{}", task.id.as_str()));
    let task_branch = task.task_branch.clone().expect("task branch should be set");
    git_ok(repo_dir.path(), &["branch", &task_branch, "main"]);
    let task = state
        .task_repo
        .create(task)
        .await
        .expect("task should be created");
    let workspace_path = validate_absolute_non_root_path(
        Path::new(&compute_source_update_worktree_path(
            &project,
            task.id.as_str(),
        )),
        "restart unregistered branch-update workspace",
    )
    .expect("derived workspace path should be safe");
    // codeql[rust/path-injection]
    std::fs::create_dir_all(&workspace_path).expect("unregistered workspace should exist");
    let target_identity = GitService::canonical_target_identity(repo_dir.path(), &task_branch)
        .await
        .expect("target identity should resolve");
    let mut operation = BranchUpdateOperation::new(
        task.id.clone(),
        BranchUpdateDirection::TaskBranch,
        BranchUpdateContinuation::ResumeExecution,
        "restart-unregistered-history",
        "main",
        task_branch,
        BranchUpdateWorkspaceOwnership::OperationWorktree,
        BranchUpdateCapacityOwnership::Inherited,
        target_identity,
        chrono::Utc::now(),
    );
    operation.workspace_path = Some(workspace_path.clone());
    let activation = state
        .branch_update_repo
        .activate(BranchUpdateActivation {
            operation,
            expected_status: InternalStatus::Backlog,
            update_status: InternalStatus::UpdatingTaskBranch,
            trigger: "restart_unregistered_branch_update_test".to_string(),
        })
        .await
        .expect("branch update should activate");
    assert!(matches!(
        activation,
        BranchUpdateActivationOutcome::Applied { .. }
    ));

    let error = preflight_branch_updates_for_restart(&state, &project, &[task])
        .await
        .expect_err("unregistered workspace must fail closed");

    assert!(error
        .to_string()
        .contains("workspace exists without Git registration"));
    assert!(
        workspace_path.is_dir(),
        "preflight must not delete unregistered data"
    );
}

#[tokio::test]
async fn restart_branch_update_stop_releases_authority_and_deletes_operation_worktree() {
    let state = setup_apply_state();
    let repo_dir = setup_git_repo();
    let worktree_parent = tempfile::TempDir::new().expect("worktree parent should be created");
    let mut project = Project::new(
        "Restart active branch update".to_string(),
        repo_dir.path().to_string_lossy().into_owned(),
    );
    project.worktree_parent_directory = Some(worktree_parent.path().to_string_lossy().into_owned());
    let project = state
        .project_repo
        .create(project)
        .await
        .expect("project should be created");
    let mut task = crate::domain::entities::Task::new(
        project.id.clone(),
        "Task with active branch update".to_string(),
    );
    task.task_branch = Some(format!("task/update-{}", task.id.as_str()));
    let task_branch = task.task_branch.clone().expect("task branch should be set");
    git_ok(repo_dir.path(), &["branch", &task_branch, "main"]);
    let task = state
        .task_repo
        .create(task)
        .await
        .expect("task should be created");
    let workspace_path = validate_absolute_non_root_path(
        Path::new(&compute_source_update_worktree_path(
            &project,
            task.id.as_str(),
        )),
        "restart branch-update test workspace",
    )
    .expect("derived workspace path should be safe");
    GitService::checkout_existing_branch_worktree_strict(
        repo_dir.path(),
        &workspace_path,
        &task_branch,
    )
    .await
    .expect("branch-update operation worktree should be registered");
    let target_identity = GitService::canonical_target_identity(repo_dir.path(), &task_branch)
        .await
        .expect("target identity should resolve");
    let mut operation = BranchUpdateOperation::new(
        task.id.clone(),
        BranchUpdateDirection::TaskBranch,
        BranchUpdateContinuation::ResumeExecution,
        "restart-history-1",
        "main",
        task_branch,
        BranchUpdateWorkspaceOwnership::OperationWorktree,
        BranchUpdateCapacityOwnership::Inherited,
        target_identity,
        chrono::Utc::now(),
    );
    operation.workspace_path = Some(workspace_path.clone());
    let activation = state
        .branch_update_repo
        .activate(BranchUpdateActivation {
            operation,
            expected_status: InternalStatus::Backlog,
            update_status: InternalStatus::UpdatingTaskBranch,
            trigger: "restart_branch_update_stop_test".to_string(),
        })
        .await
        .expect("branch update should activate");
    assert!(matches!(
        activation,
        BranchUpdateActivationOutcome::Applied { .. }
    ));
    let plan_task = state
        .task_repo
        .create(crate::domain::entities::Task::new(
            project.id.clone(),
            "Plan branch update without materialized worktree".to_string(),
        ))
        .await
        .expect("plan update task should be created");
    let plan_workspace_path = validate_absolute_non_root_path(
        Path::new(&compute_plan_update_worktree_path(
            &project,
            plan_task.id.as_str(),
        )),
        "restart plan branch-update test workspace",
    )
    .expect("derived plan update workspace should be safe");
    let plan_target_identity = GitService::canonical_target_identity(repo_dir.path(), "main")
        .await
        .expect("plan target identity should resolve");
    let mut plan_operation = BranchUpdateOperation::new(
        plan_task.id.clone(),
        BranchUpdateDirection::PlanBranch,
        BranchUpdateContinuation::RetryPendingMerge,
        "restart-history-plan",
        task.task_branch
            .as_deref()
            .expect("task branch should remain available"),
        "main",
        BranchUpdateWorkspaceOwnership::OperationWorktree,
        BranchUpdateCapacityOwnership::Inherited,
        plan_target_identity,
        chrono::Utc::now(),
    );
    plan_operation.workspace_path = Some(plan_workspace_path.clone());
    let plan_activation = state
        .branch_update_repo
        .activate(BranchUpdateActivation {
            operation: plan_operation,
            expected_status: InternalStatus::Backlog,
            update_status: InternalStatus::UpdatingPlanBranch,
            trigger: "restart_plan_branch_update_stop_test".to_string(),
        })
        .await
        .expect("plan branch update should activate");
    assert!(matches!(
        plan_activation,
        BranchUpdateActivationOutcome::Applied { .. }
    ));
    let queue_key = QueueKey::new(ChatContextType::BranchUpdate, task.id.as_str());
    state
        .queued_message_repo
        .enqueue_back(
            &queue_key,
            &QueuedMessage::new("queued during update".to_string()),
        )
        .await
        .expect("queued branch-update message should persist");
    state
        .running_agent_registry
        .register(
            RunningAgentKey::new(ChatContextType::BranchUpdate.to_string(), task.id.as_str()),
            42,
            "branch-update-conversation".to_string(),
            "branch-update-run".to_string(),
            Some(workspace_path.to_string_lossy().into_owned()),
            None,
        )
        .await;

    let updates =
        preflight_branch_updates_for_restart(&state, &project, &[task.clone(), plan_task.clone()])
            .await
            .expect("active branch updates should preflight");
    assert_eq!(updates.len(), 2);

    stop_branch_updates_for_restart(&state, &updates)
        .await
        .expect("restart should stop the branch update with durable authority");
    cleanup_branch_update_worktrees_for_restart(&project, &updates)
        .await
        .expect("restart should delete the owned operation worktree");

    assert!(
        state
            .branch_update_repo
            .get_active_operation(&task.id)
            .await
            .expect("operation lookup should succeed")
            .is_none(),
        "stopped branch update should no longer be active"
    );
    assert_eq!(
        state
            .task_repo
            .get_by_id(&task.id)
            .await
            .expect("task lookup should succeed")
            .expect("task should remain")
            .internal_status,
        InternalStatus::Stopped
    );
    assert_eq!(
        state
            .task_repo
            .get_by_id(&plan_task.id)
            .await
            .expect("plan task lookup should succeed")
            .expect("plan task should remain")
            .internal_status,
        InternalStatus::Stopped
    );
    assert!(
        !workspace_path.exists(),
        "owned branch-update worktree should be removed"
    );
    assert!(
        !plan_workspace_path.exists(),
        "non-materialized plan update worktree should remain absent"
    );
    assert!(
        state
            .running_agent_registry
            .get(&RunningAgentKey::new(
                ChatContextType::BranchUpdate.to_string(),
                task.id.as_str(),
            ))
            .await
            .is_none(),
        "branch-update runtime should be stopped"
    );
    assert!(
        state
            .queued_message_repo
            .list(&queue_key)
            .await
            .expect("queued messages should load")
            .is_empty(),
        "branch-update queue should be cleared"
    );
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
    let execution_state = Arc::new(ExecutionState::new());
    let apply_result = apply_proposals_core(
        &state,
        &execution_state,
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
    assert_eq!(
        refreshed_plan_branch.pr_number,
        Some(739),
        "a confirmed remote PR remains authoritative across restart"
    );
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
    assert_eq!(
        stored_workspace.publication_pr_number,
        Some(739),
        "restart must not clear the workspace PR identity"
    );
    assert_eq!(
        state
            .agent_conversation_workspace_repo
            .get_local_cleanup_status(&conversation.id)
            .await
            .expect("cleanup status should load"),
        None
    );
}
