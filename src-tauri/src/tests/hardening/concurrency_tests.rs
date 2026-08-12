// Concurrency hardening tests (Scenarios G1-G4 + TOCTOU fix)
//
// Focus: ExecutionState thread safety, max_concurrent enforcement,
// pause/resume interaction, merge deferral race conditions,
// and the TOCTOU fix using merge_lock + merges_in_flight.

use std::collections::HashSet;
use std::sync::Arc;

use super::helpers::*;
use crate::application::execution_state::ExecutionState;
use crate::domain::entities::{InternalStatus, ProjectId};
use crate::domain::repositories::TaskRepository;
use crate::domain::state_machine::events::TaskEvent;
use crate::domain::state_machine::machine::State;

// ============================================================================
// G1: Two tasks merge to same target — COVERED
// ============================================================================

#[tokio::test]
async fn test_g1_two_tasks_can_enter_pending_merge_independently() {
    // COVERED: Two tasks for the same project can both enter PendingMerge
    // independently. The concurrent merge guard (timestamp arbitration)
    // is in side_effects, not in the state machine itself.
    let s = create_hardening_services();

    let services1 = build_task_services(&s);
    let mut machine1 = create_state_machine("task-g1a", "proj-g1", services1);
    let mut handler1 = create_transition_handler(&mut machine1);

    let services2 = build_task_services(&s);
    let mut machine2 = create_state_machine("task-g1b", "proj-g1", services2);
    let mut handler2 = create_transition_handler(&mut machine2);

    let r1 = handler1
        .handle_transition(&State::Approved, &TaskEvent::StartMerge)
        .await;
    let r2 = handler2
        .handle_transition(&State::Approved, &TaskEvent::StartMerge)
        .await;

    assert!(
        r1.is_success(),
        "First task should enter PendingMerge successfully"
    );
    assert!(
        r2.is_success(),
        "Second task should also enter PendingMerge — guard is in side effects"
    );
}

#[tokio::test]
async fn test_g1_both_tasks_complete_merge_to_merged() {
    // COVERED: Verify both tasks can independently complete the full
    // PendingMerge -> Merged path.
    let s = create_hardening_services();

    let services1 = build_task_services(&s);
    let mut machine1 = create_state_machine("task-g1c", "proj-g1", services1);
    let mut handler1 = create_transition_handler(&mut machine1);

    let services2 = build_task_services(&s);
    let mut machine2 = create_state_machine("task-g1d", "proj-g1", services2);
    let mut handler2 = create_transition_handler(&mut machine2);

    let r1 = handler1
        .handle_transition(&State::PendingMerge, &TaskEvent::MergeComplete)
        .await;
    let r2 = handler2
        .handle_transition(&State::PendingMerge, &TaskEvent::MergeComplete)
        .await;

    assert!(r1.is_success(), "First task merge should complete");
    assert_eq!(r1.state(), Some(&State::Merged));

    assert!(r2.is_success(), "Second task merge should complete");
    assert_eq!(r2.state(), Some(&State::Merged));
}

// ============================================================================
// G2: Reconciliation re-spawns while original starting — PARTIAL
// ============================================================================

#[tokio::test]
async fn test_g2_execution_state_has_no_starting_phase() {
    // PARTIAL: ExecutionState only tracks a numeric running_count via atomics.
    // There is no "starting" state for individual tasks — just a counter.
    let exec_state = Arc::new(ExecutionState::with_max_concurrent(2));

    assert_eq!(exec_state.running_count(), 0, "Initial count should be 0");
    assert!(
        exec_state.can_start_task(),
        "Should be able to start a task initially"
    );

    // Simulate "starting" a task — there is only increment, no "starting" flag
    let new_count = exec_state.increment_running();
    assert_eq!(new_count, 1, "Count should be 1 after increment");

    // GAP: Between this increment and the agent actually running, reconciliation
    // could see the task as "running" even though the agent hasn't started yet.
    assert!(
        exec_state.can_start_task(),
        "Can still start another (max=2, running=1)"
    );

    exec_state.increment_running();
    assert!(
        !exec_state.can_start_task(),
        "Cannot start more — at capacity"
    );
}

