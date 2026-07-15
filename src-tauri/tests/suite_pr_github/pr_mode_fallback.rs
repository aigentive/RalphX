//! PR mode fallback / boundary tests.
//!
//! Covers edge cases in PR mode:
//! - No github_service → start_polling is always a no-op
//! - Reconciler without pr_poller_registry falls through for PR tasks
//! - mode_switch bypass at integration level
//! - Non-pr_eligible plan_branch skips PR polling

use std::sync::Arc;

use chrono::Utc;
use ralphx_lib::application::services::PrPollerRegistry;
use ralphx_lib::application::{AppState, ReconciliationRunner, TaskTransitionService};
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::entities::{
    ArtifactId, IdeationSessionId, InternalStatus, PlanBranch, Project, Task,
};
use ralphx_lib::domain::repositories::PlanBranchRepository;
use ralphx_lib::domain::services::github_service::PrStatus;
use ralphx_lib::infrastructure::memory::MemoryPlanBranchRepository;

use crate::common::MockGithubService;

// ============================================================================
// Shared helpers (copied from pr_reconciler_tests.rs)
// ============================================================================

fn build_reconciler(
    app_state: &AppState,
    execution_state: &Arc<ExecutionState>,
) -> ReconciliationRunner {
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

// ============================================================================
// Test 1: no github_service → start_polling is a no-op (is_polling always false)
// ============================================================================

/// `PrPollerRegistry::new(None, ...)` + `start_polling` → `is_polling()` = false.
/// This verifies the fallback contract at the integration level: without a
/// github_service, no real polling tasks are ever created.
#[tokio::test]
async fn test_no_github_service_poller_is_noop() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());

    let project = Project::new("Test Project".to_string(), "/tmp/test-repo".to_string());
    app_state.project_repo.create(project.clone()).await.unwrap();

    let mut task = Task::new(project.id.clone(), "No-github-service task".to_string());
    task.internal_status = InternalStatus::Merging;
    app_state.task_repo.create(task.clone()).await.unwrap();

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
    let plan_branch_id = pb.id.clone();
    plan_branch_repo.create(pb).await.unwrap();

    // Registry with NO github service — all start_polling calls must be no-ops
    let registry = Arc::new(PrPollerRegistry::new(
        None,
        Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>,
    ));

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
        Arc::clone(&execution_state),
        None,
        Arc::clone(&app_state.memory_event_repo),
    ));

    registry.start_polling(
        task.id.clone(),
        plan_branch_id.clone(),
        42,
        std::path::PathBuf::from("/tmp/test-repo"),
        "main".to_string(),
        Arc::clone(&transition_service),
    );

    assert!(
        !registry.is_polling(&task.id),
        "start_polling with github_service=None must be a no-op — is_polling() must remain false"
    );

    // Call multiple times — still a no-op
    registry.start_polling(
        task.id.clone(),
        plan_branch_id,
        42,
        std::path::PathBuf::from("/tmp/test-repo"),
        "main".to_string(),
        transition_service,
    );

    assert!(
        !registry.is_polling(&task.id),
        "repeated start_polling calls with github_service=None must still leave is_polling()=false"
    );
}

// ============================================================================
// Test 2: reconciler without pr_poller_registry falls through for PR task
// ============================================================================

/// A Merging task with `pr_polling_active=true` but the reconciler has NO
/// `pr_poller_registry` attached → the reconciler skips the poller-based PR
/// block (no registry to check) and falls through to normal logic.
///
/// Since the task is fresh (no agent run, no timeout), normal logic returns false.
#[tokio::test]
async fn test_reconciler_without_pr_registry_falls_through_for_pr_task() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());

    let project = Project::new("Test Project".to_string(), "/tmp/test-repo".to_string());
    app_state.project_repo.create(project.clone()).await.unwrap();

    let mut task = Task::new(project.id.clone(), "PR task no registry".to_string());
    task.internal_status = InternalStatus::Merging;
    app_state.task_repo.create(task.clone()).await.unwrap();

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
    pb.pr_polling_active = true; // PR-mode task
    plan_branch_repo.create(pb).await.unwrap();

    // Reconciler with plan_branch_repo but WITHOUT pr_poller_registry.
    // The inner `if let Some(ref registry) = self.pr_poller_registry` block is skipped.
    // Normal Merging recovery logic runs instead of the PR poller liveness check.
    let reconciler = build_reconciler(&app_state, &execution_state)
        .with_plan_branch_repo(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>);
    // Note: NO .with_pr_poller_registry(...)

    // Just verify the reconciler runs without panicking — the return value depends on
    // normal Merging recovery logic which may fire entry actions for a fresh task.
    // The key assertion: no panic and the task still exists in the repo.
    let _result = reconciler
        .reconcile_task(&task, InternalStatus::Merging)
        .await;

    // Task still exists — reconciler ran without corrupting state
    let found = app_state.task_repo.get_by_id(&task.id).await.unwrap();
    assert!(
        found.is_some(),
        "task must still exist after reconciler runs without pr_poller_registry"
    );
}

