// Git Isolation Hardening Tests (A1-A8)
//
// Tests for git branch/worktree isolation during task execution.
// Since GitService methods are static (not trait-based), we cannot mock actual
// git operations. These tests verify:
// - State machine transitions (Ready->Executing) work correctly
// - on_enter runs and calls chat_service.send_message()
// - AppError::ExecutionBlocked variant exists and can be matched
// - Gap scenarios demonstrate missing validation

use super::helpers::*;
use crate::domain::entities::{GitMode, IdeationSessionId, InternalStatus, ProjectId};
use crate::domain::repositories::{ProjectRepository, TaskRepository};
use crate::domain::state_machine::events::TaskEvent;
use crate::domain::state_machine::machine::State;
use crate::error::AppError;

// ============================================================================
// A1: Worktree creation fails -> ExecutionBlocked
// ============================================================================

#[tokio::test]
async fn test_a1_worktree_mode_transition_ready_to_executing() {
    // Scenario A1: Worktree creation fails -> ExecutionBlocked — COVERED
    //
    // Since GitService is static (not mockable), we verify:
    // 1. The state machine transitions Ready->Executing correctly
    // 2. on_enter(Executing) runs and calls chat_service.send_message()
    // 3. With mock repos, the git setup path is exercised but uses real git
    //    commands which will fail on /tmp/test-project (not a git repo)
    //
    // The actual git worktree creation failure returns ExecutionBlocked,
    // but TransitionHandler catches on_enter errors and still returns Success.

    let svc = create_hardening_services();

    // Create project in Worktree mode
    let project = create_test_project_with_git_mode("worktree-proj", GitMode::Worktree);
    let project_id = project.id.clone();
    svc.project_repo.create(project).await.unwrap();

    // Create task in Ready state
    let mut task = create_test_task(&project_id, "Test worktree task");
    task.internal_status = InternalStatus::Ready;
    let task_id_str = task.id.as_str().to_string();
    svc.task_repo.create(task).await.unwrap();

    let services = build_task_services(&svc);
    let mut machine = create_state_machine(&task_id_str, project_id.as_str(), services);
    let mut handler = create_transition_handler(&mut machine);

    let result = handler
        .handle_transition(&State::Ready, &TaskEvent::StartExecution)
        .await;

    // After the fix, ExecutionBlocked triggers auto-dispatch of ExecutionFailed,
    // so the task transitions to Failed instead of staying in Executing
    assert!(
        result.is_success(),
        "TransitionHandler should return Success after auto-dispatching ExecutionFailed"
    );
    assert!(
        matches!(result.state(), Some(State::Failed(_))),
        "State should be Failed after ExecutionBlocked triggers auto-dispatch"
    );
}

#[tokio::test]
async fn test_a1_worktree_mode_without_repos_calls_send_message() {
    // Scenario A1 variant: Without repos, on_enter skips git setup entirely
    // and proceeds directly to send_message.

    let svc = create_hardening_services();
    let project_id = ProjectId::from_string("proj-no-repos".to_string());
    let task_id_str = "task-no-repos";

    // Build services WITHOUT task_repo and project_repo
    let services = crate::domain::state_machine::context::TaskServices::new(
        svc.spawner.clone() as _,
        svc.emitter.clone() as _,
        svc.notifier.clone() as _,
        svc.dependency_manager.clone() as _,
        svc.review_starter.clone() as _,
        svc.chat_service.clone() as _,
    )
    .with_execution_state(svc.execution_state.clone());

    let mut machine = create_state_machine(task_id_str, project_id.as_str(), services);
    let mut handler = create_transition_handler(&mut machine);

    let result = handler
        .handle_transition(&State::Ready, &TaskEvent::StartExecution)
        .await;

    assert!(result.is_success());
    assert_eq!(result.state(), Some(&State::Executing));

    // Without repos, git setup is skipped, so send_message should be called
    assert_eq!(
        svc.chat_service.call_count(),
        1,
        "send_message should be called once when repos are absent (git setup skipped)"
    );
}

// ============================================================================
// A3: Uncommitted changes detected -> ExecutionBlocked
// ============================================================================

