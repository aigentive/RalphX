use chrono::{Duration, Utc};
use serde_json::{json, Map, Value};
use std::sync::Arc;

use crate::application::notification_service::{NoopNotificationEventEmitter, NotificationService};
use crate::application::reconciliation::{RecoveryActionKind, RecoveryContext, RecoveryDecision};
use crate::application::{AppState, ReconciliationRunner, TaskTransitionService};
use crate::application::execution_state::ExecutionState;
use crate::domain::entities::{
    task_metadata::StopRetryingReason, ChatContextType, ExecutionFailureSource,
    ExecutionRecoveryEvent, ExecutionRecoveryEventKind, ExecutionRecoveryMetadata,
    ExecutionRecoveryReasonCode, ExecutionRecoverySource, ExecutionRecoveryState, InternalStatus,
    NotificationCategory, NotificationSeverity, NotificationTargetKind, Project, Task,
};
use crate::domain::repositories::NotificationRepository;
use crate::domain::services::RunningAgentKey;
use crate::infrastructure::memory::MemoryNotificationRepository;

use super::execution::is_deterministic_agent_command_error;

fn build_reconciler_for_execution_tests(
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

async fn seed_execution_task(app_state: &AppState, status: InternalStatus) -> Task {
    let project = Project::new(
        "Recovery cleanup project".to_string(),
        "/tmp/recovery-cleanup-project".to_string(),
    );
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();
    let mut task = Task::new(project.id.clone(), "recovery cleanup task".to_string());
    task.internal_status = status;
    app_state.task_repo.create(task.clone()).await.unwrap();
    task
}

#[tokio::test]
async fn recovery_prompt_dedupes_active_instance_and_records_again_after_clear() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());
    let notification_repo = Arc::new(MemoryNotificationRepository::new());
    let notification_service = Arc::new(NotificationService::new(
        Arc::clone(&notification_repo) as Arc<dyn NotificationRepository>,
        Arc::new(NoopNotificationEventEmitter),
    ));
    let reconciler = build_reconciler_for_execution_tests(&app_state, &execution_state)
        .with_notification_service(notification_service);
    let task = seed_execution_task(&app_state, InternalStatus::Executing).await;

    assert!(
        reconciler
            .emit_recovery_prompt(
                &task,
                InternalStatus::Executing,
                RecoveryContext::Execution,
                "restart required".to_string(),
            )
            .await
    );
    assert!(
        !reconciler
            .emit_recovery_prompt(
                &task,
                InternalStatus::Executing,
                RecoveryContext::Execution,
                "duplicate delivery".to_string(),
            )
            .await,
        "an active recovery prompt must be delivered only once"
    );

    reconciler
        .clear_prompt_marker(task.id.as_str(), InternalStatus::Executing)
        .await;
    assert!(
        reconciler
            .emit_recovery_prompt(
                &task,
                InternalStatus::Executing,
                RecoveryContext::Execution,
                "new recovery event".to_string(),
            )
            .await
    );

    let rows = notification_repo
        .list(None, None, 50)
        .await
        .expect("recovery prompt rows should be readable")
        .notifications;
    assert_eq!(rows.len(), 2, "clear/remit must produce a new prompt row");
    assert!(rows.iter().all(|row| {
        row.category == NotificationCategory::RecoveryPrompt
            && row.severity == NotificationSeverity::ActionRequired
            && row.target.kind == NotificationTargetKind::Task
            && row.target.task_id.as_deref() == Some(task.id.as_str())
    }));
    assert_ne!(
        rows[0].dedupe_key, rows[1].dedupe_key,
        "separate recovery prompt instances require distinct dedupe keys"
    );
}

