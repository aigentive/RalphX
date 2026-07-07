use super::*;

use crate::domain::entities::{
    task_metadata::StopRetryingReason, MergeRecoveryEvent, MergeRecoveryReasonCode,
    MergeRecoverySource, ProjectId,
};

fn task_with_metadata(metadata: String) -> Task {
    let mut task = Task::new(
        ProjectId("project".to_string()),
        "metadata test".to_string(),
    );
    task.metadata = Some(metadata);
    task
}

fn merge_event(kind: MergeRecoveryEventKind, source: MergeFailureSource) -> MergeRecoveryEvent {
    MergeRecoveryEvent::new(
        kind,
        MergeRecoverySource::Auto,
        MergeRecoveryReasonCode::GitError,
        "merge recovery event",
    )
    .with_failure_source(source)
}

fn task_with_merge_recovery(recovery: MergeRecoveryMetadata) -> Task {
    task_with_metadata(
        recovery
            .update_task_metadata(None)
            .expect("serialize merge recovery"),
    )
}

fn task_with_execution_recovery(recovery: ExecutionRecoveryMetadata) -> Task {
    task_with_metadata(
        recovery
            .update_task_metadata(None)
            .expect("serialize execution recovery"),
    )
}

#[test]
fn failure_source_to_reason_code_maps_agent_incomplete() {
    assert_eq!(
        ReconciliationRunner::<tauri::Wry>::failure_source_to_reason_code(
            ExecutionFailureSource::AgentIncomplete,
        ),
        ExecutionRecoveryReasonCode::IncompleteSteps,
    );
}

#[test]
fn stop_retrying_reason_to_code_maps_git_branch_lost() {
    assert_eq!(
        stop_retrying_reason_to_code(&StopRetryingReason::GitBranchLost),
        ExecutionRecoveryReasonCode::GitBranchLost,
    );
}

#[test]
fn stop_retrying_reason_to_code_maps_structural_git_error() {
    assert_eq!(
        stop_retrying_reason_to_code(&StopRetryingReason::StructuralGitError),
        ExecutionRecoveryReasonCode::StructuralGitError,
    );
}

#[test]
fn stop_retrying_reason_to_code_maps_git_isolation_exhausted() {
    assert_eq!(
        stop_retrying_reason_to_code(&StopRetryingReason::GitIsolationExhausted),
        ExecutionRecoveryReasonCode::GitIsolationExhausted,
    );
}

#[test]
fn stop_retrying_reason_to_code_maps_agent_command_invalid() {
    assert_eq!(
        stop_retrying_reason_to_code(&StopRetryingReason::AgentCommandInvalid),
        ExecutionRecoveryReasonCode::AgentCommandInvalid,
    );
}

#[test]
fn stop_retrying_reason_to_code_maps_other_variants_to_unknown() {
    assert_eq!(
        stop_retrying_reason_to_code(&StopRetryingReason::MaxRetriesExceeded),
        ExecutionRecoveryReasonCode::Unknown,
    );
    assert_eq!(
        stop_retrying_reason_to_code(&StopRetryingReason::ManualStop),
        ExecutionRecoveryReasonCode::Unknown,
    );
    assert_eq!(
        stop_retrying_reason_to_code(&StopRetryingReason::Unknown),
        ExecutionRecoveryReasonCode::Unknown,
    );
}

#[test]
fn merge_conflict_auto_retry_count_excludes_target_branch_busy_deferrals() {
    let mut recovery = MergeRecoveryMetadata::new();
    recovery.append_event(merge_event(
        MergeRecoveryEventKind::AutoRetryTriggered,
        MergeFailureSource::TransientGit,
    ));
    recovery.append_event(merge_event(
        MergeRecoveryEventKind::AutoRetryTriggered,
        MergeFailureSource::TargetBranchBusy,
    ));
    recovery.append_event(merge_event(
        MergeRecoveryEventKind::AttemptFailed,
        MergeFailureSource::TransientGit,
    ));
    let task = task_with_merge_recovery(recovery);

    assert_eq!(
        ReconciliationRunner::<tauri::Wry>::merge_conflict_auto_retry_count(&task),
        1
    );
}

