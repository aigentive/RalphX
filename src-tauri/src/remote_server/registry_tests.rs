use std::sync::Arc;

use axum::extract::{Path, State};

use crate::application::AppState;
use crate::domain::entities::{ProjectId, Task, TaskCategory, TaskStep};
use crate::http_server::project_scope::ProjectScope;
use crate::http_server::types::HttpServerState;

#[tokio::test]
async fn worker_task_context_http_serializes_only_the_task_allowlist() {
    const BLOCKED_SENTINEL: &str = "P17H_BLOCKED_REASON_POISON";
    const METADATA_SENTINEL: &str = "P17H_RAW_METADATA_POISON";
    const RESTART_NOTE: &str = "P17H_RESTART_NOTE_VISIBLE";

    let app_state = Arc::new(AppState::new_test());
    let mut task = Task::new(ProjectId::new(), "Worker projection".to_string());
    task.description = Some("Allowed description".to_string());
    task.priority = 1_337_017;
    task.blocked_reason = Some(BLOCKED_SENTINEL.to_string());
    task.metadata = Some(format!(
        r#"{{"restart_note":"{RESTART_NOTE}","poison":"{METADATA_SENTINEL}"}}"#
    ));
    let task = app_state.task_repo.create(task).await.unwrap();

    let response = crate::http_server::handlers::get_task_context(
        State(HttpServerState::new_test(Arc::clone(&app_state))),
        ProjectScope(None),
        Path(task.id.as_str().to_string()),
    )
    .await
    .unwrap();
    let payload = serde_json::to_value(response.0).unwrap();
    let serialized = serde_json::to_string(&payload).unwrap();
    let task_payload = payload.get("task").unwrap().as_object().unwrap();

    for banned in ["category", "priority", "blocked_reason", "metadata"] {
        assert!(
            !task_payload.contains_key(banned),
            "leaked banned field {banned}"
        );
    }
    assert!(!serialized.contains(BLOCKED_SENTINEL));
    assert!(!serialized.contains(METADATA_SENTINEL));
    assert!(serialized.contains(RESTART_NOTE));
}

#[tokio::test]
async fn step_context_http_serializes_only_the_task_summary_allowlist() {
    const BLOCKED_SENTINEL: &str = "P17H_STEP_BLOCKED_REASON_POISON";
    const METADATA_SENTINEL: &str = "P17H_STEP_RAW_METADATA_POISON";

    let app_state = Arc::new(AppState::new_test());
    let mut task = Task::new(ProjectId::new(), "Step projection".to_string());
    task.description = Some("Allowed step description".to_string());
    task.priority = 1_337_018;
    task.blocked_reason = Some(BLOCKED_SENTINEL.to_string());
    task.metadata = Some(format!(r#"{{"poison":"{METADATA_SENTINEL}"}}"#));
    let task = app_state.task_repo.create(task).await.unwrap();
    let step = app_state
        .task_step_repo
        .create(TaskStep::new(
            task.id,
            "Projected step".to_string(),
            0,
            "agent".to_string(),
        ))
        .await
        .unwrap();

    let response = crate::http_server::handlers::get_step_context_http(
        State(HttpServerState::new_test(Arc::clone(&app_state))),
        Path(step.id.as_str().to_string()),
    )
    .await
    .unwrap();
    let payload = serde_json::to_value(response.0).unwrap();
    let serialized = serde_json::to_string(&payload).unwrap();
    let summary = payload.get("task_summary").unwrap().as_object().unwrap();

    for banned in ["category", "priority", "blocked_reason", "metadata"] {
        assert!(
            !summary.contains_key(banned),
            "leaked banned field {banned}"
        );
    }
    assert!(!serialized.contains(BLOCKED_SENTINEL));
    assert!(!serialized.contains(METADATA_SENTINEL));
}

/// P-17h, the half the `update_task_authz` comment used to get wrong.
///
/// `/api/get_task_details` does NOT go through `WorkerTaskView`; it serialises the task with
/// `task_to_response`, which includes `category` and `priority`. The invariant that keeps them
/// at `ui:operate` is therefore not "no worker payload carries them" but "neither can carry
/// attacker-chosen text": `category` is a closed enum and `priority` an `i32`. This pins both
/// the inclusion and the containment, so widening `TaskResponse` with a free-text field — or
/// leaking one of the fields the projection deliberately drops — fails here.
#[tokio::test]
async fn task_to_response_carries_no_free_text_outside_the_declared_contract() {
    const BLOCKED_SENTINEL: &str = "P17H_RESPONSE_BLOCKED_REASON_POISON";
    const METADATA_SENTINEL: &str = "P17H_RESPONSE_RAW_METADATA_POISON";

    let mut task = Task::new(ProjectId::new(), "Response projection".to_string());
    task.description = Some("Allowed description".to_string());
    task.priority = 1_337_019;
    task.blocked_reason = Some(BLOCKED_SENTINEL.to_string());
    task.metadata = Some(format!(r#"{{"poison":"{METADATA_SENTINEL}"}}"#));

    let payload =
        serde_json::to_value(crate::http_server::handlers::task_to_response(&task)).unwrap();
    let serialized = serde_json::to_string(&payload).unwrap();
    let object = payload.as_object().expect("response is an object");

    // The exact contract — a new field cannot appear without updating this gate.
    assert_eq!(
        object.keys().cloned().collect::<Vec<_>>(),
        vec![
            "category".to_string(),
            "created_at".to_string(),
            "description".to_string(),
            "id".to_string(),
            "priority".to_string(),
            "status".to_string(),
            "title".to_string(),
            "updated_at".to_string(),
        ]
    );

    // Fields the projection drops must not reappear anywhere in the payload.
    assert!(!serialized.contains(BLOCKED_SENTINEL));
    assert!(!serialized.contains(METADATA_SENTINEL));
    for banned in ["blocked_reason", "metadata"] {
        assert!(!object.contains_key(banned), "leaked banned field {banned}");
    }

    // `category`/`priority` ARE present — and are structurally incapable of free text.
    assert!(object.contains_key("category") && object.contains_key("priority"));
    assert_eq!(payload["priority"], serde_json::json!("1337019"));
    let category = payload["category"].as_str().expect("category is a string");
    assert!(
        [TaskCategory::Regular, TaskCategory::PlanMerge]
            .iter()
            .any(|known| known.to_string() == category),
        "category `{category}` is outside the closed TaskCategory enum"
    );
}