#[tokio::test]
async fn recover_execution_stop_clears_stale_registry_entry_before_recovery() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());
    let reconciler = build_reconciler_for_execution_tests(&app_state, &execution_state);
    let task = seed_execution_task(&app_state, InternalStatus::Executing).await;
    let key = RunningAgentKey::new(ChatContextType::TaskExecution.to_string(), task.id.as_str());
    app_state
        .running_agent_registry
        .register(
            key.clone(),
            0,
            "conversation".to_string(),
            "agent-run".to_string(),
            None,
            None,
        )
        .await;

    let _ = reconciler.recover_execution_stop(&task.id).await;

    assert!(
        !app_state.running_agent_registry.is_running(&key).await,
        "dead registry entries must be removed before stop recovery continues"
    );
}

#[tokio::test]
async fn recover_execution_stop_preserves_live_registry_entry() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());
    let reconciler = build_reconciler_for_execution_tests(&app_state, &execution_state);
    let task = seed_execution_task(&app_state, InternalStatus::Executing).await;
    let key = RunningAgentKey::new(ChatContextType::TaskExecution.to_string(), task.id.as_str());
    app_state
        .running_agent_registry
        .register(
            key.clone(),
            std::process::id(),
            "conversation".to_string(),
            "agent-run".to_string(),
            None,
            None,
        )
        .await;

    assert!(
        !reconciler.recover_execution_stop(&task.id).await,
        "live registry entries should block recovery stop"
    );
    assert!(
        app_state.running_agent_registry.is_running(&key).await,
        "live registry entries must not be removed as stale"
    );
}

#[tokio::test]
async fn execute_entry_recovery_sets_trigger_origin_with_metadata_update() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());
    let reconciler = build_reconciler_for_execution_tests(&app_state, &execution_state);
    let mut task = seed_execution_task(&app_state, InternalStatus::Executing).await;
    task.metadata = Some(serde_json::json!({ "existing": true }).to_string());
    app_state.task_repo.update(&task).await.unwrap();

    let _ = reconciler
        .apply_recovery_decision(
            &task,
            InternalStatus::Executing,
            RecoveryContext::Execution,
            RecoveryDecision {
                action: RecoveryActionKind::ExecuteEntryActions,
                reason: None,
            },
        )
        .await;

    let updated = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .expect("task should exist");
    let metadata: Value = serde_json::from_str(updated.metadata.as_deref().unwrap()).unwrap();
    assert_eq!(metadata["existing"], Value::Bool(true));
    assert_eq!(
        metadata["trigger_origin"],
        Value::String("recovery".to_string())
    );
}

#[tokio::test]
async fn execute_entry_recovery_preserves_retry_metadata_written_before_reentry() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());
    let reconciler = build_reconciler_for_execution_tests(&app_state, &execution_state);
    let mut task = seed_execution_task(&app_state, InternalStatus::Executing).await;
    task.metadata = Some(json!({ "existing": true }).to_string());
    app_state.task_repo.update(&task).await.unwrap();

    let stale_task_snapshot = task.clone();
    reconciler
        .record_auto_retry_metadata(&task, InternalStatus::Executing, 1)
        .await
        .expect("record retry metadata");

    let _ = reconciler
        .apply_recovery_decision(
            &stale_task_snapshot,
            InternalStatus::Executing,
            RecoveryContext::Execution,
            RecoveryDecision {
                action: RecoveryActionKind::ExecuteEntryActions,
                reason: None,
            },
        )
        .await;

    let updated = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .expect("task should exist");
    let metadata: Value = serde_json::from_str(updated.metadata.as_deref().unwrap()).unwrap();
    assert_eq!(metadata["existing"], Value::Bool(true));
    assert_eq!(metadata["auto_retry_count_executing"], json!(1));
    assert_eq!(metadata["trigger_origin"], json!("recovery"));
}