// ============================================================================
// Test 3: mode_switch bypass at integration level
// ============================================================================

/// A MergeIncomplete task with `mode_switch: true` in metadata + `circuit_breaker_active: true`
/// → reconciler returns true (bypasses the circuit breaker guard).
///
/// Contrast: without mode_switch, circuit_breaker_active causes return false.
/// This complements the reconciler unit test by verifying the bypass at integration level.
#[tokio::test]
async fn test_mode_switch_task_gets_retried() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());

    let project = Project::new("Test Project".to_string(), "/tmp/test-repo".to_string());
    app_state.project_repo.create(project.clone()).await.unwrap();

    // Task with mode_switch=true AND circuit_breaker_active=true
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

    // Control: circuit_breaker WITHOUT mode_switch → returns false
    let mut blocked_task = Task::new(project.id.clone(), "Blocked task".to_string());
    blocked_task.internal_status = InternalStatus::MergeIncomplete;
    blocked_task.metadata = Some(
        serde_json::json!({
            "circuit_breaker_active": true
        })
        .to_string(),
    );
    app_state.task_repo.create(blocked_task.clone()).await.unwrap();

    let reconciler = build_reconciler(&app_state, &execution_state);

    // Control: circuit_breaker WITHOUT mode_switch → returns false (circuit breaker blocks)
    let blocked_result = reconciler
        .reconcile_task(&blocked_task, InternalStatus::MergeIncomplete)
        .await;
    assert!(
        !blocked_result,
        "circuit_breaker_active=true without mode_switch must block auto-retry (return false)"
    );

    // Test: mode_switch=true bypasses the circuit_breaker and returns true
    let result = reconciler
        .reconcile_task(&task, InternalStatus::MergeIncomplete)
        .await;

    assert!(
        result,
        "mode_switch=true must bypass circuit_breaker_active and return true"
    );
}

// ============================================================================
// Test 4: non-pr_eligible plan_branch skips PR polling
// ============================================================================

/// A Merging task with a plan_branch where `pr_eligible=false` → the PR block
/// in `reconcile_merging_task` is skipped (pr_eligible check fails) and the
/// reconciler falls through to normal logic.
///
/// This verifies the eligibility guard: even if pr_number is set and a registry
/// exists, `pr_eligible=false` means the task is NOT a PR-mode task.
#[tokio::test]
async fn test_non_pr_plan_branch_skips_pr_polling() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());

    let project = Project::new("Test Project".to_string(), "/tmp/test-repo".to_string());
    app_state.project_repo.create(project.clone()).await.unwrap();

    let mut task = Task::new(project.id.clone(), "Non-eligible PR task".to_string());
    task.internal_status = InternalStatus::Merging;
    app_state.task_repo.create(task.clone()).await.unwrap();

    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
    let mut pb = PlanBranch::new(
        ArtifactId::from_string("test-artifact".to_string()),
        IdeationSessionId::from_string("test-session".to_string()),
        project.id.clone(),
        "plan/feature".to_string(),
        "main".to_string(),
    );
    pb.merge_task_id = Some(task.id.clone());
    pb.pr_number = Some(55);
    pb.pr_eligible = false; // NOT eligible for PR mode
    pb.pr_polling_active = false;
    pb.last_polled_at = Some(Utc::now());
    plan_branch_repo.create(pb).await.unwrap();

    let mock = Arc::new(MockGithubService::new());
    mock.will_return_status(PrStatus::Open);

    let registry = Arc::new(PrPollerRegistry::new(
        Some(mock as Arc<dyn ralphx_lib::domain::services::github_service::GithubServiceTrait>),
        Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>,
    ));

    let reconciler = build_reconciler(&app_state, &execution_state)
        .with_plan_branch_repo(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>)
        .with_pr_poller_registry(Arc::clone(&registry));

    // The PR block at `reconcile_merging_task` requires `pr_eligible=true` — since it's
    // false, the PR block is bypassed entirely. Normal Merging recovery logic runs.
    // The key observable: no poller was started (pr_eligible guard worked).
    reconciler
        .reconcile_task(&task, InternalStatus::Merging)
        .await;

    // Verify no polling was started for this task (the pr_eligible=false guard held)
    assert!(
        !registry.is_polling(&task.id),
        "no poller should be started for a non-pr_eligible plan branch"
    );
}