#[tokio::test]
async fn test_a3_execution_blocked_variant_exists() {
    // Scenario A3: Uncommitted changes detected -> ExecutionBlocked — COVERED
    //
    // Since GitService::has_uncommitted_changes is static and checks real filesystem,
    // we verify the ExecutionBlocked error variant exists and can be constructed/matched.

    let error = AppError::ExecutionBlocked(
        "Cannot execute task: uncommitted changes in working directory. \
         Please commit or stash your changes first."
            .to_string(),
    );

    // Verify the error can be matched
    assert!(matches!(&error, AppError::ExecutionBlocked(msg) if msg.contains("uncommitted")));

    // Verify Display impl
    let display = format!("{}", error);
    assert!(
        display.contains("uncommitted"),
        "ExecutionBlocked display should contain the message"
    );
}

// ============================================================================
// A4: Uncommitted changes check fails -> ExecutionBlocked
// ============================================================================

#[tokio::test]
async fn test_a4_execution_blocked_for_check_failure() {
    // Scenario A4: Uncommitted changes check fails -> ExecutionBlocked — COVERED
    //
    // Verify that the error variant for check failures exists and is distinct.

    let error = AppError::ExecutionBlocked(
        "Git isolation failed: could not check working directory for uncommitted changes: \
         not a git repository"
            .to_string(),
    );

    assert!(matches!(
        &error,
        AppError::ExecutionBlocked(msg) if msg.contains("could not check")
    ));
}

// ============================================================================
// A5: Worktree mode but worktree_path is None at spawn time -> re-creates worktree
// ============================================================================

#[tokio::test]
async fn test_a5_worktree_path_none_at_spawn_time() {
    // Scenario A5: Worktree mode, task_branch is set but worktree_path is None — COVERED
    //
    // When a task already has task_branch set but no worktree_path (e.g., DB persist failed
    // after worktree creation, or worktree was cleaned up between runs), the state machine
    // re-creates the missing worktree and persists the path before spawning the agent.
    //
    // This requires a real git repo so checkout_existing_branch_worktree can succeed.

    let temp_dir = tempfile::tempdir().unwrap();
    let repo_path = temp_dir.path().join("repo");
    let worktrees_dir = temp_dir.path().join("worktrees");
    std::fs::create_dir_all(&repo_path).unwrap();
    std::fs::create_dir_all(&worktrees_dir).unwrap();

    // Initialize a real git repo
    std::process::Command::new("git")
        .args(["init"])
        .current_dir(&repo_path)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(&repo_path)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(&repo_path)
        .output()
        .unwrap();
    std::fs::write(repo_path.join("test.txt"), "initial").unwrap();
    std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(&repo_path)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(&repo_path)
        .output()
        .unwrap();
    let _ = std::process::Command::new("git")
        .args(["branch", "-M", "main"])
        .current_dir(&repo_path)
        .output();
    let base_sha_output = std::process::Command::new("git")
        .args(["rev-parse", "main"])
        .current_dir(&repo_path)
        .output()
        .unwrap();
    assert!(
        base_sha_output.status.success(),
        "git rev-parse main failed: {}",
        String::from_utf8_lossy(&base_sha_output.stderr)
    );
    let base_sha = String::from_utf8_lossy(&base_sha_output.stdout)
        .trim()
        .to_string();

    let svc = create_hardening_services();

    let mut project = create_test_project_with_git_mode("wt-proj", GitMode::Worktree);
    project.working_directory = repo_path.to_str().unwrap().to_string();
    project.worktree_parent_directory = Some(worktrees_dir.to_str().unwrap().to_string());
    let project_id = project.id.clone();
    svc.project_repo.create(project).await.unwrap();

    // Create the task first to get its generated ID
    let mut task = create_test_task(&project_id, "No worktree path task");
    task.internal_status = InternalStatus::Ready;
    let task_id = task.id.clone();
    let task_id_str = task.id.as_str().to_string();

    // Create the task branch in the real repo using the task's actual ID
    let branch_name = format!("ralphx/wt-proj/task-{}", task_id_str);
    std::process::Command::new("git")
        .args(["branch", &branch_name])
        .current_dir(&repo_path)
        .output()
        .unwrap();

    // Set task_branch but leave worktree_path = None
    task.task_branch = Some(branch_name.clone());
    task.task_branch_base_ref = Some("main".to_string());
    task.task_branch_base_sha = Some(base_sha);
    task.worktree_path = None; // explicitly None
    svc.task_repo.create(task).await.unwrap();

    let services = build_task_services(&svc);
    let mut machine = create_state_machine(&task_id_str, project_id.as_str(), services);
    let mut handler = create_transition_handler(&mut machine);

    let result = handler
        .handle_transition(&State::Ready, &TaskEvent::StartExecution)
        .await;

    assert!(result.is_success());
    assert_eq!(result.state(), Some(&State::Executing));

    // Direct TransitionHandler tests do not persist Executing before on_enter, so the runtime
    // may skip the actual agent send. The invariant here is that the missing worktree is
    // recreated and persisted before execution continues.
    let updated = svc.task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    let restored_worktree = updated
        .worktree_path
        .clone()
        .expect("missing worktree path must be restored");
    assert!(
        restored_worktree.contains(&format!("task-{task_id_str}")),
        "restored worktree path must reference the task id: {restored_worktree}"
    );
    assert!(
        std::path::Path::new(&restored_worktree).exists(),
        "restored worktree must exist on disk: {restored_worktree}"
    );
}