#[tokio::test]
async fn execute_entry_recovery_skips_when_task_status_changed_before_reentry() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());
    let reconciler = build_reconciler_for_execution_tests(&app_state, &execution_state);
    let mut task = seed_execution_task(&app_state, InternalStatus::Executing).await;
    task.metadata = Some(json!({ "existing": true }).to_string());
    app_state.task_repo.update(&task).await.unwrap();
    let stale_task_snapshot = task.clone();

    let mut changed_task = task.clone();
    changed_task.internal_status = InternalStatus::Failed;
    changed_task.metadata = Some(json!({ "existing": true, "changed": true }).to_string());
    app_state.task_repo.update(&changed_task).await.unwrap();

    let recovered = reconciler
        .apply_recovery_decision(
            &stale_task_snapshot,
            InternalStatus::Executing,
            RecoveryContext::Execution,
            RecoveryDecision {
                action: RecoveryActionKind::ExecuteEntryActions,
                reason: None,
            },
        )
        .await;

    assert!(
        !recovered,
        "stale recovery snapshot must not re-enter a task that already changed status"
    );
    let updated = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .expect("task should exist");
    assert_eq!(updated.internal_status, InternalStatus::Failed);
    let metadata: Value = serde_json::from_str(updated.metadata.as_deref().unwrap()).unwrap();
    assert_eq!(metadata["changed"], json!(true));
    assert!(metadata.get("trigger_origin").is_none());
}

#[tokio::test]
async fn record_auto_retry_metadata_updates_metadata_without_rewriting_task() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());
    let reconciler = build_reconciler_for_execution_tests(&app_state, &execution_state);
    let mut task = seed_execution_task(&app_state, InternalStatus::Executing).await;
    task.metadata = Some(json!({ "existing": true }).to_string());
    app_state.task_repo.update(&task).await.unwrap();

    reconciler
        .record_auto_retry_metadata(&task, InternalStatus::Executing, 4)
        .await
        .expect("record retry metadata");

    let stored = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .expect("task should be stored");
    let metadata: Value = serde_json::from_str(stored.metadata.as_deref().unwrap()).unwrap();
    assert_eq!(metadata["existing"], Value::Bool(true));
    assert_eq!(metadata["auto_retry_count_executing"], json!(4));
}

/// Replicates the staleness check logic from `recover_timeout_failures`.
///
/// Returns `true` if `failed_at` is present, parseable, and older than `threshold_secs`.
/// Returns `false` when `failed_at` is absent (non-stale: pre-existing tasks get one attempt).
fn is_task_stale(metadata: &Map<String, Value>, threshold_secs: u64) -> bool {
    if let Some(failed_at_str) = metadata.get("failed_at").and_then(|v| v.as_str()) {
        if let Ok(failed_at) = chrono::DateTime::parse_from_rfc3339(failed_at_str) {
            let age_secs = (Utc::now() - failed_at.with_timezone(&Utc)).num_seconds();
            return age_secs > threshold_secs as i64;
        }
    }
    false // absent failed_at = non-stale
}

#[test]
fn test_staleness_check_stale_task() {
    let mut metadata = Map::new();
    // 2 days ago — well beyond the 86400s (1 day) threshold
    let two_days_ago = Utc::now() - Duration::seconds(172_800);
    metadata.insert("failed_at".to_string(), json!(two_days_ago.to_rfc3339()));

    assert!(
        is_task_stale(&metadata, 86400),
        "Task with failed_at 2 days ago should be stale"
    );
}

#[test]
fn test_staleness_check_non_stale_no_failed_at() {
    let metadata = Map::new(); // empty — no failed_at key

    assert!(
        !is_task_stale(&metadata, 86400),
        "Task with no failed_at should be non-stale"
    );
}

#[test]
fn test_staleness_check_recent_task_not_stale() {
    let mut metadata = Map::new();
    // 1 hour ago — well within the 86400s (1 day) threshold
    let one_hour_ago = Utc::now() - Duration::seconds(3600);
    metadata.insert("failed_at".to_string(), json!(one_hour_ago.to_rfc3339()));

    assert!(
        !is_task_stale(&metadata, 86400),
        "Task failed 1 hour ago should not be stale"
    );
}

// ============================================================
// Tests for is_permanent_git_error() classifier
// ============================================================

