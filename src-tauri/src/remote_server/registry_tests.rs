use std::sync::Arc;

use axum::extract::{Path, State};

use crate::application::AppState;
use crate::domain::entities::{ProjectId, Task, TaskCategory, TaskStep};
use crate::http_server::project_scope::ProjectScope;
use crate::http_server::types::HttpServerState;

const BLOCKED_SENTINEL: &str = "P17H_BLOCKED_REASON_POISON";
const METADATA_SENTINEL: &str = "P17H_RAW_METADATA_POISON";
const RESTART_NOTE: &str = "P17H_RESTART_NOTE_VISIBLE";

/// Builds the shared `get_task_context` fixture and returns the context the LOCAL path produces.
async fn local_worker_task_context() -> crate::domain::entities::TaskContext {
    let app_state = Arc::new(AppState::new_test());
    let mut task = Task::new(ProjectId::new(), "Worker projection".to_string());
    task.description = Some("Allowed description".to_string());
    task.priority = 1_337_017;
    task.blocked_reason = Some(BLOCKED_SENTINEL.to_string());
    // Every optional field asserted below must be populated, or `skip_serializing_if` hides it
    // and the assertion would pass for the wrong reason.
    task.plan_artifact_id = Some(crate::domain::entities::ArtifactId::new());
    task.ideation_session_id = Some(crate::domain::entities::IdeationSessionId::new());
    task.merge_pipeline_active = Some("2026-07-30T00:00:00Z".to_string());
    task.metadata = Some(format!(
        r#"{{"restart_note":"{RESTART_NOTE}","poison":"{METADATA_SENTINEL}"}}"#
    ));
    let task = app_state.task_repo.create(task).await.unwrap();

    crate::http_server::handlers::get_task_context(
        State(HttpServerState::new_test(Arc::clone(&app_state))),
        ProjectScope(None),
        Path(task.id.as_str().to_string()),
    )
    .await
    .unwrap()
    .0
}

#[test]
fn ticketing_credential_spending_read_projections_carry_no_secret_reference() {
    use crate::commands::ticketing_commands::{
        TicketDetailResponse, TicketFilterOptionsResponse, TicketLabelOptionResponse,
        TicketPageResponse, TicketRefInput, TicketStateResponse, TicketSummaryResponse,
        TicketTransitionOptionResponse, TicketingContainerResponse,
    };
    use crate::remote_server::registry::{find_spec, serialize_ok};

    let summary = TicketSummaryResponse {
        ref_: TicketRefInput {
            provider: "linear".into(),
            id: "issue-1".into(),
            key: None,
        },
        title: "Ticket".into(),
        state: TicketStateResponse {
            id: "todo".into(),
            name: "Todo".into(),
            category: "todo".into(),
            color: None,
        },
        assignee: None,
        assignees: vec![],
        watchers: vec![],
        reporter: None,
        labels: vec![],
        sprints: vec![],
        project: None,
        priority: None,
        updated_at: "2026-08-05T12:00:00Z".into(),
        url: None,
        association_count: 0,
        open_pr_count: 0,
        open_pr_number: None,
        open_pr_url: None,
        open_pr_status: None,
        current_user_assigned: false,
        current_user_watching: false,
    };
    let projections = [
        serialize_ok(vec![TicketingContainerResponse {
            provider: "linear".into(),
            id: "team-1".into(),
            key: None,
            name: "Team".into(),
            kind: "team".into(),
            parent_id: None,
            ticket_count: None,
        }])
        .unwrap(),
        serialize_ok(TicketPageResponse {
            items: vec![],
            next_cursor: None,
            total: Some(0),
            fetched_at: None,
        })
        .unwrap(),
        serialize_ok(TicketFilterOptionsResponse {
            assignees: vec![],
            sprints: vec![],
            complete: true,
            truncated: false,
        })
        .unwrap(),
        serialize_ok(TicketDetailResponse {
            summary,
            description_markdown: None,
            description_text: None,
            acceptance_criteria_markdown: None,
            comments: vec![],
            attachments: vec![],
            transitions: vec![],
            fetched_at: None,
        })
        .unwrap(),
        serialize_ok(vec![TicketTransitionOptionResponse {
            to_state_id: "done".into(),
            provider_transition_id: None,
            name: "Done".into(),
            category: "done".into(),
            disabled_reason: None,
        }])
        .unwrap(),
        serialize_ok(vec![TicketLabelOptionResponse {
            id: None,
            name: "bug".into(),
        }])
        .unwrap(),
    ];

    for (command, projection) in [
        "list_ticketing_containers",
        "list_tickets",
        "list_ticket_filter_options",
        "get_ticket_detail",
        "list_ticket_transitions",
        "list_ticket_labels",
    ]
    .into_iter()
    .zip(projections)
    {
        let serialized = serde_json::to_string(&projection)
            .unwrap()
            .to_ascii_lowercase();
        for forbidden in ["secret_ref", "secretref", "token", "credential"] {
            assert!(
                !serialized.contains(forbidden),
                "{command} leaked `{forbidden}`"
            );
        }
        let spec = find_spec(command).expect("ticketing read is registered");
        assert_eq!(spec.class, ralphx_remote_protocol::RiskClass::Read);
        assert!(spec.capabilities.is_empty());
    }
}

