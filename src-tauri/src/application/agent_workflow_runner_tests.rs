use super::{require_successful_exit, require_transition};
use crate::error::AppError;

#[test]
fn accepted_lifecycle_transition_allows_runner_to_finish() {
    assert!(require_transition(true, "pause").is_ok());
}

#[test]
fn rejected_lifecycle_transition_prevents_stale_runner_success() {
    let error = require_transition(false, "cancellation").expect_err("stale CAS must fail closed");

    assert!(matches!(
        error,
        AppError::Conflict(message) if message == "Stale workflow cancellation rejected"
    ));
}

#[test]
fn unsuccessful_runner_exit_prevents_completion() {
    let error = require_successful_exit(false).expect_err("non-zero exit must fail closed");

    assert!(matches!(
        error,
        AppError::Infrastructure(message)
            if message == "Workflow runner exited unsuccessfully after completion"
    ));
}

#[test]
fn successful_runner_exit_allows_completion_transition() {
    assert!(require_successful_exit(true).is_ok());
}
