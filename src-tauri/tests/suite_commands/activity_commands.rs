use ralphx_lib::application::AppState;
use ralphx_lib::commands::activity_commands::{
    count_session_activity_events, count_task_activity_events, list_all_activity_events,
    list_session_activity_events, list_task_activity_events, ActivityEventFilterInput,
};
use ralphx_lib::domain::entities::{
    ActivityEvent, ActivityEventRole, ActivityEventType, IdeationSessionId, InternalStatus, TaskId,
};
use tauri::Manager;

fn activity_command_app() -> tauri::App<tauri::test::MockRuntime> {
    tauri::test::mock_builder()
        .manage(AppState::new_test())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app should build")
}

#[test]
fn activity_event_filter_input_to_domain_empty() {
    let input = ActivityEventFilterInput::default();
    let filter = input.to_domain_filter();
    assert!(filter.is_empty());
}

#[test]
fn activity_event_filter_input_to_domain_with_event_types() {
    let input = ActivityEventFilterInput {
        event_types: Some(vec!["thinking".to_string(), "text".to_string()]),
        roles: None,
        statuses: None,
        task_id: None,
        session_id: None,
    };
    let filter = input.to_domain_filter();
    assert!(!filter.is_empty());
    assert!(filter.event_types.is_some());
    assert_eq!(filter.event_types.unwrap().len(), 2);
}

#[test]
fn activity_event_filter_input_to_domain_with_roles() {
    let input = ActivityEventFilterInput {
        event_types: None,
        roles: Some(vec!["agent".to_string()]),
        statuses: None,
        task_id: None,
        session_id: None,
    };
    let filter = input.to_domain_filter();
    assert!(!filter.is_empty());
    assert!(filter.roles.is_some());
}

#[test]
fn activity_event_filter_input_to_domain_with_statuses() {
    let input = ActivityEventFilterInput {
        event_types: None,
        roles: None,
        statuses: Some(vec!["executing".to_string()]),
        task_id: None,
        session_id: None,
    };
    let filter = input.to_domain_filter();
    assert!(!filter.is_empty());
    assert!(filter.statuses.is_some());
}

#[test]
fn activity_event_filter_input_to_domain_ignores_invalid() {
    let input = ActivityEventFilterInput {
        event_types: Some(vec!["invalid_type".to_string()]),
        roles: Some(vec!["invalid_role".to_string()]),
        statuses: Some(vec!["invalid_status".to_string()]),
        task_id: None,
        session_id: None,
    };
    let filter = input.to_domain_filter();
    // Invalid values are filtered out, leaving an empty filter
    assert!(filter.is_empty());
}

#[test]
fn activity_event_filter_input_to_domain_with_task_id() {
    let input = ActivityEventFilterInput {
        event_types: None,
        roles: None,
        statuses: None,
        task_id: Some("test-task-123".to_string()),
        session_id: None,
    };
    let filter = input.to_domain_filter();
    assert!(!filter.is_empty());
    assert!(filter.task_id.is_some());
    assert_eq!(filter.task_id.unwrap().as_str(), "test-task-123");
}

#[test]
fn activity_event_filter_input_to_domain_with_session_id() {
    let input = ActivityEventFilterInput {
        event_types: None,
        roles: None,
        statuses: None,
        task_id: None,
        session_id: Some("test-session-456".to_string()),
    };
    let filter = input.to_domain_filter();
    assert!(!filter.is_empty());
    assert!(filter.session_id.is_some());
    assert_eq!(filter.session_id.unwrap().as_str(), "test-session-456");
}

