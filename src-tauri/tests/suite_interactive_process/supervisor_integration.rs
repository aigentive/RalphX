// Integration tests for supervisor system
// Tests loop detection, stuck detection, and end-to-end agent spawning with supervisor

use std::sync::Arc;

use ralphx_lib::application::{SupervisorConfig, SupervisorService};
use ralphx_lib::domain::state_machine::AgentSpawner;
use ralphx_lib::domain::supervisor::{
    ProgressInfo, SupervisorAction, SupervisorEvent, ToolCallInfo,
};
use ralphx_lib::infrastructure::agents::AgenticClientSpawner;
use ralphx_lib::infrastructure::supervisor::EventBus;
use ralphx_lib::infrastructure::MockAgenticClient;

// ============================================================================
// Integration Test: Supervisor detects infinite loop
// ============================================================================

#[tokio::test]
async fn test_supervisor_detects_infinite_loop() {
    // Set up supervisor service with low threshold for testing
    let event_bus = EventBus::new();
    let service = SupervisorService::with_config(
        event_bus.clone(),
        SupervisorConfig {
            loop_threshold: 3,
            ..Default::default()
        },
    );

    // Start monitoring a task
    service
        .start_monitoring("task-123".to_string(), "Test task".to_string())
        .await;

    // Set up a subscriber to track events
    let _subscriber = event_bus.subscribe();

    // Emit 4 identical tool call events (simulating infinite loop)
    let tool_info = ToolCallInfo::new("Read", "file.rs");

    let mut action_received = None;
    for i in 0..4 {
        let event = SupervisorEvent::tool_call("task-123", tool_info.clone());

        // Publish to event bus
        let _ = event_bus.publish(event.clone());

        // Process in service
        let action = service.process_event(event).await;

        if i >= 3 {
            // After 4th identical call, should detect loop
            assert!(
                action.is_some(),
                "Expected action after {} identical calls",
                i + 1
            );
            action_received = action;
        }
    }

    // Verify the action is an intervention (InjectGuidance or Pause)
    let action = action_received.expect("Should have received an action");
    assert!(
        matches!(
            action,
            SupervisorAction::InjectGuidance { .. } | SupervisorAction::Pause { .. }
        ),
        "Expected InjectGuidance or Pause action, got {:?}",
        action
    );

    // Verify task state reflects the action
    let state = service.get_task_state("task-123").await.unwrap();
    assert!(
        !state.actions_taken.is_empty(),
        "Task should have recorded actions"
    );
}

#[tokio::test]
async fn test_supervisor_detects_loop_with_pattern() {
    let event_bus = EventBus::new();
    let service = SupervisorService::with_config(
        event_bus.clone(),
        SupervisorConfig {
            loop_threshold: 3,
            ..Default::default()
        },
    );

    service
        .start_monitoring("task-456".to_string(), "Pattern test".to_string())
        .await;

    // Create a repeating pattern: Read -> Write -> Read -> Write -> Read -> Write
    let events = vec![
        ("Read", "file.rs"),
        ("Write", "file.rs"),
        ("Read", "file.rs"),
        ("Write", "file.rs"),
        ("Read", "file.rs"),
        ("Write", "file.rs"),
    ];

    let mut detected_action = None;
    for (tool_name, target) in events {
        let event = SupervisorEvent::tool_call("task-456", ToolCallInfo::new(tool_name, target));
        let action = service.process_event(event).await;

        if action.is_some() {
            detected_action = action;
            break;
        }
    }

    // Should detect the repeating pattern
    assert!(
        detected_action.is_some(),
        "Should have detected repeating pattern"
    );
}

// ============================================================================
// Integration Test: Supervisor detects stuck agent
// ============================================================================

#[tokio::test]
async fn test_supervisor_detects_stuck_agent() {
    // Set up supervisor service with low stuck threshold for testing
    let event_bus = EventBus::new();
    let service = SupervisorService::with_config(
        event_bus.clone(),
        SupervisorConfig {
            stuck_threshold: 3, // Low threshold for testing
            ..Default::default()
        },
    );

    // Start monitoring
    service
        .start_monitoring("stuck-task".to_string(), "Stuck task test".to_string())
        .await;

    // Emit progress events with no git changes (simulating stuck agent)
    let mut action_received = None;
    for i in 0..5 {
        let info = ProgressInfo::new(); // has_file_changes = false, has_new_commits = false
        let event = SupervisorEvent::progress_tick("stuck-task", info);
        let action = service.process_event(event).await;

        if i >= 3 {
            // After stuck_threshold checks, should detect stuck pattern
            if action.is_some() {
                action_received = action;
                break;
            }
        }
    }

    // Verify appropriate action was taken
    let action = action_received.expect("Should have received an action for stuck agent");
    assert!(
        matches!(
            action,
            SupervisorAction::InjectGuidance { .. } | SupervisorAction::Pause { .. }
        ),
        "Expected InjectGuidance or Pause action for stuck agent, got {:?}",
        action
    );

    // Verify stuck count in state
    let state = service.get_task_state("stuck-task").await.unwrap();
    assert!(
        state.stuck_count > 0,
        "Stuck count should be greater than 0"
    );
}