#[test]
fn ticketing_writes_require_ui_agent_before_dispatch() {
    use crate::remote_server::registry::{enforce_scope, find_spec};
    use ralphx_remote_protocol::{ErrorCode, RiskClass, Scope};

    for command in [
        "list_ticketing_columns",
        "refresh_ticketing_status_catalog",
        "update_ticketing_status_presentation",
        "transition_ticket_status",
        "assign_ticket",
        "clear_ticket_assignee",
        "add_ticket_comment",
        "set_ticket_labels",
    ] {
        let spec = find_spec(command).expect("ticketing write is registered");
        assert_eq!(spec.class, RiskClass::AgentControl);
        let error = enforce_scope(
            spec,
            &[Scope::UiRead, Scope::UiOperate],
            &serde_json::json!({}),
        )
        .expect_err("a client without ui:agent must be refused");
        assert_eq!(error.code, ErrorCode::RemoteForbidden);
    }
}

/// Direction 1 — the LOCAL producer is NOT narrowed.
///
/// `:3847` (internal MCP, local harness agents) and the local Tauri command share this payload
/// with the desktop frontend. `WorkerTaskView` bounds the REMOTE wire only; applying it here
/// silently dropped ~11 load-bearing fields for every user, including users who never enable
/// `remote_host`. The named fields are load-bearing: `priority`/`category` render the
/// `ContextWidget` badges, `plan_artifact_id` is a `TaskDetailContext` fallback, and
/// `blocked_reason`/`merge_pipeline_active` drive agent decisions.
#[tokio::test]
async fn local_task_context_http_serves_the_full_task() {
    let payload = serde_json::to_value(local_worker_task_context().await).unwrap();
    let serialized = serde_json::to_string(&payload).unwrap();
    let task_payload = payload.get("task").unwrap().as_object().unwrap();

    for required in [
        "priority",
        "category",
        "blocked_reason",
        "metadata",
        "needs_review_point",
        "merge_pipeline_active",
        "plan_artifact_id",
        "created_at",
        "updated_at",
    ] {
        assert!(
            task_payload.contains_key(required),
            "local task context lost load-bearing field {required}"
        );
    }
    assert_eq!(task_payload["priority"], serde_json::json!(1_337_017));
    assert_eq!(
        task_payload["blocked_reason"],
        serde_json::json!(BLOCKED_SENTINEL)
    );
    assert!(serialized.contains(METADATA_SENTINEL));
    assert!(serialized.contains(RESTART_NOTE));
}

/// Direction 2 — the REMOTE facade still serves ONLY the 6-field allowlist.
///
/// `remote_server::task_projection` is the sole remote entry point for `get_task_context`
/// (pinned by `get_task_context_is_registered_through_the_remote_projection`), so this is the
/// exact payload a paired device receives.
#[tokio::test]
async fn remote_facade_task_context_serializes_only_the_task_allowlist() {
    let context = local_worker_task_context().await;
    let payload = crate::remote_server::task_projection::project_task_context(context).unwrap();
    let serialized = serde_json::to_string(&payload).unwrap();
    let task_payload = payload.get("task").unwrap().as_object().unwrap();

    assert_eq!(
        task_payload.keys().cloned().collect::<Vec<_>>(),
        vec![
            "description".to_string(),
            "id".to_string(),
            "ideation_session_id".to_string(),
            "internal_status".to_string(),
            "project_id".to_string(),
            "title".to_string(),
        ],
        "the remote worker task view is a closed 6-field allowlist"
    );
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

/// The wiring half: registering the raw command would restore the full `Task` on the wire.
#[test]
fn get_task_context_is_registered_through_the_remote_projection() {
    let spec = crate::remote_server::registry::find_spec("get_task_context")
        .expect("get_task_context is registered");
    assert_eq!(
        spec.target, "crate::remote_server::task_projection::get_task_context",
        "the facade must dispatch through the narrowing shim, not the raw command"
    );
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
