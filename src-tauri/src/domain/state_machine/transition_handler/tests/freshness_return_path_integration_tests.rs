// Integration tests for freshness return-path routing.
//
// Verifies the full routing flow that occurs in handle_freshness_return_routing:
//   1. Task in Merging state with freshness metadata (origin = "executing")
//      → metadata cleared + task transitions to Ready (auto-transitions may follow)
//   2. Task in Merging state with freshness metadata (origin = "reviewing")
//      → metadata cleared + task transitions to PendingReview (auto-transitions to Reviewing)
//
// Uses AppState::new_test() + TaskTransitionService (the same components used in
// handle_freshness_return_routing) to verify the end-to-end state machine routing.
//
// Design rationale: handle_freshness_return_routing is a private fn in
// chat_service_merge.rs. These tests exercise the observable outcomes (status +
// metadata) via the same path: FreshnessMetadata::clear_from + transition_task.
// This matches Option C from the task spec (simplest, cleanest, most robust).
//
// Note on auto-transitions:
//   PendingReview → Reviewing fires automatically in the state machine.
//   The reviewing origin test therefore expects Reviewing as the final status.
// Note on metadata after entry actions:
//   on_enter(Ready) and on_enter(Reviewing) may update task metadata via
//   the state machine. The tests verify freshness metadata is cleared in the
//   repo BEFORE the transition executes, and verify the final task status.

use crate::application::AppState;
use crate::application::TaskTransitionService;
use crate::application::execution_state::ExecutionState;
use crate::domain::entities::{InternalStatus, Project, Task};
use crate::domain::services::{MemoryRunningAgentRegistry, MessageQueue};
use crate::domain::state_machine::transition_handler::freshness::FreshnessMetadata;
use serde_json::json;
use std::sync::Arc;

// ============================================================================
// Shared helpers
// ============================================================================

fn build_transition_service(app_state: &AppState) -> TaskTransitionService {
    let execution_state = Arc::new(ExecutionState::new());
    let message_queue = Arc::new(MessageQueue::new());
    let running_registry = Arc::new(MemoryRunningAgentRegistry::new());

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
        message_queue,
        running_registry,
        execution_state,
        None,
        Arc::clone(&app_state.memory_event_repo),
    )
}

/// Build a task in Merging state with freshness metadata set to the given origin.
///
/// `plan_update_conflict` and `source_update_conflict` are set based on which
/// conflict type is being simulated (mirrors real freshness routing scenarios).
fn make_merging_task_with_freshness(
    project_id: &crate::domain::entities::ProjectId,
    title: &str,
    origin_state: &str,
    plan_update_conflict: bool,
    source_update_conflict: bool,
) -> Task {
    let meta = json!({
        "branch_freshness_conflict": true,
        "freshness_origin_state": origin_state,
        "freshness_conflict_count": 1,
        "plan_update_conflict": plan_update_conflict,
        "source_update_conflict": source_update_conflict,
        "source_branch": "task/some-feature",
        "target_branch": "plan/my-plan",
        // A non-freshness key that should survive the metadata clear step
        "trigger_origin": "scheduler",
    });

    let mut task = Task::new(project_id.clone(), title.to_string());
    task.internal_status = InternalStatus::Merging;
    task.metadata = Some(meta.to_string());
    // Set worktree_path so on_enter(Reviewing) worktree guard passes on auto-transitions
    task.worktree_path = Some(std::env::temp_dir().to_string_lossy().to_string());
    task
}

// ============================================================================
// Test 1: executing origin → Ready
// ============================================================================