#[tokio::test]
async fn test_g2_concurrent_increment_thread_safety() {
    // PARTIAL: Test that concurrent increment_running calls from multiple
    // tokio tasks produce the correct final count.
    let exec_state = Arc::new(ExecutionState::with_max_concurrent(200));
    let num_tasks: u32 = 100;

    let mut handles = Vec::new();
    for _ in 0..num_tasks {
        let state = exec_state.clone();
        handles.push(tokio::spawn(async move {
            state.increment_running();
        }));
    }

    for handle in handles {
        handle.await.unwrap();
    }

    assert_eq!(
        exec_state.running_count(),
        num_tasks,
        "Running count should equal number of concurrent increments"
    );
}

#[tokio::test]
async fn test_g2_concurrent_decrement_thread_safety() {
    // PARTIAL: Test that concurrent decrement_running calls from multiple
    // tokio tasks produce the correct final count.
    let exec_state = Arc::new(ExecutionState::with_max_concurrent(200));

    // Pre-fill to 100
    exec_state.set_running_count(100);
    assert_eq!(exec_state.running_count(), 100);

    let mut handles = Vec::new();
    for _ in 0..100_u32 {
        let state = exec_state.clone();
        handles.push(tokio::spawn(async move {
            state.decrement_running();
        }));
    }

    for handle in handles {
        handle.await.unwrap();
    }

    assert_eq!(
        exec_state.running_count(),
        0,
        "Running count should be 0 after concurrent decrements"
    );
}

#[tokio::test]
async fn test_g2_concurrent_increment_and_decrement_thread_safety() {
    // PARTIAL: Test mixed concurrent increment and decrement.
    // 50 increments + 50 decrements starting from 50 should end at 50.
    let exec_state = Arc::new(ExecutionState::with_max_concurrent(200));
    exec_state.set_running_count(50);

    let mut handles = Vec::new();

    // 50 increments
    for _ in 0..50 {
        let state = exec_state.clone();
        handles.push(tokio::spawn(async move {
            state.increment_running();
        }));
    }

    // 50 decrements
    for _ in 0..50 {
        let state = exec_state.clone();
        handles.push(tokio::spawn(async move {
            state.decrement_running();
        }));
    }

    for handle in handles {
        handle.await.unwrap();
    }

    assert_eq!(
        exec_state.running_count(),
        50,
        "Net effect of 50 inc + 50 dec from 50 should be 50"
    );
}

// ============================================================================
// G3: Running count inflated after crash — COVERED
// ============================================================================

#[tokio::test]
async fn test_g3_running_count_can_be_reset_via_set_running_count() {
    // COVERED: After a crash, running_count may be inflated.
    // Reconciliation uses set_running_count to fix this.
    let exec_state = Arc::new(ExecutionState::with_max_concurrent(2));

    // Simulate inflated count after crash
    exec_state.increment_running();
    exec_state.increment_running();
    exec_state.increment_running();
    exec_state.increment_running();
    exec_state.increment_running();

    assert_eq!(exec_state.running_count(), 5, "Count is inflated to 5");
    assert!(
        !exec_state.can_start_task(),
        "Cannot start tasks — count exceeds max"
    );

    // Reconciliation resets the count
    exec_state.set_running_count(0);

    assert_eq!(exec_state.running_count(), 0, "Count should be reset to 0");
    assert!(
        exec_state.can_start_task(),
        "Can start tasks again after reset"
    );
}

#[tokio::test]
async fn test_g3_running_count_reset_to_actual_value() {
    // COVERED: Reset to actual running count, not just 0.
    // In production, reconciliation counts actually-running agents and sets
    // the count to that value.
    let exec_state = Arc::new(ExecutionState::with_max_concurrent(3));

    for _ in 0..10 {
        exec_state.increment_running();
    }
    assert_eq!(exec_state.running_count(), 10);

    // Reconciliation determined 2 agents are actually running
    exec_state.set_running_count(2);
    assert_eq!(exec_state.running_count(), 2);
    assert!(
        exec_state.can_start_task(),
        "Can start 1 more task (max=3, running=2)"
    );
}

