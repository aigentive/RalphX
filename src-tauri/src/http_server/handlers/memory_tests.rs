use std::sync::Arc;

use axum::{extract::State, Json};

use super::upsert_memories;
use crate::application::{AppState, TeamService, TeamStateTracker};
use crate::commands::ExecutionState;
use crate::domain::entities::{MemoryBucket, ProjectId};
use crate::http_server::types::{HttpServerState, MemoryEntryInput, UpsertMemoriesRequest};

fn test_state(app_state: Arc<AppState>) -> HttpServerState {
    let tracker = TeamStateTracker::new();
    let team_service = Arc::new(TeamService::new_without_events(Arc::new(tracker.clone())));
    HttpServerState {
        app_state,
        execution_state: Arc::new(ExecutionState::new()),
        team_tracker: tracker,
        team_service,
        delegation_service: Default::default(),
    }
}

fn memory_input(title: &str) -> MemoryEntryInput {
    MemoryEntryInput {
        bucket: "implementation_discoveries".to_string(),
        title: title.to_string(),
        summary: "A reusable implementation discovery from a completed run.".to_string(),
        details_markdown: "The completed run exposed a durable implementation detail.".to_string(),
        scope_paths: vec!["src-tauri/src/application/**".to_string()],
        source_context_type: Some("task_execution".to_string()),
        source_context_id: Some("task-1".to_string()),
        source_conversation_id: Some("conversation-1".to_string()),
        quality_score: Some(0.91),
    }
}

#[tokio::test]
async fn upsert_memories_creates_memory_entry_and_capture_event() {
    let app_state = Arc::new(AppState::new_test());
    let project_id = ProjectId::from_string("project-memory-capture".to_string());

    let response = upsert_memories(
        State(test_state(Arc::clone(&app_state))),
        Json(UpsertMemoriesRequest {
            project_id: project_id.as_str().to_string(),
            memories: vec![memory_input("Capture actual implementation discovery")],
        }),
    )
    .await
    .unwrap()
    .0;

    assert_eq!(response.inserted, 1);
    assert_eq!(response.skipped, 0);
    assert_eq!(response.failed, 0);

    let entries = app_state
        .memory_entry_repo
        .get_by_project_and_bucket(&project_id, MemoryBucket::ImplementationDiscoveries)
        .await
        .unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].source_conversation_id.as_deref(),
        Some("conversation-1")
    );
    assert_eq!(entries[0].quality_score, Some(0.91));

    let events = app_state
        .memory_event_repo
        .get_by_type("memory_capture_decision")
        .await
        .unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].details["inserted"], 1);
    assert_eq!(events[0].details["skipped"], 0);
    assert_eq!(events[0].details["failed"], 0);
}

#[tokio::test]
async fn upsert_memories_skips_duplicate_and_records_no_capture_decision() {
    let app_state = Arc::new(AppState::new_test());
    let project_id = ProjectId::from_string("project-memory-duplicate".to_string());
    let input = memory_input("Avoid duplicate memory capture");

    let first = upsert_memories(
        State(test_state(Arc::clone(&app_state))),
        Json(UpsertMemoriesRequest {
            project_id: project_id.as_str().to_string(),
            memories: vec![input],
        }),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(first.inserted, 1);

    let duplicate = upsert_memories(
        State(test_state(Arc::clone(&app_state))),
        Json(UpsertMemoriesRequest {
            project_id: project_id.as_str().to_string(),
            memories: vec![memory_input("Avoid duplicate memory capture")],
        }),
    )
    .await
    .unwrap()
    .0;

    assert_eq!(duplicate.inserted, 0);
    assert_eq!(duplicate.skipped, 1);

    let entries = app_state
        .memory_entry_repo
        .get_by_project(&project_id)
        .await
        .unwrap();
    assert_eq!(entries.len(), 1);

    let events = app_state
        .memory_event_repo
        .get_by_type("memory_capture_decision")
        .await
        .unwrap();
    assert_eq!(events.len(), 2);
    assert!(events.iter().any(|event| event.details["skipped"] == 1));
}