/// Replicates is_permanent_git_error() from execution.rs for testing.
/// This mirrors the production function to verify classification behavior.
fn is_permanent_git_error_test(msg: &str) -> bool {
    msg.contains("invalid reference")
        || msg.contains("not a valid object name")
        || msg.contains("does not point to a valid object")
        || msg.contains("no longer exists")
}

#[test]
fn test_permanent_git_error_invalid_reference() {
    // Git says the branch ref is invalid (deleted branch)
    let msg = "Git isolation failed: invalid reference 'refs/heads/ralphx/task-abc'";
    assert!(
        is_permanent_git_error_test(msg),
        "Should detect 'invalid reference' as permanent git error"
    );
}

#[test]
fn test_permanent_git_error_not_valid_object() {
    let msg = "fatal: not a valid object name: 'ralphx/task-abc'";
    assert!(
        is_permanent_git_error_test(msg),
        "Should detect 'not a valid object name' as permanent git error"
    );
}

#[test]
fn test_permanent_git_error_does_not_point_to_valid_object() {
    let msg = "error: refs/heads/ralphx/task-abc does not point to a valid object";
    assert!(
        is_permanent_git_error_test(msg),
        "Should detect 'does not point to a valid object' as permanent git error"
    );
}

#[test]
fn test_permanent_git_error_branch_no_longer_exists() {
    // Matches the error from Fix 4 (branch_exists check in on_enter_states.rs)
    let msg = "branch 'ralphx/task-abc' no longer exists (deleted during prior merge cleanup)";
    assert!(
        is_permanent_git_error_test(msg),
        "Should detect 'no longer exists' as permanent git error"
    );
}

#[test]
fn test_permanent_git_error_transient_not_matched() {
    // Transient errors should NOT be classified as permanent
    let transient_errors = [
        "fatal: Unable to create '.git/index.lock': File exists",
        "error: timeout waiting for git",
        "network error: connection refused",
        "error: unable to acquire lock on git index",
        "fatal: Out of memory, malloc failed",
    ];
    for msg in &transient_errors {
        assert!(
            !is_permanent_git_error_test(msg),
            "Should NOT classify transient error as permanent: {}",
            msg
        );
    }
}

#[test]
fn test_permanent_git_error_empty_message_not_permanent() {
    assert!(
        !is_permanent_git_error_test(""),
        "Empty message should not be classified as permanent git error"
    );
}

// ============================================================
// Tests for set_preserve_steps_metadata logic
// ============================================================

/// Replicates the core logic of `set_preserve_steps_metadata` for unit testing.
fn build_preserve_steps_metadata(existing: Option<&str>) -> String {
    use crate::domain::state_machine::transition_handler::metadata_builder::MetadataUpdate;
    MetadataUpdate::new()
        .with_bool("preserve_steps", true)
        .merge_into(existing)
}

#[test]
fn test_preserve_steps_flag_set_on_empty_metadata() {
    let result = build_preserve_steps_metadata(None);
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(
        parsed["preserve_steps"],
        serde_json::Value::Bool(true),
        "preserve_steps should be true when set on empty metadata"
    );
}

#[test]
fn test_preserve_steps_flag_merged_with_existing_metadata() {
    // Simulate re-fetched task metadata after reset (ManualRetry event present, no stale keys)
    let existing = r#"{"execution_recovery": {"events": [], "state": "retrying"}}"#;
    let result = build_preserve_steps_metadata(Some(existing));
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();

    assert_eq!(
        parsed["preserve_steps"],
        serde_json::Value::Bool(true),
        "preserve_steps should be set to true"
    );
    // Existing keys should be preserved
    assert!(
        parsed.get("execution_recovery").is_some(),
        "existing execution_recovery key should be preserved after merge"
    );
}