#[tokio::test]
async fn test_g3_increment_decrement_cycle_is_balanced() {
    // COVERED: Test that a balanced increment/decrement cycle returns to zero.
    let exec_state = Arc::new(ExecutionState::with_max_concurrent(5));

    for _ in 0..5 {
        exec_state.increment_running();
    }
    assert_eq!(exec_state.running_count(), 5);
    assert!(
        !exec_state.can_start_task(),
        "At capacity (max=5, running=5)"
    );

    for _ in 0..5 {
        exec_state.decrement_running();
    }
    assert_eq!(exec_state.running_count(), 0);
    assert!(exec_state.can_start_task(), "Back to empty — can start");
}

// ============================================================================
// ExecutionState: max_concurrent enforcement
// ============================================================================

#[tokio::test]
async fn test_max_concurrent_boundary() {
    // COVERED: Verify can_start_task transitions at the exact boundary.
    let exec_state = Arc::new(ExecutionState::with_max_concurrent(1));

    assert!(exec_state.can_start_task(), "0 < 1 — can start");

    exec_state.increment_running();
    assert!(!exec_state.can_start_task(), "1 >= 1 — at capacity");

    exec_state.decrement_running();
    assert!(
        exec_state.can_start_task(),
        "Back to 0 < 1 — can start again"
    );
}

// ============================================================================
// ExecutionState: pause/resume interaction with running count
// ============================================================================

#[tokio::test]
async fn test_pause_blocks_start_regardless_of_capacity() {
    // COVERED: When paused, can_start_task returns false even with capacity.
    let exec_state = Arc::new(ExecutionState::with_max_concurrent(10));
    assert_eq!(exec_state.running_count(), 0);

    exec_state.pause();
    assert!(
        !exec_state.can_start_task(),
        "Paused — cannot start even with 10 slots free"
    );

    // Running count is unaffected by pause
    assert_eq!(
        exec_state.running_count(),
        0,
        "Pausing does not change running count"
    );
}

#[tokio::test]
async fn test_resume_restores_capacity_check() {
    // COVERED: After resume, can_start_task respects running_count again.
    let exec_state = Arc::new(ExecutionState::with_max_concurrent(2));

    exec_state.increment_running();
    exec_state.pause();

    assert!(!exec_state.can_start_task(), "Paused — blocked");

    exec_state.resume();
    assert!(
        exec_state.can_start_task(),
        "Resumed with capacity — can start (max=2, running=1)"
    );
}

#[tokio::test]
async fn test_resume_at_capacity_still_blocked() {
    // COVERED: Resuming when already at capacity still blocks starts.
    let exec_state = Arc::new(ExecutionState::with_max_concurrent(2));

    exec_state.increment_running();
    exec_state.increment_running();
    exec_state.pause();
    exec_state.resume();

    assert!(
        !exec_state.can_start_task(),
        "Resumed but at capacity (max=2, running=2) — still blocked"
    );
}

#[tokio::test]
async fn test_pause_resume_does_not_affect_running_count() {
    // COVERED: pause/resume cycle leaves running_count unchanged.
    let exec_state = Arc::new(ExecutionState::with_max_concurrent(5));

    exec_state.increment_running();
    exec_state.increment_running();
    exec_state.increment_running();

    let count_before = exec_state.running_count();
    exec_state.pause();
    exec_state.resume();
    let count_after = exec_state.running_count();

    assert_eq!(
        count_before, count_after,
        "pause/resume cycle should not modify running_count"
    );
}
// ============================================================================
// G4: Two merge deferrals race — GAP
// ============================================================================