#[test]
fn failure_source_helpers_read_flat_metadata_sources() {
    let agent_reported = task_with_metadata(
        serde_json::json!({ "merge_failure_source": "agent_reported" }).to_string(),
    );
    assert!(ReconciliationRunner::<tauri::Wry>::is_agent_reported_failure(&agent_reported));

    let validation_failed = task_with_metadata(
        serde_json::json!({
            "merge_failure_source": "validation_failed",
            "validation_revert_count": 2,
            "consecutive_validation_failures": 3,
            "last_retried_at": "2026-06-24T12:00:00Z"
        })
        .to_string(),
    );

    assert!(ReconciliationRunner::<tauri::Wry>::is_validation_failure(
        &validation_failed
    ));
    assert_eq!(
        ReconciliationRunner::<tauri::Wry>::validation_revert_count(&validation_failed),
        2
    );
    assert_eq!(
        ReconciliationRunner::<tauri::Wry>::consecutive_validation_failures(&validation_failed),
        3
    );
    assert_eq!(
        ReconciliationRunner::<tauri::Wry>::last_retried_at(&validation_failed)
            .expect("last retried timestamp")
            .to_rfc3339(),
        "2026-06-24T12:00:00+00:00"
    );
}

#[test]
fn should_circuit_break_counts_only_auto_retryable_recent_failures() {
    let mut recovery = MergeRecoveryMetadata::new();
    recovery.append_event(merge_event(
        MergeRecoveryEventKind::AutoRetryTriggered,
        MergeFailureSource::TargetBranchBusy,
    ));
    recovery.append_event(merge_event(
        MergeRecoveryEventKind::AttemptFailed,
        MergeFailureSource::ValidationFailed,
    ));
    for _ in 0..3 {
        recovery.append_event(merge_event(
            MergeRecoveryEventKind::Deferred,
            MergeFailureSource::WorktreeMissing,
        ));
    }
    let task = task_with_merge_recovery(recovery);

    let reason = ReconciliationRunner::<tauri::Wry>::should_circuit_break(&task, 3, 5)
        .expect("three auto-retryable failures should trip circuit breaker");

    assert!(reason.contains("3/5"));
    assert!(reason.contains("worktree_missing"));
}

#[test]
fn should_circuit_break_ignores_insufficient_or_unclassified_failures() {
    let mut recovery = MergeRecoveryMetadata::new();
    recovery.append_event(MergeRecoveryEvent::new(
        MergeRecoveryEventKind::AttemptFailed,
        MergeRecoverySource::Auto,
        MergeRecoveryReasonCode::GitError,
        "unclassified",
    ));
    recovery.append_event(merge_event(
        MergeRecoveryEventKind::AttemptStarted,
        MergeFailureSource::WorktreeMissing,
    ));
    let task = task_with_merge_recovery(recovery);

    assert!(ReconciliationRunner::<tauri::Wry>::should_circuit_break(&task, 1, 3).is_none());
}

#[test]
fn execution_retry_helpers_read_structured_recovery_metadata() {
    let mut recovery = ExecutionRecoveryMetadata::new();
    let mut old_startup = ExecutionRecoveryEvent::new(
        ExecutionRecoveryEventKind::AutoRetryTriggered,
        ExecutionRecoverySource::Startup,
        ExecutionRecoveryReasonCode::Timeout,
        "old startup retry",
    );
    old_startup.at = chrono::Utc::now() - chrono::Duration::seconds(90);
    recovery.append_event_with_state(old_startup, ExecutionRecoveryState::Retrying);
    recovery.append_event_with_state(
        ExecutionRecoveryEvent::new(
            ExecutionRecoveryEventKind::AutoRetryTriggered,
            ExecutionRecoverySource::Auto,
            ExecutionRecoveryReasonCode::Timeout,
            "auto retry",
        )
        .with_failure_source(ExecutionFailureSource::TransientTimeout),
        ExecutionRecoveryState::Retrying,
    );

    let task = task_with_execution_recovery(recovery);

    let default_delay = ReconciliationRunner::<tauri::Wry>::execution_failed_retry_delay(1, None);
    let git_delay = ReconciliationRunner::<tauri::Wry>::execution_failed_retry_delay(
        1,
        Some(ExecutionFailureSource::GitIsolation),
    );
    assert!(git_delay < default_delay);
    assert!(ReconciliationRunner::<tauri::Wry>::execution_next_retry_at(
        &task,
        Some(ExecutionFailureSource::GitIsolation),
    )
    .is_some());
    assert!(!ReconciliationRunner::<tauri::Wry>::has_recent_startup_recovery(&task));
}

