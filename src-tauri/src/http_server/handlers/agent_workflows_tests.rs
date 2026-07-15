use std::sync::Arc;

use axum::extract::State;
use axum::Json;

use super::agent_workflows::*;
use crate::application::agent_capability_gate::AgentCapabilities;
use crate::application::AppState;
use crate::domain::agents::AgentHarnessKind;
use crate::domain::entities::{AgentWorkflowMeta, ChatConversation, CoordinationMode, ProjectId};
use crate::http_server::types::HttpServerState;

fn workflow_meta() -> AgentWorkflowMeta {
    AgentWorkflowMeta {
        name: "Review".into(),
        description: "Review safely".into(),
        phases: vec!["review".into()],
        max_concurrency: 2,
        max_invocations: 4,
    }
}

#[tokio::test]
async fn create_script_rejects_disabled_workflow_capability() {
    let state = HttpServerState::new_test(Arc::new(AppState::new_test()));
    let result = create_agent_workflow_script(
        State(state),
        Json(CreateWorkflowScriptRequest {
            conversation_id: "conversation-1".into(),
            project_id: "project-1".into(),
            script: "return {};".into(),
            meta: workflow_meta(),
            permission_summary: serde_json::json!({}),
            estimated_fanout: 0,
        }),
    )
    .await;
    assert_eq!(result.unwrap_err().0, axum::http::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn launch_rejects_script_that_user_has_not_hash_approved() {
    let app_state = Arc::new(AppState::new_test());
    app_state.agent_capability_gate.replace(AgentCapabilities {
        team: false,
        workflows: true,
    });
    let mut conversation =
        ChatConversation::new_project(ProjectId::from_string("project-1".into()));
    conversation.coordination_mode = CoordinationMode::RxNativeWorkflow;
    app_state
        .chat_conversation_repo
        .create(conversation.clone())
        .await
        .unwrap();
    let state = HttpServerState::new_test(app_state);
    let script = create_agent_workflow_script(
        State(state.clone()),
        Json(CreateWorkflowScriptRequest {
            conversation_id: conversation.id.to_string(),
            project_id: "project-1".into(),
            script: "return {};".into(),
            meta: workflow_meta(),
            permission_summary: serde_json::json!({ "filesystem": "read-only" }),
            estimated_fanout: 0,
        }),
    )
    .await
    .unwrap()
    .0;

    let result = start_agent_workflow_run(
        State(state),
        Json(StartWorkflowRunRequest {
            script_id: script.id.to_string(),
            script_hash: script.script_hash,
            permission_hash: script.permission_hash,
            args: serde_json::json!({}),
            harness: Some(AgentHarnessKind::Codex),
            caller_agent_name: Some("ralphx-general-worker".into()),
            caller_agent_profile: None,
        }),
    )
    .await;
    assert_eq!(result.unwrap_err().0, axum::http::StatusCode::CONFLICT);
}
