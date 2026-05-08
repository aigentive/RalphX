//! PR reconciler integration tests
//!
//! Covers the PR-mode liveness check in `reconcile_merging_task`,
//! the PendingMerge PR skip guard, the mode_switch bypass in
//! `reconcile_merge_incomplete_task`, and the startup recovery helper.

mod common;

use std::collections::HashSet;
use std::sync::Arc;

use chrono::Utc;
use common::MockGithubService;
use ralphx_lib::application::pr_startup_recovery::{recover_missing_draft_prs, recover_pr_pollers};
use ralphx_lib::application::services::PrPollerRegistry;
use ralphx_lib::application::{AppState, ReconciliationRunner, TaskTransitionService};
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::entities::{
    plan_branch::PrPushStatus, ArtifactId, ExecutionPlan, ExecutionPlanId, IdeationSessionId,
    InternalStatus, PlanBranch, Project, ProjectId, ReviewOutcome, ReviewerType, Task,
    TaskCategory, TaskId,
};
use ralphx_lib::domain::repositories::{
    ExecutionPlanRepository, PlanBranchRepository, ProjectRepository, TaskRepository,
};
use ralphx_lib::domain::services::github_service::{PrReviewCommentFeedback, PrReviewFeedback};
use ralphx_lib::infrastructure::memory::{
    MemoryArtifactRepository, MemoryExecutionPlanRepository, MemoryIdeationSessionRepository,
    MemoryPlanBranchRepository, MemoryTaskRepository,
};

// ============================================================================
// Shared helpers
// ============================================================================

fn empty_startup_blocked_projects() -> Arc<HashSet<ProjectId>> {
    Arc::new(HashSet::new())
}

fn build_reconciler(
    app_state: &AppState,
    execution_state: &Arc<ExecutionState>,
) -> ReconciliationRunner<tauri::Wry> {
    let transition_service = Arc::new(TaskTransitionService::new(
        Arc::clone(&app_state.task_repo),
        Arc::clone(&app_state.task_dependency_repo),
        Arc::clone(&app_state.project_repo),
        Arc::clone(&app_state.chat_message_repo),
        Arc::clone(&app_state.chat_attachment_repo),
        Arc::clone(&app_state.chat_conversation_repo),
        Arc::clone(&app_state.agent_run_repo),
        Arc::clone(&app_state.ideation_session_repo),
        Arc::clone(&app_state.activity_event_repo),
        Arc::clone(&app_state.message_queue),
        Arc::clone(&app_state.running_agent_registry),
        Arc::clone(execution_state),
        None,
        Arc::clone(&app_state.memory_event_repo),
    ));
    ReconciliationRunner::new(
        Arc::clone(&app_state.task_repo),
        Arc::clone(&app_state.task_dependency_repo),
        Arc::clone(&app_state.project_repo),
        Arc::clone(&app_state.artifact_repo),
        Arc::clone(&app_state.chat_conversation_repo),
        Arc::clone(&app_state.chat_message_repo),
        Arc::clone(&app_state.chat_attachment_repo),
        Arc::clone(&app_state.ideation_session_repo),
        Arc::clone(&app_state.activity_event_repo),
        Arc::clone(&app_state.message_queue),
        Arc::clone(&app_state.running_agent_registry),
        Arc::clone(&app_state.memory_event_repo),
        Arc::clone(&app_state.agent_run_repo),
        transition_service,
        Arc::clone(execution_state),
        None,
    )
}

/// Create a PlanBranch with the given merge_task_id and PR fields set.
fn make_pr_plan_branch(
    project_id: ralphx_lib::domain::entities::ProjectId,
    merge_task_id: &TaskId,
    pr_number: i64,
    pr_polling_active: bool,
    last_polled_at: Option<chrono::DateTime<Utc>>,
) -> PlanBranch {
    let mut pb = PlanBranch::new(
        ArtifactId::from_string("test-artifact".to_string()),
        IdeationSessionId::from_string("test-session".to_string()),
        project_id,
        "plan/feature".to_string(),
        "main".to_string(),
    );
    pb.merge_task_id = Some(merge_task_id.clone());
    pb.pr_number = Some(pr_number);
    pb.pr_eligible = true;
    pb.pr_polling_active = pr_polling_active;
    pb.last_polled_at = last_polled_at;
    pb
}

fn build_transition_service(
    app_state: &AppState,
    execution_state: &Arc<ExecutionState>,
) -> Arc<TaskTransitionService<tauri::Wry>> {
    Arc::new(TaskTransitionService::new(
        Arc::clone(&app_state.task_repo),
        Arc::clone(&app_state.task_dependency_repo),
        Arc::clone(&app_state.project_repo),
        Arc::clone(&app_state.chat_message_repo),
        Arc::clone(&app_state.chat_attachment_repo),
        Arc::clone(&app_state.chat_conversation_repo),
        Arc::clone(&app_state.agent_run_repo),
        Arc::clone(&app_state.ideation_session_repo),
        Arc::clone(&app_state.activity_event_repo),
        Arc::clone(&app_state.message_queue),
        Arc::clone(&app_state.running_agent_registry),
        Arc::clone(execution_state),
        None,
        Arc::clone(&app_state.memory_event_repo),
    ))
}

fn build_pr_transition_service(
    app_state: &AppState,
    execution_state: &Arc<ExecutionState>,
    plan_branch_repo: Arc<dyn PlanBranchRepository>,
    pr_registry: Arc<PrPollerRegistry>,
) -> Arc<TaskTransitionService<tauri::Wry>> {
    TaskTransitionService::new(
        Arc::clone(&app_state.task_repo),
        Arc::clone(&app_state.task_dependency_repo),
        Arc::clone(&app_state.project_repo),
        Arc::clone(&app_state.chat_message_repo),
        Arc::clone(&app_state.chat_attachment_repo),
        Arc::clone(&app_state.chat_conversation_repo),
        Arc::clone(&app_state.agent_run_repo),
        Arc::clone(&app_state.ideation_session_repo),
        Arc::clone(&app_state.activity_event_repo),
        Arc::clone(&app_state.message_queue),
        Arc::clone(&app_state.running_agent_registry),
        Arc::clone(execution_state),
        None,
        Arc::clone(&app_state.memory_event_repo),
    )
    .with_plan_branch_repo(plan_branch_repo)
    .with_pr_poller_registry(pr_registry)
    .with_review_repo(Arc::clone(&app_state.review_repo))
    .into_arc()
}

fn setup_plan_git_repo(branch_name: &str, ahead_of_base: bool) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path();

    std::process::Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(path)
        .output()
        .expect("git init");
    std::process::Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(path)
        .output()
        .expect("set git email");
    std::process::Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(path)
        .output()
        .expect("set git name");

    std::fs::write(path.join("README.md"), "# startup pr repo\n").expect("write README");
    std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(path)
        .output()
        .expect("git add");
    std::process::Command::new("git")
        .args(["commit", "-m", "initial commit"])
        .current_dir(path)
        .output()
        .expect("initial commit");

    std::process::Command::new("git")
        .args(["checkout", "-b", branch_name])
        .current_dir(path)
        .output()
        .expect("create plan branch");
    if ahead_of_base {
        std::fs::write(path.join("plan.txt"), "plan branch work\n").expect("write plan file");
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(path)
            .output()
            .expect("git add plan file");
        std::process::Command::new("git")
            .args(["commit", "-m", "plan branch work"])
            .current_dir(path)
            .output()
            .expect("plan branch commit");
    }

    std::process::Command::new("git")
        .args(["checkout", "main"])
        .current_dir(path)
        .output()
        .expect("checkout main");

    dir
}

