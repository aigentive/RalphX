use super::{build_execution_status_response, ExecutionStatusInput};

#[test]
fn build_execution_status_response_derives_can_start_and_blocked_until() {
    let response = build_execution_status_response(ExecutionStatusInput {
        is_paused: false,
        halt_mode: "running".to_string(),
        running_count: 1,
        max_concurrent: 3,
        global_max_concurrent: 10,
        queued_count: 2,
        queued_message_count: 1,
        provider_blocked: false,
        provider_blocked_until_epoch: 0,
        total_project_active: 2,
        global_running_count: 4,
        ideation_active: 1,
        ideation_idle: 0,
        ideation_waiting: 1,
        ideation_max_project: 5,
        ideation_max_global: 4,
    });

    assert!(response.can_start_task);
    assert_eq!(response.provider_blocked_until, None);
    assert_eq!(response.ideation_waiting, 1);
}

#[test]
fn build_execution_status_response_blocks_when_paused_or_at_capacity() {
    let paused = build_execution_status_response(ExecutionStatusInput {
        is_paused: true,
        halt_mode: "paused".to_string(),
        running_count: 0,
        max_concurrent: 2,
        global_max_concurrent: 5,
        queued_count: 0,
        queued_message_count: 0,
        provider_blocked: false,
        provider_blocked_until_epoch: 0,
        total_project_active: 0,
        global_running_count: 0,
        ideation_active: 0,
        ideation_idle: 0,
        ideation_waiting: 0,
        ideation_max_project: 2,
        ideation_max_global: 4,
    });
    assert!(!paused.can_start_task);

    let blocked = build_execution_status_response(ExecutionStatusInput {
        is_paused: false,
        halt_mode: "running".to_string(),
        running_count: 2,
        max_concurrent: 2,
        global_max_concurrent: 5,
        queued_count: 1,
        queued_message_count: 2,
        provider_blocked: true,
        provider_blocked_until_epoch: 123,
        total_project_active: 2,
        global_running_count: 5,
        ideation_active: 1,
        ideation_idle: 1,
        ideation_waiting: 2,
        ideation_max_project: 2,
        ideation_max_global: 4,
    });
    assert!(!blocked.can_start_task);
    assert_eq!(blocked.provider_blocked_until, Some(123));
}