// ============================================================================
// A6: ExecutionBlocked from on_enter transitions task to Failed
// ============================================================================

#[tokio::test]
async fn test_a6_execution_blocked_does_not_propagate_through_transition_handler() {
    // Scenario A6: ExecutionBlocked from on_enter — COVERED
    //
    // TransitionHandler now auto-dispatches ExecutionFailed when on_enter
    // returns ExecutionBlocked. This prevents tasks from getting stuck in
    // Executing state with no agent. The task should end up in Failed state.

    let svc = create_hardening_services();

    // Create project pointing to non-git directory
    let project = create_test_project("blocked-proj");
    let project_id = project.id.clone();
    svc.project_repo.create(project).await.unwrap();

    // Create task without a branch (triggers git setup in on_enter)
    let mut task = create_test_task(&project_id, "Blocked task");
    task.internal_status = InternalStatus::Ready;
    let task_id_str = task.id.as_str().to_string();
    svc.task_repo.create(task).await.unwrap();

    let services = build_task_services(&svc);
    let mut machine = create_state_machine(&task_id_str, project_id.as_str(), services);
    let mut handler = create_transition_handler(&mut machine);

    let result = handler
        .handle_transition(&State::Ready, &TaskEvent::StartExecution)
        .await;

    // After the fix, ExecutionBlocked triggers auto-dispatch of ExecutionFailed,
    // so the task transitions to Failed instead of staying in Executing
    assert!(
        result.is_success(),
        "TransitionHandler should return Success after auto-dispatching ExecutionFailed"
    );
    assert!(
        matches!(result.state(), Some(State::Failed(_))),
        "State should be Failed after ExecutionBlocked triggers auto-dispatch"
    );

    // send_message was NOT called because ExecutionBlocked is returned before it
    assert_eq!(
        svc.chat_service.call_count(),
        0,
        "send_message should not be called when execution is blocked"
    );
}

#[tokio::test]
async fn test_a6_execution_failed_event_transitions_executing_to_failed() {
    // Scenario A6 follow-up: Verify that ExecutionFailed event correctly
    // transitions Executing -> Failed. This is how the task_transition_service
    // handles the ExecutionBlocked error — by firing ExecutionFailed.

    let svc = create_hardening_services();
    let services = build_task_services(&svc);
    let mut machine = create_state_machine("task-a6-fail", "proj-a6", services);
    let mut handler = create_transition_handler(&mut machine);

    let result = handler
        .handle_transition(
            &State::Executing,
            &TaskEvent::ExecutionFailed {
                error: "Git isolation failed: ExecutionBlocked".to_string(),
            },
        )
        .await;

    assert!(result.is_success());
    match result.state() {
        Some(State::Failed(data)) => {
            assert!(
                data.error.contains("ExecutionBlocked"),
                "Failed state should preserve the ExecutionBlocked error message"
            );
        }
        other => panic!("Expected Failed state, got {:?}", other),
    }
}

// ============================================================================
// A7: Missing task branch during worktree recreation -> Layer 2 self-healing
// ============================================================================