/// When a task was Executing and freshness detected a branch conflict,
/// the task was routed to Merging for the merger agent to resolve.
/// After the merger agent finishes, handle_freshness_return_routing fires:
///   1. freshness metadata is cleared from the task
///   2. task transitions to Ready (so the worker can resume execution)
///
/// This test verifies:
///   - Before the transition: freshness keys present in repo (sanity check)
///   - Metadata clear step: all FreshnessMetadata keys removed from repo
///   - After transition_task(Ready): task status is Ready (or a valid auto-transition target)
///   - Post-clear metadata in repo: freshness keys absent, non-freshness key preserved
#[tokio::test]
async fn test_executing_freshness_conflict_routes_to_ready_after_merge_resolution() {
    let app_state = AppState::new_test();

    // Wire a project
    let project = Project::new(
        "test-project".to_string(),
        "/tmp/test-freshness-executing".to_string(),
    );
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Create the task in Merging state with plan_update_conflict freshness metadata
    let task = make_merging_task_with_freshness(
        &project.id,
        "Freshness routing: executing → Ready",
        "executing",
        /*plan_update_conflict=*/ true,
        /*source_update_conflict=*/ false,
    );
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.unwrap();

    // --- Verify freshness metadata is present before the routing step ---
    {
        let stored = app_state
            .task_repo
            .get_by_id(&task_id)
            .await
            .unwrap()
            .expect("task must exist");
        assert_eq!(
            stored.internal_status,
            InternalStatus::Merging,
            "Task must start in Merging state"
        );
        let meta_val: serde_json::Value =
            serde_json::from_str(stored.metadata.as_deref().unwrap_or("{}")).unwrap();
        let freshness = FreshnessMetadata::from_task_metadata(&meta_val);
        assert!(
            freshness.branch_freshness_conflict,
            "branch_freshness_conflict must be true before routing"
        );
        assert_eq!(
            freshness.freshness_origin_state.as_deref(),
            Some("executing"),
            "freshness_origin_state must be 'executing'"
        );
        assert!(
            freshness.plan_update_conflict,
            "plan_update_conflict must be true for executing scenario"
        );
        // Non-freshness key preserved before clear
        assert_eq!(
            meta_val["trigger_origin"], "scheduler",
            "trigger_origin must be present before clear"
        );
    }

    // --- Step 1: Simulate handle_freshness_return_routing's metadata-clear step ---
    // Read the task back, clear freshness metadata, persist (exactly as handle_freshness_return_routing does)
    let mut stored = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("task must exist");

    let mut meta_val: serde_json::Value =
        serde_json::from_str(stored.metadata.as_deref().unwrap_or("{}")).unwrap();
    FreshnessMetadata::clear_from(&mut meta_val);
    stored.metadata = Some(meta_val.to_string());
    stored.touch();
    app_state.task_repo.update(&stored).await.unwrap();

    // --- Verify metadata was cleared in the repo (before transition fires) ---
    {
        let post_clear = app_state
            .task_repo
            .get_by_id(&task_id)
            .await
            .unwrap()
            .expect("task must exist after clear");

        let post_meta: serde_json::Value =
            serde_json::from_str(post_clear.metadata.as_deref().unwrap_or("{}")).unwrap();
        let post_freshness = FreshnessMetadata::from_task_metadata(&post_meta);

        assert!(
            !post_freshness.branch_freshness_conflict,
            "branch_freshness_conflict must be false after metadata clear"
        );
        assert!(
            post_freshness.freshness_origin_state.is_none(),
            "freshness_origin_state must be absent after metadata clear"
        );
        assert_eq!(
            post_freshness.freshness_conflict_count, 0,
            "freshness_conflict_count must be 0 after metadata clear"
        );
        assert!(
            !post_freshness.plan_update_conflict,
            "plan_update_conflict must be false after metadata clear"
        );
        // Non-freshness key survives the clear
        assert_eq!(
            post_meta["trigger_origin"], "scheduler",
            "Non-freshness key 'trigger_origin' must be preserved by FreshnessMetadata::clear_from"
        );
    }

    // --- Step 2: Transition via TaskTransitionService (same as handle_freshness_return_routing) ---
    let service = build_transition_service(&app_state);
    let result = service
        .transition_task_corrective_with_exit(&task_id, InternalStatus::Ready, None, "system")
        .await;

    assert!(
        result.is_ok(),
        "corrective transition to Ready must succeed: {:?}",
        result.err()
    );

    // --- Step 3: Verify the task ended up in Ready (or a valid auto-transition from Ready) ---
    // on_enter(Ready) may spawn a worker (no-op with MockAgenticClient in test),
    // leaving the task in Ready state. We accept Ready as the confirmed outcome.
    let final_task = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("task must exist after transition");

    assert_eq!(
        final_task.internal_status,
        InternalStatus::Ready,
        "Task must be Ready after freshness return routing from executing origin. Got: {:?}",
        final_task.internal_status
    );

    // The freshness keys must not appear in the final task metadata
    let final_meta: serde_json::Value =
        serde_json::from_str(final_task.metadata.as_deref().unwrap_or("{}")).unwrap();
    let final_freshness = FreshnessMetadata::from_task_metadata(&final_meta);
    assert!(
        !final_freshness.branch_freshness_conflict,
        "branch_freshness_conflict must not appear in final task metadata"
    );
    assert!(
        final_freshness.freshness_origin_state.is_none(),
        "freshness_origin_state must not appear in final task metadata"
    );
}

// ============================================================================
// Test 2: reviewing origin → PendingReview (auto-transitions to Reviewing)
// ============================================================================

