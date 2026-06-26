use super::*;

use crate::domain::entities::task_metadata::StopRetryingReason;

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