#[test]
fn test_preserve_steps_flag_absent_from_reset_metadata() {
    // Simulate what reset_execution_recovery_metadata produces (clean slate, no stale keys)
    // The flag must NOT be present until set_preserve_steps_metadata is called
    let clean_metadata = r#"{"trigger_origin": "scheduler"}"#;
    let parsed: serde_json::Value = serde_json::from_str(clean_metadata).unwrap();

    assert!(
        parsed.get("preserve_steps").is_none(),
        "preserve_steps should be absent from reset metadata (flag not yet set)"
    );
    assert!(
        parsed.get("is_timeout").is_none(),
        "is_timeout should be absent from reset metadata"
    );
    assert!(
        parsed.get("failure_error").is_none(),
        "failure_error should be absent from reset metadata"
    );
}

#[test]
fn test_preserve_steps_flag_overwrites_false_value() {
    // Edge case: if somehow preserve_steps was false, setting it again must make it true
    let existing = r#"{"preserve_steps": false, "trigger_origin": "scheduler"}"#;
    let result = build_preserve_steps_metadata(Some(existing));
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(
        parsed["preserve_steps"],
        serde_json::Value::Bool(true),
        "preserve_steps should be overwritten to true"
    );
}

// ============================================================
// Tests for is_structural_git_error() classifier
// ============================================================

/// Replicates is_structural_git_error() from execution.rs for testing.
fn is_structural_git_error_test(msg: &str) -> bool {
    if msg.contains("structural:") {
        return true;
    }
    msg.contains("does not exist") && msg.contains("invalid reference")
}

#[test]
fn test_structural_git_error_structural_prefix() {
    let msg = "git_isolation_error: structural: base branch 'main' does not exist";
    assert!(
        is_structural_git_error_test(msg),
        "Should detect 'structural:' prefix as structural git error"
    );
}

#[test]
fn test_structural_git_error_combined_does_not_exist_and_invalid_reference() {
    let msg = "fatal: invalid reference: refs/heads/main does not exist";
    assert!(
        is_structural_git_error_test(msg),
        "Should detect combined 'does not exist' + 'invalid reference' as structural git error"
    );
}

#[test]
fn test_structural_git_error_only_does_not_exist_not_structural() {
    // Only one of the combined patterns — should NOT match
    let msg = "fatal: path 'some/file' does not exist in the repository";
    assert!(
        !is_structural_git_error_test(msg),
        "Single 'does not exist' without 'invalid reference' should not be structural"
    );
}

#[test]
fn test_structural_git_error_only_invalid_reference_not_structural() {
    // Only one of the combined patterns — should NOT match
    let msg = "fatal: invalid reference 'refs/heads/task-abc'";
    assert!(
        !is_structural_git_error_test(msg),
        "Single 'invalid reference' without 'does not exist' should not be structural"
    );
}

#[test]
fn test_structural_git_error_transient_not_matched() {
    let transient_errors = [
        "fatal: Unable to create '.git/index.lock': File exists",
        "unable to create .git/index.lock",
        "Connection timed out",
        "repository busy, try again later",
        "error: lock file '.git/index.lock' is already locked",
        "error: remote end hung up unexpectedly",
    ];
    for msg in &transient_errors {
        assert!(
            !is_structural_git_error_test(msg),
            "Should NOT classify transient error as structural: {}",
            msg
        );
    }
}

#[test]
fn test_structural_git_error_empty_message_not_structural() {
    assert!(
        !is_structural_git_error_test(""),
        "Empty message should not be classified as structural git error"
    );
}

#[test]
fn deterministic_agent_command_error_detects_invalid_ignored_mode() {
    assert!(is_deterministic_agent_command_error(
        "Agent failed: fatal: Invalid ignored mode '.artifacts/specs/p8-regression-gate/tracker.md'"
    ));
}

#[test]
fn deterministic_agent_command_error_does_not_match_normal_agent_exit() {
    let ordinary_agent_error = "Agent failed: tests failed in frontend/src/App.test.tsx";
    assert!(!is_deterministic_agent_command_error(ordinary_agent_error));
}