#[test]
fn has_recent_startup_recovery_detects_recent_startup_source() {
    let mut recovery = ExecutionRecoveryMetadata::new();
    recovery.append_event_with_state(
        ExecutionRecoveryEvent::new(
            ExecutionRecoveryEventKind::AutoRetryTriggered,
            ExecutionRecoverySource::Startup,
            ExecutionRecoveryReasonCode::Timeout,
            "startup retry",
        ),
        ExecutionRecoveryState::Retrying,
    );
    let task = task_with_execution_recovery(recovery);

    assert!(ReconciliationRunner::<tauri::Wry>::has_recent_startup_recovery(&task));
}

#[tokio::test]
async fn record_auto_retry_metadata_updates_metadata_without_rewriting_task() {
    let app_state = crate::application::AppState::new_test();
    let execution_state = std::sync::Arc::new(crate::commands::ExecutionState::new());
    let transition_service = std::sync::Arc::new(crate::application::TaskTransitionService::<
        tauri::Wry,
    >::new(
        std::sync::Arc::clone(&app_state.task_repo),
        std::sync::Arc::clone(&app_state.task_dependency_repo),
        std::sync::Arc::clone(&app_state.project_repo),
        std::sync::Arc::clone(&app_state.chat_message_repo),
        std::sync::Arc::clone(&app_state.chat_attachment_repo),
        std::sync::Arc::clone(&app_state.chat_conversation_repo),
        std::sync::Arc::clone(&app_state.agent_run_repo),
        std::sync::Arc::clone(&app_state.ideation_session_repo),
        std::sync::Arc::clone(&app_state.activity_event_repo),
        std::sync::Arc::clone(&app_state.message_queue),
        std::sync::Arc::clone(&app_state.running_agent_registry),
        std::sync::Arc::clone(&execution_state),
        None,
        std::sync::Arc::clone(&app_state.memory_event_repo),
    ));
    let reconciler = ReconciliationRunner::new(
        std::sync::Arc::clone(&app_state.task_repo),
        std::sync::Arc::clone(&app_state.task_dependency_repo),
        std::sync::Arc::clone(&app_state.project_repo),
        std::sync::Arc::clone(&app_state.artifact_repo),
        std::sync::Arc::clone(&app_state.chat_conversation_repo),
        std::sync::Arc::clone(&app_state.chat_message_repo),
        std::sync::Arc::clone(&app_state.chat_attachment_repo),
        std::sync::Arc::clone(&app_state.ideation_session_repo),
        std::sync::Arc::clone(&app_state.activity_event_repo),
        std::sync::Arc::clone(&app_state.message_queue),
        std::sync::Arc::clone(&app_state.running_agent_registry),
        std::sync::Arc::clone(&app_state.memory_event_repo),
        std::sync::Arc::clone(&app_state.agent_run_repo),
        transition_service,
        execution_state,
        None,
    );

    let task = task_with_metadata(serde_json::json!({ "existing": true }).to_string());
    let task_id = task.id.clone();
    app_state.task_repo.create(task.clone()).await.unwrap();

    reconciler
        .record_auto_retry_metadata(&task, InternalStatus::Executing, 4)
        .await
        .expect("record retry metadata");

    let stored = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("task should be stored");
    let metadata: serde_json::Value =
        serde_json::from_str(stored.metadata.as_deref().unwrap()).unwrap();
    assert_eq!(metadata["existing"], serde_json::Value::Bool(true));
    assert_eq!(metadata["auto_retry_count_executing"], serde_json::json!(4));
}