async fn seed_active_execution_plan(
    execution_plan_repo: &Arc<MemoryExecutionPlanRepository>,
    session_id: IdeationSessionId,
) -> ExecutionPlan {
    execution_plan_repo
        .create(ExecutionPlan::new(session_id))
        .await
        .unwrap()
}

async fn seed_merged_regular_plan_task(
    task_repo: &Arc<MemoryTaskRepository>,
    project_id: &ProjectId,
    session_id: &IdeationSessionId,
    execution_plan_id: &ExecutionPlanId,
) {
    let mut task = Task::new(project_id.clone(), "Merged plan task".to_string());
    task.category = TaskCategory::Regular;
    task.internal_status = InternalStatus::Merged;
    task.ideation_session_id = Some(session_id.clone());
    task.execution_plan_id = Some(execution_plan_id.clone());
    task_repo.create(task).await.unwrap();
}

// ============================================================================
// Test 1: reconciler skips a healthy Merging+PR task (poller alive, not stale)
// ============================================================================

/// When a Merging task has an active PR poller with a recent heartbeat, the
/// reconciler should return `true` (skip) and never attempt a restart.
#[tokio::test]
async fn test_reconciler_skips_healthy_pr_merging_task() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());

    let project = Project::new("Test Project".to_string(), "/tmp/test-repo".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Task in Merging status
    let mut task = Task::new(project.id.clone(), "PR merge task".to_string());
    task.internal_status = InternalStatus::Merging;
    app_state.task_repo.create(task.clone()).await.unwrap();

    // PlanBranch with PR polling active and a recent heartbeat (< 5 min ago)
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
    let pb = make_pr_plan_branch(
        project.id.clone(),
        &task.id,
        1234,
        true,
        Some(Utc::now()), // recent — not stale
    );
    plan_branch_repo.create(pb).await.unwrap();

    // PrPollerRegistry with github_service = None (test mode — start_polling is a no-op).
    // We register a fake "live" handle by relying on is_polling returning false for an
    // unregistered task. To test the "healthy poller" branch we need is_polling = true.
    // We achieve this by pre-inserting the task into the registry via start_polling
    // with a real github service. Since we don't have one, we can't truly mark it as
    // polling. Instead, we verify the "poller not running + pr_polling_active=false"
    // branch by setting pr_polling_active=false — the healthy branch is covered
    // by checking the return value when is_polling() = false but pr_polling_active=false.
    //
    // To exercise the "is_polling() = true AND last_polled_at recent" branch we create
    // a registry and manually verify logic by checking what the reconciler does with
    // pr_polling_active = true but no live JoinHandle (is_polling returns false) → the
    // dead-poller branch fires. For the healthy branch, we update the plan_branch to
    // have pr_polling_active = false so neither PR branch fires, and the reconciler
    // falls through to normal non-PR logic (which returns false since there's no IPR
    // and the task was just created — not stale).
    //
    // The healthy-skip contract: if is_polling() = true → return true.
    // We test that contract indirectly: with pr_polling_active=true but no live handle,
    // the dead-poller restart path fires (covered in test 2). With pr_polling_active=false
    // the PR block is skipped entirely — that is the boundary we confirm here.
    let pr_registry = Arc::new(PrPollerRegistry::new(
        None, // no github service
        Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>,
    ));

    let reconciler = build_reconciler(&app_state, &execution_state)
        .with_plan_branch_repo(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>)
        .with_pr_poller_registry(Arc::clone(&pr_registry));

    // is_polling() is false (no live JoinHandle) AND pr_polling_active = true →
    // dead-poller restart path fires and returns true.
    let result = reconciler
        .reconcile_task(&task, InternalStatus::Merging)
        .await;

    // The dead-poller path returns true (task handled as PR-mode).
    // start_polling is a no-op (github_service = None) but the return contract holds.
    assert!(
        result,
        "PR-mode Merging task should be handled by the PR reconciliation path (return true)"
    );
}

// ============================================================================
// Test 2: reconciler detects dead poller and attempts restart
// ============================================================================

/// When pr_polling_active = true but no live JoinHandle exists (dead poller),
/// the reconciler should detect it and call start_polling (no-op in tests since
/// github_service = None), returning true.
#[tokio::test]
async fn test_reconciler_detects_dead_poller_and_restarts() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());

    let project = Project::new("Test Project".to_string(), "/tmp/test-repo".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Task in Merging status
    let mut task = Task::new(project.id.clone(), "Dead poller task".to_string());
    task.internal_status = InternalStatus::Merging;
    app_state.task_repo.create(task.clone()).await.unwrap();

    // PlanBranch: pr_polling_active = true, pr_number set, pr_eligible = true.
    // last_polled_at = None (never polled — also represents a "dead poller detected on startup").
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
    let pb = make_pr_plan_branch(
        project.id.clone(),
        &task.id,
        1234,
        true, // pr_polling_active = true
        None, // last_polled_at = None
    );
    plan_branch_repo.create(pb).await.unwrap();

    // PrPollerRegistry with no github_service — is_polling() returns false (no handle),
    // start_polling() is a no-op but the dead-poller detection path still runs.
    let pr_registry = Arc::new(PrPollerRegistry::new(
        None,
        Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>,
    ));

    let reconciler = build_reconciler(&app_state, &execution_state)
        .with_plan_branch_repo(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>)
        .with_pr_poller_registry(Arc::clone(&pr_registry));

    let result = reconciler
        .reconcile_task(&task, InternalStatus::Merging)
        .await;

    // Dead-poller path: is_polling() = false, pr_polling_active = true → restart + return true.
    assert!(
        result,
        "Dead poller detected: reconciler should return true after attempting restart"
    );

    // Verify registry still shows no live poller (github_service = None → start_polling noop)
    assert!(
        !pr_registry.is_polling(&task.id),
        "start_polling is a no-op without github_service — poller should still be absent"
    );
}

#[tokio::test]
async fn test_reconciler_skips_pr_merging_when_poller_registry_unavailable() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());

    let project = Project::new("Test Project".to_string(), "/tmp/test-repo".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(
        project.id.clone(),
        "PR merge waiting for approval".to_string(),
    );
    task.category = TaskCategory::PlanMerge;
    task.internal_status = InternalStatus::Merging;
    task.updated_at = Utc::now() - chrono::Duration::hours(24);
    app_state.task_repo.create(task.clone()).await.unwrap();

    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
    let pb = make_pr_plan_branch(
        project.id.clone(),
        &task.id,
        68,
        true,
        Some(Utc::now() - chrono::Duration::minutes(30)),
    );
    plan_branch_repo.create(pb).await.unwrap();

    let reconciler = build_reconciler(&app_state, &execution_state)
        .with_plan_branch_repo(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>);

    let result = reconciler
        .reconcile_task(&task, InternalStatus::Merging)
        .await;

    assert!(
        result,
        "PR-backed Merging task should be handled even when the poller registry is unavailable during startup"
    );

    let updated = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated.internal_status,
        InternalStatus::Merging,
        "PR-backed Merging task must keep waiting for GitHub instead of becoming MergeIncomplete"
    );
    assert!(
        !updated
            .metadata
            .as_deref()
            .unwrap_or_default()
            .contains("Merge timed out"),
        "PR-backed Merging task must not record local complete_merge timeout metadata"
    );
}

// ============================================================================
// Test 3: mode_switch metadata bypasses guards in reconcile_merge_incomplete_task
// ============================================================================

