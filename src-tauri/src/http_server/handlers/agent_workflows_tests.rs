use std::sync::Arc;

use axum::extract::State;
use axum::Json;
use chrono::{Duration, Utc};

use super::agent_workflows::*;
use crate::application::agent_capability_gate::AgentCapabilities;
use crate::application::AppState;
use crate::domain::agents::AgentHarnessKind;
use crate::domain::entities::{
    AgentWorkflowMeta, AgentWorkflowRun, AgentWorkflowRunId, AgentWorkflowRunStatus,
    ChatConversation, CoordinationMode, ProjectId,
};
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

#[test]
fn workflow_agent_output_schema_accepts_typed_required_result() {
    let schema = serde_json::json!({
        "type": "object",
        "required": ["approved"],
        "properties": { "approved": { "type": "boolean" } },
        "additionalProperties": false
    });

    let value = validate_workflow_agent_output(r#"{"approved":true}"#, Some(&schema)).unwrap();

    assert_eq!(value, serde_json::json!({ "approved": true }));
}

#[test]
fn workflow_agent_output_schema_rejects_missing_or_null_required_result() {
    let schema = serde_json::json!({
        "type": "object",
        "required": ["approved"],
        "properties": { "approved": { "type": "boolean" } }
    });

    assert!(validate_workflow_agent_output("{}", Some(&schema)).is_err());
    assert!(validate_workflow_agent_output(r#"{"approved":null}"#, Some(&schema)).is_err());
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
async fn create_script_uses_backend_derived_permission_and_invocation_ceiling() {
    let app_state = Arc::new(AppState::new_test());
    app_state.agent_capability_gate.replace(AgentCapabilities {
        team: false,
        workflows: true,
        autopilot: false,
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
        State(state),
        Json(CreateWorkflowScriptRequest {
            conversation_id: conversation.id.to_string(),
            project_id: "project-1".into(),
            script: "return {};".into(),
            meta: workflow_meta(),
            permission_summary: serde_json::json!({ "filesystem": "unrestricted" }),
            estimated_fanout: 1,
        }),
    )
    .await
    .unwrap()
    .0;

    let summary: serde_json::Value = serde_json::from_str(&script.permission_summary_json).unwrap();
    assert_eq!(summary["enforcement"], "inherits_parent_agent_workspace");
    assert_eq!(summary["directScriptOsAccess"], false);
    assert_eq!(script.estimated_fanout, workflow_meta().max_invocations);
}

#[tokio::test]
async fn launch_rejects_script_that_user_has_not_hash_approved() {
    let app_state = Arc::new(AppState::new_test());
    app_state.agent_capability_gate.replace(AgentCapabilities {
        team: false,
        workflows: true,
        autopilot: false,
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
            launch_id: None,
            args: serde_json::json!({}),
            harness: Some(AgentHarnessKind::Codex),
            caller_agent_name: Some("ralphx-general-worker".into()),
            caller_agent_profile: None,
        }),
    )
    .await;
    assert_eq!(result.unwrap_err().0, axum::http::StatusCode::CONFLICT);
}

#[tokio::test]
async fn approval_rejects_script_after_conversation_leaves_workflow_mode() {
    let app_state = Arc::new(AppState::new_test());
    app_state.agent_capability_gate.replace(AgentCapabilities {
        team: false,
        workflows: true,
        autopilot: false,
    });
    let mut conversation =
        ChatConversation::new_project(ProjectId::from_string("project-1".into()));
    conversation.coordination_mode = CoordinationMode::RxNativeWorkflow;
    app_state
        .chat_conversation_repo
        .create(conversation.clone())
        .await
        .unwrap();
    let state = HttpServerState::new_test(app_state.clone());
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
    app_state
        .chat_conversation_repo
        .update_coordination_mode(&conversation.id, CoordinationMode::Solo)
        .await
        .unwrap();

    let result = approve_agent_workflow_script(
        State(state),
        Json(ApproveWorkflowScriptRequest {
            script_id: script.id.to_string(),
            script_hash: script.script_hash,
            permission_hash: script.permission_hash,
        }),
    )
    .await;

    assert_eq!(result.unwrap_err().0, axum::http::StatusCode::CONFLICT);
}

#[tokio::test]
async fn launch_rejects_approved_script_after_conversation_leaves_workflow_mode() {
    let app_state = Arc::new(AppState::new_test());
    app_state.agent_capability_gate.replace(AgentCapabilities {
        team: false,
        workflows: true,
        autopilot: false,
    });
    let mut conversation =
        ChatConversation::new_project(ProjectId::from_string("project-1".into()));
    conversation.coordination_mode = CoordinationMode::RxNativeWorkflow;
    app_state
        .chat_conversation_repo
        .create(conversation.clone())
        .await
        .unwrap();
    let state = HttpServerState::new_test(app_state.clone());
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
    let _ = approve_agent_workflow_script(
        State(state.clone()),
        Json(ApproveWorkflowScriptRequest {
            script_id: script.id.to_string(),
            script_hash: script.script_hash.clone(),
            permission_hash: script.permission_hash.clone(),
        }),
    )
    .await
    .unwrap();
    app_state
        .chat_conversation_repo
        .update_coordination_mode(&conversation.id, CoordinationMode::Solo)
        .await
        .unwrap();

    let result = start_agent_workflow_run(
        State(state),
        Json(StartWorkflowRunRequest {
            script_id: script.id.to_string(),
            script_hash: script.script_hash,
            permission_hash: script.permission_hash,
            launch_id: None,
            args: serde_json::json!({}),
            harness: Some(AgentHarnessKind::Codex),
            caller_agent_name: Some("ralphx-general-worker".into()),
            caller_agent_profile: None,
        }),
    )
    .await;

    assert_eq!(result.unwrap_err().0, axum::http::StatusCode::CONFLICT);
}

#[tokio::test]
async fn resume_rejects_run_after_conversation_leaves_workflow_mode() {
    let app_state = Arc::new(AppState::new_test());
    app_state.agent_capability_gate.replace(AgentCapabilities {
        team: false,
        workflows: true,
        autopilot: false,
    });
    let mut conversation =
        ChatConversation::new_project(ProjectId::from_string("project-1".into()));
    conversation.coordination_mode = CoordinationMode::RxNativeWorkflow;
    app_state
        .chat_conversation_repo
        .create(conversation.clone())
        .await
        .unwrap();
    let state = HttpServerState::new_test(app_state.clone());
    let script = create_agent_workflow_script(
        State(state.clone()),
        Json(CreateWorkflowScriptRequest {
            conversation_id: conversation.id.to_string(),
            project_id: "project-1".into(),
            script: "return {};".into(),
            meta: workflow_meta(),
            permission_summary: serde_json::json!({}),
            estimated_fanout: 0,
        }),
    )
    .await
    .unwrap()
    .0;
    let _ = approve_agent_workflow_script(
        State(state.clone()),
        Json(ApproveWorkflowScriptRequest {
            script_id: script.id.to_string(),
            script_hash: script.script_hash.clone(),
            permission_hash: script.permission_hash.clone(),
        }),
    )
    .await
    .unwrap();
    let now = Utc::now();
    let run = app_state
        .agent_workflow_repo
        .create_run(AgentWorkflowRun {
            id: AgentWorkflowRunId::new(),
            script_id: script.id.clone(),
            conversation_id: script.conversation_id.clone(),
            project_id: script.project_id.clone(),
            harness: AgentHarnessKind::Codex,
            script_hash: script.script_hash.clone(),
            permission_hash: script.permission_hash.clone(),
            args_json: "{}".into(),
            status: AgentWorkflowRunStatus::Paused,
            attempt: 0,
            runner_instance_id: None,
            lease_expires_at: None,
            heartbeat_at: None,
            pause_requested: true,
            cancel_requested: false,
            result_json: None,
            error: None,
            created_at: now,
            updated_at: now,
            completed_at: None,
        })
        .await
        .unwrap();
    app_state
        .chat_conversation_repo
        .update_coordination_mode(&conversation.id, CoordinationMode::Solo)
        .await
        .unwrap();

    let result = resume_agent_workflow_run(
        State(state),
        Json(ResumeWorkflowRunRequest {
            run_id: run.id.to_string(),
            caller_agent_name: None,
            caller_agent_profile: None,
        }),
    )
    .await;

    assert_eq!(result.unwrap_err().0, axum::http::StatusCode::CONFLICT);
}

#[tokio::test]
async fn recovery_fails_unclaimed_run_after_conversation_leaves_workflow_mode() {
    let app_state = Arc::new(AppState::new_test());
    app_state.agent_capability_gate.replace(AgentCapabilities {
        team: false,
        workflows: true,
        autopilot: false,
    });
    let mut conversation =
        ChatConversation::new_project(ProjectId::from_string("project-1".into()));
    conversation.coordination_mode = CoordinationMode::RxNativeWorkflow;
    app_state
        .chat_conversation_repo
        .create(conversation.clone())
        .await
        .unwrap();
    let state = HttpServerState::new_test(app_state.clone());
    let script = create_agent_workflow_script(
        State(state.clone()),
        Json(CreateWorkflowScriptRequest {
            conversation_id: conversation.id.to_string(),
            project_id: "project-1".into(),
            script: "return {};".into(),
            meta: workflow_meta(),
            permission_summary: serde_json::json!({}),
            estimated_fanout: 0,
        }),
    )
    .await
    .unwrap()
    .0;
    let _ = approve_agent_workflow_script(
        State(state.clone()),
        Json(ApproveWorkflowScriptRequest {
            script_id: script.id.to_string(),
            script_hash: script.script_hash.clone(),
            permission_hash: script.permission_hash.clone(),
        }),
    )
    .await
    .unwrap();
    let now = Utc::now();
    let run = app_state
        .agent_workflow_repo
        .create_run(AgentWorkflowRun {
            id: AgentWorkflowRunId::new(),
            script_id: script.id.clone(),
            conversation_id: script.conversation_id.clone(),
            project_id: script.project_id.clone(),
            harness: AgentHarnessKind::Codex,
            script_hash: script.script_hash.clone(),
            permission_hash: script.permission_hash.clone(),
            args_json: "{}".into(),
            status: AgentWorkflowRunStatus::Queued,
            attempt: 0,
            runner_instance_id: None,
            lease_expires_at: None,
            heartbeat_at: None,
            pause_requested: false,
            cancel_requested: false,
            result_json: None,
            error: None,
            created_at: now,
            updated_at: now,
            completed_at: None,
        })
        .await
        .unwrap();
    app_state
        .chat_conversation_repo
        .update_coordination_mode(&conversation.id, CoordinationMode::Solo)
        .await
        .unwrap();

    assert_eq!(recover_agent_workflow_runs(&state).await.unwrap(), 0);
    let recovered = app_state
        .agent_workflow_repo
        .get_run(&run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(recovered.status, AgentWorkflowRunStatus::Failed);
    assert!(recovered.error.unwrap().contains("Workflow mode"));
}

#[tokio::test]
async fn disabled_startup_recovery_settles_expired_running_run_as_paused() {
    let app_state = Arc::new(AppState::new_test());
    app_state.agent_capability_gate.replace(AgentCapabilities {
        team: false,
        workflows: true,
        autopilot: false,
    });
    let mut conversation =
        ChatConversation::new_project(ProjectId::from_string("project-1".into()));
    conversation.coordination_mode = CoordinationMode::RxNativeWorkflow;
    app_state
        .chat_conversation_repo
        .create(conversation.clone())
        .await
        .unwrap();
    let state = HttpServerState::new_test(app_state.clone());
    let script = create_agent_workflow_script(
        State(state.clone()),
        Json(CreateWorkflowScriptRequest {
            conversation_id: conversation.id.to_string(),
            project_id: "project-1".into(),
            script: "return {};".into(),
            meta: workflow_meta(),
            permission_summary: serde_json::json!({}),
            estimated_fanout: 0,
        }),
    )
    .await
    .unwrap()
    .0;
    let _ = approve_agent_workflow_script(
        State(state.clone()),
        Json(ApproveWorkflowScriptRequest {
            script_id: script.id.to_string(),
            script_hash: script.script_hash.clone(),
            permission_hash: script.permission_hash.clone(),
        }),
    )
    .await
    .unwrap();
    let now = Utc::now();
    let run = app_state
        .agent_workflow_repo
        .create_run(AgentWorkflowRun {
            id: AgentWorkflowRunId::new(),
            script_id: script.id.clone(),
            conversation_id: script.conversation_id.clone(),
            project_id: script.project_id.clone(),
            harness: AgentHarnessKind::Codex,
            script_hash: script.script_hash.clone(),
            permission_hash: script.permission_hash.clone(),
            args_json: "{}".into(),
            status: AgentWorkflowRunStatus::Running,
            attempt: 1,
            runner_instance_id: Some("dead-runner".into()),
            lease_expires_at: Some(now - Duration::seconds(5)),
            heartbeat_at: Some(now - Duration::seconds(35)),
            pause_requested: false,
            cancel_requested: false,
            result_json: None,
            error: None,
            created_at: now - Duration::minutes(1),
            updated_at: now - Duration::seconds(35),
            completed_at: None,
        })
        .await
        .unwrap();
    app_state.agent_capability_gate.replace(AgentCapabilities {
        team: false,
        workflows: false,
        autopilot: false,
    });

    assert_eq!(recover_agent_workflow_runs(&state).await.unwrap(), 0);
    let recovered = app_state
        .agent_workflow_repo
        .get_run(&run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(recovered.status, AgentWorkflowRunStatus::Paused);
    assert!(recovered.pause_requested);
}