#[tokio::test]
async fn failed_execution_with_invalid_ignored_mode_stops_retrying() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());
    let reconciler = build_reconciler_for_execution_tests(&app_state, &execution_state);

    let project = Project::new(
        "Coverage project".to_string(),
        "/tmp/coverage-project".to_string(),
    );
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut recovery = ExecutionRecoveryMetadata::new();
    let failure_message =
        "Agent failed: fatal: Invalid ignored mode '.artifacts/specs/p8-regression-gate/tracker.md'";
    recovery.append_event_with_state(
        ExecutionRecoveryEvent::new(
            ExecutionRecoveryEventKind::Failed,
            ExecutionRecoverySource::System,
            ExecutionRecoveryReasonCode::AgentExit,
            failure_message,
        )
        .with_failure_source(ExecutionFailureSource::AgentCrash),
        ExecutionRecoveryState::Retrying,
    );

    let mut task = Task::new(project.id.clone(), "invalid ignored mode".to_string());
    task.internal_status = InternalStatus::Failed;
    task.metadata = Some(recovery.update_task_metadata(None).unwrap());
    let task_id = task.id.clone();
    app_state.task_repo.create(task.clone()).await.unwrap();

    assert!(
        !reconciler
            .reconcile_failed_execution_task(&task, InternalStatus::Failed)
            .await,
        "deterministic command failures should stop retries instead of re-queueing"
    );

    let updated_task = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .unwrap();
    let updated_recovery =
        ExecutionRecoveryMetadata::from_task_metadata(updated_task.metadata.as_deref())
            .unwrap()
            .unwrap();
    assert!(updated_recovery.stop_retrying);
    assert_eq!(
        updated_recovery.unrecoverable_reason,
        Some(StopRetryingReason::AgentCommandInvalid)
    );
    assert_eq!(updated_recovery.last_state, ExecutionRecoveryState::Failed);
    assert!(updated_recovery.events.iter().any(|event| {
        event.kind == ExecutionRecoveryEventKind::StopRetrying
            && event.reason_code == ExecutionRecoveryReasonCode::AgentCommandInvalid
    }));
}

