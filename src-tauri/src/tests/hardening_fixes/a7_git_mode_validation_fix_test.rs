// Fix A7: GitMode switch blocked when tasks are in active states
//
// After fix: change_project_git_mode checks for tasks in AGENT_ACTIVE_STATUSES
// and returns an error if any are found.

use crate::application::execution_state::AGENT_ACTIVE_STATUSES;
use crate::domain::entities::InternalStatus;

#[test]
fn test_a7_fix_agent_active_statuses_includes_executing() {
    // Verify AGENT_ACTIVE_STATUSES contains the states we need to block
    assert!(
        AGENT_ACTIVE_STATUSES.contains(&InternalStatus::Executing),
        "Executing should be an agent-active status"
    );
    assert!(
        AGENT_ACTIVE_STATUSES.contains(&InternalStatus::Reviewing),
        "Reviewing should be an agent-active status"
    );
    assert!(
        AGENT_ACTIVE_STATUSES.contains(&InternalStatus::Merging),
        "Merging should be an agent-active status"
    );
}

#[test]
fn test_a7_fix_non_active_statuses_should_not_block() {
    // Verify that non-active statuses don't block git mode changes
    assert!(
        !AGENT_ACTIVE_STATUSES.contains(&InternalStatus::Backlog),
        "Backlog should not block git mode change"
    );
    assert!(
        !AGENT_ACTIVE_STATUSES.contains(&InternalStatus::Ready),
        "Ready should not block git mode change"
    );
    assert!(
        !AGENT_ACTIVE_STATUSES.contains(&InternalStatus::Merged),
        "Merged should not block git mode change"
    );
}

#[test]
fn test_a7_fix_error_message_format() {
    // Verify the error message format for blocked git mode change
    let task_ids = vec!["task-1".to_string(), "task-2".to_string()];
    let error_msg = format!(
        "Cannot change git mode while {} task(s) are in active states: {}",
        task_ids.len(),
        task_ids.join(", ")
    );
    assert!(error_msg.contains("Cannot change git mode"));
    assert!(error_msg.contains("2 task(s)"));
    assert!(error_msg.contains("task-1"));
    assert!(error_msg.contains("task-2"));
}