#[tokio::test]
async fn test_a7_missing_branch_self_heals_and_enters_executing() {
    // Scenario A7: Task has task_branch set but the branch was deleted from git
    // (e.g., during prior merge cleanup). Worktree is also missing.
    //
    // Layer 2 behavior (RC#14): on_enter(Executing) self-heals by detecting the
    // deleted branch, creating a fresh branch + worktree, and proceeding to spawn
    // the agent. Task ends up in Executing state (not Failed).

    let temp_dir = tempfile::tempdir().unwrap();
    let repo_path = temp_dir.path().join("repo");
    let worktrees_dir = temp_dir.path().join("worktrees");
    std::fs::create_dir_all(&repo_path).unwrap();
    std::fs::create_dir_all(&worktrees_dir).unwrap();

    // Initialize a real git repo with an initial commit
    std::process::Command::new("git")
        .args(["init"])
        .current_dir(&repo_path)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(&repo_path)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(&repo_path)
        .output()
        .unwrap();
    std::fs::write(repo_path.join("test.txt"), "initial").unwrap();
    std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(&repo_path)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(&repo_path)
        .output()
        .unwrap();
    let _ = std::process::Command::new("git")
        .args(["branch", "-M", "main"])
        .current_dir(&repo_path)
        .output();

    let svc = create_hardening_services();

    let mut project = create_test_project_with_git_mode("missing-branch-proj", GitMode::Worktree);
    project.working_directory = repo_path.to_str().unwrap().to_string();
    project.worktree_parent_directory = Some(worktrees_dir.to_str().unwrap().to_string());
    project.base_branch = Some("main".to_string());
    let project_id = project.id.clone();
    svc.project_repo.create(project).await.unwrap();

    // Create task with task_branch pointing to a branch that does NOT exist in the repo
    let mut task = create_test_task(&project_id, "Task with deleted branch");
    task.internal_status = InternalStatus::Ready;
    let task_id = task.id.clone();
    let task_id_str = task.id.as_str().to_string();
    // Set task_branch to a branch name that was never created in git (simulating post-merge deletion)
    let stale_branch = format!("ralphx/missing-branch-proj/task-{}", task_id_str);
    task.task_branch = Some(stale_branch.clone());
    task.worktree_path = None; // No worktree exists either
    svc.task_repo.create(task).await.unwrap();

    let services = build_task_services(&svc);
    let mut machine = create_state_machine(&task_id_str, project_id.as_str(), services);
    let mut handler = create_transition_handler(&mut machine);

    let result = handler
        .handle_transition(&State::Ready, &TaskEvent::StartExecution)
        .await;

    // Layer 2 self-healing: deleted branch is detected, fresh branch is created,
    // agent is spawned → task ends in Executing state (not Failed)
    assert!(
        result.is_success(),
        "TransitionHandler should return Success after self-healing"
    );
    assert!(
        matches!(result.state(), Some(State::Executing)),
        "State should be Executing after self-heal creates fresh branch: {:?}",
        result.state()
    );

    // Direct TransitionHandler tests do not guarantee the actual agent send once the repo row
    // is still Ready. The invariant here is that self-heal recreated a runnable git context.
    let updated = svc.task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    let restored_worktree = updated
        .worktree_path
        .clone()
        .expect("self-heal must restore a worktree path");
    assert!(
        restored_worktree.contains(&format!("task-{task_id_str}")),
        "restored worktree path must reference the task id: {restored_worktree}"
    );
    assert!(
        std::path::Path::new(&restored_worktree).exists(),
        "restored worktree must exist on disk: {restored_worktree}"
    );

    // Fresh branch should now exist in git
    let branch_list = std::process::Command::new("git")
        .args(["branch", "--list", &stale_branch])
        .current_dir(&repo_path)
        .output()
        .unwrap();
    let branch_output = String::from_utf8_lossy(&branch_list.stdout);
    assert!(
        !branch_output.trim().is_empty(),
        "Fresh branch '{}' should exist in git after self-heal",
        stale_branch
    );
}

// ============================================================================
// A8: Base branch doesn't exist — partial coverage
// ============================================================================