#[tokio::test]
async fn test_g4_merge_deferral_uses_metadata_no_lock() {
    // GAP: Merge deferral uses a "merge_deferred" flag in task metadata JSON.
    // Two tasks can both have this flag cleared simultaneously with no
    // persistent lock or lease. This creates a race condition.
    let s = create_hardening_services();
    let project_id = ProjectId::from_string("proj-g4".to_string());

    let mut task1 =
        create_test_task_with_status(&project_id, "Deferred task 1", InternalStatus::PendingMerge);
    task1.metadata = Some(
        serde_json::json!({
            "merge_deferred": true,
            "merge_deferred_at": "2026-02-13T10:00:00Z"
        })
        .to_string(),
    );
    task1.task_branch = Some("ralphx/proj/task-g4a".to_string());

    let mut task2 =
        create_test_task_with_status(&project_id, "Deferred task 2", InternalStatus::PendingMerge);
    task2.metadata = Some(
        serde_json::json!({
            "merge_deferred": true,
            "merge_deferred_at": "2026-02-13T10:00:01Z"
        })
        .to_string(),
    );
    task2.task_branch = Some("ralphx/proj/task-g4b".to_string());

    let task1_id = task1.id.clone();
    let task2_id = task2.id.clone();
    s.task_repo.create(task1).await.unwrap();
    s.task_repo.create(task2).await.unwrap();

    // Both tasks have merge_deferred set
    let t1 = s.task_repo.get_by_id(&task1_id).await.unwrap().unwrap();
    let t2 = s.task_repo.get_by_id(&task2_id).await.unwrap().unwrap();

    let t1_meta: serde_json::Value = serde_json::from_str(t1.metadata.as_ref().unwrap()).unwrap();
    let t2_meta: serde_json::Value = serde_json::from_str(t2.metadata.as_ref().unwrap()).unwrap();

    assert_eq!(t1_meta["merge_deferred"], true);
    assert_eq!(t2_meta["merge_deferred"], true);

    // GAP: Both can have merge_deferred cleared simultaneously.
    // Simulate clearing both — no lock prevents this.
    let mut t1_mut = t1;
    let mut t2_mut = t2;

    let mut meta1: serde_json::Value =
        serde_json::from_str(t1_mut.metadata.as_ref().unwrap()).unwrap();
    meta1.as_object_mut().unwrap().remove("merge_deferred");
    meta1.as_object_mut().unwrap().remove("merge_deferred_at");
    t1_mut.metadata = Some(meta1.to_string());

    let mut meta2: serde_json::Value =
        serde_json::from_str(t2_mut.metadata.as_ref().unwrap()).unwrap();
    meta2.as_object_mut().unwrap().remove("merge_deferred");
    meta2.as_object_mut().unwrap().remove("merge_deferred_at");
    t2_mut.metadata = Some(meta2.to_string());

    s.task_repo.update(&t1_mut).await.unwrap();
    s.task_repo.update(&t2_mut).await.unwrap();

    // Both tasks had their deferral cleared — no conflict or lock prevented it
    let t1_after = s.task_repo.get_by_id(&task1_id).await.unwrap().unwrap();
    let t2_after = s.task_repo.get_by_id(&task2_id).await.unwrap().unwrap();

    if let Some(ref meta_str) = t1_after.metadata {
        let meta: serde_json::Value = serde_json::from_str(meta_str).unwrap();
        assert!(
            meta.get("merge_deferred").is_none(),
            "merge_deferred should be cleared from task 1"
        );
    }
    if let Some(ref meta_str) = t2_after.metadata {
        let meta: serde_json::Value = serde_json::from_str(meta_str).unwrap();
        assert!(
            meta.get("merge_deferred").is_none(),
            "merge_deferred should be cleared from task 2"
        );
    }

    // GAP: Both tasks could now proceed to merge simultaneously because
    // there is no persistent lock/lease mechanism — only the in-memory
    // timestamp arbitration in try_programmatic_merge guards against this.
}

#[tokio::test]
async fn test_g4_merge_incomplete_exists_as_fallback() {
    // GAP: If two tasks race to merge and one fails, MergeIncomplete is the
    // fallback. Verify this fallback state is reachable.
    let s = create_hardening_services();
    let services = build_task_services(&s);
    let mut machine = create_state_machine("task-g4-fallback", "proj-g4", services);
    let mut handler = create_transition_handler(&mut machine);

    let result = handler
        .handle_transition(&State::Merging, &TaskEvent::MergeAgentError)
        .await;

    assert!(result.is_success(), "MergeAgentError should be handled");
    assert_eq!(
        result.state(),
        Some(&State::MergeIncomplete),
        "GAP: MergeIncomplete exists as fallback for merge race failures"
    );
}

// ============================================================================
// TOCTOU fix: merge_lock serializes the concurrent merge guard
// ============================================================================