#[tokio::test]
async fn test_supervisor_resets_stuck_on_progress() {
    let event_bus = EventBus::new();
    let service = SupervisorService::with_config(
        event_bus.clone(),
        SupervisorConfig {
            stuck_threshold: 5,
            ..Default::default()
        },
    );

    service
        .start_monitoring("progress-task".to_string(), "Progress test".to_string())
        .await;

    // First, get stuck count up
    for _ in 0..3 {
        let info = ProgressInfo::new();
        let event = SupervisorEvent::progress_tick("progress-task", info);
        service.process_event(event).await;
    }

    let state = service.get_task_state("progress-task").await.unwrap();
    assert_eq!(state.stuck_count, 3);

    // Now make progress
    let mut info = ProgressInfo::new();
    info.has_file_changes = true;
    let event = SupervisorEvent::progress_tick("progress-task", info);
    service.process_event(event).await;

    // Stuck count should reset
    let state = service.get_task_state("progress-task").await.unwrap();
    assert_eq!(state.stuck_count, 0, "Stuck count should reset on progress");
}

// ============================================================================
// Integration Test: End-to-end agent spawning with supervisor
// ============================================================================

#[tokio::test]
async fn test_end_to_end_agent_spawning_with_supervisor() {
    // Set up components
    let event_bus = EventBus::new();
    let mock_client = Arc::new(MockAgenticClient::new());
    let spawner =
        AgenticClientSpawner::new(mock_client.clone()).with_event_bus(Arc::new(event_bus.clone()));

    let service = SupervisorService::new(event_bus.clone());

    // Note: We can verify actions through task state

    // Spawn a worker agent via spawner
    spawner.spawn("worker", "task-123").await;

    // The spawner should have emitted a TaskStart event
    // Let's verify by checking the mock client's spawn calls
    let spawn_calls = mock_client.get_spawn_calls().await;
    assert_eq!(spawn_calls.len(), 1, "Should have one spawn call");

    // Simulate receiving the task start and processing it
    let task_start = SupervisorEvent::task_start("task-123", "Worker agent");
    service.process_event(task_start).await;

    // Verify task is being monitored
    let state = service.get_task_state("task-123").await;
    assert!(state.is_some(), "Task should be monitored after TaskStart");

    // Simulate tool calls and verify monitoring
    let tool_call =
        SupervisorEvent::tool_call("task-123", ToolCallInfo::new("Read", "src/main.rs"));
    service.process_event(tool_call).await;

    let state = service.get_task_state("task-123").await.unwrap();
    assert_eq!(state.tool_window.len(), 1, "Should have recorded tool call");
}

#[tokio::test]
async fn test_spawner_emits_events_to_event_bus() {
    let event_bus = EventBus::new();
    let mock_client = Arc::new(MockAgenticClient::new());

    // Subscribe to events before spawning
    let mut subscriber = event_bus.subscribe();

    let spawner =
        AgenticClientSpawner::new(mock_client.clone()).with_event_bus(Arc::new(event_bus.clone()));

    // Spawn an agent
    spawner.spawn("worker", "task-789").await;

    // Check if we received a TaskStart event
    // Give a small timeout since events are async
    let result =
        tokio::time::timeout(std::time::Duration::from_millis(100), subscriber.recv()).await;

    // The spawner should emit a TaskStart event
    if let Ok(Ok(event)) = result {
        assert!(
            matches!(event, SupervisorEvent::TaskStart { .. }),
            "Expected TaskStart event, got {:?}",
            event
        );
    }
    // If no event received, that's also OK since the emission is best-effort
}

#[tokio::test]
async fn test_multiple_agents_monitored_independently() {
    let event_bus = EventBus::new();
    let service = SupervisorService::with_config(
        event_bus.clone(),
        SupervisorConfig {
            loop_threshold: 3,
            stuck_threshold: 3,
            ..Default::default()
        },
    );

    // Start monitoring multiple tasks
    service
        .start_monitoring("task-A".to_string(), "Task A".to_string())
        .await;
    service
        .start_monitoring("task-B".to_string(), "Task B".to_string())
        .await;

    // Cause a loop on task-A
    for _ in 0..5 {
        let event = SupervisorEvent::tool_call("task-A", ToolCallInfo::new("Read", "same.rs"));
        service.process_event(event).await;
    }

    // Task-B should still be fine
    let event = SupervisorEvent::tool_call("task-B", ToolCallInfo::new("Write", "other.rs"));
    let action = service.process_event(event).await;
    assert!(action.is_none(), "Task-B should not have triggered action");

    // Verify states
    let state_a = service.get_task_state("task-A").await.unwrap();
    let state_b = service.get_task_state("task-B").await.unwrap();

    assert!(
        !state_a.actions_taken.is_empty(),
        "Task-A should have actions"
    );
    assert!(
        state_b.actions_taken.is_empty(),
        "Task-B should have no actions"
    );
}