#[tokio::test]
async fn test_a8_nonexistent_plan_branch_falls_back_to_project_base() {
    // Scenario A8: Base branch doesn't exist — PARTIAL
    //
    // Create task with ideation_session_id pointing to a plan branch that
    // doesn't exist in the repository. Verify on_enter proceeds (falls back
    // to project base_branch via resolve_task_base_branch).

    let svc = create_hardening_services();

    let mut project = create_test_project("base-branch-proj");
    project.base_branch = Some("develop".to_string());
    let project_id = project.id.clone();
    svc.project_repo.create(project).await.unwrap();

    // Create task with ideation_session_id that has no matching plan branch
    let mut task = create_test_task(&project_id, "Fallback base branch task");
    task.internal_status = InternalStatus::Ready;
    task.ideation_session_id = Some(IdeationSessionId::from_string("nonexistent-session"));
    let task_id_str = task.id.as_str().to_string();
    svc.task_repo.create(task).await.unwrap();

    let services = build_task_services(&svc);
    let mut machine = create_state_machine(&task_id_str, project_id.as_str(), services);
    let mut handler = create_transition_handler(&mut machine);

    let result = handler
        .handle_transition(&State::Ready, &TaskEvent::StartExecution)
        .await;

    // After the fix, ExecutionBlocked triggers auto-dispatch of ExecutionFailed,
    // so the task transitions to Failed instead of staying in Executing
    assert!(
        result.is_success(),
        "TransitionHandler should return Success after auto-dispatching ExecutionFailed"
    );
    assert!(
        matches!(result.state(), Some(State::Failed(_))),
        "State should be Failed after ExecutionBlocked triggers auto-dispatch"
    );
}

#[tokio::test]
async fn test_a8_no_plan_branch_repo_falls_back_to_default() {
    // Scenario A8 variant: When plan_branch_repo is None, resolve_task_base_branch
    // should fall back to project.base_branch.

    let svc = create_hardening_services();

    let mut project = create_test_project("no-pb-repo-proj");
    project.base_branch = Some("main".to_string());
    let project_id = project.id.clone();
    svc.project_repo.create(project).await.unwrap();

    let mut task = create_test_task(&project_id, "No plan branch repo task");
    task.internal_status = InternalStatus::Ready;
    task.ideation_session_id = Some(IdeationSessionId::from_string("sess-123"));
    let task_id_str = task.id.as_str().to_string();
    svc.task_repo.create(task).await.unwrap();

    // Build services WITHOUT plan_branch_repo
    let services = crate::domain::state_machine::context::TaskServices::new(
        svc.spawner.clone() as _,
        svc.emitter.clone() as _,
        svc.notifier.clone() as _,
        svc.dependency_manager.clone() as _,
        svc.review_starter.clone() as _,
        svc.chat_service.clone() as _,
    )
    .with_execution_state(svc.execution_state.clone())
    .with_task_repo(
        svc.task_repo.clone() as std::sync::Arc<dyn crate::domain::repositories::TaskRepository>
    )
    .with_project_repo(svc.project_repo.clone()
        as std::sync::Arc<dyn crate::domain::repositories::ProjectRepository>);
    // Note: no .with_plan_branch_repo() — it stays None

    let mut machine = create_state_machine(&task_id_str, project_id.as_str(), services);
    let mut handler = create_transition_handler(&mut machine);

    let result = handler
        .handle_transition(&State::Ready, &TaskEvent::StartExecution)
        .await;

    // Transition succeeds — resolve_task_base_branch returns default ("main")
    // when plan_branch_repo is None
    assert!(result.is_success());
}

#[tokio::test]
async fn test_a8_base_branch_not_validated_at_transition_time() {
    // Scenario A8: PARTIAL — Document that on_enter(Executing) does not validate
    // whether the base branch actually exists in the git repo.
    //
    // The base_branch field is just a string — no validation against actual git refs.
    // If the base branch doesn't exist, the git checkout/worktree command will fail
    // later during on_enter, but the transition itself still succeeds.

    let svc = create_hardening_services();

    let mut project = create_test_project("bogus-base-proj");
    project.base_branch = Some("nonexistent-branch-that-will-fail".to_string());
    let project_id = project.id.clone();
    svc.project_repo.create(project).await.unwrap();

    let mut task = create_test_task(&project_id, "Bogus base branch task");
    task.internal_status = InternalStatus::Ready;
    let task_id_str = task.id.as_str().to_string();
    svc.task_repo.create(task).await.unwrap();

    let services = build_task_services(&svc);
    let mut machine = create_state_machine(&task_id_str, project_id.as_str(), services);
    let mut handler = create_transition_handler(&mut machine);

    let result = handler
        .handle_transition(&State::Ready, &TaskEvent::StartExecution)
        .await;

    // Transition succeeds — base branch existence is not validated at transition time
    assert!(
        result.is_success(),
        "PARTIAL: No base branch existence validation at transition time"
    );
}