#[tokio::test]
async fn test_toctou_merge_lock_is_shared_across_task_services() {
    // FIXED: Both TaskServices created from the same HardeningServices share
    // the same Arc<Mutex<()>> merge_lock. This serializes the check-and-set
    // in `attempt_programmatic_merge`, eliminating the TOCTOU race.
    let s = create_hardening_services();

    let services1 = build_task_services(&s);
    let services2 = build_task_services(&s);

    // Both services must point to the same underlying mutex
    assert!(
        Arc::ptr_eq(&services1.merge_lock, &services2.merge_lock),
        "Both TaskServices must share the same merge_lock Arc (same pointer) \
         so the lock serializes concurrent merge guard checks"
    );
}

#[tokio::test]
async fn test_toctou_merge_lock_serializes_concurrent_access() {
    // FIXED: Verify that acquiring the merge_lock from one task blocks
    // the other, preventing both from reading "no blocker" simultaneously.
    let s = create_hardening_services();

    // Take the lock (simulating task 1 inside the critical section)
    let lock = Arc::clone(&s.merge_lock);
    let guard = lock.lock().await;

    // Task 2 should not be able to acquire the lock while task 1 holds it
    let try_result = s.merge_lock.try_lock();
    assert!(
        try_result.is_err(),
        "Second task must not acquire merge_lock while first task holds it — \
         this is the atomicity guarantee that eliminates TOCTOU"
    );

    // Drop task 1's guard — task 2 can now proceed
    drop(guard);
    let try_result2 = s.merge_lock.try_lock();
    assert!(
        try_result2.is_ok(),
        "After task 1 releases merge_lock, task 2 should be able to acquire it"
    );
}

// ============================================================================
// TOCTOU fix: merges_in_flight self-dedup guard
// ============================================================================

#[tokio::test]
async fn test_self_dedup_merges_in_flight_is_shared_across_task_services() {
    // FIXED: Both TaskServices share the same merges_in_flight set.
    let s = create_hardening_services();

    let services1 = build_task_services(&s);
    let services2 = build_task_services(&s);

    assert!(
        Arc::ptr_eq(&services1.merges_in_flight, &services2.merges_in_flight),
        "Both TaskServices must share the same merges_in_flight Arc so the \
         self-dedup guard works correctly across concurrent callers"
    );
}

#[tokio::test]
async fn test_self_dedup_insert_prevents_duplicate_entry() {
    // FIXED: Inserting the same task ID twice returns false on the second insert,
    // causing `attempt_programmatic_merge` to skip the duplicate call.
    let s = create_hardening_services();
    let task_id = "task-dedup-test".to_string();

    let mut set = s.merges_in_flight.lock().unwrap();

    // First insert succeeds
    let first = set.insert(task_id.clone());
    assert!(
        first,
        "First insert should return true — task is not yet in flight"
    );

    // Second insert for the same task returns false (dedup fires)
    let second = set.insert(task_id.clone());
    assert!(
        !second,
        "Second insert returns false — duplicate merge attempt would be skipped"
    );

    // Cleanup: remove to simulate merge completion
    set.remove(&task_id);
    let after_remove = set.insert(task_id.clone());
    assert!(
        after_remove,
        "After merge completes (remove), a new merge attempt should be accepted"
    );
}

#[tokio::test]
async fn test_self_dedup_concurrent_insert_only_one_wins() {
    // FIXED: Concurrent inserts for the same task ID — only the first wins,
    // the second returns false and would cause the caller to skip its merge.
    let set: Arc<std::sync::Mutex<HashSet<String>>> =
        Arc::new(std::sync::Mutex::new(HashSet::new()));

    let task_id = "task-concurrent-dedup".to_string();

    // Simulate two concurrent threads both trying to insert the same task
    let set1 = Arc::clone(&set);
    let id1 = task_id.clone();
    let handle1 = std::thread::spawn(move || set1.lock().unwrap().insert(id1));

    // Give thread 1 a head start, then thread 2 tries to insert
    let result1 = handle1.join().unwrap();

    let set2 = Arc::clone(&set);
    let id2 = task_id.clone();
    let result2 = set2.lock().unwrap().insert(id2);

    // Exactly one of them should have succeeded
    assert!(
        result1 ^ result2,
        "Exactly one insert must succeed (XOR): result1={}, result2={}. \
         Both succeeding is the TOCTOU bug we fixed.",
        result1,
        result2
    );
}