/// When a task was in Review and freshness detected a branch conflict,
/// the task was routed to Merging for the merger agent to resolve.
/// After the merger agent finishes, handle_freshness_return_routing fires:
///   1. freshness metadata is cleared from the task
///   2. task transitions to PendingReview
///   3. state machine auto-transitions PendingReview → Reviewing
///
/// This test verifies:
///   - Before the transition: freshness keys present in repo (sanity check)
///   - Metadata clear step: all FreshnessMetadata keys removed from repo
///   - After transition_task(PendingReview): task status is PendingReview or Reviewing
///     (Reviewing is the expected auto-transition outcome)
///   - Post-clear metadata in repo: freshness keys absent
#[tokio::test]
async fn test_reviewing_freshness_conflict_routes_to_pending_review_after_merge_resolution() {
    let app_state = AppState::new_test();

    // Wire a project
    let project = Project::new(
        "test-project".to_string(),
        "/tmp/test-freshness-reviewing".to_string(),
    );
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Create the task in Merging state with source_update_conflict freshness metadata
    let task = make_merging_task_with_freshness(
        &project.id,
        "Freshness routing: reviewing → PendingReview",
        "reviewing",
        /*plan_update_conflict=*/ false,
        /*source_update_conflict=*/ true,
    );
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.unwrap();

    // --- Verify freshness metadata is present before the routing step ---
    {
        let stored = app_state
            .task_repo
            .get_by_id(&task_id)
            .await
            .unwrap()
            .expect("task must exist");
        assert_eq!(
            stored.internal_status,
            InternalStatus::Merging,
            "Task must start in Merging state"
        );
        let meta_val: serde_json::Value =
            serde_json::from_str(stored.metadata.as_deref().unwrap_or("{}")).unwrap();
        let freshness = FreshnessMetadata::from_task_metadata(&meta_val);
        assert!(
            freshness.branch_freshness_conflict,
            "branch_freshness_conflict must be true before routing"
        );
        assert_eq!(
            freshness.freshness_origin_state.as_deref(),
            Some("reviewing"),
            "freshness_origin_state must be 'reviewing'"
        );
        assert!(
            freshness.source_update_conflict,
            "source_update_conflict must be true for reviewing scenario"
        );
        assert_eq!(
            meta_val["trigger_origin"], "scheduler",
            "trigger_origin must be present before clear"
        );
    }

    // --- Step 1: Simulate handle_freshness_return_routing's metadata-clear step ---
    let mut stored = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("task must exist");

    let mut meta_val: serde_json::Value =
        serde_json::from_str(stored.metadata.as_deref().unwrap_or("{}")).unwrap();
    FreshnessMetadata::clear_from(&mut meta_val);
    stored.metadata = Some(meta_val.to_string());
    stored.touch();
    app_state.task_repo.update(&stored).await.unwrap();

    // --- Verify metadata was cleared in the repo (before transition fires) ---
    {
        let post_clear = app_state
            .task_repo
            .get_by_id(&task_id)
            .await
            .unwrap()
            .expect("task must exist after clear");

        let post_meta: serde_json::Value =
            serde_json::from_str(post_clear.metadata.as_deref().unwrap_or("{}")).unwrap();
        let post_freshness = FreshnessMetadata::from_task_metadata(&post_meta);

        assert!(
            !post_freshness.branch_freshness_conflict,
            "branch_freshness_conflict must be false after metadata clear"
        );
        assert!(
            post_freshness.freshness_origin_state.is_none(),
            "freshness_origin_state must be absent after metadata clear"
        );
        assert_eq!(
            post_freshness.freshness_conflict_count, 0,
            "freshness_conflict_count must be 0 after metadata clear"
        );
        assert!(
            !post_freshness.source_update_conflict,
            "source_update_conflict must be false after metadata clear"
        );
        // Non-freshness key survives the clear
        assert_eq!(
            post_meta["trigger_origin"], "scheduler",
            "Non-freshness key 'trigger_origin' must be preserved by FreshnessMetadata::clear_from"
        );
    }

    // --- Step 2: Transition via TaskTransitionService (same as handle_freshness_return_routing) ---
    let service = build_transition_service(&app_state);
    let result = service
        .transition_task_corrective_with_exit(
            &task_id,
            InternalStatus::PendingReview,
            None,
            "system",
        )
        .await;

    assert!(
        result.is_ok(),
        "corrective transition to PendingReview must succeed: {:?}",
        result.err()
    );

    // --- Step 3: Verify the task ended up in PendingReview or Reviewing ---
    // The state machine auto-transitions PendingReview → Reviewing on entry,
    // so the final status may be Reviewing. Both are valid outcomes of routing
    // from a "reviewing" origin after freshness conflict resolution.
    let final_task = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("task must exist after transition");

    let valid_final_statuses = [InternalStatus::PendingReview, InternalStatus::Reviewing];
    assert!(
        valid_final_statuses.contains(&final_task.internal_status),
        "Task must be PendingReview or Reviewing after freshness return routing from reviewing origin. Got: {:?}",
        final_task.internal_status
    );

    // The freshness keys must not appear in the final task metadata
    let final_meta: serde_json::Value =
        serde_json::from_str(final_task.metadata.as_deref().unwrap_or("{}")).unwrap();
    let final_freshness = FreshnessMetadata::from_task_metadata(&final_meta);
    assert!(
        !final_freshness.branch_freshness_conflict,
        "branch_freshness_conflict must not appear in final task metadata"
    );
    assert!(
        final_freshness.freshness_origin_state.is_none(),
        "freshness_origin_state must not appear in final task metadata"
    );
}