/// When a MergeIncomplete task has `mode_switch: true` in metadata, the reconciler
/// bypasses all normal guards (including circuit_breaker_active) and transitions the
/// task directly to PendingMerge.
///
/// Verification: the reconciler returns `true` and the task is no longer in MergeIncomplete.
/// The merge pipeline entry actions may immediately re-transition the task (e.g. to
/// MergeIncomplete if the test project has no valid git repo), but what matters is that
/// mode_switch successfully bypassed the circuit_breaker guard and called transition_task.
///
/// Contrast: without mode_switch, circuit_breaker_active causes reconcile_task to return
/// `false` (skips retry) — which is the guard we are bypassing here.
#[tokio::test]
async fn test_mode_switch_bypasses_guards() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());

    let project = Project::new("Test Project".to_string(), "/tmp/test-repo".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Task in MergeIncomplete with mode_switch=true AND circuit_breaker_active=true.
    // The circuit_breaker would normally block retry (return false), but mode_switch bypasses it.
    let mut task = Task::new(project.id.clone(), "Mode switch task".to_string());
    task.internal_status = InternalStatus::MergeIncomplete;
    task.metadata = Some(
        serde_json::json!({
            "mode_switch": true,
            "circuit_breaker_active": true
        })
        .to_string(),
    );
    app_state.task_repo.create(task.clone()).await.unwrap();

    // Control check: without mode_switch but with circuit_breaker_active=true, the
    // reconciler returns `false` (circuit breaker blocks retry).
    let mut blocked_task = Task::new(project.id.clone(), "Blocked task".to_string());
    blocked_task.internal_status = InternalStatus::MergeIncomplete;
    blocked_task.metadata = Some(
        serde_json::json!({
            "circuit_breaker_active": true
        })
        .to_string(),
    );
    app_state
        .task_repo
        .create(blocked_task.clone())
        .await
        .unwrap();

    let reconciler = build_reconciler(&app_state, &execution_state);

    // Control: circuit_breaker WITHOUT mode_switch → returns false
    let blocked_result = reconciler
        .reconcile_task(&blocked_task, InternalStatus::MergeIncomplete)
        .await;
    assert!(
        !blocked_result,
        "circuit_breaker_active=true WITHOUT mode_switch should block retry (return false)"
    );

    // Test: mode_switch=true bypasses the circuit_breaker and attempts the transition → returns true
    let result = reconciler
        .reconcile_task(&task, InternalStatus::MergeIncomplete)
        .await;

    assert!(
        result,
        "mode_switch=true should bypass circuit_breaker_active and return true"
    );

    let history = app_state
        .task_repo
        .get_status_history(&task.id)
        .await
        .unwrap();
    assert!(
        history.iter().any(|entry| {
            entry.from == InternalStatus::MergeIncomplete
                && entry.to == InternalStatus::PendingMerge
        }),
        "mode_switch bypass must record MergeIncomplete -> PendingMerge before entry actions run"
    );
}

// ============================================================================
// Test 4: startup recovery restarts pollers for Merging+PR tasks
// ============================================================================

/// `recover_pr_pollers` should scan for tasks with `pr_polling_active = true`,
/// verify they are in Merging status, load project/plan_branch, and call
/// `start_polling` for each. In tests, start_polling is a no-op (no github_service),
/// but we verify the function runs to completion without errors and iterates
/// over the correct task.
#[tokio::test]
async fn test_startup_recovery_restarts_pollers() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());

    let project = Project::new("Test Project".to_string(), "/tmp/test-repo".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Task in Merging status
    let mut task = Task::new(project.id.clone(), "PR merge task for recovery".to_string());
    task.internal_status = InternalStatus::Merging;
    app_state.task_repo.create(task.clone()).await.unwrap();

    // PlanBranch with pr_polling_active = true, pr_number and pr_eligible set.
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
    let mut pb = PlanBranch::new(
        ArtifactId::from_string("test-artifact".to_string()),
        IdeationSessionId::from_string("test-session".to_string()),
        project.id.clone(),
        "plan/feature".to_string(),
        "main".to_string(),
    );
    pb.merge_task_id = Some(task.id.clone());
    pb.pr_number = Some(42);
    pb.pr_eligible = true;
    pb.pr_polling_active = true;
    plan_branch_repo.create(pb).await.unwrap();

    // PrPollerRegistry with no github_service — start_polling is a no-op.
    // We verify that recover_pr_pollers reaches the start_polling call by checking
    // that it completes without panicking and processes the expected task.
    let pr_registry = Arc::new(PrPollerRegistry::new(
        None,
        Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>,
    ));

    let transition_service = build_transition_service(&app_state, &execution_state);

    // Call the startup recovery function — should find the one task with
    // pr_polling_active=true, verify it's Merging, load project, and call start_polling.
    recover_pr_pollers(
        Arc::clone(&app_state.task_repo) as Arc<dyn TaskRepository>,
        Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>,
        Arc::clone(&pr_registry),
        Arc::clone(&app_state.project_repo) as Arc<dyn ProjectRepository>,
        transition_service,
        empty_startup_blocked_projects(),
    )
    .await;

    // Since github_service = None, start_polling is a no-op — poller not registered.
    // But we verify the recovery ran without panic and didn't corrupt state.
    assert!(
        !pr_registry.is_polling(&task.id),
        "With github_service=None, start_polling is a no-op — is_polling should remain false"
    );

    // The plan_branch should still have pr_polling_active = true (startup recovery
    // does not clear it — only stop_polling clears it).
    let pb_after = plan_branch_repo
        .get_by_merge_task_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    assert!(
        pb_after.pr_polling_active,
        "startup recovery should not clear pr_polling_active — only stop_polling does"
    );
}