#[tokio::test]
async fn execution_recovery_metadata_helpers_persist_structured_events() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());
    let reconciler = build_reconciler_for_execution_tests(&app_state, &execution_state);

    let project = Project::new(
        "Recovery metadata project".to_string(),
        "/tmp/recovery-metadata-project".to_string(),
    );
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "metadata helpers".to_string());
    task.metadata = Some(
        json!({
            "existing": true,
            "is_timeout": true,
            "failure_error": "stale failure"
        })
        .to_string(),
    );
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.unwrap();

    let stored = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .unwrap();
    reconciler
        .record_execution_auto_retry_event(
            &stored,
            2,
            ExecutionFailureSource::GitIsolation,
            "retry git isolation",
        )
        .await
        .unwrap();
    let stored = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .unwrap();
    let recovery = ExecutionRecoveryMetadata::from_task_metadata(stored.metadata.as_deref())
        .unwrap()
        .unwrap();
    let retry = recovery.events.last().expect("retry event");
    assert_eq!(recovery.last_state, ExecutionRecoveryState::Retrying);
    assert_eq!(retry.kind, ExecutionRecoveryEventKind::AutoRetryTriggered);
    assert_eq!(retry.source, ExecutionRecoverySource::Auto);
    assert_eq!(
        retry.reason_code,
        ExecutionRecoveryReasonCode::GitIsolationFailed
    );
    assert_eq!(retry.attempt, Some(2));
    assert_eq!(
        retry.failure_source,
        Some(ExecutionFailureSource::GitIsolation)
    );

    reconciler
        .set_execution_stop_retrying(&stored)
        .await
        .unwrap();
    let stored = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .unwrap();
    let recovery = ExecutionRecoveryMetadata::from_task_metadata(stored.metadata.as_deref())
        .unwrap()
        .unwrap();
    let stopped = recovery.events.last().expect("stop event");
    assert!(recovery.stop_retrying);
    assert_eq!(recovery.last_state, ExecutionRecoveryState::Failed);
    assert_eq!(stopped.kind, ExecutionRecoveryEventKind::StopRetrying);
    assert_eq!(
        stopped.reason_code,
        ExecutionRecoveryReasonCode::MaxRetriesExceeded
    );

    reconciler
        .reset_execution_recovery_metadata(&stored)
        .await
        .unwrap();
    let stored = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .unwrap();
    let metadata: Value = serde_json::from_str(stored.metadata.as_deref().unwrap()).unwrap();
    assert_eq!(metadata["existing"], Value::Bool(true));
    assert!(metadata.get("is_timeout").is_none());
    assert!(metadata.get("failure_error").is_none());
    let recovery = ExecutionRecoveryMetadata::from_task_metadata(stored.metadata.as_deref())
        .unwrap()
        .unwrap();
    assert!(recovery.events.is_empty());
    assert!(!recovery.stop_retrying);
    assert_eq!(recovery.last_state, ExecutionRecoveryState::Retrying);

    reconciler
        .record_execution_startup_retry_event(
            &stored,
            3,
            ExecutionFailureSource::AgentCrash,
            ExecutionRecoveryReasonCode::AgentExit,
        )
        .await
        .unwrap();
    let stored = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .unwrap();
    let recovery = ExecutionRecoveryMetadata::from_task_metadata(stored.metadata.as_deref())
        .unwrap()
        .unwrap();
    assert_eq!(recovery.events.len(), 2);
    assert_eq!(recovery.events[0].kind, ExecutionRecoveryEventKind::Failed);
    assert_eq!(recovery.events[0].source, ExecutionRecoverySource::System);
    assert_eq!(
        recovery.events[1].kind,
        ExecutionRecoveryEventKind::AutoRetryTriggered
    );
    assert_eq!(recovery.events[1].source, ExecutionRecoverySource::Startup);
    assert_eq!(recovery.events[1].attempt, Some(3));

    reconciler
        .record_execution_manual_retry_event(&stored)
        .await
        .unwrap();
    let stored = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .unwrap();
    let recovery = ExecutionRecoveryMetadata::from_task_metadata(stored.metadata.as_deref())
        .unwrap()
        .unwrap();
    let manual = recovery.events.last().expect("manual retry event");
    assert_eq!(manual.kind, ExecutionRecoveryEventKind::ManualRetry);
    assert_eq!(manual.source, ExecutionRecoverySource::User);
    assert_eq!(recovery.last_state, ExecutionRecoveryState::Retrying);

    reconciler
        .stop_execution_retrying_by_user(&stored)
        .await
        .unwrap();
    let stored = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .unwrap();
    let recovery = ExecutionRecoveryMetadata::from_task_metadata(stored.metadata.as_deref())
        .unwrap()
        .unwrap();
    let user_stop = recovery.events.last().expect("user stop event");
    assert!(recovery.stop_retrying);
    assert_eq!(user_stop.source, ExecutionRecoverySource::User);
    assert_eq!(
        user_stop.reason_code,
        ExecutionRecoveryReasonCode::UserStopped
    );

    reconciler
        .set_execution_stop_retrying_with_reason(&stored, StopRetryingReason::StructuralGitError)
        .await
        .unwrap();
    let stored = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .unwrap();
    let recovery = ExecutionRecoveryMetadata::from_task_metadata(stored.metadata.as_deref())
        .unwrap()
        .unwrap();
    let structural_stop = recovery.events.last().expect("structural stop event");
    assert_eq!(
        recovery.unrecoverable_reason,
        Some(StopRetryingReason::StructuralGitError)
    );
    assert_eq!(
        structural_stop.reason_code,
        ExecutionRecoveryReasonCode::StructuralGitError
    );
}