#[tokio::test]
async fn list_task_activity_events_clamps_limit_and_applies_filter() {
    let app = activity_command_app();
    let state = app.state::<AppState>();
    let task_id = TaskId::from_string("task-activity-command".to_string());

    state
        .activity_event_repo
        .save(
            ActivityEvent::new_task_event(task_id.clone(), ActivityEventType::Text, "visible")
                .with_role(ActivityEventRole::Agent)
                .with_status(InternalStatus::Executing)
                .with_metadata(r#"{"tool":"bash"}"#),
        )
        .await
        .expect("visible event saves");
    state
        .activity_event_repo
        .save(
            ActivityEvent::new_task_event(task_id.clone(), ActivityEventType::Error, "hidden")
                .with_role(ActivityEventRole::System)
                .with_status(InternalStatus::Backlog),
        )
        .await
        .expect("hidden event saves");

    let page = list_task_activity_events(
        task_id.as_str().to_string(),
        None,
        Some(250),
        Some(ActivityEventFilterInput {
            event_types: Some(vec!["text".to_string()]),
            roles: Some(vec!["agent".to_string()]),
            statuses: Some(vec!["executing".to_string()]),
            task_id: None,
            session_id: None,
        }),
        app.state::<AppState>(),
    )
    .await
    .expect("task events should list");

    assert_eq!(page.events.len(), 1);
    assert_eq!(page.events[0].content, "visible");
    assert_eq!(page.events[0].event_type, "text");
    assert_eq!(page.events[0].role, "agent");
    assert_eq!(page.events[0].internal_status.as_deref(), Some("executing"));
    assert_eq!(page.events[0].metadata.as_deref(), Some(r#"{"tool":"bash"}"#));
    assert!(!page.has_more);
}

#[tokio::test]
async fn list_session_activity_events_and_count_use_session_filter() {
    let app = activity_command_app();
    let state = app.state::<AppState>();
    let session_id = IdeationSessionId::from_string("session-activity-command".to_string());

    state
        .activity_event_repo
        .save(ActivityEvent::new_session_event(
            session_id.clone(),
            ActivityEventType::ToolCall,
            "visible session event",
        ))
        .await
        .expect("session event saves");
    state
        .activity_event_repo
        .save(ActivityEvent::new_session_event(
            IdeationSessionId::from_string("other-session".to_string()),
            ActivityEventType::ToolCall,
            "other session event",
        ))
        .await
        .expect("other session event saves");

    let page = list_session_activity_events(
        session_id.as_str().to_string(),
        None,
        None,
        None,
        app.state::<AppState>(),
    )
    .await
    .expect("session events should list");
    assert_eq!(page.events.len(), 1);
    assert_eq!(page.events[0].ideation_session_id.as_deref(), Some(session_id.as_str()));

    let count = count_session_activity_events(
        session_id.as_str().to_string(),
        Some(ActivityEventFilterInput {
            event_types: Some(vec!["tool_call".to_string()]),
            roles: None,
            statuses: None,
            task_id: None,
            session_id: None,
        }),
        app.state::<AppState>(),
    )
    .await
    .expect("session events should count");
    assert_eq!(count, 1);
}

#[tokio::test]
async fn list_all_activity_events_can_narrow_to_task_and_count_task_events() {
    let app = activity_command_app();
    let state = app.state::<AppState>();
    let task_id = TaskId::from_string("task-all-activity-command".to_string());

    state
        .activity_event_repo
        .save(ActivityEvent::new_task_event(
            task_id.clone(),
            ActivityEventType::Thinking,
            "visible task event",
        ))
        .await
        .expect("task event saves");
    state
        .activity_event_repo
        .save(ActivityEvent::new_session_event(
            IdeationSessionId::from_string("session-all-activity-command".to_string()),
            ActivityEventType::Thinking,
            "session event",
        ))
        .await
        .expect("session event saves");

    let page = list_all_activity_events(
        None,
        Some(10),
        Some(ActivityEventFilterInput {
            event_types: None,
            roles: None,
            statuses: None,
            task_id: Some(task_id.as_str().to_string()),
            session_id: None,
        }),
        app.state::<AppState>(),
    )
    .await
    .expect("all events should list");
    assert_eq!(page.events.len(), 1);
    assert_eq!(page.events[0].task_id.as_deref(), Some(task_id.as_str()));

    let count = count_task_activity_events(task_id.as_str().to_string(), None, app.state())
        .await
        .expect("task events should count");
    assert_eq!(count, 1);
}