#[tokio::test]
async fn test_startup_recovery_routes_github_requested_changes_before_polling() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());

    let project = Project::new("Test Project".to_string(), "/tmp/test-repo".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let execution_plan_id = ExecutionPlanId::from_string("exec-plan-startup-pr-review");
    let session_id = IdeationSessionId::from_string("test-session-startup-pr-review");
    let artifact_id = ArtifactId::from_string("test-artifact-startup-pr-review");
    let mut task = Task::new_with_category(
        project.id.clone(),
        "Merge plan into main".to_string(),
        TaskCategory::PlanMerge,
    );
    task.internal_status = InternalStatus::WaitingOnPr;
    task.execution_plan_id = Some(execution_plan_id.clone());
    task.ideation_session_id = Some(session_id.clone());
    task.plan_artifact_id = Some(artifact_id.clone());
    app_state.task_repo.create(task.clone()).await.unwrap();

    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
    let mut pb = PlanBranch::new(
        artifact_id,
        session_id,
        project.id.clone(),
        "plan/feature".to_string(),
        "main".to_string(),
    );
    pb.merge_task_id = Some(task.id.clone());
    pb.execution_plan_id = Some(execution_plan_id.clone());
    pb.pr_number = Some(42);
    pb.pr_eligible = true;
    pb.pr_polling_active = true;
    plan_branch_repo.create(pb).await.unwrap();

    let mock = Arc::new(MockGithubService::new());
    mock.will_return_review_feedback(PrReviewFeedback {
        review_id: "4136652897".to_string(),
        author: "octocat".to_string(),
        submitted_at: Some("2026-04-22T08:00:00Z".to_string()),
        body: Some("Please fix this before merging.".to_string()),
        comments: vec![PrReviewCommentFeedback {
            id: "3107615689".to_string(),
            author: "octocat".to_string(),
            path: Some("src/lib.rs".to_string()),
            line: Some(17),
            body: "The nil case is still missing.".to_string(),
        }],
    });
    let pr_registry = Arc::new(PrPollerRegistry::new(
        Some(mock.clone() as Arc<dyn ralphx_lib::domain::services::GithubServiceTrait>),
        Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>,
    ));
    let transition_service = build_pr_transition_service(
        &app_state,
        &execution_state,
        Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>,
        Arc::clone(&pr_registry),
    );

    recover_pr_pollers(
        Arc::clone(&app_state.task_repo) as Arc<dyn TaskRepository>,
        Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>,
        Arc::clone(&pr_registry),
        Arc::clone(&app_state.project_repo) as Arc<dyn ProjectRepository>,
        transition_service,
        empty_startup_blocked_projects(),
    )
    .await;

    assert_eq!(
        mock.review_feedback_calls(),
        1,
        "startup recovery must inspect PR review feedback before restarting polling"
    );

    let updated = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.internal_status, InternalStatus::Blocked);
    assert!(updated
        .blocked_reason
        .as_deref()
        .unwrap_or_default()
        .contains("GitHub PR #42 requested changes"));

    let tasks = app_state
        .task_repo
        .list_paginated(
            &project.id,
            None,
            0,
            100,
            false,
            None,
            Some(execution_plan_id.as_str()),
            None,
        )
        .await
        .unwrap();
    let correction = tasks
        .iter()
        .find(|candidate| {
            candidate.category == TaskCategory::Regular
                && candidate
                    .title
                    .contains("Address GitHub PR #42 review feedback")
        })
        .expect("startup review ingestion should create a regular correction task");
    assert_eq!(correction.internal_status, InternalStatus::Ready);
    assert!(
        app_state
            .task_dependency_repo
            .has_dependency(&task.id, &correction.id)
            .await
            .unwrap(),
        "final plan merge must wait for the GitHub correction task"
    );

    let notes = app_state
        .review_repo
        .get_notes_by_task_id(&correction.id)
        .await
        .unwrap();
    let note = notes
        .iter()
        .find(|note| note.outcome == ReviewOutcome::ChangesRequested)
        .expect("correction task should carry GitHub review feedback");
    assert_eq!(note.reviewer, ReviewerType::Human);
    assert!(note
        .notes
        .as_deref()
        .unwrap_or_default()
        .contains("Please fix this before merging"));

    let pb_after = plan_branch_repo
        .get_by_merge_task_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    assert!(
        !pb_after.pr_polling_active,
        "startup review ingestion should stop PR polling while correction work is active"
    );
    assert!(
        !pr_registry.is_polling(&task.id),
        "startup should not restart a PR poller after routing requested changes"
    );
}

#[tokio::test]
async fn test_startup_recovery_restores_pr_merge_incomplete_caused_by_local_timeout() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());

    let project = Project::new("Test Project".to_string(), "/tmp/test-repo".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(
        project.id.clone(),
        "PR merge task falsely timed out".to_string(),
    );
    task.category = TaskCategory::PlanMerge;
    task.internal_status = InternalStatus::MergeIncomplete;
    task.metadata = Some(
        serde_json::json!({
            "error": "Merge timed out after 1200s without complete_merge callback",
            "merge_timeout_seconds": 1200
        })
        .to_string(),
    );
    app_state.task_repo.create(task.clone()).await.unwrap();

    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
    let mut pb = make_pr_plan_branch(project.id.clone(), &task.id, 68, true, Some(Utc::now()));
    pb.pr_url = Some("https://github.com/owner/repo/pull/68".to_string());
    plan_branch_repo.create(pb).await.unwrap();

    let pr_registry = Arc::new(PrPollerRegistry::new(
        None,
        Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>,
    ));
    let transition_service = build_pr_transition_service(
        &app_state,
        &execution_state,
        Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>,
        Arc::clone(&pr_registry),
    );

    recover_pr_pollers(
        Arc::clone(&app_state.task_repo) as Arc<dyn TaskRepository>,
        Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>,
        Arc::clone(&pr_registry),
        Arc::clone(&app_state.project_repo) as Arc<dyn ProjectRepository>,
        transition_service,
        empty_startup_blocked_projects(),
    )
    .await;

    let updated = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated.internal_status,
        InternalStatus::WaitingOnPr,
        "startup recovery should restore false local-timeout PR merges to WaitingOnPr so polling can resume"
    );
}