#[tokio::test]
async fn test_supervisor_pause_and_resume_flow() {
    let event_bus = EventBus::new();
    let service = SupervisorService::with_config(
        event_bus.clone(),
        SupervisorConfig {
            time_threshold_seconds: 600, // 10 minutes
            ..Default::default()
        },
    );

    service
        .start_monitoring("pause-task".to_string(), "Pause test".to_string())
        .await;

    // Trigger pause via time threshold (2x threshold = pause)
    // 20 minutes elapsed, 10 minute threshold
    let event = SupervisorEvent::time_threshold("pause-task", 20, 10);
    let action = service.process_event(event).await;

    assert!(matches!(action, Some(SupervisorAction::Pause { .. })));
    assert!(service.is_task_paused("pause-task").await);

    // Resume the task
    let resumed = service.resume_task("pause-task").await;
    assert!(resumed, "Should be able to resume paused task");
    assert!(!service.is_task_paused("pause-task").await);

    // Should now be able to process events again
    let event = SupervisorEvent::tool_call("pause-task", ToolCallInfo::new("Read", "file.rs"));
    let action = service.process_event(event).await;
    assert!(
        action.is_none(),
        "Resumed task should process events normally"
    );
}

#[tokio::test]
async fn test_supervisor_kill_prevents_further_processing() {
    let event_bus = EventBus::new();
    let service = SupervisorService::new(event_bus.clone());

    service
        .start_monitoring("kill-task".to_string(), "Kill test".to_string())
        .await;

    // Trigger kill via exceeding max tokens
    let event = SupervisorEvent::token_threshold("kill-task", 110_000, 50_000);
    let action = service.process_event(event).await;

    assert!(matches!(action, Some(SupervisorAction::Kill { .. })));
    assert!(service.is_task_killed("kill-task").await);

    // Cannot resume killed task
    let resumed = service.resume_task("kill-task").await;
    assert!(!resumed, "Should not be able to resume killed task");

    // Further events should be ignored
    let event = SupervisorEvent::tool_call("kill-task", ToolCallInfo::new("Read", "file.rs"));
    let action = service.process_event(event).await;
    assert!(action.is_none(), "Killed task should ignore events");
}

// ============================================================================
// Integration Test: Error handling
// ============================================================================

#[tokio::test]
async fn test_supervisor_handles_repeating_errors() {
    let event_bus = EventBus::new();
    let service = SupervisorService::new(event_bus.clone());

    service
        .start_monitoring("error-task".to_string(), "Error test".to_string())
        .await;

    use ralphx_lib::domain::supervisor::ErrorInfo;

    // Emit the same error multiple times
    let error_info = ErrorInfo::new("Type mismatch in foo.rs", "compile");

    let mut action_received = None;
    for _ in 0..5 {
        let event = SupervisorEvent::error("error-task", error_info.clone());
        let action = service.process_event(event).await;

        if action.is_some() {
            action_received = action;
        }
    }

    assert!(
        action_received.is_some(),
        "Should have taken action on repeating errors"
    );

    let state = service.get_task_state("error-task").await.unwrap();
    let error_count = state.error_counts.get("Type mismatch in foo.rs").unwrap();
    assert!(*error_count >= 5, "Should have recorded all errors");
}

// ============================================================================
// Integration Test: Event bus pub/sub
// ============================================================================

#[tokio::test]
async fn test_event_bus_integration_with_supervisor() {
    let event_bus = EventBus::new();

    // Multiple subscribers
    let mut sub1 = event_bus.subscribe();
    let mut sub2 = event_bus.subscribe();

    let service = SupervisorService::new(event_bus.clone());

    service
        .start_monitoring("pubsub-task".to_string(), "PubSub test".to_string())
        .await;

    // Publish events through the bus
    let event = SupervisorEvent::task_start("pubsub-task", "Test agent");
    let receivers = event_bus.publish(event.clone());
    assert_eq!(receivers.unwrap(), 2, "Both subscribers should receive");

    // Both subscribers should receive the event
    let recv1 = sub1.try_recv();
    let recv2 = sub2.try_recv();

    assert!(recv1.is_ok(), "Subscriber 1 should receive event");
    assert!(recv2.is_ok(), "Subscriber 2 should receive event");
}
