// Integration tests for structural git error detection in on_enter(Failed).
//
// Step 3: on_enter(Failed) with a structural error message writes
//         stop_retrying=true + StructuralGitError reason to execution_recovery.
//
// Step 4: The metadata written by persist_failed_task_metadata() round-trips
//         correctly through from_task_metadata() deserialization, and the
//         create_git_isolation_recovery_metadata_json() production path
//         (which runs in a separate code path) produces expected behavior
//         when given existing structural-error metadata.

use super::helpers::*;
use crate::domain::entities::task_metadata::GIT_ISOLATION_ERROR_PREFIX;
use crate::domain::entities::{ExecutionRecoveryMetadata, ExecutionRecoveryState, Project, Task};
use crate::domain::state_machine::types::FailedData;
use crate::domain::state_machine::{State, TaskStateMachine, TransitionHandler};
use crate::infrastructure::memory::MemoryTaskRepository;
use std::sync::Arc;

// ============================================================================
// Step 3: on_enter(Failed) with structural error sets stop_retrying=true
// ============================================================================

/// Test: on_enter(Failed) with a structural error (base branch missing) writes
/// stop_retrying=true and StructuralGitError reason into execution_recovery.
///
/// This covers the fallback path in persist_failed_task_metadata() that detects
/// "structural:" in data.error and marks the task as permanently unretryable.
#[tokio::test]
async fn test_on_enter_failed_structural_error_sets_stop_retrying() {
    use crate::domain::entities::task_metadata::StopRetryingReason;

    let project = Project::new("test-project".to_string(), "/test/path".to_string());
    let task = Task::new(project.id.clone(), "Test task".to_string());

    let task_repo: Arc<dyn crate::domain::repositories::TaskRepository> =
        Arc::new(MemoryTaskRepository::new());
    task_repo.create(task.clone()).await.unwrap();

    let mut services = TaskServices::new_mock();
    services.task_repo = Some(task_repo.clone());

    let context = create_context_with_services(task.id.as_str(), project.id.as_str(), services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    // Simulate the error message produced by create_fresh_branch_and_worktree()
    // when the base branch doesn't exist.
    let structural_error = format!(
        "{}: structural: base branch 'main' does not exist",
        GIT_ISOLATION_ERROR_PREFIX
    );
    let failed_data = FailedData::new(&structural_error);
    let _ = handler.on_enter(&State::Failed(failed_data)).await;

    let updated_task = task_repo.get_by_id(&task.id).await.unwrap().unwrap();
    let metadata_json = updated_task.metadata.expect("Metadata should be written");

    let recovery = ExecutionRecoveryMetadata::from_task_metadata(Some(&metadata_json))
        .expect("Parsing should succeed")
        .expect("execution_recovery should be present after structural error");

    assert!(
        recovery.stop_retrying,
        "stop_retrying must be true for structural git errors — retrying cannot fix a missing base branch"
    );
    assert_eq!(
        recovery.last_state,
        ExecutionRecoveryState::Failed,
        "last_state must be Failed for structural errors (task is terminal, not retrying)"
    );
    assert_eq!(
        recovery.events.len(),
        1,
        "Exactly one Failed event should be written by the structural error path"
    );
    assert_eq!(
        recovery.unrecoverable_reason,
        Some(StopRetryingReason::StructuralGitError),
        "unrecoverable_reason must be StructuralGitError"
    );
}

/// Test: on_enter(Failed) with a NON-structural error still writes the default
/// fallback (stop_retrying=false, Retrying state) — structural detection is
/// scoped to errors containing "structural:".
#[tokio::test]
async fn test_on_enter_failed_non_structural_error_writes_retrying_fallback() {
    let project = Project::new("test-project".to_string(), "/test/path".to_string());
    let task = Task::new(project.id.clone(), "Test task".to_string());

    let task_repo: Arc<dyn crate::domain::repositories::TaskRepository> =
        Arc::new(MemoryTaskRepository::new());
    task_repo.create(task.clone()).await.unwrap();

    let mut services = TaskServices::new_mock();
    services.task_repo = Some(task_repo.clone());

    let context = create_context_with_services(task.id.as_str(), project.id.as_str(), services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    // Non-structural git isolation error (transient worktree failure, not structural)
    let transient_error = format!(
        "{}: could not create worktree at '/tmp/test': git error",
        GIT_ISOLATION_ERROR_PREFIX
    );
    let failed_data = FailedData::new(&transient_error);
    let _ = handler.on_enter(&State::Failed(failed_data)).await;

    let updated_task = task_repo.get_by_id(&task.id).await.unwrap().unwrap();
    let metadata_json = updated_task.metadata.expect("Metadata should be written");

    let recovery = ExecutionRecoveryMetadata::from_task_metadata(Some(&metadata_json))
        .expect("Parsing should succeed")
        .expect("execution_recovery should be present for non-structural error too");

    assert!(
        !recovery.stop_retrying,
        "stop_retrying must be false for transient (non-structural) errors — reconciler should retry"
    );
    assert_eq!(
        recovery.last_state,
        ExecutionRecoveryState::Retrying,
        "last_state must be Retrying for non-structural errors"
    );
    assert_eq!(
        recovery.unrecoverable_reason, None,
        "unrecoverable_reason must be None for non-structural errors"
    );
}

/// Test: on_enter(Failed) with structural error skips fallback when execution_recovery
/// is already pre-written (existing recovery is preserved, not overwritten).
#[tokio::test]
async fn test_on_enter_failed_structural_error_skips_when_recovery_prewritten() {
    use crate::domain::entities::{
        ExecutionRecoveryEvent, ExecutionRecoveryEventKind, ExecutionRecoveryReasonCode,
        ExecutionRecoverySource,
    };

    let project = Project::new("test-project".to_string(), "/test/path".to_string());
    let mut task = Task::new(project.id.clone(), "Test task".to_string());

    // Pre-write execution_recovery (e.g., from the production apply_corrective_transition path)
    let mut pre_recovery = ExecutionRecoveryMetadata::new();
    pre_recovery.stop_retrying = false; // production path sets false
    pre_recovery.append_event_with_state(
        ExecutionRecoveryEvent::new(
            ExecutionRecoveryEventKind::Failed,
            ExecutionRecoverySource::Auto,
            ExecutionRecoveryReasonCode::GitIsolationFailed,
            "git isolation pre-written",
        ),
        ExecutionRecoveryState::Retrying,
    );
    let recovery_json = serde_json::to_string(&pre_recovery).unwrap();
    task.metadata = Some(format!(r#"{{"execution_recovery":{}}}"#, recovery_json));

    let task_repo: Arc<dyn crate::domain::repositories::TaskRepository> =
        Arc::new(MemoryTaskRepository::new());
    task_repo.create(task.clone()).await.unwrap();

    let mut services = TaskServices::new_mock();
    services.task_repo = Some(task_repo.clone());

    let context = create_context_with_services(task.id.as_str(), project.id.as_str(), services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    // Call on_enter(Failed) with structural error — fallback should be skipped
    // because execution_recovery already exists
    let structural_error = format!(
        "{}: structural: base branch 'main' does not exist",
        GIT_ISOLATION_ERROR_PREFIX
    );
    let failed_data = FailedData::new(&structural_error);
    let _ = handler.on_enter(&State::Failed(failed_data)).await;

    let updated_task = task_repo.get_by_id(&task.id).await.unwrap().unwrap();
    let metadata_json = updated_task.metadata.expect("Metadata should exist");

    let recovery = ExecutionRecoveryMetadata::from_task_metadata(Some(&metadata_json))
        .expect("Parsing should succeed")
        .expect("execution_recovery should still be present");

    // Fallback must NOT overwrite the pre-written recovery
    assert!(
        !recovery.stop_retrying,
        "stop_retrying must remain false (pre-written value) — fallback must NOT overwrite"
    );
    assert_eq!(
        recovery.last_state,
        ExecutionRecoveryState::Retrying,
        "last_state must remain Retrying (pre-written value) — fallback must NOT change it"
    );
    assert_eq!(
        recovery.events.len(),
        1,
        "Event count must not change — fallback must NOT append a new event"
    );
}

// ============================================================================
// Step 4: from_task_metadata() round-trip after persist_failed_task_metadata
// ============================================================================

/// Test: metadata written by persist_failed_task_metadata() for a structural error
/// round-trips correctly through from_task_metadata() deserialization.
///
/// Specifically verifies that stop_retrying=true and StructuralGitError reason
/// survive the JSON serialization/deserialization cycle without corruption.
#[tokio::test]
async fn test_structural_error_metadata_round_trips_through_deserialization() {
    use crate::domain::entities::task_metadata::StopRetryingReason;

    let project = Project::new("test-project".to_string(), "/test/path".to_string());
    let task = Task::new(project.id.clone(), "Test task".to_string());

    let task_repo: Arc<dyn crate::domain::repositories::TaskRepository> =
        Arc::new(MemoryTaskRepository::new());
    task_repo.create(task.clone()).await.unwrap();

    let mut services = TaskServices::new_mock();
    services.task_repo = Some(task_repo.clone());

    let context = create_context_with_services(task.id.as_str(), project.id.as_str(), services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let structural_error = format!(
        "{}: structural: base branch 'feature-plan' does not exist",
        GIT_ISOLATION_ERROR_PREFIX
    );
    let failed_data = FailedData::new(&structural_error);
    let _ = handler.on_enter(&State::Failed(failed_data)).await;

    // Read back metadata from repo
    let updated_task = task_repo.get_by_id(&task.id).await.unwrap().unwrap();
    let metadata_json = updated_task.metadata.expect("Metadata must be written");

    // Verify round-trip: from_task_metadata() correctly deserializes stop_retrying=true
    let recovery = ExecutionRecoveryMetadata::from_task_metadata(Some(&metadata_json))
        .expect("JSON must be valid")
        .expect("execution_recovery key must be present");

    assert!(
        recovery.stop_retrying,
        "stop_retrying=true must survive JSON round-trip"
    );
    assert_eq!(
        recovery.unrecoverable_reason,
        Some(StopRetryingReason::StructuralGitError),
        "StructuralGitError reason must survive JSON round-trip"
    );
    assert_eq!(
        recovery.last_state,
        ExecutionRecoveryState::Failed,
        "Failed state must survive JSON round-trip"
    );

    // Verify create_git_isolation_recovery_metadata_json() — the production path —
    // unconditionally overwrites execution_recovery with stop_retrying=false.
    // This documents that the two paths (test via handle_transition vs production via
    // apply_corrective_transition) are independent, not cumulative.
    let overwritten_json = crate::application::task_transition_service::create_git_isolation_recovery_metadata_json(
        &structural_error,
        Some(&metadata_json),
    )
    .expect("create_git_isolation_recovery_metadata_json should return Some for git isolation prefix");

    let overwritten_recovery =
        ExecutionRecoveryMetadata::from_task_metadata(Some(&overwritten_json))
            .expect("JSON must be valid")
            .expect("execution_recovery must be present in overwritten json");

    // Document: production path (create_git_isolation_recovery_metadata_json) unconditionally
    // writes stop_retrying=false because it creates a fresh ExecutionRecoveryMetadata.
    // The persist_failed_task_metadata structural detection only applies to the test
    // code path (handle_transition → on_enter(Failed)) — the two paths are independent.
    assert!(
        !overwritten_recovery.stop_retrying,
        "create_git_isolation_recovery_metadata_json unconditionally writes stop_retrying=false \
         (production path, independent from persist_failed_task_metadata fallback path)"
    );
}