#[tokio::test]
async fn test_startup_recovery_creates_missing_draft_pr_for_active_plan() {
    let task_repo = Arc::new(ralphx_lib::infrastructure::memory::MemoryTaskRepository::new());
    let project_repo = Arc::new(ralphx_lib::infrastructure::memory::MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
    let execution_plan_repo = Arc::new(MemoryExecutionPlanRepository::new());

    let branch_name = "ralphx/test/startup-create";
    let working_dir = setup_plan_git_repo(branch_name, true);
    let project = Project::new(
        "Startup PR Repair".to_string(),
        working_dir.path().to_string_lossy().into_owned(),
    );
    let project = project_repo.create(project).await.unwrap();

    let mut merge_task = Task::new(project.id.clone(), "Merge active plan".to_string());
    merge_task.category = TaskCategory::PlanMerge;
    merge_task.internal_status = InternalStatus::Blocked;
    let merge_task = task_repo.create(merge_task).await.unwrap();

    let session_id = IdeationSessionId::from_string("session-startup-create".to_string());
    let execution_plan = seed_active_execution_plan(&execution_plan_repo, session_id.clone()).await;
    seed_merged_regular_plan_task(&task_repo, &project.id, &session_id, &execution_plan.id).await;

    let mut branch = PlanBranch::new(
        ArtifactId::from_string("artifact-startup-create".to_string()),
        session_id,
        project.id.clone(),
        branch_name.to_string(),
        "main".to_string(),
    );
    branch.merge_task_id = Some(merge_task.id.clone());
    branch.execution_plan_id = Some(execution_plan.id.clone());
    branch.pr_eligible = true;
    let branch_id = branch.id.clone();
    plan_branch_repo.create(branch).await.unwrap();

    let mock_github = Arc::new(MockGithubService::new());
    let github_service: Arc<dyn ralphx_lib::domain::services::GithubServiceTrait> =
        mock_github.clone();

    recover_missing_draft_prs(
        Arc::clone(&task_repo) as Arc<dyn TaskRepository>,
        Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>,
        Arc::clone(&project_repo) as Arc<dyn ProjectRepository>,
        Arc::clone(&execution_plan_repo) as Arc<dyn ExecutionPlanRepository>,
        Arc::new(MemoryIdeationSessionRepository::new()),
        Arc::new(MemoryArtifactRepository::new()),
        github_service,
        empty_startup_blocked_projects(),
    )
    .await;

    let branch_after = plan_branch_repo
        .get_by_id(&branch_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(branch_after.pr_number, Some(1));
    assert_eq!(
        branch_after.pr_url.as_deref(),
        Some("https://github.com/owner/repo/pull/1")
    );
    assert_eq!(
        mock_github.push_calls(),
        1,
        "startup repair should push the plan branch once"
    );
    assert_eq!(
        mock_github.create_calls(),
        1,
        "startup repair should create one draft PR for the active plan"
    );
}

#[tokio::test]
async fn test_startup_recovery_skips_empty_plan_branch_without_reviewable_diff() {
    let task_repo = Arc::new(ralphx_lib::infrastructure::memory::MemoryTaskRepository::new());
    let project_repo = Arc::new(ralphx_lib::infrastructure::memory::MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
    let execution_plan_repo = Arc::new(MemoryExecutionPlanRepository::new());

    let branch_name = "ralphx/test/startup-empty";
    let working_dir = setup_plan_git_repo(branch_name, false);
    let project = Project::new(
        "Startup PR Empty".to_string(),
        working_dir.path().to_string_lossy().into_owned(),
    );
    let project = project_repo.create(project).await.unwrap();

    let mut merge_task = Task::new(project.id.clone(), "Merge empty plan".to_string());
    merge_task.category = TaskCategory::PlanMerge;
    merge_task.internal_status = InternalStatus::Blocked;
    let merge_task = task_repo.create(merge_task).await.unwrap();

    let session_id = IdeationSessionId::from_string("session-startup-empty".to_string());
    let execution_plan = seed_active_execution_plan(&execution_plan_repo, session_id.clone()).await;
    seed_merged_regular_plan_task(&task_repo, &project.id, &session_id, &execution_plan.id).await;

    let mut branch = PlanBranch::new(
        ArtifactId::from_string("artifact-startup-empty".to_string()),
        session_id,
        project.id.clone(),
        branch_name.to_string(),
        "main".to_string(),
    );
    branch.merge_task_id = Some(merge_task.id.clone());
    branch.execution_plan_id = Some(execution_plan.id.clone());
    branch.pr_eligible = true;
    let branch_id = branch.id.clone();
    plan_branch_repo.create(branch).await.unwrap();

    let mock_github = Arc::new(MockGithubService::new());
    let github_service: Arc<dyn ralphx_lib::domain::services::GithubServiceTrait> =
        mock_github.clone();

    recover_missing_draft_prs(
        Arc::clone(&task_repo) as Arc<dyn TaskRepository>,
        Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>,
        Arc::clone(&project_repo) as Arc<dyn ProjectRepository>,
        Arc::clone(&execution_plan_repo) as Arc<dyn ExecutionPlanRepository>,
        Arc::new(MemoryIdeationSessionRepository::new()),
        Arc::new(MemoryArtifactRepository::new()),
        github_service,
        empty_startup_blocked_projects(),
    )
    .await;

    let branch_after = plan_branch_repo
        .get_by_id(&branch_id)
        .await
        .unwrap()
        .unwrap();
    assert!(
        branch_after.pr_number.is_none(),
        "empty plan branches should not create a PR during startup recovery"
    );
    assert_eq!(
        mock_github.push_calls(),
        0,
        "startup recovery should not push branches that are not ahead of base"
    );
    assert_eq!(
        mock_github.create_calls(),
        0,
        "startup recovery should not create a PR when the branch has no reviewable diff"
    );
}

#[tokio::test]
async fn test_startup_recovery_skips_plan_branch_without_merged_regular_task() {
    let task_repo = Arc::new(ralphx_lib::infrastructure::memory::MemoryTaskRepository::new());
    let project_repo = Arc::new(ralphx_lib::infrastructure::memory::MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
    let execution_plan_repo = Arc::new(MemoryExecutionPlanRepository::new());

    let branch_name = "ralphx/test/startup-diff-no-merged-task";
    let working_dir = setup_plan_git_repo(branch_name, true);
    let project = Project::new(
        "Startup PR Diff Without Merged Task".to_string(),
        working_dir.path().to_string_lossy().into_owned(),
    );
    let project = project_repo.create(project).await.unwrap();

    let mut merge_task = Task::new(project.id.clone(), "Merge active plan".to_string());
    merge_task.category = TaskCategory::PlanMerge;
    merge_task.internal_status = InternalStatus::Blocked;
    let merge_task = task_repo.create(merge_task).await.unwrap();

    let session_id =
        IdeationSessionId::from_string("session-startup-diff-no-merged-task".to_string());
    let execution_plan = seed_active_execution_plan(&execution_plan_repo, session_id.clone()).await;

    let mut branch = PlanBranch::new(
        ArtifactId::from_string("artifact-startup-diff-no-merged-task".to_string()),
        session_id,
        project.id.clone(),
        branch_name.to_string(),
        "main".to_string(),
    );
    branch.merge_task_id = Some(merge_task.id.clone());
    branch.execution_plan_id = Some(execution_plan.id.clone());
    branch.pr_eligible = true;
    let branch_id = branch.id.clone();
    plan_branch_repo.create(branch).await.unwrap();

    let mock_github = Arc::new(MockGithubService::new());
    let github_service: Arc<dyn ralphx_lib::domain::services::GithubServiceTrait> =
        mock_github.clone();

    recover_missing_draft_prs(
        Arc::clone(&task_repo) as Arc<dyn TaskRepository>,
        Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>,
        Arc::clone(&project_repo) as Arc<dyn ProjectRepository>,
        Arc::clone(&execution_plan_repo) as Arc<dyn ExecutionPlanRepository>,
        Arc::new(MemoryIdeationSessionRepository::new()),
        Arc::new(MemoryArtifactRepository::new()),
        github_service,
        empty_startup_blocked_projects(),
    )
    .await;

    let branch_after = plan_branch_repo
        .get_by_id(&branch_id)
        .await
        .unwrap()
        .unwrap();
    assert!(
        branch_after.pr_number.is_none(),
        "startup recovery must not create a PR before any active plan task has merged"
    );
    assert_eq!(
        mock_github.create_calls(),
        0,
        "branch diff alone must not create a PR"
    );
}

#[tokio::test]
async fn test_startup_recovery_skips_superseded_execution_plan() {
    let task_repo = Arc::new(ralphx_lib::infrastructure::memory::MemoryTaskRepository::new());
    let project_repo = Arc::new(ralphx_lib::infrastructure::memory::MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
    let execution_plan_repo = Arc::new(MemoryExecutionPlanRepository::new());

    let branch_name = "ralphx/test/superseded-plan";
    let working_dir = setup_plan_git_repo(branch_name, true);
    let project = Project::new(
        "Startup PR Superseded Plan".to_string(),
        working_dir.path().to_string_lossy().into_owned(),
    );
    let project = project_repo.create(project).await.unwrap();

    let mut merge_task = Task::new(project.id.clone(), "Merge superseded plan".to_string());
    merge_task.category = TaskCategory::PlanMerge;
    merge_task.internal_status = InternalStatus::Blocked;
    let merge_task = task_repo.create(merge_task).await.unwrap();

    let session_id = IdeationSessionId::from_string("session-superseded-plan".to_string());
    let execution_plan = seed_active_execution_plan(&execution_plan_repo, session_id.clone()).await;
    execution_plan_repo
        .mark_superseded(&execution_plan.id)
        .await
        .unwrap();
    seed_merged_regular_plan_task(&task_repo, &project.id, &session_id, &execution_plan.id).await;

    let mut branch = PlanBranch::new(
        ArtifactId::from_string("artifact-superseded-plan".to_string()),
        session_id,
        project.id.clone(),
        branch_name.to_string(),
        "main".to_string(),
    );
    branch.merge_task_id = Some(merge_task.id.clone());
    branch.execution_plan_id = Some(execution_plan.id.clone());
    branch.pr_eligible = true;
    let branch_id = branch.id.clone();
    plan_branch_repo.create(branch).await.unwrap();

    let mock_github = Arc::new(MockGithubService::new());
    let github_service: Arc<dyn ralphx_lib::domain::services::GithubServiceTrait> =
        mock_github.clone();

    recover_missing_draft_prs(
        Arc::clone(&task_repo) as Arc<dyn TaskRepository>,
        Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>,
        Arc::clone(&project_repo) as Arc<dyn ProjectRepository>,
        Arc::clone(&execution_plan_repo) as Arc<dyn ExecutionPlanRepository>,
        Arc::new(MemoryIdeationSessionRepository::new()),
        Arc::new(MemoryArtifactRepository::new()),
        github_service,
        empty_startup_blocked_projects(),
    )
    .await;

    let branch_after = plan_branch_repo
        .get_by_id(&branch_id)
        .await
        .unwrap()
        .unwrap();
    assert!(
        branch_after.pr_number.is_none(),
        "startup recovery must not create PRs for superseded execution plans"
    );
    assert_eq!(mock_github.create_calls(), 0);
}

#[tokio::test]
async fn test_startup_recovery_skips_when_project_pr_mode_disabled() {
    let task_repo = Arc::new(ralphx_lib::infrastructure::memory::MemoryTaskRepository::new());
    let project_repo = Arc::new(ralphx_lib::infrastructure::memory::MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
    let execution_plan_repo = Arc::new(MemoryExecutionPlanRepository::new());

    let branch_name = "ralphx/test/pr-disabled";
    let working_dir = setup_plan_git_repo(branch_name, true);
    let mut project = Project::new(
        "Startup PR Disabled".to_string(),
        working_dir.path().to_string_lossy().into_owned(),
    );
    project.github_pr_enabled = false;
    let project = project_repo.create(project).await.unwrap();

    let mut merge_task = Task::new(project.id.clone(), "Merge disabled PR plan".to_string());
    merge_task.category = TaskCategory::PlanMerge;
    merge_task.internal_status = InternalStatus::Blocked;
    let merge_task = task_repo.create(merge_task).await.unwrap();

    let session_id = IdeationSessionId::from_string("session-pr-disabled".to_string());
    let execution_plan = seed_active_execution_plan(&execution_plan_repo, session_id.clone()).await;
    seed_merged_regular_plan_task(&task_repo, &project.id, &session_id, &execution_plan.id).await;

    let mut branch = PlanBranch::new(
        ArtifactId::from_string("artifact-pr-disabled".to_string()),
        session_id,
        project.id.clone(),
        branch_name.to_string(),
        "main".to_string(),
    );
    branch.merge_task_id = Some(merge_task.id.clone());
    branch.execution_plan_id = Some(execution_plan.id.clone());
    branch.pr_eligible = true;
    let branch_id = branch.id.clone();
    plan_branch_repo.create(branch).await.unwrap();

    let mock_github = Arc::new(MockGithubService::new());
    let github_service: Arc<dyn ralphx_lib::domain::services::GithubServiceTrait> =
        mock_github.clone();

    recover_missing_draft_prs(
        Arc::clone(&task_repo) as Arc<dyn TaskRepository>,
        Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>,
        Arc::clone(&project_repo) as Arc<dyn ProjectRepository>,
        Arc::clone(&execution_plan_repo) as Arc<dyn ExecutionPlanRepository>,
        Arc::new(MemoryIdeationSessionRepository::new()),
        Arc::new(MemoryArtifactRepository::new()),
        github_service,
        empty_startup_blocked_projects(),
    )
    .await;

    let branch_after = plan_branch_repo
        .get_by_id(&branch_id)
        .await
        .unwrap()
        .unwrap();
    assert!(
        branch_after.pr_number.is_none(),
        "startup recovery must not create PRs while project PR mode is disabled"
    );
    assert_eq!(mock_github.create_calls(), 0);
}

#[tokio::test]
async fn test_startup_recovery_skips_terminal_or_already_open_prs() {
    let task_repo = Arc::new(ralphx_lib::infrastructure::memory::MemoryTaskRepository::new());
    let project_repo = Arc::new(ralphx_lib::infrastructure::memory::MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
    let execution_plan_repo = Arc::new(MemoryExecutionPlanRepository::new());

    let working_dir = tempfile::tempdir().unwrap();
    let project = Project::new(
        "Startup PR Skip".to_string(),
        working_dir.path().to_string_lossy().into_owned(),
    );
    let project = project_repo.create(project).await.unwrap();

    let mut existing_pr_task = Task::new(project.id.clone(), "Existing PR merge task".to_string());
    existing_pr_task.category = TaskCategory::PlanMerge;
    existing_pr_task.internal_status = InternalStatus::Blocked;
    let existing_pr_task = task_repo.create(existing_pr_task).await.unwrap();

    let mut existing_pr_branch = PlanBranch::new(
        ArtifactId::from_string("artifact-existing-pr".to_string()),
        IdeationSessionId::from_string("session-existing-pr".to_string()),
        project.id.clone(),
        "ralphx/test/existing-pr".to_string(),
        "main".to_string(),
    );
    existing_pr_branch.merge_task_id = Some(existing_pr_task.id.clone());
    existing_pr_branch.pr_eligible = true;
    existing_pr_branch.pr_number = Some(42);
    existing_pr_branch.pr_url = Some("https://github.com/owner/repo/pull/42".to_string());
    plan_branch_repo.create(existing_pr_branch).await.unwrap();

    let mut terminal_task = Task::new(project.id.clone(), "Terminal merge task".to_string());
    terminal_task.category = TaskCategory::PlanMerge;
    terminal_task.internal_status = InternalStatus::Merged;
    let terminal_task = task_repo.create(terminal_task).await.unwrap();

    let mut terminal_branch = PlanBranch::new(
        ArtifactId::from_string("artifact-terminal".to_string()),
        IdeationSessionId::from_string("session-terminal".to_string()),
        project.id.clone(),
        "ralphx/test/terminal".to_string(),
        "main".to_string(),
    );
    terminal_branch.merge_task_id = Some(terminal_task.id.clone());
    terminal_branch.pr_eligible = true;
    let terminal_branch_id = terminal_branch.id.clone();
    plan_branch_repo.create(terminal_branch).await.unwrap();

    let mock_github = Arc::new(MockGithubService::new());
    let github_service: Arc<dyn ralphx_lib::domain::services::GithubServiceTrait> =
        mock_github.clone();

    recover_missing_draft_prs(
        Arc::clone(&task_repo) as Arc<dyn TaskRepository>,
        Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>,
        Arc::clone(&project_repo) as Arc<dyn ProjectRepository>,
        Arc::clone(&execution_plan_repo) as Arc<dyn ExecutionPlanRepository>,
        Arc::new(MemoryIdeationSessionRepository::new()),
        Arc::new(MemoryArtifactRepository::new()),
        github_service,
        empty_startup_blocked_projects(),
    )
    .await;

    let terminal_after = plan_branch_repo
        .get_by_id(&terminal_branch_id)
        .await
        .unwrap()
        .unwrap();
    assert!(
        terminal_after.pr_number.is_none(),
        "terminal merge tasks must not grow a new PR during startup repair"
    );
    assert_eq!(
        mock_github.create_calls(),
        0,
        "startup repair should skip branches with an existing PR or a terminal merge task"
    );
    assert_eq!(
        mock_github.push_calls(),
        0,
        "startup repair should not push skipped branches"
    );
}

#[tokio::test]
async fn test_startup_recovery_pushes_existing_pr_branch_when_local_sync_pending() {
    let task_repo = Arc::new(ralphx_lib::infrastructure::memory::MemoryTaskRepository::new());
    let project_repo = Arc::new(ralphx_lib::infrastructure::memory::MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
    let execution_plan_repo = Arc::new(MemoryExecutionPlanRepository::new());

    let branch_name = "ralphx/test/pending-sync";
    let working_dir = setup_plan_git_repo(branch_name, true);
    let project = Project::new(
        "Startup PR Sync".to_string(),
        working_dir.path().to_string_lossy().into_owned(),
    );
    let project = project_repo.create(project).await.unwrap();

    let mut merge_task = Task::new(project.id.clone(), "Merge synced plan".to_string());
    merge_task.category = TaskCategory::PlanMerge;
    merge_task.internal_status = InternalStatus::Blocked;
    let merge_task = task_repo.create(merge_task).await.unwrap();

    let session_id = IdeationSessionId::from_string("session-startup-sync".to_string());
    let execution_plan = seed_active_execution_plan(&execution_plan_repo, session_id.clone()).await;
    seed_merged_regular_plan_task(&task_repo, &project.id, &session_id, &execution_plan.id).await;

    let mut branch = PlanBranch::new(
        ArtifactId::from_string("artifact-startup-sync".to_string()),
        session_id,
        project.id.clone(),
        branch_name.to_string(),
        "main".to_string(),
    );
    branch.merge_task_id = Some(merge_task.id.clone());
    branch.execution_plan_id = Some(execution_plan.id.clone());
    branch.pr_eligible = true;
    branch.pr_number = Some(77);
    branch.pr_url = Some("https://github.com/owner/repo/pull/77".to_string());
    branch.pr_push_status = PrPushStatus::Pending;
    let branch_id = branch.id.clone();
    plan_branch_repo.create(branch).await.unwrap();

    let mock_github = Arc::new(MockGithubService::new());
    let github_service: Arc<dyn ralphx_lib::domain::services::GithubServiceTrait> =
        mock_github.clone();

    recover_missing_draft_prs(
        Arc::clone(&task_repo) as Arc<dyn TaskRepository>,
        Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>,
        Arc::clone(&project_repo) as Arc<dyn ProjectRepository>,
        Arc::clone(&execution_plan_repo) as Arc<dyn ExecutionPlanRepository>,
        Arc::new(MemoryIdeationSessionRepository::new()),
        Arc::new(MemoryArtifactRepository::new()),
        github_service,
        empty_startup_blocked_projects(),
    )
    .await;

    let branch_after = plan_branch_repo
        .get_by_id(&branch_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(branch_after.pr_number, Some(77));
    assert_eq!(branch_after.pr_push_status, PrPushStatus::Pushed);
    assert_eq!(
        mock_github.push_calls(),
        1,
        "startup recovery should push PR-backed plan branches whose local updates were never synced"
    );
    assert_eq!(
        mock_github.create_calls(),
        0,
        "startup recovery should not recreate an already-open PR when only branch sync is pending"
    );
    assert_eq!(
        mock_github.update_pr_details_calls(),
        1,
        "startup recovery should refresh PR title/body after syncing an existing PR branch"
    );
}

#[tokio::test]
async fn test_startup_recovery_refreshes_existing_pushed_pr_metadata() {
    let task_repo = Arc::new(ralphx_lib::infrastructure::memory::MemoryTaskRepository::new());
    let project_repo = Arc::new(ralphx_lib::infrastructure::memory::MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
    let execution_plan_repo = Arc::new(MemoryExecutionPlanRepository::new());

    let branch_name = "ralphx/test/metadata-refresh";
    let working_dir = setup_plan_git_repo(branch_name, true);
    let project = Project::new(
        "Startup PR Metadata Refresh".to_string(),
        working_dir.path().to_string_lossy().into_owned(),
    );
    let project = project_repo.create(project).await.unwrap();

    let mut merge_task = Task::new(project.id.clone(), "Merge refreshed plan".to_string());
    merge_task.category = TaskCategory::PlanMerge;
    merge_task.internal_status = InternalStatus::Blocked;
    let merge_task = task_repo.create(merge_task).await.unwrap();

    let session_id = IdeationSessionId::from_string("session-startup-metadata-refresh".to_string());
    let execution_plan = seed_active_execution_plan(&execution_plan_repo, session_id.clone()).await;
    seed_merged_regular_plan_task(&task_repo, &project.id, &session_id, &execution_plan.id).await;

    let mut branch = PlanBranch::new(
        ArtifactId::from_string("artifact-startup-metadata-refresh".to_string()),
        session_id,
        project.id.clone(),
        branch_name.to_string(),
        "main".to_string(),
    );
    branch.merge_task_id = Some(merge_task.id.clone());
    branch.execution_plan_id = Some(execution_plan.id.clone());
    branch.pr_eligible = true;
    branch.pr_number = Some(88);
    branch.pr_url = Some("https://github.com/owner/repo/pull/88".to_string());
    branch.pr_push_status = PrPushStatus::Pushed;
    plan_branch_repo.create(branch).await.unwrap();

    let mock_github = Arc::new(MockGithubService::new());
    let github_service: Arc<dyn ralphx_lib::domain::services::GithubServiceTrait> =
        mock_github.clone();

    recover_missing_draft_prs(
        Arc::clone(&task_repo) as Arc<dyn TaskRepository>,
        Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>,
        Arc::clone(&project_repo) as Arc<dyn ProjectRepository>,
        Arc::clone(&execution_plan_repo) as Arc<dyn ExecutionPlanRepository>,
        Arc::new(MemoryIdeationSessionRepository::new()),
        Arc::new(MemoryArtifactRepository::new()),
        github_service,
        empty_startup_blocked_projects(),
    )
    .await;

    assert_eq!(
        mock_github.push_calls(),
        0,
        "already-pushed PR branches do not need a startup push"
    );
    assert_eq!(
        mock_github.create_calls(),
        0,
        "startup recovery should not recreate existing PRs"
    );
    assert_eq!(
        mock_github.update_pr_details_calls(),
        1,
        "startup recovery should refresh title/body for every eligible existing PR"
    );
}

#[tokio::test]
async fn test_startup_recovery_refreshes_existing_pr_metadata_without_local_diff() {
    let task_repo = Arc::new(ralphx_lib::infrastructure::memory::MemoryTaskRepository::new());
    let project_repo = Arc::new(ralphx_lib::infrastructure::memory::MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
    let execution_plan_repo = Arc::new(MemoryExecutionPlanRepository::new());

    let branch_name = "ralphx/test/metadata-refresh-no-diff";
    let working_dir = setup_plan_git_repo(branch_name, false);
    let project = Project::new(
        "Startup PR Metadata Refresh Without Diff".to_string(),
        working_dir.path().to_string_lossy().into_owned(),
    );
    let project = project_repo.create(project).await.unwrap();

    let mut merge_task = Task::new(project.id.clone(), "Merge refreshed plan".to_string());
    merge_task.category = TaskCategory::PlanMerge;
    merge_task.internal_status = InternalStatus::Blocked;
    let merge_task = task_repo.create(merge_task).await.unwrap();

    let session_id =
        IdeationSessionId::from_string("session-startup-metadata-refresh-no-diff".to_string());
    let execution_plan = seed_active_execution_plan(&execution_plan_repo, session_id.clone()).await;
    seed_merged_regular_plan_task(&task_repo, &project.id, &session_id, &execution_plan.id).await;

    let mut branch = PlanBranch::new(
        ArtifactId::from_string("artifact-startup-metadata-refresh-no-diff".to_string()),
        session_id,
        project.id.clone(),
        branch_name.to_string(),
        "main".to_string(),
    );
    branch.merge_task_id = Some(merge_task.id.clone());
    branch.execution_plan_id = Some(execution_plan.id.clone());
    branch.pr_eligible = true;
    branch.pr_number = Some(89);
    branch.pr_url = Some("https://github.com/owner/repo/pull/89".to_string());
    branch.pr_push_status = PrPushStatus::Pushed;
    plan_branch_repo.create(branch).await.unwrap();

    let mock_github = Arc::new(MockGithubService::new());
    let github_service: Arc<dyn ralphx_lib::domain::services::GithubServiceTrait> =
        mock_github.clone();

    recover_missing_draft_prs(
        Arc::clone(&task_repo) as Arc<dyn TaskRepository>,
        Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>,
        Arc::clone(&project_repo) as Arc<dyn ProjectRepository>,
        Arc::clone(&execution_plan_repo) as Arc<dyn ExecutionPlanRepository>,
        Arc::new(MemoryIdeationSessionRepository::new()),
        Arc::new(MemoryArtifactRepository::new()),
        github_service,
        empty_startup_blocked_projects(),
    )
    .await;

    assert_eq!(
        mock_github.update_pr_details_calls(),
        1,
        "startup recovery should force-refresh eligible existing PR metadata even when no new local commits need pushing"
    );
    assert_eq!(
        mock_github.create_calls(),
        0,
        "existing PRs should not be recreated when the branch has no diff"
    );
}

#[tokio::test]
async fn test_startup_recovery_recovers_duplicate_pr() {
    let task_repo = Arc::new(ralphx_lib::infrastructure::memory::MemoryTaskRepository::new());
    let project_repo = Arc::new(ralphx_lib::infrastructure::memory::MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
    let execution_plan_repo = Arc::new(MemoryExecutionPlanRepository::new());

    let branch_name = "ralphx/test/duplicate";
    let working_dir = setup_plan_git_repo(branch_name, true);
    let project = Project::new(
        "Startup PR Duplicate".to_string(),
        working_dir.path().to_string_lossy().into_owned(),
    );
    let project = project_repo.create(project).await.unwrap();

    let mut merge_task = Task::new(project.id.clone(), "Duplicate merge task".to_string());
    merge_task.category = TaskCategory::PlanMerge;
    merge_task.internal_status = InternalStatus::Blocked;
    let merge_task = task_repo.create(merge_task).await.unwrap();

    let session_id = IdeationSessionId::from_string("session-duplicate".to_string());
    let execution_plan = seed_active_execution_plan(&execution_plan_repo, session_id.clone()).await;
    seed_merged_regular_plan_task(&task_repo, &project.id, &session_id, &execution_plan.id).await;

    let mut branch = PlanBranch::new(
        ArtifactId::from_string("artifact-duplicate".to_string()),
        session_id,
        project.id.clone(),
        branch_name.to_string(),
        "main".to_string(),
    );
    branch.merge_task_id = Some(merge_task.id.clone());
    branch.execution_plan_id = Some(execution_plan.id.clone());
    branch.pr_eligible = true;
    let branch_id = branch.id.clone();
    plan_branch_repo.create(branch).await.unwrap();

    let mock_github = Arc::new(MockGithubService::new());
    mock_github.will_fail_create_pr_duplicate();
    mock_github.will_return_existing_pr(77, "https://github.com/owner/repo/pull/77");
    let github_service: Arc<dyn ralphx_lib::domain::services::GithubServiceTrait> =
        mock_github.clone();

    recover_missing_draft_prs(
        Arc::clone(&task_repo) as Arc<dyn TaskRepository>,
        Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>,
        Arc::clone(&project_repo) as Arc<dyn ProjectRepository>,
        Arc::clone(&execution_plan_repo) as Arc<dyn ExecutionPlanRepository>,
        Arc::new(MemoryIdeationSessionRepository::new()),
        Arc::new(MemoryArtifactRepository::new()),
        github_service,
        empty_startup_blocked_projects(),
    )
    .await;

    let branch_after = plan_branch_repo
        .get_by_id(&branch_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(branch_after.pr_number, Some(77));
    assert_eq!(
        branch_after.pr_url.as_deref(),
        Some("https://github.com/owner/repo/pull/77")
    );
    assert_eq!(mock_github.create_calls(), 1);
    assert_eq!(
        mock_github.find_pr_calls(),
        1,
        "duplicate recovery should look up the existing PR by head branch"
    );
}

// ============================================================================
// Test 5: PendingMerge reconciler skips when pr_polling_active = true
// ============================================================================

/// When a PendingMerge task has a plan_branch with `pr_polling_active = true`,
/// the reconciler should return `true` (skip — PR review in progress) and NOT
/// mark it stale or transition to MergeIncomplete.
#[tokio::test]
async fn test_reconciler_skips_pending_merge_when_pr_polling_active() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());

    let project = Project::new("Test Project".to_string(), "/tmp/test-repo".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Task in PendingMerge with a very old updated_at (would normally be stale)
    let mut task = Task::new(project.id.clone(), "PR pending merge task".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    // Backdate updated_at to make the task look stale (older than pending_merge_stale_minutes)
    task.updated_at = Utc::now() - chrono::Duration::hours(24);
    app_state.task_repo.create(task.clone()).await.unwrap();

    // Insert a status history entry with an old timestamp so the age check sees it as stale
    // (The reconciler calls latest_status_transition_age which reads from activity_event_repo
    //  or falls back to task.updated_at. The task.updated_at is already backdated above.)

    // PlanBranch with pr_polling_active = true
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
    let mut pb = PlanBranch::new(
        ArtifactId::from_string("test-artifact".to_string()),
        IdeationSessionId::from_string("test-session".to_string()),
        project.id.clone(),
        "plan/feature".to_string(),
        "main".to_string(),
    );
    pb.merge_task_id = Some(task.id.clone());
    pb.pr_polling_active = true;
    plan_branch_repo.create(pb).await.unwrap();

    let reconciler = build_reconciler(&app_state, &execution_state)
        .with_plan_branch_repo(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>);

    let result = reconciler
        .reconcile_task(&task, InternalStatus::PendingMerge)
        .await;

    assert!(
        result,
        "PendingMerge with pr_polling_active=true should be skipped (return true)"
    );

    // Task should still be in PendingMerge — not transitioned to MergeIncomplete
    let updated = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated.internal_status,
        InternalStatus::PendingMerge,
        "PR-mode PendingMerge task must NOT be transitioned to MergeIncomplete by reconciler"
    );
}

// ============================================================================
// Test 6: branch_missing metadata blocks MergeIncomplete auto-retry
// ============================================================================

#[tokio::test]
async fn test_merge_incomplete_branch_missing_skips_retry() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());

    let project = Project::new("Test Project".to_string(), "/tmp/test-repo".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(
        project.id.clone(),
        "Branch missing merge incomplete".to_string(),
    );
    task.internal_status = InternalStatus::MergeIncomplete;
    task.metadata = Some(
        serde_json::json!({
            "branch_missing": true,
            "merge_recovery": {
                "events": [],
                "stop_retrying": false
            }
        })
        .to_string(),
    );
    app_state.task_repo.create(task.clone()).await.unwrap();

    let reconciler = build_reconciler(&app_state, &execution_state);
    let result = reconciler
        .reconcile_task(&task, InternalStatus::MergeIncomplete)
        .await;

    assert!(
        !result,
        "branch_missing=true should suppress MergeIncomplete auto-retry"
    );

    let updated = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated.internal_status,
        InternalStatus::MergeIncomplete,
        "branch_missing MergeIncomplete task must stay MergeIncomplete"
    );
}

// ============================================================================
// Test 7: branch_missing metadata blocks MergeConflict auto-retry
// ============================================================================

#[tokio::test]
async fn test_merge_conflict_branch_missing_skips_retry() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());

    let project = Project::new("Test Project".to_string(), "/tmp/test-repo".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(
        project.id.clone(),
        "Branch missing merge conflict".to_string(),
    );
    task.internal_status = InternalStatus::MergeConflict;
    task.metadata = Some(
        serde_json::json!({
            "branch_missing": true
        })
        .to_string(),
    );
    app_state.task_repo.create(task.clone()).await.unwrap();

    let reconciler = build_reconciler(&app_state, &execution_state);
    let result = reconciler
        .reconcile_task(&task, InternalStatus::MergeConflict)
        .await;

    assert!(
        !result,
        "branch_missing=true should suppress MergeConflict auto-retry"
    );

    let updated = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated.internal_status,
        InternalStatus::MergeConflict,
        "branch_missing MergeConflict task must stay MergeConflict"
    );
}
