use std::collections::HashSet;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use ralphx_lib::application::agent_conversation_workspace::{
    prepare_agent_conversation_workspace, AgentConversationWorkspaceBaseSelection,
};
use ralphx_lib::application::chat_service::AgentRuntimeStatus;
use ralphx_lib::application::interactive_process_registry::InteractiveProcessMetadata;
use ralphx_lib::application::pr_startup_recovery::{
    cleanup_terminal_agent_workspace_local_artifacts_on_startup,
    cleanup_terminal_plan_branch_local_artifacts_on_startup,
};
use ralphx_lib::application::{
    AppState, InteractiveProcessKey, MockChatService, PrPollerRegistry, SendResult,
};
use ralphx_lib::commands::unified_chat_commands::{
    agent_workspace_post_repair_action_from_events, create_agent_conversation,
    get_agent_running_states_for_service, mark_agent_workspace_publish_failure,
    mark_agent_workspace_publish_failure_with_target, parse_context_type,
    send_agent_workspace_publish_repair_message, switch_agent_conversation_mode_for_state,
    switch_agent_conversation_mode_for_state_allowing_running,
    switch_agent_conversation_mode_for_state_stopping_running_agent,
    switch_agent_conversation_persona_for_state_stopping_running_agent,
    switch_agent_conversation_persona_for_state_with_provider_session_reset,
    update_agent_conversation_coordination_mode, validate_persona_builder_team_intent_for_send,
    AgentConversationResponse, AgentConversationWorkspaceRepairTarget, AgentRunStatusResponse,
    AgentWorkspacePostRepairAction, AgentWorkspaceRepairRuntimeOverrides,
    CreateAgentConversationInput, ModeSwitchInitiator, QueuedMessageResponse,
    SendAgentMessageResponse, SwitchAgentConversationModeInput,
    SwitchAgentConversationPersonaInput, UpdateAgentConversationCoordinationModeInput,
    AUTOMATION_RUN_MODE_LOCKED_ERROR_CODE,
};
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::agents::{AgentHarnessKind, LogicalEffort, ProviderSessionRef};
use ralphx_lib::domain::entities::plan_branch::PrStatus as DbPrStatus;
use ralphx_lib::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode,
    AgentConversationWorkspacePublicationEvent, AgentRun, ArtifactId, AutomationId,
    AutomationRunId, ChatContextType, ChatConversation, ChatConversationId, CoordinationMode,
    ExecutionPlan, ExecutionPlanStatus, IdeationAnalysisBaseRefKind, IdeationSession,
    IdeationSessionId, Persona, PersonaId, PersonaStatus, PlanBranch, PlanBranchStatus, Project,
    ProjectId, TaskId,
};
use ralphx_lib::domain::services::github_service::{
    GithubServiceTrait, PrStatus as GithubPrStatus,
};
use ralphx_lib::domain::services::{MemoryRunningAgentRegistry, QueuedMessage, RunningAgentKey};
use ralphx_lib::infrastructure::agents::claude::agent_names::AGENT_WORKSPACE_REPAIR;
use tauri::Manager;

#[test]
fn test_parse_context_type() {
    assert!(matches!(
        parse_context_type("ideation"),
        Ok(ChatContextType::Ideation)
    ));
    assert!(matches!(
        parse_context_type("task"),
        Ok(ChatContextType::Task)
    ));
    assert!(matches!(
        parse_context_type("project"),
        Ok(ChatContextType::Project)
    ));
    assert!(matches!(
        parse_context_type("task_execution"),
        Ok(ChatContextType::TaskExecution)
    ));
    assert!(parse_context_type("invalid").is_err());
}

#[test]
fn test_send_agent_message_response_from() {
    let result = SendResult {
        conversation_id: "conv-123".to_string(),
        agent_run_id: "run-456".to_string(),
        is_new_conversation: true,
        was_queued: false,
        queued_message_id: None,
        queued_as_pending: false,
    };

    let response = SendAgentMessageResponse::from(result);
    assert_eq!(response.conversation_id, "conv-123");
    assert_eq!(response.agent_run_id, "run-456");
    assert!(response.is_new_conversation);
    assert!(!response.was_queued);
    assert!(response.queued_message_id.is_none());
    assert!(!response.queued_as_pending);
}

#[test]
fn test_send_agent_message_response_queued() {
    let result = SendResult {
        conversation_id: "conv-existing".to_string(),
        agent_run_id: "run-existing".to_string(),
        is_new_conversation: false,
        was_queued: true,
        queued_message_id: Some("queued-msg-123".to_string()),
        queued_as_pending: false,
    };

    let response = SendAgentMessageResponse::from(result);
    assert_eq!(response.conversation_id, "conv-existing");
    assert_eq!(response.agent_run_id, "run-existing");
    assert!(!response.is_new_conversation);
    assert!(response.was_queued);
    assert_eq!(
        response.queued_message_id.as_deref(),
        Some("queued-msg-123")
    );
    assert!(!response.queued_as_pending);
}

#[test]
fn test_send_agent_message_response_pending_capacity() {
    let result = SendResult {
        conversation_id: "conv-pending".to_string(),
        agent_run_id: "run-pending".to_string(),
        is_new_conversation: true,
        was_queued: true,
        queued_message_id: None,
        queued_as_pending: true,
    };

    let response = SendAgentMessageResponse::from(result);
    assert_eq!(response.conversation_id, "conv-pending");
    assert_eq!(response.agent_run_id, "run-pending");
    assert!(response.is_new_conversation);
    assert!(response.was_queued);
    assert!(response.queued_message_id.is_none());
    assert!(response.queued_as_pending);
}

#[test]
fn test_queued_message_response_from() {
    let msg = QueuedMessage::new("Test content".to_string());
    let response = QueuedMessageResponse::from(msg.clone());

    assert_eq!(response.id, msg.id);
    assert_eq!(response.content, "Test content");
    assert!(!response.is_editing);
}

#[test]
fn test_response_serialization() {
    let response = SendAgentMessageResponse {
        conversation_id: "conv-123".to_string(),
        agent_run_id: "run-456".to_string(),
        is_new_conversation: true,
        was_queued: false,
        queued_message_id: None,
        queued_as_pending: false,
    };

    let json = serde_json::to_string(&response).unwrap();
    assert!(json.contains("conversation_id")); // snake_case (Rust default)
    assert!(json.contains("agent_run_id"));
    assert!(json.contains("is_new_conversation"));
    assert!(json.contains("queued_as_pending"));
}

#[test]
fn agent_conversation_response_serializes_builder_result_persona_id_present() {
    let mut conversation =
        ChatConversation::new_project(ProjectId::from_string("project-1".to_string()));
    conversation.builder_result_persona_id = Some("persona-approved".to_string());

    let json = serde_json::to_value(AgentConversationResponse::from(conversation))
        .expect("conversation response should serialize");

    assert_eq!(json["builder_draft_id"], serde_json::Value::Null);
    assert_eq!(json["builder_result_persona_id"], "persona-approved");
}

#[test]
fn agent_conversation_response_serializes_builder_result_persona_id_absent_as_null() {
    let conversation =
        ChatConversation::new_project(ProjectId::from_string("project-1".to_string()));

    let json = serde_json::to_value(AgentConversationResponse::from(conversation))
        .expect("conversation response should serialize");

    assert!(json.get("builder_result_persona_id").is_some());
    assert_eq!(json["builder_draft_id"], serde_json::Value::Null);
    assert_eq!(json["builder_result_persona_id"], serde_json::Value::Null);
}

fn test_agent_workspace() -> AgentConversationWorkspace {
    AgentConversationWorkspace::new(
        ChatConversationId::from_string("00000000-0000-0000-0000-000000000123".to_string()),
        ProjectId::from_string("project-1".to_string()),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::CurrentBranch,
        "feature/agent-screen".to_string(),
        Some("Current branch (feature/agent-screen)".to_string()),
        Some("base-sha".to_string()),
        "ralphx/ralphx/agent-1234".to_string(),
        "/tmp/agent-1234".to_string(),
    )
}

fn test_agent_workspace_with_git_target() -> (
    tempfile::TempDir,
    AgentConversationWorkspace,
    AgentConversationWorkspaceRepairTarget,
) {
    let temp = tempfile::tempdir().expect("repair target tempdir should exist");
    let repository_path = temp.path().join("repair-target");
    setup_publish_repo(&repository_path);
    git(&repository_path, &["branch", "ralphx/ralphx/agent-1234"]);
    let mut workspace = test_agent_workspace();
    workspace.worktree_path = repository_path.to_string_lossy().to_string();
    let target = AgentConversationWorkspaceRepairTarget {
        branch_name: workspace.branch_name.clone(),
        base_ref: workspace.base_ref.clone(),
        base_display_name: workspace.base_display_name.clone(),
        worktree_path: Some(repository_path),
    };
    (temp, workspace, target)
}

async fn seed_mode_switch_workspace(
    state: &AppState,
    conversation_id: ChatConversationId,
    project_id: ProjectId,
    mode: AgentConversationWorkspaceMode,
) {
    let mut project = Project::new(
        "Mode Switch Project".to_string(),
        "/tmp/project".to_string(),
    );
    project.id = project_id.clone();
    state
        .project_repo
        .create(project)
        .await
        .expect("project persisted");

    let mut conversation = ChatConversation::new_project(project_id.clone());
    conversation.id = conversation_id;
    conversation.set_agent_mode(Some(mode));
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("conversation persisted");

    let workspace = AgentConversationWorkspace::new(
        conversation_id,
        project_id,
        mode,
        IdeationAnalysisBaseRefKind::CurrentBranch,
        "feature/mode-switch".to_string(),
        Some("Current branch (feature/mode-switch)".to_string()),
        Some("base-sha".to_string()),
        "ralphx/project/agent-mode-switch".to_string(),
        "/tmp/ralphx-agent-mode-switch".to_string(),
    );
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace persisted");
}

fn enable_personas_for_test() -> crate::support::env::EnvVarGuard {
    crate::support::env::EnvVarGuard::set("RALPHX_UI_AGENT_PERSONAS", "true")
}

async fn seed_mode_locked_conversation_without_workspace(
    state: &AppState,
    conversation_id: ChatConversationId,
    project_id: ProjectId,
    mode: AgentConversationWorkspaceMode,
) {
    let mut project = Project::new(
        "Mode-locked conversation project".to_string(),
        "/tmp/persona-builder-mode-lock".to_string(),
    );
    project.id = project_id.clone();
    state
        .project_repo
        .create(project)
        .await
        .expect("project persisted");

    let mut conversation = ChatConversation::new_project(project_id);
    conversation.id = conversation_id;
    conversation.set_agent_mode(Some(mode));
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("conversation persisted");
}

async fn seed_persona_switch_project_conversation(
    state: &AppState,
    conversation_id: ChatConversationId,
    project_id: ProjectId,
) -> ChatConversation {
    let mut project = Project::new(
        "Persona Switch Project".to_string(),
        "/tmp/persona-switch-project".to_string(),
    );
    project.id = project_id.clone();
    state
        .project_repo
        .create(project)
        .await
        .expect("project persisted");

    let mut conversation = ChatConversation::new_project(project_id);
    conversation.id = conversation_id;
    state
        .chat_conversation_repo
        .create(conversation.clone())
        .await
        .expect("project conversation persisted");
    conversation
}

async fn seed_persona_for_switch(state: &AppState, id: &str, status: PersonaStatus) -> Persona {
    let now = Utc::now();
    let persona = Persona {
        id: PersonaId::from(id),
        artifact_id: None,

        project_id: None,
        slug: format!("{id}-slug"),
        name: format!("{id} name"),
        description: "persona switch fixture".to_string(),
        content: "Use the requested project voice.".to_string(),
        status,
        version: 1,
        content_hash: format!("{id}-hash"),
        source_session_id: None,
        source_persona_id: None,
        source_content_hash: None,
        source_json: "{}".to_string(),
        created_at: now,
        updated_at: now,
    };
    state
        .persona_repo
        .create(persona.clone())
        .await
        .expect("persona persisted");
    persona
}

async fn seed_scoped_persona_for_switch(
    state: &AppState,
    id: &str,
    project_id: &ProjectId,
) -> Persona {
    let now = Utc::now();
    let persona = Persona {
        id: PersonaId::from(id),
        artifact_id: None,

        project_id: Some(project_id.clone()),
        slug: format!("{id}-slug"),
        name: format!("{id} name"),
        description: "scoped persona switch fixture".to_string(),
        content: "Use the scoped project voice.".to_string(),
        status: PersonaStatus::Active,
        version: 1,
        content_hash: format!("{id}-hash"),
        source_session_id: None,
        source_persona_id: None,
        source_content_hash: None,
        source_json: "{}".to_string(),
        created_at: now,
        updated_at: now,
    };
    state.persona_repo.create(persona.clone()).await.unwrap();
    persona
}

fn persona_switch_input(
    conversation_id: &ChatConversationId,
    persona_id: Option<&PersonaId>,
) -> SwitchAgentConversationPersonaInput {
    SwitchAgentConversationPersonaInput {
        conversation_id: conversation_id.as_str(),
        persona_id: persona_id.map(|id| id.as_str().to_string()),
    }
}

#[tokio::test]
async fn persona_switch_updates_binding_when_idle() {
    let _persona_feature = enable_personas_for_test();
    let state = AppState::new_test();
    let conversation_id = ChatConversationId::from_string("12121212-1212-4212-8212-121212121212");
    let conversation = seed_persona_switch_project_conversation(
        &state,
        conversation_id,
        ProjectId::from_string("project-persona-switch-idle".to_string()),
    )
    .await;
    let persona =
        seed_persona_for_switch(&state, "persona-switch-idle", PersonaStatus::Active).await;
    let service = MockChatService::new();

    let response = switch_agent_conversation_persona_for_state_stopping_running_agent(
        persona_switch_input(&conversation.id, Some(&persona.id)),
        &state,
        &service,
    )
    .await
    .expect("idle persona switch should succeed");

    assert_eq!(
        response.conversation.persona_id.as_deref(),
        Some(persona.id.as_str())
    );
    let stored = state
        .chat_conversation_repo
        .get_by_id(&conversation.id)
        .await
        .expect("conversation lookup succeeds")
        .expect("conversation exists");
    assert_eq!(stored.persona_id.as_deref(), Some(persona.id.as_str()));
    assert!(service.get_stop_agent_calls().await.is_empty());
}

#[tokio::test]
async fn persona_switch_stopping_running_agent_stops_run_and_preserves_provider_session() {
    let _persona_feature = enable_personas_for_test();
    let state = AppState::new_test();
    let conversation_id = ChatConversationId::from_string("13131313-1313-4313-8313-131313131313");
    let conversation = seed_persona_switch_project_conversation(
        &state,
        conversation_id,
        ProjectId::from_string("project-persona-switch-running".to_string()),
    )
    .await;
    let persona =
        seed_persona_for_switch(&state, "persona-switch-running", PersonaStatus::Active).await;
    let provider_session = ProviderSessionRef {
        harness: AgentHarnessKind::Codex,
        provider_session_id: "codex-persona-switch-thread".to_string(),
    };
    state
        .chat_conversation_repo
        .update_provider_session_ref(&conversation.id, &provider_session)
        .await
        .expect("provider session should persist");

    let running_key = RunningAgentKey::new(
        ChatContextType::Project.to_string(),
        conversation.id.as_str(),
    );
    state
        .running_agent_registry
        .register(
            running_key.clone(),
            0,
            conversation.id.as_str().to_string(),
            "run-persona-switch".to_string(),
            None,
            None,
        )
        .await;
    let interactive_key = InteractiveProcessKey::new(
        ChatContextType::Project.to_string(),
        conversation.id.as_str(),
    );
    let mut child = tokio::process::Command::new("cat")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("interactive stdin observer should spawn");
    state
        .interactive_process_registry
        .register_with_metadata(
            interactive_key.clone(),
            child.stdin.take().expect("interactive stdin should exist"),
            InteractiveProcessMetadata {
                agent_run_id: None,
                harness: Some(AgentHarnessKind::Codex),
                provider_session_id: Some(provider_session.provider_session_id.clone()),
                persona_id: None,
                persona_content_hash: None,
            },
        )
        .await;

    let service = state.build_chat_service_with_execution_state(Arc::new(ExecutionState::new()));
    let response = switch_agent_conversation_persona_for_state_stopping_running_agent(
        persona_switch_input(&conversation.id, Some(&persona.id)),
        &state,
        &service,
    )
    .await
    .expect("stop-and-switch persona change should succeed");

    assert_eq!(
        response.conversation.persona_id.as_deref(),
        Some(persona.id.as_str())
    );
    assert!(
        !state.running_agent_registry.is_running(&running_key).await,
        "running agent registry entry should be removed before binding changes"
    );
    assert!(
        !state
            .interactive_process_registry
            .has_process(&interactive_key)
            .await,
        "stop should remove the interactive process entry"
    );
    let stored = state
        .chat_conversation_repo
        .get_by_id(&conversation.id)
        .await
        .expect("conversation lookup succeeds")
        .expect("conversation exists");
    assert_eq!(
        stored
            .provider_session_ref()
            .map(|session| session.provider_session_id),
        Some(provider_session.provider_session_id)
    );
    assert_eq!(stored.persona_id.as_deref(), Some(persona.id.as_str()));
    child
        .wait()
        .await
        .expect("interactive observer should exit");
}

#[tokio::test]
async fn persona_switch_forces_fresh_provider_session_when_resume_fallback_enabled() {
    let _persona_feature = enable_personas_for_test();
    let state = AppState::new_test();
    let retained_conversation = seed_persona_switch_project_conversation(
        &state,
        ChatConversationId::from_string("14141414-1414-4414-8414-141414141410"),
        ProjectId::from_string("project-persona-switch-resume-default".to_string()),
    )
    .await;
    let fresh_conversation = seed_persona_switch_project_conversation(
        &state,
        ChatConversationId::from_string("14141414-1414-4414-8414-141414141411"),
        ProjectId::from_string("project-persona-switch-fresh-fallback".to_string()),
    )
    .await;
    let persona = seed_persona_for_switch(
        &state,
        "persona-switch-session-fallback",
        PersonaStatus::Active,
    )
    .await;
    let retained_session = ProviderSessionRef {
        harness: AgentHarnessKind::Claude,
        provider_session_id: "claude-persona-switch-resume".to_string(),
    };
    let fresh_session = ProviderSessionRef {
        harness: AgentHarnessKind::Claude,
        provider_session_id: "claude-persona-switch-fresh".to_string(),
    };
    state
        .chat_conversation_repo
        .update_provider_session_ref(&retained_conversation.id, &retained_session)
        .await
        .expect("default conversation session should persist");
    state
        .chat_conversation_repo
        .update_provider_session_ref(&fresh_conversation.id, &fresh_session)
        .await
        .expect("fallback conversation session should persist");
    let service = MockChatService::new();

    switch_agent_conversation_persona_for_state_with_provider_session_reset(
        persona_switch_input(&retained_conversation.id, Some(&persona.id)),
        &state,
        &service,
        false,
    )
    .await
    .expect("default switch should succeed");
    switch_agent_conversation_persona_for_state_with_provider_session_reset(
        persona_switch_input(&fresh_conversation.id, Some(&persona.id)),
        &state,
        &service,
        true,
    )
    .await
    .expect("fallback switch should succeed");

    let retained = state
        .chat_conversation_repo
        .get_by_id(&retained_conversation.id)
        .await
        .expect("default conversation lookup succeeds")
        .expect("default conversation exists");
    let fresh = state
        .chat_conversation_repo
        .get_by_id(&fresh_conversation.id)
        .await
        .expect("fallback conversation lookup succeeds")
        .expect("fallback conversation exists");
    assert_eq!(
        retained
            .provider_session_ref()
            .map(|session| session.provider_session_id),
        Some(retained_session.provider_session_id),
        "default-off must retain the session that the next Claude send resumes"
    );
    assert!(
        fresh.provider_session_ref().is_none(),
        "fallback-on must clear the session so the next send starts fresh"
    );
}

#[tokio::test]
async fn persona_switch_clears_binding_with_null_persona_id() {
    let _persona_feature = enable_personas_for_test();
    let state = AppState::new_test();
    let conversation_id = ChatConversationId::from_string("14141414-1414-4414-8414-141414141414");
    let conversation = seed_persona_switch_project_conversation(
        &state,
        conversation_id,
        ProjectId::from_string("project-persona-switch-clear".to_string()),
    )
    .await;
    let persona =
        seed_persona_for_switch(&state, "persona-switch-clear", PersonaStatus::Active).await;
    state
        .chat_conversation_repo
        .update_persona_binding(&conversation.id, Some(persona.id.as_str()))
        .await
        .expect("original binding should persist");
    let service = MockChatService::new();

    let response = switch_agent_conversation_persona_for_state_stopping_running_agent(
        persona_switch_input(&conversation.id, None),
        &state,
        &service,
    )
    .await
    .expect("null persona id should clear the binding");

    assert!(response.conversation.persona_id.is_none());
    let stored = state
        .chat_conversation_repo
        .get_by_id(&conversation.id)
        .await
        .expect("conversation lookup succeeds")
        .expect("conversation exists");
    assert!(stored.persona_id.is_none());
    assert!(service.get_stop_agent_calls().await.is_empty());
}

#[tokio::test]
async fn persona_switch_rejects_missing_or_archived_persona() {
    let _persona_feature = enable_personas_for_test();
    let state = AppState::new_test();
    let conversation_id = ChatConversationId::from_string("15151515-1515-4515-8515-151515151515");
    let conversation = seed_persona_switch_project_conversation(
        &state,
        conversation_id,
        ProjectId::from_string("project-persona-switch-unavailable".to_string()),
    )
    .await;
    let original =
        seed_persona_for_switch(&state, "persona-switch-original", PersonaStatus::Active).await;
    let draft = seed_persona_for_switch(&state, "persona-switch-draft", PersonaStatus::Draft).await;
    let archived =
        seed_persona_for_switch(&state, "persona-switch-archived", PersonaStatus::Archived).await;
    state
        .chat_conversation_repo
        .update_persona_binding(&conversation.id, Some(original.id.as_str()))
        .await
        .expect("original binding should persist");
    let service = MockChatService::new();

    for persona_id in [
        Some(PersonaId::from("persona-switch-missing")),
        Some(draft.id),
        Some(archived.id),
    ] {
        let error = switch_agent_conversation_persona_for_state_stopping_running_agent(
            persona_switch_input(&conversation.id, persona_id.as_ref()),
            &state,
            &service,
        )
        .await
        .expect_err("missing, draft, and archived personas must fail closed");
        assert!(
            error.starts_with("[Persona unavailable:"),
            "unexpected unavailable persona error: {error}"
        );
        let stored = state
            .chat_conversation_repo
            .get_by_id(&conversation.id)
            .await
            .expect("conversation lookup succeeds")
            .expect("conversation exists");
        assert_eq!(stored.persona_id.as_deref(), Some(original.id.as_str()));
    }
    assert!(
        service.get_stop_agent_calls().await.is_empty(),
        "invalid persona input must not stop an agent"
    );
}

#[tokio::test]
async fn persona_switch_rejects_cross_project_persona_without_clearing_existing_binding() {
    let _persona_feature = enable_personas_for_test();
    let state = AppState::new_test();
    let conversation_project_id =
        ProjectId::from_string("project-persona-switch-scope-a".to_string());
    let conversation = seed_persona_switch_project_conversation(
        &state,
        ChatConversationId::from_string("18181818-1818-4818-8818-181818181818"),
        conversation_project_id,
    )
    .await;
    let original =
        seed_persona_for_switch(&state, "persona-switch-global", PersonaStatus::Active).await;
    let scoped = seed_scoped_persona_for_switch(
        &state,
        "persona-switch-scope-b",
        &ProjectId::from_string("project-persona-switch-scope-b".to_string()),
    )
    .await;
    state
        .chat_conversation_repo
        .update_persona_binding(&conversation.id, Some(original.id.as_str()))
        .await
        .unwrap();
    let service = MockChatService::new();

    let error = switch_agent_conversation_persona_for_state_stopping_running_agent(
        persona_switch_input(&conversation.id, Some(&scoped.id)),
        &state,
        &service,
    )
    .await
    .expect_err("cross-project persona must not bind");

    assert!(error.starts_with("[Persona unavailable:"));
    let stored = state
        .chat_conversation_repo
        .get_by_id(&conversation.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.persona_id.as_deref(), Some(original.id.as_str()));
    assert!(service.get_stop_agent_calls().await.is_empty());
}

#[tokio::test]
async fn persona_switch_rejects_non_project_conversation() {
    let _persona_feature = enable_personas_for_test();
    let state = AppState::new_test();
    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_task(TaskId::from_string(
            "task-persona-switch-non-project".to_string(),
        )))
        .await
        .expect("task conversation should persist");
    let service = MockChatService::new();

    let error = switch_agent_conversation_persona_for_state_stopping_running_agent(
        persona_switch_input(&conversation.id, None),
        &state,
        &service,
    )
    .await
    .expect_err("persona bindings require Project conversation context");

    assert_eq!(error, "Only project agent conversations can change persona");
    assert!(service.get_stop_agent_calls().await.is_empty());
}

#[tokio::test]
async fn persona_switch_rejects_when_flag_off() {
    let state = AppState::new_test();
    let conversation_id = ChatConversationId::from_string("16161616-1616-4616-8616-161616161616");
    let conversation = seed_persona_switch_project_conversation(
        &state,
        conversation_id,
        ProjectId::from_string("project-persona-switch-disabled".to_string()),
    )
    .await;
    let service = MockChatService::new();

    let error = switch_agent_conversation_persona_for_state_stopping_running_agent(
        persona_switch_input(&conversation.id, None),
        &state,
        &service,
    )
    .await
    .expect_err("disabled persona feature should reject the switch");

    assert!(error.starts_with("[Personas disabled:"));
    assert!(service.get_stop_agent_calls().await.is_empty());
}

#[tokio::test]
async fn persona_switch_errors_when_agent_cannot_stop() {
    let _persona_feature = enable_personas_for_test();
    let state = AppState::new_test();
    let conversation_id = ChatConversationId::from_string("17171717-1717-4717-8717-171717171717");
    let conversation = seed_persona_switch_project_conversation(
        &state,
        conversation_id,
        ProjectId::from_string("project-persona-switch-cannot-stop".to_string()),
    )
    .await;
    let original = seed_persona_for_switch(
        &state,
        "persona-switch-cannot-stop-original",
        PersonaStatus::Active,
    )
    .await;
    let replacement = seed_persona_for_switch(
        &state,
        "persona-switch-cannot-stop-replacement",
        PersonaStatus::Active,
    )
    .await;
    state
        .chat_conversation_repo
        .update_persona_binding(&conversation.id, Some(original.id.as_str()))
        .await
        .expect("original binding should persist");
    let running_key = RunningAgentKey::new(
        ChatContextType::Project.to_string(),
        conversation.id.as_str(),
    );
    state
        .running_agent_registry
        .register(
            running_key.clone(),
            0,
            conversation.id.as_str().to_string(),
            "run-persona-switch-cannot-stop".to_string(),
            None,
            None,
        )
        .await;
    let service = MockChatService::new();

    let error = switch_agent_conversation_persona_for_state_stopping_running_agent(
        persona_switch_input(&conversation.id, Some(&replacement.id)),
        &state,
        &service,
    )
    .await
    .expect_err("a still-running agent must block the binding update");

    assert_eq!(error, "Cannot change persona while the agent is running");
    assert_eq!(
        service.get_stop_agent_calls().await,
        vec![(
            ChatContextType::Project,
            conversation.id.as_str().to_string()
        )]
    );
    assert!(state.running_agent_registry.is_running(&running_key).await);
    let stored = state
        .chat_conversation_repo
        .get_by_id(&conversation.id)
        .await
        .expect("conversation lookup succeeds")
        .expect("conversation exists");
    assert_eq!(stored.persona_id.as_deref(), Some(original.id.as_str()));
}

async fn seed_automation_mode_switch_workspace(
    state: &AppState,
    conversation_id: ChatConversationId,
    project_id: ProjectId,
    mode: AgentConversationWorkspaceMode,
) {
    let mut project = Project::new(
        "Automation Mode Switch Project".to_string(),
        "/tmp/project".to_string(),
    );
    project.id = project_id.clone();
    state
        .project_repo
        .create(project)
        .await
        .expect("project persisted");

    let mut conversation = ChatConversation::new_project(project_id.clone());
    conversation.id = conversation_id;
    conversation.automation_id = Some(AutomationId::from_string("automation-1"));
    conversation.automation_run_id = Some(AutomationRunId::from_string("run-1"));
    conversation.set_agent_mode(Some(mode));
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("automation conversation persisted");

    let workspace = AgentConversationWorkspace::new(
        conversation_id,
        project_id,
        mode,
        IdeationAnalysisBaseRefKind::CurrentBranch,
        "feature/mode-switch".to_string(),
        Some("Current branch (feature/mode-switch)".to_string()),
        Some("base-sha".to_string()),
        "ralphx/project/agent-mode-switch".to_string(),
        "/tmp/ralphx-agent-mode-switch".to_string(),
    );
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("automation workspace persisted");
}

#[tokio::test]
async fn unlinked_ideation_conversation_can_switch_to_chat_and_updates_workspace_mode() {
    let state = AppState::new_test();
    let project_id = ProjectId::from_string("project-unlinked-mode-switch".to_string());
    let conversation_id = ChatConversationId::from_string("22222222-2222-4222-8222-222222222222");
    seed_mode_switch_workspace(
        &state,
        conversation_id,
        project_id,
        AgentConversationWorkspaceMode::Ideation,
    )
    .await;

    let response = switch_agent_conversation_mode_for_state(
        SwitchAgentConversationModeInput {
            conversation_id: conversation_id.as_str(),
            mode: "chat".to_string(),
            runtime_override: None,
            base_ref_kind: None,
            base_branch_mode: None,
            base_ref: None,
            base_display_name: None,
            base_source_pull_request: None,
        },
        &state,
    )
    .await
    .expect("unlinked ideation mode can switch to chat");

    assert_eq!(response.conversation.agent_mode.as_deref(), Some("chat"));
    let response_workspace = response.workspace.expect("workspace should remain");
    assert_eq!(response_workspace.mode, "chat");
    assert!(!response_workspace.mode_switch_locked);

    let stored = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace load succeeds")
        .expect("workspace exists");
    assert_eq!(stored.mode, AgentConversationWorkspaceMode::Chat);
    assert!(stored.linked_ideation_session_id.is_none());
    assert!(stored.linked_plan_branch_id.is_none());
}

#[tokio::test]
async fn user_mode_switch_rejects_automation_run_conversation() {
    let state = AppState::new_test();
    let project_id = ProjectId::from_string("project-automation-user-mode-switch".to_string());
    let conversation_id = ChatConversationId::from_string("37373737-3737-4737-8737-373737373737");
    seed_automation_mode_switch_workspace(
        &state,
        conversation_id,
        project_id,
        AgentConversationWorkspaceMode::Plan,
    )
    .await;

    let error = switch_agent_conversation_mode_for_state(
        SwitchAgentConversationModeInput {
            conversation_id: conversation_id.as_str(),
            mode: "edit".to_string(),
            runtime_override: None,
            base_ref_kind: None,
            base_branch_mode: None,
            base_ref: None,
            base_display_name: None,
            base_source_pull_request: None,
        },
        &state,
    )
    .await
    .expect_err("user switch should not bypass automation plan gate");

    assert!(error.contains(AUTOMATION_RUN_MODE_LOCKED_ERROR_CODE));
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should still exist");
    assert_eq!(workspace.mode, AgentConversationWorkspaceMode::Plan);
}

#[tokio::test]
async fn switch_mode_rejects_persona_builder_target() {
    let state = AppState::new_test();
    let error = switch_agent_conversation_mode_for_state(
        SwitchAgentConversationModeInput {
            conversation_id: "persona-builder-target".to_string(),
            mode: "persona_builder".to_string(),
            runtime_override: None,
            base_ref_kind: None,
            base_branch_mode: None,
            base_ref: None,
            base_display_name: None,
            base_source_pull_request: None,
        },
        &state,
    )
    .await
    .expect_err("generic mode switch must reject PersonaBuilder before loading a conversation");

    assert!(
        error.contains("PersonaBuilder"),
        "unexpected error: {error}"
    );
}

#[tokio::test]
async fn create_agent_conversation_persona_builder_is_flag_gated_and_persists_mode() {
    use ralphx_lib::infrastructure::agents::{
        reset_agent_personas_override_for_test, set_agent_personas_override,
    };
    let app = tauri::test::mock_builder()
        .manage(AppState::new_test())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app should build");
    let input = || CreateAgentConversationInput {
        context_type: ChatContextType::Project.to_string(),
        context_id: Some("project-builder-create".to_string()),
        title: Some("Builder seed".to_string()),
        mode: Some("persona_builder".to_string()),
        team_intent: None,
    };

    set_agent_personas_override(Some(false));
    let disabled = create_agent_conversation(input(), app.state())
        .await
        .expect_err("builder create must reject while agent_personas is disabled");
    assert!(disabled.contains("agent_personas"));

    set_agent_personas_override(Some(true));
    let created = create_agent_conversation(input(), app.state())
        .await
        .expect("builder create should use the standard pipeline when enabled");
    assert_eq!(created.agent_mode.as_deref(), Some("persona_builder"));
    let state = app.state::<AppState>();
    let app_data_dir = state.app_paths.app_data_dir();
    let workspace =
        ralphx_lib::application::standalone_workspace::resolve_workspace(app_data_dir, &created.id)
            .expect("builder pre-send creation must materialize its private workspace");
    assert!(workspace.join("manifest.json").is_file());
    reset_agent_personas_override_for_test();
}

#[tokio::test]
async fn create_agent_conversation_project_persona_builder_rejects_team_intent() {
    use ralphx_lib::infrastructure::agents::{
        reset_agent_personas_override_for_test, set_agent_personas_override,
    };
    set_agent_personas_override(Some(true));
    let app = tauri::test::mock_builder()
        .manage(AppState::new_test())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app should build");
    let error = create_agent_conversation(
        CreateAgentConversationInput {
            context_type: ChatContextType::Project.to_string(),
            context_id: Some("project-builder-team-create".to_string()),
            title: None,
            mode: Some("persona_builder".to_string()),
            team_intent: Some(ralphx_lib::domain::entities::TeamIntent::rx_native(None)),
        },
        app.state(),
    )
    .await
    .expect_err("Project builder create must reject Team intent");
    assert!(error.contains("Team mode"));
    reset_agent_personas_override_for_test();
}

#[tokio::test]
async fn create_agent_conversation_rejects_persona_builder_outside_project_or_standalone() {
    use ralphx_lib::infrastructure::agents::{
        reset_agent_personas_override_for_test, set_agent_personas_override,
    };
    set_agent_personas_override(Some(true));
    let app = tauri::test::mock_builder()
        .manage(AppState::new_test())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app should build");

    for context_type in [ChatContextType::Task, ChatContextType::Ideation] {
        let context_id = format!("invalid-builder-{context_type}");
        let error = create_agent_conversation(
            CreateAgentConversationInput {
                context_type: context_type.to_string(),
                context_id: Some(context_id.clone()),
                title: None,
                mode: Some("persona_builder".to_string()),
                team_intent: None,
            },
            app.state(),
        )
        .await
        .expect_err("PersonaBuilder must reject unsupported contexts before persistence");

        assert!(
            error.contains("Project or Standalone"),
            "unexpected error: {error}"
        );
        assert!(app
            .state::<AppState>()
            .chat_conversation_repo
            .get_by_context(context_type, &context_id)
            .await
            .expect("conversation lookup should succeed")
            .is_empty());
    }

    reset_agent_personas_override_for_test();
}

#[test]
fn send_agent_message_rejects_team_flip_for_project_persona_builder() {
    let project_id = ProjectId::from_string("project-builder-send-team".to_string());
    let mut conversation = ChatConversation::new_project(project_id);
    conversation.set_agent_mode(Some(AgentConversationWorkspaceMode::PersonaBuilder));
    let error = validate_persona_builder_team_intent_for_send(
        ChatContextType::Project,
        Some(&conversation),
        CoordinationMode::RxNativeTeam,
    )
    .expect_err("send-time Team flip must reject for Project builder conversations");
    assert!(
        error.contains("persona builder"),
        "unexpected error: {error}"
    );
}

#[tokio::test]
async fn update_coordination_mode_rejects_builder_and_allows_project_chat() {
    let state = AppState::new_test();
    state.agent_capability_gate.replace(
        ralphx_lib::application::agent_capability_gate::AgentCapabilities {
            team: true,
            workflows: false,
            autopilot: false,
        },
    );
    let project_id = ProjectId::from_string("project-coordination-builder-guard".to_string());
    let mut builder = ChatConversation::new_project(project_id.clone());
    builder.set_agent_mode(Some(AgentConversationWorkspaceMode::PersonaBuilder));
    let builder = state
        .chat_conversation_repo
        .create(builder)
        .await
        .expect("builder conversation should persist");
    let chat = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project_id))
        .await
        .expect("chat conversation should persist");
    let app = tauri::test::mock_builder()
        .manage(state)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app should build");

    let error = update_agent_conversation_coordination_mode(
        UpdateAgentConversationCoordinationModeInput {
            conversation_id: builder.id.as_str(),
            coordination_mode: "rx_native_team".to_string(),
            model_override: None,
        },
        app.state(),
    )
    .await
    .expect_err("builder coordination must remain solo");
    assert!(error.contains("persona builder"));
    let stored_builder = app
        .state::<AppState>()
        .chat_conversation_repo
        .get_by_id(&builder.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored_builder.coordination_mode, CoordinationMode::Solo);

    let response = update_agent_conversation_coordination_mode(
        UpdateAgentConversationCoordinationModeInput {
            conversation_id: chat.id.as_str(),
            coordination_mode: "rx_native_team".to_string(),
            model_override: None,
        },
        app.state(),
    )
    .await
    .expect("Project chat coordination should still update");
    assert_eq!(response.coordination_mode, "rx_native_team");
}

#[tokio::test]
async fn mode_switch_rejected_from_persona_builder_keyed_on_conversation_agent_mode() {
    let state = AppState::new_test();
    let project_id = ProjectId::from_string("project-persona-builder-mode-lock".to_string());
    let conversation_id =
        ChatConversationId::from_string("56565656-5656-4565-8565-565656565656".to_string());
    seed_mode_locked_conversation_without_workspace(
        &state,
        conversation_id,
        project_id,
        AgentConversationWorkspaceMode::PersonaBuilder,
    )
    .await;

    let error = switch_agent_conversation_mode_for_state(
        SwitchAgentConversationModeInput {
            conversation_id: conversation_id.as_str(),
            mode: "edit".to_string(),
            runtime_override: None,
            base_ref_kind: None,
            base_branch_mode: None,
            base_ref: None,
            base_display_name: None,
            base_source_pull_request: None,
        },
        &state,
    )
    .await
    .expect_err("PersonaBuilder source mode must lock without a workspace row");

    assert!(
        error.contains("PersonaBuilder"),
        "unexpected error: {error}"
    );
    assert!(state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup succeeds")
        .is_none());
}

#[tokio::test]
async fn mode_switch_rejected_from_automation_conversation_backend() {
    let state = AppState::new_test();
    let project_id = ProjectId::from_string("project-automation-mode-lock".to_string());
    let conversation_id =
        ChatConversationId::from_string("57575757-5757-4575-8575-575757575757".to_string());
    seed_mode_locked_conversation_without_workspace(
        &state,
        conversation_id,
        project_id,
        AgentConversationWorkspaceMode::Automation,
    )
    .await;

    let error = switch_agent_conversation_mode_for_state(
        SwitchAgentConversationModeInput {
            conversation_id: conversation_id.as_str(),
            mode: "edit".to_string(),
            runtime_override: None,
            base_ref_kind: None,
            base_branch_mode: None,
            base_ref: None,
            base_display_name: None,
            base_source_pull_request: None,
        },
        &state,
    )
    .await
    .expect_err("Automation source mode must be locked without automation-run metadata");

    assert!(error.contains("Automation"), "unexpected error: {error}");
}

#[tokio::test]
async fn system_mode_switch_allows_automation_run_conversation() {
    let state = AppState::new_test();
    let project_id = ProjectId::from_string("project-automation-system-mode-switch".to_string());
    let conversation_id = ChatConversationId::from_string("38383838-3838-4838-8838-383838383838");
    seed_automation_mode_switch_workspace(
        &state,
        conversation_id,
        project_id,
        AgentConversationWorkspaceMode::Plan,
    )
    .await;

    let response = switch_agent_conversation_mode_for_state_allowing_running(
        SwitchAgentConversationModeInput {
            conversation_id: conversation_id.as_str(),
            mode: "edit".to_string(),
            runtime_override: None,
            base_ref_kind: None,
            base_branch_mode: None,
            base_ref: None,
            base_display_name: None,
            base_source_pull_request: None,
        },
        &state,
        ModeSwitchInitiator::System,
    )
    .await
    .expect("system switch should be allowed for automation run conversations");

    assert_eq!(response.conversation.agent_mode.as_deref(), Some("edit"));
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should still exist");
    assert_eq!(workspace.mode, AgentConversationWorkspaceMode::Edit);
}

#[tokio::test]
async fn user_mode_switch_still_allows_non_automation_conversation() {
    let state = AppState::new_test();
    let project_id = ProjectId::from_string("project-non-automation-mode-switch".to_string());
    let conversation_id = ChatConversationId::from_string("39393939-3939-4939-8939-393939393939");
    seed_mode_switch_workspace(
        &state,
        conversation_id,
        project_id,
        AgentConversationWorkspaceMode::Plan,
    )
    .await;

    let response = switch_agent_conversation_mode_for_state(
        SwitchAgentConversationModeInput {
            conversation_id: conversation_id.as_str(),
            mode: "edit".to_string(),
            runtime_override: None,
            base_ref_kind: None,
            base_branch_mode: None,
            base_ref: None,
            base_display_name: None,
            base_source_pull_request: None,
        },
        &state,
    )
    .await
    .expect("non-automation user switch should remain allowed");

    assert_eq!(response.conversation.agent_mode.as_deref(), Some("edit"));
}

#[tokio::test]
async fn plan_to_edit_mode_switch_clears_the_planning_provider_session() {
    let state = AppState::new_test();
    let project_id = ProjectId::from_string("project-plan-to-edit-session-boundary".to_string());
    let conversation_id = ChatConversationId::from_string("3a3a3a3a-3a3a-4a3a-8a3a-3a3a3a3a3a3a");
    seed_mode_switch_workspace(
        &state,
        conversation_id,
        project_id,
        AgentConversationWorkspaceMode::Plan,
    )
    .await;
    state
        .chat_conversation_repo
        .update_provider_session_ref(
            &conversation_id,
            &ProviderSessionRef {
                harness: AgentHarnessKind::Codex,
                provider_session_id: "planning-thread".to_string(),
            },
        )
        .await
        .expect("planning session should persist");

    let response = switch_agent_conversation_mode_for_state(
        SwitchAgentConversationModeInput {
            conversation_id: conversation_id.as_str(),
            mode: "edit".to_string(),
            runtime_override: None,
            base_ref_kind: None,
            base_branch_mode: None,
            base_ref: None,
            base_display_name: None,
            base_source_pull_request: None,
        },
        &state,
    )
    .await
    .expect("Plan-to-Edit switch should succeed");

    assert_eq!(response.conversation.agent_mode.as_deref(), Some("edit"));
    assert!(response.conversation.provider_session_id.is_none());
    assert!(response.conversation.provider_harness.is_none());
    let stored = state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .expect("conversation lookup should succeed")
        .expect("conversation should exist");
    assert!(
        stored.provider_session_ref().is_none(),
        "the next implementation send must create a provider session instead of resuming Plan mode"
    );
}

#[tokio::test]
async fn active_linked_ideation_session_blocks_mode_switch() {
    let state = AppState::new_test();
    let project_id = ProjectId::from_string("project-active-ideation-link".to_string());
    let conversation_id = ChatConversationId::from_string("33333333-3333-4333-8333-333333333333");
    seed_mode_switch_workspace(
        &state,
        conversation_id,
        project_id.clone(),
        AgentConversationWorkspaceMode::Ideation,
    )
    .await;
    let session = state
        .ideation_session_repo
        .create(IdeationSession::new(project_id))
        .await
        .expect("ideation session persisted");
    state
        .agent_conversation_workspace_repo
        .update_links(&conversation_id, Some(&session.id), None)
        .await
        .expect("workspace linked to session");

    let error = switch_agent_conversation_mode_for_state(
        SwitchAgentConversationModeInput {
            conversation_id: conversation_id.as_str(),
            mode: "edit".to_string(),
            runtime_override: None,
            base_ref_kind: None,
            base_branch_mode: None,
            base_ref: None,
            base_display_name: None,
            base_source_pull_request: None,
        },
        &state,
    )
    .await
    .expect_err("active linked ideation should lock mode switch");

    assert!(error.contains("Ideation session is still active"));
}

#[tokio::test]
async fn mode_switch_stopping_running_agent_stops_current_run_and_switches() {
    let state = AppState::new_test();
    let project_id = ProjectId::from_string("project-stop-before-mode-switch".to_string());
    let conversation_id = ChatConversationId::from_string("35353535-3535-4535-8535-353535353535");
    seed_mode_switch_workspace(
        &state,
        conversation_id,
        project_id,
        AgentConversationWorkspaceMode::Plan,
    )
    .await;

    let running_key = RunningAgentKey::new(
        ChatContextType::Project.to_string(),
        conversation_id.as_str(),
    );
    state
        .running_agent_registry
        .register(
            running_key.clone(),
            0,
            conversation_id.as_str().to_string(),
            "run-plan-proposal".to_string(),
            None,
            None,
        )
        .await;

    let service = state.build_chat_service_with_execution_state(Arc::new(ExecutionState::new()));
    let response = switch_agent_conversation_mode_for_state_stopping_running_agent(
        SwitchAgentConversationModeInput {
            conversation_id: conversation_id.as_str(),
            mode: "edit".to_string(),
            runtime_override: None,
            base_ref_kind: None,
            base_branch_mode: None,
            base_ref: None,
            base_display_name: None,
            base_source_pull_request: None,
        },
        &state,
        &service,
    )
    .await
    .expect("stop-and-switch mode change should succeed");

    assert_eq!(response.conversation.agent_mode.as_deref(), Some("edit"));
    assert_eq!(
        response.workspace.expect("workspace should remain").mode,
        "edit"
    );
    assert!(
        !state.running_agent_registry.is_running(&running_key).await,
        "running agent registry entry should be stopped before switching"
    );
}

#[tokio::test]
async fn mode_switch_stopping_running_agent_applies_to_other_valid_mode_switches() {
    let state = AppState::new_test();
    let project_id = ProjectId::from_string("project-stop-before-plan-switch".to_string());
    let conversation_id = ChatConversationId::from_string("36363636-3636-4636-8636-363636363636");
    seed_mode_switch_workspace(
        &state,
        conversation_id,
        project_id,
        AgentConversationWorkspaceMode::Edit,
    )
    .await;

    let running_key = RunningAgentKey::new(
        ChatContextType::Project.to_string(),
        conversation_id.as_str(),
    );
    state
        .running_agent_registry
        .register(
            running_key.clone(),
            0,
            conversation_id.as_str().to_string(),
            "run-edit-work".to_string(),
            None,
            None,
        )
        .await;

    let service = state.build_chat_service_with_execution_state(Arc::new(ExecutionState::new()));
    let response = switch_agent_conversation_mode_for_state_stopping_running_agent(
        SwitchAgentConversationModeInput {
            conversation_id: conversation_id.as_str(),
            mode: "plan".to_string(),
            runtime_override: None,
            base_ref_kind: None,
            base_branch_mode: None,
            base_ref: None,
            base_display_name: None,
            base_source_pull_request: None,
        },
        &state,
        &service,
    )
    .await
    .expect("stop-and-switch should apply to any valid mode switch");

    assert_eq!(response.conversation.agent_mode.as_deref(), Some("plan"));
    assert_eq!(
        response.workspace.expect("workspace should remain").mode,
        "plan"
    );
    assert!(
        !state.running_agent_registry.is_running(&running_key).await,
        "running agent registry entry should be stopped before switching"
    );
}

#[tokio::test]
async fn abandoned_pipeline_link_can_switch_to_edit_and_detaches_links() {
    let state = AppState::new_test();
    let project_id = ProjectId::from_string("project-abandoned-pipeline-link".to_string());
    let conversation_id = ChatConversationId::from_string("44444444-4444-4444-8444-444444444444");
    seed_mode_switch_workspace(
        &state,
        conversation_id,
        project_id.clone(),
        AgentConversationWorkspaceMode::Ideation,
    )
    .await;
    let session = state
        .ideation_session_repo
        .create(IdeationSession::new(project_id.clone()))
        .await
        .expect("ideation session persisted");
    let mut plan_branch = PlanBranch::new(
        ArtifactId::from_string("artifact-abandoned"),
        session.id.clone(),
        project_id,
        "plan-abandoned".to_string(),
        "main".to_string(),
    );
    plan_branch.status = PlanBranchStatus::Abandoned;
    let plan_branch = state
        .plan_branch_repo
        .create(plan_branch)
        .await
        .expect("plan branch persisted");
    state
        .agent_conversation_workspace_repo
        .update_links(&conversation_id, Some(&session.id), Some(&plan_branch.id))
        .await
        .expect("workspace linked to abandoned pipeline");

    let response = switch_agent_conversation_mode_for_state(
        SwitchAgentConversationModeInput {
            conversation_id: conversation_id.as_str(),
            mode: "edit".to_string(),
            runtime_override: None,
            base_ref_kind: None,
            base_branch_mode: None,
            base_ref: None,
            base_display_name: None,
            base_source_pull_request: None,
        },
        &state,
    )
    .await
    .expect("abandoned pipeline should not lock mode switch");

    let response_workspace = response.workspace.expect("workspace should remain");
    assert_eq!(response_workspace.mode, "edit");
    assert!(response_workspace.linked_ideation_session_id.is_none());
    assert!(response_workspace.linked_plan_branch_id.is_none());
    assert!(!response_workspace.mode_switch_locked);

    let stored = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace load succeeds")
        .expect("workspace exists");
    assert_eq!(stored.mode, AgentConversationWorkspaceMode::Edit);
    assert!(stored.linked_ideation_session_id.is_none());
    assert!(stored.linked_plan_branch_id.is_none());
}

#[tokio::test]
async fn superseded_execution_plan_link_can_switch_to_edit_and_detaches_links() {
    let state = AppState::new_test();
    let project_id = ProjectId::from_string("project-superseded-pipeline-link".to_string());
    let conversation_id = ChatConversationId::from_string("66666666-6666-4666-8666-666666666666");
    seed_mode_switch_workspace(
        &state,
        conversation_id,
        project_id.clone(),
        AgentConversationWorkspaceMode::Ideation,
    )
    .await;
    let session = state
        .ideation_session_repo
        .create(IdeationSession::new(project_id.clone()))
        .await
        .expect("ideation session persisted");
    let mut execution_plan = ExecutionPlan::new(session.id.clone());
    execution_plan.status = ExecutionPlanStatus::Superseded;
    let execution_plan = state
        .execution_plan_repo
        .create(execution_plan)
        .await
        .expect("execution plan persisted");
    let mut plan_branch = PlanBranch::new(
        ArtifactId::from_string("artifact-superseded"),
        session.id.clone(),
        project_id,
        "plan-superseded".to_string(),
        "main".to_string(),
    );
    plan_branch.execution_plan_id = Some(execution_plan.id);
    let plan_branch = state
        .plan_branch_repo
        .create(plan_branch)
        .await
        .expect("plan branch persisted");
    state
        .agent_conversation_workspace_repo
        .update_links(&conversation_id, Some(&session.id), Some(&plan_branch.id))
        .await
        .expect("workspace linked to superseded pipeline");

    let response = switch_agent_conversation_mode_for_state(
        SwitchAgentConversationModeInput {
            conversation_id: conversation_id.as_str(),
            mode: "edit".to_string(),
            runtime_override: None,
            base_ref_kind: None,
            base_branch_mode: None,
            base_ref: None,
            base_display_name: None,
            base_source_pull_request: None,
        },
        &state,
    )
    .await
    .expect("superseded execution plan should not lock mode switch");

    let response_workspace = response.workspace.expect("workspace should remain");
    assert_eq!(response_workspace.mode, "edit");
    assert!(response_workspace.linked_ideation_session_id.is_none());
    assert!(response_workspace.linked_plan_branch_id.is_none());
    assert!(!response_workspace.mode_switch_locked);
}

#[tokio::test]
async fn active_pipeline_link_blocks_mode_switch() {
    let state = AppState::new_test();
    let project_id = ProjectId::from_string("project-active-pipeline-link".to_string());
    let conversation_id = ChatConversationId::from_string("55555555-5555-4555-8555-555555555555");
    seed_mode_switch_workspace(
        &state,
        conversation_id,
        project_id.clone(),
        AgentConversationWorkspaceMode::Ideation,
    )
    .await;
    let session = state
        .ideation_session_repo
        .create(IdeationSession::new(project_id.clone()))
        .await
        .expect("ideation session persisted");
    let plan_branch = state
        .plan_branch_repo
        .create(PlanBranch::new(
            ArtifactId::from_string("artifact-active"),
            session.id.clone(),
            project_id,
            "plan-active".to_string(),
            "main".to_string(),
        ))
        .await
        .expect("plan branch persisted");
    state
        .agent_conversation_workspace_repo
        .update_links(&conversation_id, Some(&session.id), Some(&plan_branch.id))
        .await
        .expect("workspace linked to active pipeline");

    let error = switch_agent_conversation_mode_for_state(
        SwitchAgentConversationModeInput {
            conversation_id: conversation_id.as_str(),
            mode: "edit".to_string(),
            runtime_override: None,
            base_ref_kind: None,
            base_branch_mode: None,
            base_ref: None,
            base_display_name: None,
            base_source_pull_request: None,
        },
        &state,
    )
    .await
    .expect_err("active pipeline should lock mode switch");

    assert!(error.contains("Plan execution is still active"));
}

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git command should spawn");
    assert!(
        output.status.success(),
        "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn setup_publish_repo(repo_path: &Path) -> String {
    std::fs::create_dir_all(repo_path).expect("repo root should be created");
    git(repo_path, &["init", "-b", "main"]);
    git(repo_path, &["config", "user.email", "test@example.com"]);
    git(repo_path, &["config", "user.name", "Test User"]);
    std::fs::write(repo_path.join("README.md"), "base\n").expect("fixture file should be written");
    git(repo_path, &["add", "README.md"]);
    git(repo_path, &["commit", "-m", "base"]);
    git(repo_path, &["rev-parse", "HEAD"])
}

fn branch_exists(repo: &Path, branch: &str) -> bool {
    Command::new("git")
        .args(["rev-parse", "--verify", branch])
        .current_dir(repo)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

async fn setup_ipc_workspace_state(
    suffix: &str,
    capture_base_commit: bool,
    publication_pr_number: Option<i64>,
    github: Arc<crate::common::MockGithubService>,
) -> (
    tempfile::TempDir,
    AppState,
    ChatConversationId,
    Arc<crate::common::MockGithubService>,
) {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    let main_sha = setup_publish_repo(&repo_path);

    let mut project = Project::new(
        format!("IPC Workspace {suffix}"),
        repo_path.to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
    let conversation_id = ChatConversationId::from_string(format!("conversation-ipc-{suffix}"));
    let mut workspace = prepare_agent_conversation_workspace(
        &project,
        &conversation_id,
        AgentConversationWorkspaceMode::Edit,
        AgentConversationWorkspaceBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
            branch_mode: None,
            base_ref: Some("main".to_string()),
            display_name: None,
            source_pull_request: None,
        },
    )
    .await
    .expect("workspace should be prepared");
    workspace.base_ref = "feature/deleted-base".to_string();
    workspace.base_display_name = Some("Current branch (feature/deleted-base)".to_string());
    workspace.base_commit = capture_base_commit.then_some(main_sha);
    workspace.publication_pr_number = publication_pr_number;
    workspace.publication_pr_url =
        publication_pr_number.map(|number| format!("https://github.com/mock/repo/pull/{number}"));
    workspace.publication_pr_status = publication_pr_number.map(|_| "open".to_string());

    let mut state = AppState::new_test();
    let github_trait: Arc<dyn GithubServiceTrait> = github.clone();
    state.github_service = Some(github_trait);
    state
        .project_repo
        .create(project.clone())
        .await
        .expect("project should be persisted");
    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.id = conversation_id;
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("conversation should be persisted");
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should be persisted");

    (temp, state, conversation_id, github)
}

#[tokio::test]
async fn ipc_contract_startup_terminal_pr_cleanup_removes_plan_and_workspace_artifacts() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_publish_repo(&repo_path);

    let mut project = Project::new(
        "IPC Terminal Cleanup".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
    let state = AppState::new_test();
    let project = state
        .project_repo
        .create(project)
        .await
        .expect("project should persist");

    let plan_branch_name = "ralphx/ipc-cleanup/plan-merged";
    git(&repo_path, &["checkout", "-b", plan_branch_name]);
    std::fs::write(repo_path.join("plan.txt"), "plan\n").expect("plan file should write");
    git(&repo_path, &["add", "."]);
    git(&repo_path, &["commit", "-m", "plan work"]);
    git(&repo_path, &["checkout", "main"]);
    git(
        &repo_path,
        &["merge", "--no-ff", plan_branch_name, "-m", "merge plan"],
    );

    let mut plan_branch = PlanBranch::new(
        ArtifactId::from_string("ipc-cleanup-artifact"),
        IdeationSessionId::from_string("ipc-cleanup-session"),
        project.id.clone(),
        plan_branch_name.to_string(),
        "main".to_string(),
    );
    plan_branch.status = PlanBranchStatus::Merged;
    plan_branch.pr_eligible = true;
    plan_branch.pr_number = Some(201);
    plan_branch.pr_status = Some(DbPrStatus::Merged);
    state
        .plan_branch_repo
        .create(plan_branch)
        .await
        .expect("plan branch should persist");

    let conversation_id = ChatConversationId::from_string("ipc-terminal-cleanup-conversation");
    let mut workspace = prepare_agent_conversation_workspace(
        &project,
        &conversation_id,
        AgentConversationWorkspaceMode::Edit,
        AgentConversationWorkspaceBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
            branch_mode: None,
            base_ref: Some("main".to_string()),
            display_name: None,
            source_pull_request: None,
        },
    )
    .await
    .expect("workspace should be prepared");
    let workspace_branch = workspace.branch_name.clone();
    let workspace_path = std::path::PathBuf::from(&workspace.worktree_path);
    std::fs::write(workspace_path.join("agent.txt"), "agent\n").expect("agent file should write");
    git(&workspace_path, &["add", "."]);
    git(&workspace_path, &["commit", "-m", "agent work"]);
    git(
        &repo_path,
        &["merge", "--no-ff", &workspace_branch, "-m", "merge agent"],
    );
    workspace.publication_pr_number = Some(202);
    workspace.publication_pr_status = Some("merged".to_string());
    workspace.publication_push_status = Some("pushed".to_string());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");

    cleanup_terminal_plan_branch_local_artifacts_on_startup(
        Arc::clone(&state.plan_branch_repo),
        Arc::clone(&state.project_repo),
        None,
        Arc::new(HashSet::new()),
        Arc::clone(&state.running_agent_registry),
    )
    .await;
    cleanup_terminal_agent_workspace_local_artifacts_on_startup(
        Arc::clone(&state.agent_conversation_workspace_repo),
        Arc::clone(&state.plan_branch_repo),
        Arc::clone(&state.project_repo),
        None,
        Arc::new(HashSet::new()),
        Arc::clone(&state.running_agent_registry),
    )
    .await;

    assert!(!branch_exists(&repo_path, plan_branch_name));
    assert!(!workspace_path.exists());
    assert!(!branch_exists(&repo_path, &workspace_branch));
}

#[tokio::test]
async fn ipc_contract_startup_terminal_pr_cleanup_respects_safety_guards() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_publish_repo(&repo_path);

    let mut project = Project::new(
        "IPC Cleanup Guards".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
    let state = AppState::new_test();
    let project = state
        .project_repo
        .create(project)
        .await
        .expect("project should persist");

    let unmerged_plan_branch = "ralphx/ipc-cleanup/plan-unmerged";
    git(&repo_path, &["checkout", "-b", unmerged_plan_branch]);
    std::fs::write(repo_path.join("unmerged-plan.txt"), "plan\n")
        .expect("unmerged plan file should write");
    git(&repo_path, &["add", "."]);
    git(&repo_path, &["commit", "-m", "unmerged plan"]);
    git(&repo_path, &["checkout", "main"]);
    let mut unmerged_plan = PlanBranch::new(
        ArtifactId::from_string("ipc-cleanup-unmerged-artifact"),
        IdeationSessionId::from_string("ipc-cleanup-guards-session"),
        project.id.clone(),
        unmerged_plan_branch.to_string(),
        "main".to_string(),
    );
    unmerged_plan.status = PlanBranchStatus::Merged;
    unmerged_plan.pr_eligible = true;
    unmerged_plan.pr_number = Some(401);
    unmerged_plan.pr_status = Some(DbPrStatus::Merged);
    state
        .plan_branch_repo
        .create(unmerged_plan)
        .await
        .expect("unmerged plan branch should persist");

    let missing_target_branch = "ralphx/ipc-cleanup/plan-missing-target";
    git(&repo_path, &["checkout", "-b", missing_target_branch]);
    git(&repo_path, &["checkout", "main"]);
    let mut missing_target_plan = PlanBranch::new(
        ArtifactId::from_string("ipc-cleanup-missing-target-artifact"),
        IdeationSessionId::from_string("ipc-cleanup-guards-session"),
        project.id.clone(),
        missing_target_branch.to_string(),
        "main".to_string(),
    );
    missing_target_plan.status = PlanBranchStatus::Merged;
    missing_target_plan.pr_eligible = true;
    missing_target_plan.pr_number = Some(402);
    missing_target_plan.pr_status = Some(DbPrStatus::Merged);
    missing_target_plan.base_branch_override = Some("missing-base".to_string());
    state
        .plan_branch_repo
        .create(missing_target_plan)
        .await
        .expect("missing-target plan branch should persist");

    let closed_conversation_id =
        ChatConversationId::from_string("ipc-closed-terminal-cleanup-conversation");
    let mut closed_workspace = prepare_agent_conversation_workspace(
        &project,
        &closed_conversation_id,
        AgentConversationWorkspaceMode::Edit,
        AgentConversationWorkspaceBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
            branch_mode: None,
            base_ref: Some("main".to_string()),
            display_name: None,
            source_pull_request: None,
        },
    )
    .await
    .expect("closed workspace should be prepared");
    let closed_branch = closed_workspace.branch_name.clone();
    let closed_path = std::path::PathBuf::from(&closed_workspace.worktree_path);
    closed_workspace.publication_pr_number = Some(403);
    closed_workspace.publication_pr_status = Some("closed".to_string());
    closed_workspace.publication_push_status = Some("pushed".to_string());
    state
        .agent_conversation_workspace_repo
        .create_or_update(closed_workspace)
        .await
        .expect("closed workspace should persist");

    let dirty_conversation_id =
        ChatConversationId::from_string("ipc-dirty-terminal-cleanup-conversation");
    let mut dirty_workspace = prepare_agent_conversation_workspace(
        &project,
        &dirty_conversation_id,
        AgentConversationWorkspaceMode::Edit,
        AgentConversationWorkspaceBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
            branch_mode: None,
            base_ref: Some("main".to_string()),
            display_name: None,
            source_pull_request: None,
        },
    )
    .await
    .expect("dirty workspace should be prepared");
    let dirty_branch = dirty_workspace.branch_name.clone();
    let dirty_path = std::path::PathBuf::from(&dirty_workspace.worktree_path);
    std::fs::write(dirty_path.join("dirty.txt"), "dirty\n").expect("dirty file should write");
    dirty_workspace.publication_pr_number = Some(404);
    dirty_workspace.publication_pr_status = Some("merged".to_string());
    dirty_workspace.publication_push_status = Some("pushed".to_string());
    state
        .agent_conversation_workspace_repo
        .create_or_update(dirty_workspace)
        .await
        .expect("dirty workspace should persist");

    cleanup_terminal_plan_branch_local_artifacts_on_startup(
        Arc::clone(&state.plan_branch_repo),
        Arc::clone(&state.project_repo),
        None,
        Arc::new(HashSet::new()),
        Arc::clone(&state.running_agent_registry),
    )
    .await;
    cleanup_terminal_agent_workspace_local_artifacts_on_startup(
        Arc::clone(&state.agent_conversation_workspace_repo),
        Arc::clone(&state.plan_branch_repo),
        Arc::clone(&state.project_repo),
        None,
        Arc::new(HashSet::new()),
        Arc::clone(&state.running_agent_registry),
    )
    .await;

    assert!(branch_exists(&repo_path, unmerged_plan_branch));
    assert!(branch_exists(&repo_path, missing_target_branch));
    assert!(!closed_path.exists());
    assert!(!branch_exists(&repo_path, &closed_branch));
    assert!(!dirty_path.exists());
    assert!(!branch_exists(&repo_path, &dirty_branch));
}

#[tokio::test]
async fn ipc_contract_agent_workspace_poller_cleans_merged_pr_artifacts() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_publish_repo(&repo_path);

    let mut project = Project::new(
        "IPC Poller Cleanup".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
    let state = AppState::new_test();
    let project = state
        .project_repo
        .create(project)
        .await
        .expect("project should persist");
    let conversation_id = ChatConversationId::from_string("ipc-poller-cleanup-conversation");
    let mut workspace = prepare_agent_conversation_workspace(
        &project,
        &conversation_id,
        AgentConversationWorkspaceMode::Edit,
        AgentConversationWorkspaceBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
            branch_mode: None,
            base_ref: Some("main".to_string()),
            display_name: None,
            source_pull_request: None,
        },
    )
    .await
    .expect("workspace should be prepared");
    let workspace_branch = workspace.branch_name.clone();
    let workspace_path = std::path::PathBuf::from(&workspace.worktree_path);
    std::fs::write(workspace_path.join("agent.txt"), "agent\n").expect("agent file should write");
    git(&workspace_path, &["add", "."]);
    git(&workspace_path, &["commit", "-m", "agent work"]);
    git(
        &repo_path,
        &["merge", "--no-ff", &workspace_branch, "-m", "merge agent"],
    );
    workspace.publication_pr_number = Some(303);
    workspace.publication_pr_status = Some("open".to_string());
    workspace.publication_push_status = Some("pushed".to_string());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");

    let github = Arc::new(crate::common::MockGithubService::new());
    github.will_return_status(GithubPrStatus::Merged {
        merge_commit_sha: None,
        merged_at: None,
    });
    let registry = PrPollerRegistry::new(
        Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>),
        Arc::clone(&state.plan_branch_repo),
    );
    registry.start_agent_workspace_polling_with_repair_repo(
        conversation_id,
        303,
        project.clone(),
        repo_path.clone(),
        Arc::clone(&state.agent_conversation_workspace_repo),
        Arc::clone(&state.agent_run_repo),
        Arc::clone(&state.agent_workspace_repair_repo),
        Arc::new(MockChatService::new()),
    );

    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            if !workspace_path.exists() && !branch_exists(&repo_path, &workspace_branch) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("poller should clean terminal artifacts");

    let updated = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should remain persisted");
    assert_eq!(updated.publication_pr_status.as_deref(), Some("merged"));
    assert_eq!(github.check_calls(), 1);
}

#[tokio::test]
async fn ipc_contract_agent_workspace_poller_cleans_closed_pr_artifacts() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_publish_repo(&repo_path);

    let mut project = Project::new(
        "IPC Poller Closed Cleanup".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
    let state = AppState::new_test();
    let project = state
        .project_repo
        .create(project)
        .await
        .expect("project should persist");
    let conversation_id = ChatConversationId::from_string("ipc-poller-closed-cleanup-conversation");
    let mut workspace = prepare_agent_conversation_workspace(
        &project,
        &conversation_id,
        AgentConversationWorkspaceMode::Edit,
        AgentConversationWorkspaceBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
            branch_mode: None,
            base_ref: Some("main".to_string()),
            display_name: None,
            source_pull_request: None,
        },
    )
    .await
    .expect("workspace should be prepared");
    let workspace_branch = workspace.branch_name.clone();
    let workspace_path = std::path::PathBuf::from(&workspace.worktree_path);
    std::fs::write(workspace_path.join("agent.txt"), "agent\n").expect("agent file should write");
    git(&workspace_path, &["add", "."]);
    git(&workspace_path, &["commit", "-m", "agent work"]);
    workspace.publication_pr_number = Some(405);
    workspace.publication_pr_status = Some("open".to_string());
    workspace.publication_push_status = Some("pushed".to_string());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");

    let github = Arc::new(crate::common::MockGithubService::new());
    github.will_return_status(GithubPrStatus::Closed);
    let registry = PrPollerRegistry::new(
        Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>),
        Arc::clone(&state.plan_branch_repo),
    );
    registry.start_agent_workspace_polling_with_repair_repo(
        conversation_id,
        405,
        project,
        repo_path.clone(),
        Arc::clone(&state.agent_conversation_workspace_repo),
        Arc::clone(&state.agent_run_repo),
        Arc::clone(&state.agent_workspace_repair_repo),
        Arc::new(MockChatService::new()),
    );

    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            if !workspace_path.exists() && !branch_exists(&repo_path, &workspace_branch) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("poller should clean closed PR artifacts");

    let updated = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should remain persisted");
    assert_eq!(updated.publication_pr_status.as_deref(), Some("closed"));
    assert_eq!(github.check_calls(), 1);
}

#[tokio::test]
async fn ipc_contract_bulk_agent_running_states_returns_requested_context_map() {
    let service = MockChatService::new();
    service
        .set_agent_running("project/conv-running".to_string(), true)
        .await;
    service
        .set_agent_running("project/conv-unrequested".to_string(), true)
        .await;

    let requested_ids = vec![
        "conv-running".to_string(),
        "conv-idle".to_string(),
        "conv-running".to_string(),
    ];
    let states =
        get_agent_running_states_for_service(&service, "project".to_string(), requested_ids)
            .await
            .expect("bulk running states should resolve");

    assert_eq!(
        states.get("conv-running").map(|state| state.is_running),
        Some(true)
    );
    assert_eq!(
        states.get("conv-running").map(|state| state.agent_status),
        Some(AgentRuntimeStatus::Generating)
    );
    assert_eq!(
        states.get("conv-idle").map(|state| state.is_running),
        Some(false)
    );
    assert_eq!(
        states.get("conv-idle").map(|state| state.agent_status),
        Some(AgentRuntimeStatus::Idle)
    );
    assert_eq!(states.get("conv-unrequested"), None);
    assert_eq!(states.len(), 2);
}

#[tokio::test]
async fn ipc_contract_get_agent_running_states_command_uses_registry_truth() {
    let registry = Arc::new(MemoryRunningAgentRegistry::new());
    registry
        .set_running(RunningAgentKey::new("project", "conv-running"))
        .await;
    registry
        .set_running(RunningAgentKey::new("project", "conv-unrequested"))
        .await;

    let app = tauri::test::mock_builder()
        .manage(AppState::new_sqlite_test_with_registry(registry))
        .manage(Arc::new(ExecutionState::new()))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app should build");

    let states = ralphx_lib::commands::unified_chat_commands::get_agent_running_states(
        "project".to_string(),
        vec!["conv-running".to_string(), "conv-idle".to_string()],
        app.state::<AppState>(),
        app.state::<Arc<ExecutionState>>(),
    )
    .await
    .expect("bulk running states command should resolve");

    assert_eq!(
        states.get("conv-running").map(|state| state.is_running),
        Some(true)
    );
    assert_eq!(
        states.get("conv-running").map(|state| state.agent_status),
        Some(AgentRuntimeStatus::Generating)
    );
    assert_eq!(
        states.get("conv-idle").map(|state| state.is_running),
        Some(false)
    );
    assert_eq!(
        states.get("conv-idle").map(|state| state.agent_status),
        Some(AgentRuntimeStatus::Idle)
    );
    assert_eq!(states.get("conv-unrequested"), None);
    assert_eq!(states.len(), 2);
}

#[tokio::test]
async fn workspace_publish_repair_message_wakes_same_agent_conversation() {
    let service = MockChatService::new();
    let workspace = test_agent_workspace();

    send_agent_workspace_publish_repair_message(
        &service,
        &workspace,
        "Failed to commit: typecheck failed",
        AgentWorkspaceRepairRuntimeOverrides::default(),
    )
    .await
    .expect("repair handoff should be sent through chat service");

    let messages = service.get_sent_messages().await;
    assert_eq!(messages.len(), 1);
    assert!(messages[0].contains("Commit & Publish failed"));
    assert!(messages[0].contains("Failed to commit: typecheck failed"));
    assert!(messages[0].contains("Workspace branch: ralphx/ralphx/agent-1234"));
    assert!(messages[0].contains("Base: Current branch (feature/agent-screen)"));
    assert!(messages[0].contains("Conversation ID: 00000000-0000-0000-0000-000000000123"));
    assert!(messages[0].contains("complete_agent_workspace_repair"));

    let options = service.get_sent_options().await;
    assert_eq!(options.len(), 1);
    assert_eq!(
        options[0].conversation_id_override,
        Some(workspace.conversation_id)
    );
    assert_eq!(
        options[0].agent_name_override.as_deref(),
        Some(AGENT_WORKSPACE_REPAIR)
    );
    assert!(options[0].force_new_provider_session);
    assert!(options[0].preserve_conversation_provider_session_ref);
}

#[tokio::test]
async fn workspace_publish_fixable_failure_is_routed_by_backend() {
    let state = AppState::new_test();
    let service = MockChatService::new();
    let (_temp, workspace, target) = test_agent_workspace_with_git_target();
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should seed");

    mark_agent_workspace_publish_failure_with_target(
        &state,
        &workspace,
        "Failed to commit workspace changes: typecheck failed",
        None,
        &service,
        &target,
    )
    .await;

    assert_eq!(service.call_count(), 1);
    let messages = service.get_sent_messages().await;
    assert_eq!(messages.len(), 1);
    assert!(messages[0].contains("typecheck failed"));
}

#[tokio::test]
async fn workspace_publish_repair_defers_to_role_runtime_but_starts_fresh_session() {
    let state = AppState::new_test();
    let service = MockChatService::new();
    let (_temp, workspace, target) = test_agent_workspace_with_git_target();
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should seed");

    let mut conversation = ChatConversation::new_project(workspace.project_id.clone());
    conversation.id = workspace.conversation_id;
    conversation.set_provider_session_ref(ProviderSessionRef {
        harness: AgentHarnessKind::Codex,
        provider_session_id: "thread-main".to_string(),
    });
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("conversation should seed");

    let mut latest_run = AgentRun::new(workspace.conversation_id);
    latest_run.harness = Some(AgentHarnessKind::Claude);
    latest_run.logical_model = Some("gpt-5.4".to_string());
    latest_run.effective_model_id = Some("gpt-5.4-provider".to_string());
    latest_run.logical_effort = Some(LogicalEffort::High);
    state
        .agent_run_repo
        .create(latest_run)
        .await
        .expect("run should seed");

    mark_agent_workspace_publish_failure_with_target(
        &state,
        &workspace,
        "Failed to commit workspace changes: merge conflict",
        None,
        &service,
        &target,
    )
    .await;

    let options = service.get_sent_options().await;
    assert_eq!(options.len(), 1);
    assert_eq!(options[0].harness_override, None);
    assert_eq!(options[0].model_override, None);
    assert_eq!(options[0].logical_effort_override, None);
    assert!(options[0].force_new_provider_session);
    assert!(options[0].preserve_conversation_provider_session_ref);
}

#[tokio::test]
async fn workspace_publish_operational_failure_is_not_routed_to_agent() {
    let state = AppState::new_test();
    let service = MockChatService::new();
    let workspace = test_agent_workspace();

    mark_agent_workspace_publish_failure(
        &state,
        &workspace,
        "GitHub integration is not available",
        None,
        &service,
    )
    .await;

    assert_eq!(service.call_count(), 0);
    assert!(service.get_sent_messages().await.is_empty());
}

#[tokio::test]
async fn workspace_publish_git_timeout_is_not_routed_to_agent() {
    let state = AppState::new_test();
    let service = MockChatService::new();
    let workspace = test_agent_workspace();

    mark_agent_workspace_publish_failure(
        &state,
        &workspace,
        "Git operation error: git command timed out after 60s",
        None,
        &service,
    )
    .await;

    assert_eq!(service.call_count(), 0);
    assert!(service.get_sent_messages().await.is_empty());
}

#[test]
fn workspace_repair_action_defaults_to_publish_for_legacy_events() {
    assert_eq!(
        agent_workspace_post_repair_action_from_events(&[]),
        AgentWorkspacePostRepairAction::Publish
    );

    let conversation_id = ChatConversationId::new();
    let events = vec![AgentConversationWorkspacePublicationEvent::new(
        conversation_id,
        "repair_requested",
        "started",
        "legacy repair request",
        Some("agent_fixable".to_string()),
    )];

    assert_eq!(
        agent_workspace_post_repair_action_from_events(&events),
        AgentWorkspacePostRepairAction::Publish
    );
}

#[test]
fn workspace_repair_action_uses_latest_requested_action() {
    let conversation_id = ChatConversationId::new();
    let events = vec![
        AgentConversationWorkspacePublicationEvent::new(
            conversation_id,
            "repair_requested",
            "started",
            "publish repair request",
            Some("agent_fixable:publish".to_string()),
        ),
        AgentConversationWorkspacePublicationEvent::new(
            conversation_id,
            "failed",
            "failed",
            "later base update failure",
            Some("agent_fixable".to_string()),
        ),
        AgentConversationWorkspacePublicationEvent::new(
            conversation_id,
            "repair_requested",
            "started",
            "base update repair request",
            Some("agent_fixable:update_only".to_string()),
        ),
    ];

    assert_eq!(
        agent_workspace_post_repair_action_from_events(&events),
        AgentWorkspacePostRepairAction::UpdateOnly
    );
}

// ── AgentRunStatusResponse model field tests ──────────────────────────────────

#[test]
fn test_agent_run_status_response_serializes_model_present() {
    let response = AgentRunStatusResponse {
        id: "run-1".to_string(),
        conversation_id: "conv-1".to_string(),
        status: "running".to_string(),
        started_at: "2024-01-01T00:00:00Z".to_string(),
        completed_at: None,
        error_message: None,
        model_id: Some("claude-sonnet-4-6".to_string()),
        model_label: Some("Sonnet 4.6".to_string()),
        persona_id: None,
        persona_slug: None,
        persona_version: None,
        persona_content_hash: None,
        persona_injected: None,
        persona_skipped_reason: None,
    };
    let json = serde_json::to_string(&response).unwrap();
    assert!(json.contains(r#""model_id":"claude-sonnet-4-6""#));
    assert!(json.contains(r#""model_label":"Sonnet 4.6""#));
}

#[test]
fn test_agent_run_status_response_serializes_model_absent() {
    let response = AgentRunStatusResponse {
        id: "run-2".to_string(),
        conversation_id: "conv-2".to_string(),
        status: "completed".to_string(),
        started_at: "2024-01-01T00:00:00Z".to_string(),
        completed_at: Some("2024-01-01T01:00:00Z".to_string()),
        error_message: None,
        model_id: None,
        model_label: None,
        persona_id: None,
        persona_slug: None,
        persona_version: None,
        persona_content_hash: None,
        persona_injected: None,
        persona_skipped_reason: None,
    };
    let json = serde_json::to_string(&response).unwrap();
    assert!(json.contains(r#""model_id":null"#));
    assert!(json.contains(r#""model_label":null"#));
}

#[test]
fn persona_agent_run_status_response_serializes_attribution_without_body() {
    let response = AgentRunStatusResponse {
        id: "run-persona".to_string(),
        conversation_id: "conv-persona".to_string(),
        status: "running".to_string(),
        started_at: "2026-07-13T06:19:00Z".to_string(),
        completed_at: None,
        error_message: None,
        model_id: Some("gpt-5.5".to_string()),
        model_label: Some("GPT-5.5".to_string()),
        persona_id: Some("persona-design-voice".to_string()),
        persona_slug: Some("design-voice".to_string()),
        persona_version: Some(2),
        persona_content_hash: Some("persona-hash".to_string()),
        persona_injected: Some(true),
        persona_skipped_reason: None,
    };

    let json = serde_json::to_string(&response).unwrap();
    assert!(json.contains(r#""persona_slug":"design-voice""#));
    assert!(json.contains(r#""persona_version":2"#));
    assert!(json.contains(r#""persona_injected":true"#));
    assert!(!json.contains("SECRET_PERSONA_BODY_SENTINEL"));
}

// ── IPC contract tests ─────────────────────────────────────────────────────────
// Verify camelCase deserialization for unified chat command input structs.

#[cfg(test)]
mod ipc_contract {
    use ralphx_lib::application::agent_conversation_workspace::{
        prepare_agent_conversation_workspace, AgentConversationWorkspaceBaseSelection,
    };
    use ralphx_lib::application::agent_workspace_bridge::{
        dispatch_prepared_agent_workspace_bridge_wakeup, prepare_agent_workspace_bridge_wakeup,
        wake_agent_workspace_for_bridge_events,
        wake_agent_workspace_for_bridge_events_with_service_factory,
    };
    use ralphx_lib::application::agent_workspace_publish_recovery::recover_stale_agent_workspace_publish_repairs_on_startup;
    use ralphx_lib::application::{AppState, MockChatService};
    use ralphx_lib::commands::agent_model_commands::{
        delete_custom_agent_model, list_agent_models, upsert_custom_agent_model,
        UpsertCustomAgentModelInput,
    };
    use ralphx_lib::commands::unified_chat_commands::archive_agent_conversation_inner;
    use ralphx_lib::commands::unified_chat_commands::{
        get_agent_conversation_messages_page_for_app_state, get_agent_conversation_summary,
        get_agent_conversation_summary_for_app_state, get_agent_conversation_workspace,
        get_agent_conversation_workspace_freshness, get_agent_message_tool_call_detail,
        publish_agent_conversation_workspace_for_app_state, start_agent_conversation,
        switch_agent_conversation_mode_for_state, AgentWorkspaceSourcePullRequestInput,
        CreateAgentConversationInput, QueueAgentMessageInput, SendAgentMessageInput,
        StartAgentConversationInput, SwitchAgentConversationModeInput,
        UpdateAgentConversationTitleInput,
    };
    use ralphx_lib::commands::ExecutionState;
    use ralphx_lib::domain::agents::{
        built_in_agent_models, default_effort_for_provider, default_efforts_for_provider,
        default_model_for_provider, lightweight_model_for_provider, AgentHarnessKind,
        AgentModelDefinition, AgentModelRegistrySnapshot, AgentModelSource, LogicalEffort,
    };
    use ralphx_lib::domain::entities::plan_branch::PrStatus as DbPrStatus;
    use ralphx_lib::domain::entities::{
        AgentConversationWorkspace, AgentConversationWorkspaceBranchMode,
        AgentConversationWorkspaceMode, AgentConversationWorkspaceStatus, AgentRun, Artifact,
        ArtifactId, ArtifactType, ChatContextType, ChatConversation, ChatConversationId,
        ChatMessage, ExecutionPlan, ExecutionPlanStatus, IdeationAnalysisBaseRefKind,
        IdeationSession, IdeationSessionFlow, IdeationSessionId, IdeationSessionStatus,
        MessageRole, PlanBranch, PlanBranchStatus, Priority, Project, ProjectId, ProposalCategory,
        Task, TaskProposal, VerificationStatus,
    };
    use ralphx_lib::domain::repositories::{
        AgentConversationWorkspaceRepository, AgentModelRegistryRepository,
    };
    use ralphx_lib::domain::services::{ComposerArtifactReference, RunningAgentKey};
    use ralphx_lib::infrastructure::memory::MemoryAgentModelRegistryRepository;
    use ralphx_lib::infrastructure::sqlite::sqlite_agent_conversation_workspace_repo::SqliteAgentConversationWorkspaceRepository;
    use ralphx_lib::testing::SqliteTestDb;
    use std::sync::Arc;
    use tauri::test::{mock_builder, mock_context, noop_assets};
    use tauri::Manager;

    fn agent_model_command_app() -> tauri::App<tauri::test::MockRuntime> {
        mock_builder()
            .manage(AppState::new_test())
            .build(mock_context(noop_assets()))
            .expect("mock app should build")
    }

    fn sqlite_workspace(conversation_id: ChatConversationId) -> AgentConversationWorkspace {
        AgentConversationWorkspace::new(
            conversation_id,
            ProjectId::from_string("project-1".to_string()),
            AgentConversationWorkspaceMode::Edit,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("Project default (main)".to_string()),
            Some("base-sha".to_string()),
            "ralphx/project/agent-ipc".to_string(),
            "/tmp/ralphx/agent-ipc".to_string(),
        )
    }

    fn seed_sqlite_workspace_conversation(db: &SqliteTestDb, conversation_id: &ChatConversationId) {
        db.with_connection(|conn| {
            conn.execute(
                "INSERT INTO chat_conversations (
                    id, context_type, context_id, title, message_count, created_at, updated_at
                 ) VALUES (
                    ?1, 'project', 'project-1', 'Workspace chat', 0,
                    '2026-04-26T09:00:00Z', '2026-04-26T09:00:00Z'
                 )",
                rusqlite::params![conversation_id.as_str()],
            )
            .expect("conversation should seed");
        });
    }

    #[tokio::test]
    async fn ipc_contract_workspace_freshness_blocks_stale_base_without_commit() {
        let (_temp, state, conversation_id, _github) = super::setup_ipc_workspace_state(
            "freshness-blocked",
            false,
            None,
            std::sync::Arc::new(crate::common::MockGithubService::new()),
        )
        .await;
        let app = mock_builder()
            .manage(state)
            .build(mock_context(noop_assets()))
            .expect("mock app should build");

        let response = get_agent_conversation_workspace_freshness(
            conversation_id.as_str(),
            Some("full".to_string()),
            app.state(),
        )
        .await
        .expect("freshness should return blocked state");

        assert_eq!(response.base_status, "blocked");
        assert_eq!(response.base_ref, "feature/deleted-base");
        assert_eq!(response.effective_base_ref, None);
        assert_eq!(
            response.base_block_reason.as_deref(),
            Some(
                "Saved base branch is unavailable and the workspace is missing its captured base commit"
            )
        );
        assert_eq!(response.target_ref, "");
    }

    #[tokio::test]
    async fn ipc_contract_workspace_freshness_reports_retargeted_base() {
        let (_temp, state, conversation_id, _github) = super::setup_ipc_workspace_state(
            "freshness-retargeted",
            true,
            None,
            std::sync::Arc::new(crate::common::MockGithubService::new()),
        )
        .await;
        let app = mock_builder()
            .manage(state)
            .build(mock_context(noop_assets()))
            .expect("mock app should build");

        let response = get_agent_conversation_workspace_freshness(
            conversation_id.as_str(),
            Some("full".to_string()),
            app.state(),
        )
        .await
        .expect("freshness should resolve retargeted base");

        assert_eq!(response.base_status, "retargeted");
        assert_eq!(response.base_ref, "feature/deleted-base");
        assert_eq!(response.effective_base_ref.as_deref(), Some("main"));
        assert_eq!(
            response.effective_base_display_name.as_deref(),
            Some("Project default (main)")
        );
        assert_eq!(response.target_ref, "main");
        assert!(!response.is_base_ahead);
    }

    #[tokio::test]
    async fn ipc_contract_workspace_freshness_rejects_plan_mode_without_repair_mutation() {
        let state = AppState::new_test();
        let conversation_id =
            ChatConversationId::from_string("91919191-9191-9191-9191-919191919191");
        let mut workspace = sqlite_workspace(conversation_id);
        workspace.mode = AgentConversationWorkspaceMode::Plan;
        workspace.linked_ideation_session_id = Some(IdeationSessionId::from_string(
            "planning-session-1".to_string(),
        ));
        workspace.publication_pr_number = Some(42);
        workspace.publication_pr_status = Some("failed".to_string());
        workspace.publication_push_status = Some("needs_agent".to_string());
        workspace.worktree_path = "/missing/plan-mode-workspace".to_string();
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .expect("workspace should seed");
        let app = mock_builder()
            .manage(state)
            .build(mock_context(noop_assets()))
            .expect("mock app should build");

        let error = get_agent_conversation_workspace_freshness(
            conversation_id.as_str(),
            Some("full".to_string()),
            app.state(),
        )
        .await
        .expect_err("Plan-mode freshness should be rejected");

        assert!(error.contains("Only edit workspaces and ideation workspaces"));
        let persisted = app
            .state::<AppState>()
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should remain persisted");
        assert_eq!(
            persisted.publication_push_status.as_deref(),
            Some("needs_agent")
        );
        assert_eq!(persisted.publication_pr_status.as_deref(), Some("failed"));
    }

    #[tokio::test]
    async fn ipc_contract_workspace_freshness_rejects_plan_mode_with_stale_edit_cache() {
        let (_temp, state, conversation_id, _github) = super::setup_ipc_workspace_state(
            "freshness-plan-mode-stale-cache",
            true,
            Some(43),
            std::sync::Arc::new(crate::common::MockGithubService::new()),
        )
        .await;
        let app = mock_builder()
            .manage(state)
            .build(mock_context(noop_assets()))
            .expect("mock app should build");
        get_agent_conversation_workspace_freshness(
            conversation_id.as_str(),
            Some("local".to_string()),
            app.state(),
        )
        .await
        .expect("edit freshness should seed the cache");

        let mut workspace = app
            .state::<AppState>()
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        workspace.mode = AgentConversationWorkspaceMode::Plan;
        workspace.publication_pr_status = Some("failed".to_string());
        workspace.publication_push_status = Some("needs_agent".to_string());
        app.state::<AppState>()
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .expect("Plan workspace should persist without cache invalidation");

        let error = get_agent_conversation_workspace_freshness(
            conversation_id.as_str(),
            Some("local".to_string()),
            app.state(),
        )
        .await
        .expect_err("cached edit freshness must not authorize Plan mode");

        assert!(error.contains("Only edit workspaces and ideation workspaces"));
        let persisted = app
            .state::<AppState>()
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should remain persisted");
        assert_eq!(
            persisted.publication_push_status.as_deref(),
            Some("needs_agent")
        );
    }

    #[tokio::test]
    async fn ipc_contract_workspace_response_recovers_stale_needs_agent_publish_lock() {
        let (_temp, state, conversation_id, _github) = super::setup_ipc_workspace_state(
            "workspace-stale-repair-response",
            true,
            Some(765),
            std::sync::Arc::new(crate::common::MockGithubService::new()),
        )
        .await;
        let mut workspace = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        workspace.publication_pr_status = Some("failed".to_string());
        workspace.publication_push_status = Some("needs_agent".to_string());
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .expect("workspace update should persist");
        let run = state
            .agent_run_repo
            .create(AgentRun::new(conversation_id))
            .await
            .expect("agent run should seed");
        state
            .agent_run_repo
            .fail(&run.id, "repair agent exited")
            .await
            .expect("agent run should fail");
        let app = mock_builder()
            .manage(state)
            .build(mock_context(noop_assets()))
            .expect("mock app should build");

        let response = get_agent_conversation_workspace(conversation_id.as_str(), app.state())
            .await
            .expect("workspace response should load")
            .expect("workspace response should exist");

        assert_eq!(
            response.publication_push_status.as_deref(),
            Some("needs_agent")
        );
    }

    #[tokio::test]
    async fn ipc_contract_workspace_freshness_recovers_stale_needs_agent_publish_lock() {
        let (_temp, state, conversation_id, _github) = super::setup_ipc_workspace_state(
            "workspace-stale-repair-freshness",
            true,
            Some(766),
            std::sync::Arc::new(crate::common::MockGithubService::new()),
        )
        .await;
        let mut workspace = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        workspace.publication_pr_status = Some("failed".to_string());
        workspace.publication_push_status = Some("needs_agent".to_string());
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .expect("workspace update should persist");
        let run = state
            .agent_run_repo
            .create(AgentRun::new(conversation_id))
            .await
            .expect("agent run should seed");
        state
            .agent_run_repo
            .fail(&run.id, "repair agent exited")
            .await
            .expect("agent run should fail");
        let app = mock_builder()
            .manage(state)
            .build(mock_context(noop_assets()))
            .expect("mock app should build");

        get_agent_conversation_workspace_freshness(
            conversation_id.as_str(),
            Some("full".to_string()),
            app.state(),
        )
        .await
        .expect("freshness should load");
        let refreshed = app
            .state::<AppState>()
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");

        assert_eq!(
            refreshed.publication_push_status.as_deref(),
            Some("needs_agent")
        );
    }

    #[tokio::test]
    async fn ipc_contract_startup_publish_recovery_clears_stale_lock() {
        let (_temp, state, conversation_id, _github) = super::setup_ipc_workspace_state(
            "workspace-stale-repair-startup",
            true,
            Some(767),
            std::sync::Arc::new(crate::common::MockGithubService::new()),
        )
        .await;

        recover_stale_agent_workspace_publish_repairs_on_startup(
            std::sync::Arc::clone(&state.agent_conversation_workspace_repo),
            std::sync::Arc::clone(&state.agent_run_repo),
        )
        .await;

        let mut workspace = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        workspace.publication_pr_status = Some("failed".to_string());
        workspace.publication_push_status = Some("needs_agent".to_string());
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .expect("workspace update should persist");
        let run = state
            .agent_run_repo
            .create(AgentRun::new(conversation_id))
            .await
            .expect("agent run should seed");
        state
            .agent_run_repo
            .fail(&run.id, "repair agent exited")
            .await
            .expect("agent run should fail");

        recover_stale_agent_workspace_publish_repairs_on_startup(
            std::sync::Arc::clone(&state.agent_conversation_workspace_repo),
            std::sync::Arc::clone(&state.agent_run_repo),
        )
        .await;
        let refreshed = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");

        assert_eq!(refreshed.publication_push_status.as_deref(), Some("failed"));
    }

    #[tokio::test]
    async fn ipc_contract_sqlite_needs_agent_workspace_filter_round_trips() {
        let db = SqliteTestDb::new("ipc-contract-needs-agent-workspace-filter");
        let repo = SqliteAgentConversationWorkspaceRepository::from_shared(db.shared_conn());

        let needs_agent_id =
            ChatConversationId::from_string("10101010-1010-1010-1010-101010101010");
        seed_sqlite_workspace_conversation(&db, &needs_agent_id);
        let mut needs_agent = sqlite_workspace(needs_agent_id);
        needs_agent.publication_pr_number = Some(91);
        needs_agent.publication_pr_status = Some("failed".to_string());
        needs_agent.publication_push_status = Some("needs_agent".to_string());
        repo.create_or_update(needs_agent.clone())
            .await
            .expect("needs-agent workspace should persist");

        let merged_id = ChatConversationId::from_string("20202020-2020-2020-2020-202020202020");
        seed_sqlite_workspace_conversation(&db, &merged_id);
        let mut merged = sqlite_workspace(merged_id);
        merged.publication_pr_number = Some(92);
        merged.publication_pr_status = Some("merged".to_string());
        merged.publication_push_status = Some("needs_agent".to_string());
        repo.create_or_update(merged)
            .await
            .expect("merged workspace should persist");

        let archived_id = ChatConversationId::from_string("30303030-3030-3030-3030-303030303030");
        seed_sqlite_workspace_conversation(&db, &archived_id);
        let mut archived = sqlite_workspace(archived_id);
        archived.status = AgentConversationWorkspaceStatus::Archived;
        archived.publication_pr_number = Some(93);
        archived.publication_pr_status = Some("failed".to_string());
        archived.publication_push_status = Some("needs_agent".to_string());
        repo.create_or_update(archived)
            .await
            .expect("archived workspace should persist");

        let workspaces = repo
            .list_active_needs_agent_workspaces()
            .await
            .expect("needs-agent workspaces should list");

        assert_eq!(workspaces.len(), 1);
        assert_eq!(workspaces[0].conversation_id, needs_agent.conversation_id);
    }

    #[tokio::test]
    async fn ipc_contract_publish_blocks_when_existing_pr_base_retarget_fails() {
        let github = std::sync::Arc::new(crate::common::MockGithubService::new());
        github.will_fail_update_pr_base("denied");
        let (_temp, state, conversation_id, github) =
            super::setup_ipc_workspace_state("publish-retarget-fails", true, Some(654), github)
                .await;
        let execution_state = std::sync::Arc::new(super::ExecutionState::new());

        let error = publish_agent_conversation_workspace_for_app_state(
            &state,
            &execution_state,
            conversation_id,
            false,
        )
        .await
        .expect_err("failed PR base retarget should block publish");

        assert!(error.contains("Existing PR #654 targets the deleted branch"));
        assert_eq!(github.update_pr_base_calls(), 1);
        let stored = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        assert_eq!(stored.base_ref, "feature/deleted-base");
    }

    #[tokio::test]
    async fn ipc_contract_agent_message_tool_preview_round_trips_full_detail() {
        let state = AppState::new_test();
        let project_id = ProjectId::from_string("project-preview-ipc".to_string());
        let conversation = ChatConversation::new_project(project_id.clone());
        let conversation_id = conversation.id;
        state
            .chat_conversation_repo
            .create(conversation)
            .await
            .expect("conversation should persist");

        let long_result = (1..=14)
            .map(|index| format!("line {index}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut message = ChatMessage::user_in_project(project_id, "assistant preview");
        message.role = MessageRole::Orchestrator;
        message.conversation_id = Some(conversation_id);
        message.tool_calls = Some(
            serde_json::json!([
                {
                    "id": "tool-ipc-1",
                    "name": "bash",
                    "arguments": { "command": "printf" },
                    "result": long_result,
                },
                {
                    "id": "task-ipc-1",
                    "name": "Task",
                    "arguments": { "description": "inspect" },
                    "result": {
                        "subagent_type": "Explore",
                        "content": (1..=14).map(|index| format!("task line {index}")).collect::<Vec<_>>().join("\n")
                    }
                }
            ])
            .to_string(),
        );
        message.content_blocks = Some(
            serde_json::json!([
                { "type": "text", "text": "before" },
                {
                    "type": "tool_use",
                    "id": "tool-block-ipc-1",
                    "name": "read",
                    "arguments": { "file_path": "big.txt" },
                    "result": (1..=12).map(|index| format!("block line {index}")).collect::<Vec<_>>().join("\n")
                }
            ])
            .to_string(),
        );
        let message_id = message.id.as_str().to_string();
        state
            .chat_message_repo
            .create(message)
            .await
            .expect("message should persist");

        let app = mock_builder()
            .manage(state)
            .build(mock_context(noop_assets()))
            .expect("mock app should build");

        let page = get_agent_conversation_messages_page_for_app_state(
            app.state::<AppState>().inner(),
            conversation_id,
            10,
            0,
        )
        .await
        .expect("page helper should succeed")
        .expect("conversation should exist");

        let message = page.messages.first().expect("message should be returned");
        let tool_calls = message.tool_calls.as_ref().expect("tool calls");
        let previewed_tool = &tool_calls[0];
        assert_eq!(previewed_tool["result_preview_truncated"], true);
        assert_eq!(
            previewed_tool["result"].as_str().unwrap().lines().count(),
            10
        );
        assert_eq!(previewed_tool["detail_ref"]["tool_call_id"], "tool-ipc-1");
        assert!(tool_calls[1]["result"].is_object(), "Task stays structured");

        let content_blocks = message.content_blocks.as_ref().expect("content blocks");
        assert_eq!(content_blocks[1]["result_preview_truncated"], true);
        assert_eq!(content_blocks[1]["detail_ref"]["content_block_index"], 1);

        let detail = get_agent_message_tool_call_detail(
            conversation_id.as_str().to_string(),
            message_id.clone(),
            Some("tool-ipc-1".to_string()),
            None,
            app.state::<AppState>(),
        )
        .await
        .expect("detail command should succeed")
        .expect("tool detail should exist");
        assert!(detail.tool_call["result"]
            .as_str()
            .unwrap()
            .contains("line 14"));

        let block_detail = get_agent_message_tool_call_detail(
            conversation_id.as_str().to_string(),
            message_id,
            None,
            Some(1),
            app.state::<AppState>(),
        )
        .await
        .expect("block detail command should succeed")
        .expect("content block detail should exist");
        assert!(block_detail.tool_call["result"]
            .as_str()
            .unwrap()
            .contains("block line 12"));
    }

    #[tokio::test]
    async fn agent_conversation_summary_returns_metadata_without_message_window() {
        let state = AppState::new_test();
        let project_id = ProjectId::from_string("project-summary-ipc".to_string());
        let mut conversation = ChatConversation::new_project(project_id);
        conversation.set_title("Cheap breadcrumb title");
        let conversation_id = conversation.id;
        state
            .chat_conversation_repo
            .create(conversation)
            .await
            .expect("conversation should persist");
        let app = mock_builder()
            .manage(state)
            .build(mock_context(noop_assets()))
            .expect("mock app should build");

        let summary = get_agent_conversation_summary(
            conversation_id.as_str().to_string(),
            app.state::<AppState>(),
        )
        .await
        .expect("summary command should succeed")
        .expect("conversation should exist");

        assert_eq!(summary.id, conversation_id.as_str());
        assert_eq!(summary.context_type, "project");
        assert_eq!(summary.context_id, "project-summary-ipc");
        assert_eq!(summary.title.as_deref(), Some("Cheap breadcrumb title"));
        assert_eq!(summary.message_count, 0);

        let missing = get_agent_conversation_summary_for_app_state(
            app.state::<AppState>().inner(),
            "missing-summary-conversation".to_string(),
        )
        .await
        .expect("summary helper should succeed for a missing conversation");
        assert!(missing.is_none());
    }

    #[tokio::test]
    async fn ipc_contract_messages_page_read_short_circuits_bridge_wakeup_when_unlinked() {
        let state = AppState::new_test();
        let project_id = ProjectId::from_string("project-read-no-bridge-ipc".to_string());
        let mut project = Project::new(
            "Read no bridge".to_string(),
            "/tmp/project-read-no-bridge-ipc".to_string(),
        );
        project.id = project_id.clone();
        state
            .project_repo
            .create(project)
            .await
            .expect("project should persist");

        let mut conversation = ChatConversation::new_project(project_id.clone());
        conversation.title = Some("Read no bridge".to_string());
        let conversation_id = conversation.id;
        state
            .chat_conversation_repo
            .create(conversation)
            .await
            .expect("conversation should persist");
        let workspace = AgentConversationWorkspace::new(
            conversation_id,
            project_id,
            AgentConversationWorkspaceMode::Edit,
            IdeationAnalysisBaseRefKind::CurrentBranch,
            "main".to_string(),
            Some("Current branch".to_string()),
            None,
            format!("agent-{conversation_id}"),
            "/tmp/project-read-no-bridge-ipc-workspace".to_string(),
        );
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .expect("workspace should persist");

        let wakeup = wake_agent_workspace_for_bridge_events_with_service_factory(
            &state,
            &conversation_id,
            MockChatService::new,
        )
        .await
        .expect("read bridge wake-up should prepare");
        assert!(wakeup.is_none());

        let page =
            get_agent_conversation_messages_page_for_app_state(&state, conversation_id, 1, 0)
                .await
                .expect("message page command should succeed")
                .expect("conversation should exist");

        assert_eq!(page.conversation.id, conversation_id.as_str());
        assert!(page.messages.is_empty());
    }

    #[tokio::test]
    async fn ipc_contract_read_bridge_wakeup_dispatches_linked_ideation_events() {
        let state = AppState::new_test();
        let project_id = ProjectId::from_string("project-read-bridge-ipc".to_string());
        let mut project = Project::new(
            "Read bridge".to_string(),
            "/tmp/project-read-bridge-ipc".to_string(),
        );
        project.id = project_id.clone();
        state
            .project_repo
            .create(project)
            .await
            .expect("project should persist");

        let conversation = ChatConversation::new_project(project_id.clone());
        let conversation_id = conversation.id;
        state
            .chat_conversation_repo
            .create(conversation)
            .await
            .expect("conversation should persist");
        let mut workspace = AgentConversationWorkspace::new(
            conversation_id,
            project_id.clone(),
            AgentConversationWorkspaceMode::Ideation,
            IdeationAnalysisBaseRefKind::CurrentBranch,
            "main".to_string(),
            Some("Current branch".to_string()),
            None,
            format!("agent-{conversation_id}"),
            "/tmp/project-read-bridge-ipc-workspace".to_string(),
        );
        workspace.linked_ideation_session_id =
            Some(IdeationSessionId::from_string("session-ipc".to_string()));
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .expect("workspace should persist");
        state
            .external_events_repo
            .insert_event(
                "ideation:verified",
                project_id.as_str(),
                &serde_json::json!({ "session_id": "session-ipc", "gap_score": 1 }).to_string(),
            )
            .await
            .expect("event should persist");

        let result = wake_agent_workspace_for_bridge_events_with_service_factory(
            &state,
            &conversation_id,
            MockChatService::new,
        )
        .await
        .expect("read bridge wake-up should prepare")
        .expect("linked ideation event should dispatch a wake-up");

        assert_eq!(result.event_count, 1);
        assert!(!result.agent_run_id.is_empty());

        state
            .external_events_repo
            .insert_event(
                "ideation:proposals_ready",
                project_id.as_str(),
                &serde_json::json!({ "session_id": "session-ipc", "proposal_count": 2 })
                    .to_string(),
            )
            .await
            .expect("event should persist");
        let prepared = prepare_agent_workspace_bridge_wakeup(&state, &conversation_id)
            .await
            .expect("read bridge wake-up should prepare")
            .expect("new linked ideation event should prepare a wake-up");
        let dispatch_service = MockChatService::new();
        let dispatched =
            dispatch_prepared_agent_workspace_bridge_wakeup(&state, &dispatch_service, prepared)
                .await
                .expect("prepared wake-up should dispatch");
        assert_eq!(dispatched.event_count, 1);
        assert_eq!(dispatch_service.call_count(), 1);

        state
            .external_events_repo
            .insert_event(
                "ideation:session_accepted",
                project_id.as_str(),
                &serde_json::json!({ "session_id": "session-ipc" }).to_string(),
            )
            .await
            .expect("event should persist");
        let eager_service = MockChatService::new();
        let eager =
            wake_agent_workspace_for_bridge_events(&state, &eager_service, &conversation_id)
                .await
                .expect("read bridge wake-up should dispatch")
                .expect("new linked ideation event should dispatch a wake-up");
        assert_eq!(eager.event_count, 1);
        assert_eq!(eager_service.call_count(), 1);
    }

    // ── SendAgentMessageInput ───────────────────────────────────────────────

    #[test]
    fn send_agent_message_input_deserializes_camel_case() {
        let json = r#"{"contextType":"task_execution","contextId":"task-123","content":"Hello agent","modelOverride":"gpt-5.5","logicalEffort":"xhigh"}"#;
        let input: SendAgentMessageInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.context_type, "task_execution");
        assert_eq!(input.context_id, "task-123");
        assert_eq!(input.content, "Hello agent");
        assert_eq!(input.model_override.as_deref(), Some("gpt-5.5"));
        assert_eq!(input.logical_effort, Some(LogicalEffort::XHigh));
    }

    #[test]
    fn send_agent_message_input_snake_case_not_accepted() {
        // context_type in snake_case must not map to context_type field
        let json = r#"{"context_type":"task","context_id":"id-1","content":"msg"}"#;
        let result: Result<SendAgentMessageInput, _> = serde_json::from_str(json);
        assert!(
            result.is_err(),
            "snake_case context_type must not deserialize (missing required camelCase fields)"
        );
    }

    // ── QueueAgentMessageInput ──────────────────────────────────────────────

    #[test]
    fn queue_agent_message_input_deserializes_camel_case() {
        let json = r#"{"contextType":"task","contextId":"task-789","content":"Queued msg","clientId":"client-abc"}"#;
        let input: QueueAgentMessageInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.context_type, "task");
        assert_eq!(input.context_id, "task-789");
        assert_eq!(input.content, "Queued msg");
        assert_eq!(input.client_id, Some("client-abc".to_string()));
    }

    #[test]
    fn queue_agent_message_input_optional_fields_absent() {
        let json = r#"{"contextType":"project","contextId":"proj-1","content":"Hello"}"#;
        let input: QueueAgentMessageInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.context_type, "project");
        assert!(input.client_id.is_none());
    }

    // ── CreateAgentConversationInput ────────────────────────────────────────

    #[test]
    fn create_agent_conversation_input_deserializes_camel_case() {
        let json =
            r#"{"contextType":"review","contextId":"task-review-123","mode":"persona_builder"}"#;
        let input: CreateAgentConversationInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.context_type, "review");
        assert_eq!(input.context_id.as_deref(), Some("task-review-123"));
        assert_eq!(input.mode.as_deref(), Some("persona_builder"));
    }

    #[test]
    fn create_agent_conversation_input_missing_context_id_deserializes_as_none() {
        // context_id is optional at the wire level as of Phase 4a.3 (standalone
        // creation self-keys and never supplies one); command-body validation,
        // not deserialization, now rejects a missing context_id for non-standalone
        // context types.
        let json = r#"{"contextType":"ideation"}"#;
        let input: CreateAgentConversationInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.context_type, "ideation");
        assert!(input.context_id.is_none());
    }

    #[test]
    fn update_agent_conversation_title_input_deserializes_camel_case() {
        let json = r#"{"conversationId":"conv-123","title":"Fix title editing"}"#;
        let input: UpdateAgentConversationTitleInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.conversation_id, "conv-123");
        assert_eq!(input.title, "Fix title editing");
    }

    #[test]
    fn start_agent_conversation_input_accepts_chat_mode_without_base() {
        let json = r#"{"projectId":"project-1","content":"What changed?","mode":"chat","providerHarness":"codex","modelOverride":"gpt-5.5","logicalEffort":"xhigh","sourcePersonaId":"persona-source"}"#;
        let input: StartAgentConversationInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.project_id.as_deref(), Some("project-1"));
        assert_eq!(input.mode.as_deref(), Some("chat"));
        assert_eq!(input.model_override.as_deref(), Some("gpt-5.5"));
        assert_eq!(input.logical_effort, Some(LogicalEffort::XHigh));
        assert_eq!(input.source_persona_id.as_deref(), Some("persona-source"));
        assert!(input.base_ref_kind.is_none());
        assert!(input.base_ref.is_none());
        assert!(input.composer_project_references.is_empty());
    }

    #[test]
    fn start_agent_conversation_input_omitted_project_id_deserializes_as_none() {
        // A standalone start never sends projectId at all (not even null) — the
        // field must be genuinely optional at the wire level, not just
        // Option-typed, or the frontend's standalone start request would fail
        // deserialization entirely before ever reaching the flag/mode gates.
        let json = r#"{"content":"Quick question","mode":"chat"}"#;
        let input: StartAgentConversationInput = serde_json::from_str(json).unwrap();
        assert!(input.project_id.is_none());
        assert_eq!(input.mode.as_deref(), Some("chat"));
    }

    #[tokio::test]
    async fn ipc_contract_start_agent_conversation_chat_mode_queues_without_workspace() {
        let _fake_claude = FakeCliOnPath::new("claude");
        let state = AppState::new_test();
        let project_id = ProjectId::from_string("project-start-agent-chat-ipc".to_string());
        let mut project = Project::new(
            "Start Agent Chat".to_string(),
            "/tmp/project-start-agent-chat-ipc".to_string(),
        );
        project.id = project_id.clone();
        state
            .project_repo
            .create(project)
            .await
            .expect("project should persist");

        let execution_state = Arc::new(ExecutionState::new());
        execution_state.pause();
        let app = mock_builder()
            .manage(state)
            .manage(Arc::clone(&execution_state))
            .build(mock_context(noop_assets()))
            .expect("mock app should build");

        let response = start_agent_conversation(
            StartAgentConversationInput {
                project_id: Some(project_id.as_str().to_string()),
                content: "Inspect the repo without editing".to_string(),
                persona_id: None,
                source_persona_id: None,
                conversation_id: None,
                parent_conversation_id: None,
                title: None,
                provider_harness: None,
                model_override: None,
                codex_fast_mode: None,
                logical_effort: Some(LogicalEffort::Medium),
                mode: Some("chat".to_string()),
                base_ref_kind: None,
                base_branch_mode: None,
                base_ref: None,
                base_display_name: None,
                base_source_pull_request: None,
                composer_project_references: Vec::new(),
                composer_integration_references: Vec::new(),
                composer_artifact_references: Vec::new(),
                composer_selection_snapshot: None,
                team_intent: None,
            },
            app.state::<AppState>(),
            app.state::<Arc<ExecutionState>>(),
            app.handle().clone(),
        )
        .await
        .expect("chat-mode start should succeed");

        assert!(response.workspace.is_none());
        assert_eq!(response.conversation.context_id, project_id.as_str());
        assert_eq!(response.conversation.agent_mode.as_deref(), Some("chat"));
        assert!(response.send_result.was_queued);
        assert_eq!(
            response.send_result.conversation_id.as_str(),
            response.conversation.id.as_str()
        );

        let workspace = app
            .state::<AppState>()
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&ChatConversationId::from_string(
                response.conversation.id.clone(),
            ))
            .await
            .expect("workspace lookup should succeed");
        assert!(workspace.is_none());
    }

    #[tokio::test]
    async fn ipc_contract_start_agent_conversation_pr_chat_mode_persists_workspace() {
        let _fake_claude = FakeCliOnPath::new("claude");
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let repo_path = temp.path().join("repo");
        let worktree_parent = temp.path().join("worktrees");
        super::setup_publish_repo(&repo_path);
        super::git(&repo_path, &["checkout", "-b", "feature/source-pr-chat"]);
        std::fs::write(repo_path.join("README.md"), "source pr chat\n")
            .expect("fixture update should be written");
        super::git(&repo_path, &["add", "README.md"]);
        super::git(&repo_path, &["commit", "-m", "source pr chat"]);
        let source_sha = super::git(&repo_path, &["rev-parse", "HEAD"]);
        super::git(&repo_path, &["checkout", "main"]);

        let state = AppState::new_test();
        let project_id = ProjectId::from_string("project-start-agent-pr-chat-ipc".to_string());
        let mut project = Project::new(
            "Start Agent PR Chat".to_string(),
            repo_path.to_string_lossy().to_string(),
        );
        project.id = project_id.clone();
        project.base_branch = Some("main".to_string());
        project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
        state
            .project_repo
            .create(project)
            .await
            .expect("project should persist");

        let execution_state = Arc::new(ExecutionState::new());
        execution_state.pause();
        let app = mock_builder()
            .manage(state)
            .manage(Arc::clone(&execution_state))
            .build(mock_context(noop_assets()))
            .expect("mock app should build");

        let response = start_agent_conversation(
            StartAgentConversationInput {
                project_id: Some(project_id.as_str().to_string()),
                content: "Review this PR".to_string(),
                persona_id: None,
                source_persona_id: None,
                conversation_id: None,
                parent_conversation_id: None,
                title: None,
                provider_harness: None,
                model_override: None,
                codex_fast_mode: None,
                logical_effort: Some(LogicalEffort::Medium),
                mode: Some("chat".to_string()),
                base_ref_kind: Some("local_branch".to_string()),
                base_branch_mode: None,
                base_ref: Some("feature/source-pr-chat".to_string()),
                base_display_name: Some("PR #42: Source PR Chat".to_string()),
                base_source_pull_request: Some(AgentWorkspaceSourcePullRequestInput {
                    number: 42,
                    url: Some("https://github.com/owner/repo/pull/42".to_string()),
                    title: Some("Source PR Chat".to_string()),
                    head_ref_name: "feature/source-pr-chat".to_string(),
                    base_ref_name: Some("main".to_string()),
                    head_ref_oid: Some(source_sha.clone()),
                }),
                composer_project_references: Vec::new(),
                composer_integration_references: Vec::new(),
                composer_artifact_references: Vec::new(),
                composer_selection_snapshot: None,
                team_intent: None,
            },
            app.state::<AppState>(),
            app.state::<Arc<ExecutionState>>(),
            app.handle().clone(),
        )
        .await
        .expect("PR-backed chat-mode start should succeed");

        assert_eq!(response.conversation.agent_mode.as_deref(), Some("chat"));
        assert!(response.send_result.was_queued);
        let workspace = response.workspace.expect("workspace should be returned");
        assert_eq!(workspace.mode, "chat");
        assert_eq!(workspace.branch_mode, "isolated");
        assert_eq!(workspace.base_ref_kind, "local_branch");
        assert_eq!(workspace.base_ref, "feature/source-pr-chat");
        assert_ne!(workspace.branch_name, "feature/source-pr-chat");
        assert!(workspace.branch_name.contains("/agent-"));
        assert_eq!(workspace.publication_pr_number, None);
        assert_eq!(workspace.publication_pr_status.as_deref(), None);
        assert_ne!(
            workspace.worktree_path.as_str(),
            repo_path.to_string_lossy().as_ref()
        );
        let source = workspace
            .source_pull_request
            .expect("source PR metadata should be returned");
        assert_eq!(source.number, 42);
        assert_eq!(source.head_ref_name, "feature/source-pr-chat");
        assert_eq!(source.base_ref_name.as_deref(), Some("main"));
        assert_eq!(source.head_ref_oid.as_deref(), Some(source_sha.as_str()));

        let conversation_id = ChatConversationId::from_string(response.conversation.id.clone());
        let persisted_workspace = app
            .state::<AppState>()
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should persist");
        assert_eq!(
            persisted_workspace.mode,
            AgentConversationWorkspaceMode::Chat
        );
        assert_eq!(
            persisted_workspace.branch_mode,
            AgentConversationWorkspaceBranchMode::Isolated
        );
        assert_eq!(
            persisted_workspace.base_ref_kind,
            IdeationAnalysisBaseRefKind::LocalBranch
        );
        assert_eq!(persisted_workspace.base_ref, "feature/source-pr-chat");
        assert_ne!(persisted_workspace.branch_name, "feature/source-pr-chat");
        assert!(persisted_workspace.branch_name.contains("/agent-"));
        assert_eq!(
            persisted_workspace
                .source_pull_request
                .as_ref()
                .map(|source| source.number),
            Some(42)
        );
        assert_eq!(persisted_workspace.publication_pr_number, None);

        let switched = switch_agent_conversation_mode_for_state(
            SwitchAgentConversationModeInput {
                conversation_id: conversation_id.as_str(),
                mode: "edit".to_string(),
                runtime_override: None,
                base_ref_kind: None,
                base_branch_mode: None,
                base_ref: None,
                base_display_name: None,
                base_source_pull_request: None,
            },
            app.state::<AppState>().inner(),
        )
        .await
        .expect("PR-backed chat workspace should switch to edit");
        let switched_workspace = switched
            .workspace
            .expect("workspace should be returned after switch");
        assert_eq!(switched_workspace.mode, "edit");
        assert_eq!(switched_workspace.branch_mode, "isolated");
        assert_eq!(switched_workspace.base_ref_kind, "local_branch");
        assert_eq!(switched_workspace.base_ref, "feature/source-pr-chat");
        assert_ne!(switched_workspace.branch_name, "feature/source-pr-chat");
        assert!(switched_workspace.branch_name.contains("/agent-"));
        assert_eq!(switched_workspace.publication_pr_number, None);
        assert_eq!(
            switched_workspace
                .source_pull_request
                .as_ref()
                .map(|source| source.number),
            Some(42)
        );
    }

    #[tokio::test]
    async fn ipc_contract_start_agent_conversation_edit_mode_persists_workspace() {
        let _fake_claude = FakeCliOnPath::new("claude");
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let repo_path = temp.path().join("repo");
        let worktree_parent = temp.path().join("worktrees");
        super::setup_publish_repo(&repo_path);

        let state = AppState::new_test();
        let project_id = ProjectId::from_string("project-start-agent-edit-ipc".to_string());
        let mut project = Project::new(
            "Start Agent Edit".to_string(),
            repo_path.to_string_lossy().to_string(),
        );
        project.id = project_id.clone();
        project.base_branch = Some("main".to_string());
        project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
        state
            .project_repo
            .create(project)
            .await
            .expect("project should persist");

        let execution_state = Arc::new(ExecutionState::new());
        execution_state.pause();
        let app = mock_builder()
            .manage(state)
            .manage(Arc::clone(&execution_state))
            .build(mock_context(noop_assets()))
            .expect("mock app should build");

        let response = start_agent_conversation(
            StartAgentConversationInput {
                project_id: Some(project_id.as_str().to_string()),
                content: "Prepare an editable workspace".to_string(),
                persona_id: None,
                source_persona_id: None,
                conversation_id: None,
                parent_conversation_id: None,
                title: None,
                provider_harness: None,
                model_override: None,
                codex_fast_mode: None,
                logical_effort: Some(LogicalEffort::Medium),
                mode: Some("edit".to_string()),
                base_ref_kind: Some("project_default".to_string()),
                base_branch_mode: None,
                base_ref: None,
                base_display_name: None,
                base_source_pull_request: None,
                composer_project_references: Vec::new(),
                composer_integration_references: Vec::new(),
                composer_artifact_references: Vec::new(),
                composer_selection_snapshot: None,
                team_intent: None,
            },
            app.state::<AppState>(),
            app.state::<Arc<ExecutionState>>(),
            app.handle().clone(),
        )
        .await
        .expect("edit-mode start should succeed");

        let workspace = response.workspace.expect("workspace should be returned");
        assert_eq!(workspace.mode, "edit");
        assert_eq!(workspace.base_ref_kind, "project_default");
        assert!(response.send_result.was_queued);

        let persisted_workspace = app
            .state::<AppState>()
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&ChatConversationId::from_string(
                response.conversation.id.clone(),
            ))
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should persist");
        assert_eq!(
            persisted_workspace.mode,
            AgentConversationWorkspaceMode::Edit
        );
    }

    struct PlanReferenceStartFixture {
        app: tauri::App<tauri::test::MockRuntime>,
        project_id: ProjectId,
        source_session_id: IdeationSessionId,
        source_artifact_id: ArtifactId,
        source_blueprint_id: ArtifactId,
        _repo_dir: tempfile::TempDir,
        _worktree_parent: tempfile::TempDir,
    }

    async fn setup_plan_reference_start_fixture(label: &str) -> PlanReferenceStartFixture {
        let repo_dir = tempfile::tempdir().expect("tempdir should be created");
        let repo_path = repo_dir.path().join("repo");
        let worktree_parent = tempfile::tempdir().expect("worktree tempdir should be created");
        super::setup_publish_repo(&repo_path);

        let state = AppState::new_test();
        let project_id =
            ProjectId::from_string(format!("project-start-agent-plan-reference-{label}"));
        let mut project = Project::new(
            format!("Start Agent Plan Reference {label}"),
            repo_path.to_string_lossy().to_string(),
        );
        project.id = project_id.clone();
        project.base_branch = Some("main".to_string());
        project.worktree_parent_directory =
            Some(worktree_parent.path().to_string_lossy().to_string());
        state
            .project_repo
            .create(project)
            .await
            .expect("project should persist");

        let source_artifact = state
            .artifact_repo
            .create(Artifact::new_inline(
                "Source Plan",
                ArtifactType::Specification,
                "# Source Plan\n\nImplement the selected plan.",
                "test",
            ))
            .await
            .expect("source artifact should persist");
        let source_blueprint = state
            .artifact_repo
            .create(Artifact::new_inline(
                "Source Blueprint",
                ArtifactType::Specification,
                "# Source Blueprint\n\nImplement the selected plan safely.",
                "test",
            ))
            .await
            .expect("source blueprint should persist");
        let mut source_session = IdeationSession::builder()
            .project_id(project_id.clone())
            .title("Accepted source session")
            .status(IdeationSessionStatus::Accepted)
            .plan_artifact_id(source_artifact.id.clone())
            .plan_contract_version(2)
            .build();
        source_session.plan_blueprint_artifact_id = Some(source_blueprint.id.clone());
        let source_session = state
            .ideation_session_repo
            .create(source_session)
            .await
            .expect("source session should persist");
        let mut source_proposal = TaskProposal::new(
            source_session.id.clone(),
            "Source proposal must not copy",
            ProposalCategory::Feature,
            Priority::High,
        );
        source_proposal.plan_artifact_id = Some(source_artifact.id.clone());
        state
            .task_proposal_repo
            .create(source_proposal)
            .await
            .expect("source proposal should persist");

        let execution_state = Arc::new(ExecutionState::new());
        execution_state.pause();
        let app = mock_builder()
            .manage(state)
            .manage(Arc::clone(&execution_state))
            .build(mock_context(noop_assets()))
            .expect("mock app should build");

        PlanReferenceStartFixture {
            app,
            project_id,
            source_session_id: source_session.id,
            source_artifact_id: source_artifact.id,
            source_blueprint_id: source_blueprint.id,
            _repo_dir: repo_dir,
            _worktree_parent: worktree_parent,
        }
    }

    fn plan_reference(fix: &PlanReferenceStartFixture) -> ComposerArtifactReference {
        ComposerArtifactReference {
            artifact_id: fix.source_artifact_id.as_str().to_string(),
            kind: "plan".to_string(),
            title: Some("Source Plan".to_string()),
            session_id: Some(fix.source_session_id.as_str().to_string()),
            version: Some(1),
            status: Some("accepted".to_string()),
        }
    }

    #[tokio::test]
    async fn ipc_contract_start_agent_conversation_plan_reference_clones_fresh_session_by_mode() {
        let _fake_claude = FakeCliOnPath::new("claude");
        let fix = setup_plan_reference_start_fixture("mode-matrix").await;
        let state = fix.app.state::<AppState>();

        for mode in ["chat", "edit", "plan", "ideation"] {
            let response = start_agent_conversation(
                StartAgentConversationInput {
                    project_id: Some(fix.project_id.as_str().to_string()),
                    content: format!("Use the selected plan in {mode} mode"),
                    persona_id: None,
                    source_persona_id: None,
                    conversation_id: None,
                    parent_conversation_id: None,
                    title: None,
                    provider_harness: None,
                    model_override: None,
                    codex_fast_mode: None,
                    logical_effort: Some(LogicalEffort::Medium),
                    mode: Some(mode.to_string()),
                    base_ref_kind: Some("project_default".to_string()),
                    base_branch_mode: None,
                    base_ref: None,
                    base_display_name: None,
                    base_source_pull_request: None,
                    composer_project_references: Vec::new(),
                    composer_integration_references: Vec::new(),
                    composer_artifact_references: vec![plan_reference(&fix)],
                    composer_selection_snapshot: None,
                    team_intent: None,
                },
                state.clone(),
                fix.app.state::<Arc<ExecutionState>>(),
                fix.app.handle().clone(),
            )
            .await
            .unwrap_or_else(|error| panic!("{mode} plan-reference start should succeed: {error}"));

            assert!(response.send_result.was_queued);
            let persisted_workspace = state
                .agent_conversation_workspace_repo
                .get_by_conversation_id(&ChatConversationId::from_string(
                    response.conversation.id.clone(),
                ))
                .await
                .expect("workspace lookup should succeed")
                .unwrap_or_else(|| panic!("{mode} should persist a workspace"));
            assert_eq!(persisted_workspace.mode.to_string(), mode);

            let linked_session_id = persisted_workspace
                .linked_ideation_session_id
                .as_ref()
                .unwrap_or_else(|| panic!("{mode} should link a fresh ideation session"));
            let new_session = state
                .ideation_session_repo
                .get_by_id(linked_session_id)
                .await
                .expect("linked session lookup should succeed")
                .unwrap_or_else(|| panic!("{mode} linked session should exist"));
            assert_eq!(new_session.session_flow, IdeationSessionFlow::Planning);
            assert_eq!(new_session.project_id, fix.project_id);
            assert_eq!(
                new_session.source_session_id.as_deref(),
                Some(fix.source_session_id.as_str())
            );
            assert!(new_session.parent_session_id.is_none());
            assert!(new_session.inherited_plan_artifact_id.is_none());
            assert_eq!(
                new_session.verification_status,
                VerificationStatus::Unverified
            );
            assert!(!new_session.verification_in_progress);

            let cloned_artifact_id = new_session
                .plan_artifact_id
                .as_ref()
                .unwrap_or_else(|| panic!("{mode} should set plan_artifact_id"));
            assert_ne!(cloned_artifact_id, &fix.source_artifact_id);
            assert_eq!(new_session.plan_contract_version, 2);
            let cloned_blueprint_id = new_session
                .plan_blueprint_artifact_id
                .as_ref()
                .unwrap_or_else(|| panic!("{mode} should clone the plan blueprint"));
            assert_ne!(cloned_blueprint_id, &fix.source_blueprint_id);

            let cloned_artifact = state
                .artifact_repo
                .get_by_id(cloned_artifact_id)
                .await
                .expect("cloned artifact lookup should succeed")
                .unwrap_or_else(|| panic!("{mode} cloned artifact should exist"));
            let source_artifact = state
                .artifact_repo
                .get_by_id(&fix.source_artifact_id)
                .await
                .expect("source artifact lookup should succeed")
                .expect("source artifact should remain");
            assert_eq!(cloned_artifact.content, source_artifact.content);
            assert_eq!(cloned_artifact.metadata.version, 1);
            assert!(
                cloned_artifact
                    .derived_from
                    .contains(&fix.source_artifact_id),
                "{mode} clone should retain source-artifact provenance"
            );
            let cloned_blueprint = state
                .artifact_repo
                .get_by_id(cloned_blueprint_id)
                .await
                .expect("cloned blueprint lookup should succeed")
                .unwrap_or_else(|| panic!("{mode} cloned blueprint should exist"));
            assert!(
                cloned_blueprint
                    .derived_from
                    .contains(&fix.source_blueprint_id),
                "{mode} clone should retain source-blueprint provenance"
            );

            let new_proposals = state
                .task_proposal_repo
                .get_by_session(&new_session.id)
                .await
                .expect("new session proposals should load");
            assert!(
                new_proposals.is_empty(),
                "{mode} should not copy source proposals"
            );
            let source_proposals = state
                .task_proposal_repo
                .get_by_session(&fix.source_session_id)
                .await
                .expect("source session proposals should load");
            assert_eq!(source_proposals.len(), 1);

            let queued = state
                .message_queue
                .get_queued(ChatContextType::Project, response.conversation.id.as_str())
                .into_iter()
                .find(|message| {
                    response.send_result.queued_message_id.as_deref() == Some(message.id.as_str())
                })
                .unwrap_or_else(|| panic!("{mode} queued message should be retained"));
            assert_eq!(queued.composer_artifact_references.len(), 2);
            let queued_reference = &queued.composer_artifact_references[0];
            assert_eq!(queued_reference.kind, "plan");
            assert_eq!(queued_reference.artifact_id, cloned_artifact_id.as_str());
            assert_eq!(
                queued_reference.session_id.as_deref(),
                Some(new_session.id.as_str())
            );
            assert_ne!(
                queued_reference.artifact_id,
                fix.source_artifact_id.as_str()
            );
            let queued_blueprint_reference = &queued.composer_artifact_references[1];
            assert_eq!(queued_blueprint_reference.kind, "plan_blueprint");
            assert_eq!(
                queued_blueprint_reference.artifact_id,
                cloned_blueprint_id.as_str()
            );
            assert_eq!(
                queued_blueprint_reference.session_id.as_deref(),
                Some(new_session.id.as_str())
            );
            assert_ne!(
                queued_blueprint_reference.artifact_id,
                fix.source_blueprint_id.as_str()
            );
        }

        let source_session = state
            .ideation_session_repo
            .get_by_id(&fix.source_session_id)
            .await
            .expect("source session lookup should succeed")
            .expect("source session should remain");
        assert_eq!(
            source_session.plan_artifact_id.as_ref(),
            Some(&fix.source_artifact_id)
        );
        assert_eq!(source_session.status, IdeationSessionStatus::Accepted);
    }

    #[tokio::test]
    async fn ipc_contract_start_agent_conversation_review_pr_requires_source_pull_request() {
        let _fake_claude = FakeCliOnPath::new("claude");
        let fix = setup_plan_reference_start_fixture("review-pr-source-required").await;
        let state = fix.app.state::<AppState>();

        let error = start_agent_conversation(
            StartAgentConversationInput {
                project_id: Some(fix.project_id.as_str().to_string()),
                content: "Review the selected PR".to_string(),
                persona_id: None,
                source_persona_id: None,
                conversation_id: None,
                parent_conversation_id: None,
                title: None,
                provider_harness: None,
                model_override: None,
                codex_fast_mode: None,
                logical_effort: Some(LogicalEffort::Medium),
                mode: Some("review_pr".to_string()),
                base_ref_kind: Some("project_default".to_string()),
                base_branch_mode: None,
                base_ref: None,
                base_display_name: None,
                base_source_pull_request: None,
                composer_project_references: Vec::new(),
                composer_integration_references: Vec::new(),
                composer_artifact_references: vec![plan_reference(&fix)],
                composer_selection_snapshot: None,
                team_intent: None,
            },
            state.clone(),
            fix.app.state::<Arc<ExecutionState>>(),
            fix.app.handle().clone(),
        )
        .await
        .expect_err("Review PR start without PR metadata should fail early");

        assert!(
            error.contains("Review PR mode requires a selected pull request"),
            "unexpected error: {error}"
        );
    }

    #[tokio::test]
    async fn ipc_contract_start_agent_conversation_rejects_multiple_plan_references() {
        let _fake_claude = FakeCliOnPath::new("claude");
        let fix = setup_plan_reference_start_fixture("multiple-plans").await;
        let state = fix.app.state::<AppState>();
        let second_artifact = state
            .artifact_repo
            .create(Artifact::new_inline(
                "Second Plan",
                ArtifactType::Specification,
                "# Second Plan",
                "test",
            ))
            .await
            .expect("second artifact should persist");
        let second_session = state
            .ideation_session_repo
            .create(
                IdeationSession::builder()
                    .project_id(fix.project_id.clone())
                    .title("Second source session")
                    .plan_artifact_id(second_artifact.id.clone())
                    .build(),
            )
            .await
            .expect("second source session should persist");

        let mut references = vec![plan_reference(&fix)];
        references.push(ComposerArtifactReference {
            artifact_id: second_artifact.id.as_str().to_string(),
            kind: "plan".to_string(),
            title: Some("Second Plan".to_string()),
            session_id: Some(second_session.id.as_str().to_string()),
            version: Some(1),
            status: None,
        });

        let error = start_agent_conversation(
            StartAgentConversationInput {
                project_id: Some(fix.project_id.as_str().to_string()),
                content: "Use both selected plans".to_string(),
                persona_id: None,
                source_persona_id: None,
                conversation_id: None,
                parent_conversation_id: None,
                title: None,
                provider_harness: None,
                model_override: None,
                codex_fast_mode: None,
                logical_effort: Some(LogicalEffort::Medium),
                mode: Some("edit".to_string()),
                base_ref_kind: Some("project_default".to_string()),
                base_branch_mode: None,
                base_ref: None,
                base_display_name: None,
                base_source_pull_request: None,
                composer_project_references: Vec::new(),
                composer_integration_references: Vec::new(),
                composer_artifact_references: references,
                composer_selection_snapshot: None,
                team_intent: None,
            },
            state,
            fix.app.state::<Arc<ExecutionState>>(),
            fix.app.handle().clone(),
        )
        .await
        .expect_err("multiple plan references should fail closed");

        assert!(
            error.contains("Multiple plan references"),
            "unexpected error: {error}"
        );
    }

    #[tokio::test]
    async fn ipc_contract_start_agent_conversation_existing_linked_workspace_self_exempts() {
        let _fake_claude = FakeCliOnPath::new("claude");
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let repo_path = temp.path().join("repo");
        let worktree_parent = temp.path().join("worktrees");
        super::setup_publish_repo(&repo_path);
        let linked_branch = "feature/existing-linked-start";
        super::git(&repo_path, &["checkout", "-b", linked_branch]);
        super::git(&repo_path, &["checkout", "main"]);

        let state = AppState::new_test();
        let project_id =
            ProjectId::from_string("project-start-agent-existing-linked-ipc".to_string());
        let mut project = Project::new(
            "Start Agent Existing Linked".to_string(),
            repo_path.to_string_lossy().to_string(),
        );
        project.id = project_id.clone();
        project.base_branch = Some("main".to_string());
        project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
        let project = state
            .project_repo
            .create(project)
            .await
            .expect("project should persist");
        let mut conversation = ChatConversation::new_project(project_id.clone());
        conversation.set_agent_mode(Some(AgentConversationWorkspaceMode::Edit));
        let conversation = state
            .chat_conversation_repo
            .create(conversation)
            .await
            .expect("draft conversation should persist");
        let workspace = prepare_agent_conversation_workspace(
            &project,
            &conversation.id,
            AgentConversationWorkspaceMode::Edit,
            AgentConversationWorkspaceBaseSelection {
                kind: Some(IdeationAnalysisBaseRefKind::LocalBranch),
                branch_mode: Some(AgentConversationWorkspaceBranchMode::Linked),
                base_ref: Some(linked_branch.to_string()),
                display_name: Some(linked_branch.to_string()),
                source_pull_request: None,
            },
        )
        .await
        .expect("linked workspace should prepare");
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .expect("linked workspace should persist");

        let execution_state = Arc::new(ExecutionState::new());
        execution_state.pause();
        let app = mock_builder()
            .manage(state)
            .manage(Arc::clone(&execution_state))
            .build(mock_context(noop_assets()))
            .expect("mock app should build");

        let response = start_agent_conversation(
            StartAgentConversationInput {
                project_id: Some(project_id.as_str().to_string()),
                content: "Continue on the linked branch".to_string(),
                persona_id: None,
                source_persona_id: None,
                conversation_id: Some(conversation.id.as_str()),
                parent_conversation_id: None,
                title: None,
                provider_harness: None,
                model_override: None,
                codex_fast_mode: None,
                logical_effort: Some(LogicalEffort::Medium),
                mode: Some("edit".to_string()),
                base_ref_kind: Some("local_branch".to_string()),
                base_branch_mode: Some("linked".to_string()),
                base_ref: Some(linked_branch.to_string()),
                base_display_name: Some(linked_branch.to_string()),
                base_source_pull_request: None,
                composer_project_references: Vec::new(),
                composer_integration_references: Vec::new(),
                composer_artifact_references: Vec::new(),
                composer_selection_snapshot: None,
                team_intent: None,
            },
            app.state::<AppState>(),
            app.state::<Arc<ExecutionState>>(),
            app.handle().clone(),
        )
        .await
        .expect("existing linked workspace should not self-conflict");

        assert_eq!(response.conversation.id, conversation.id.as_str());
        assert!(response.send_result.was_queued);
        let workspace = response.workspace.expect("workspace should be returned");
        assert_eq!(workspace.branch_name, linked_branch);
        assert_eq!(workspace.branch_mode, "linked");
    }

    #[tokio::test]
    async fn ipc_contract_start_agent_conversation_linked_conflict_returns_retryable_error() {
        let _fake_claude = FakeCliOnPath::new("claude");
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let repo_path = temp.path().join("repo");
        let worktree_parent = temp.path().join("worktrees");
        super::setup_publish_repo(&repo_path);
        let linked_branch = "feature/already-linked-start";
        super::git(&repo_path, &["checkout", "-b", linked_branch]);
        super::git(&repo_path, &["checkout", "main"]);

        let state = AppState::new_test();
        let project_id =
            ProjectId::from_string("project-start-agent-linked-conflict-ipc".to_string());
        let mut project = Project::new(
            "Start Agent Linked Conflict".to_string(),
            repo_path.to_string_lossy().to_string(),
        );
        project.id = project_id.clone();
        project.base_branch = Some("main".to_string());
        project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
        let project = state
            .project_repo
            .create(project)
            .await
            .expect("project should persist");
        let existing = state
            .chat_conversation_repo
            .create(ChatConversation::new_project(project_id.clone()))
            .await
            .expect("existing conversation should persist");
        let workspace = prepare_agent_conversation_workspace(
            &project,
            &existing.id,
            AgentConversationWorkspaceMode::Edit,
            AgentConversationWorkspaceBaseSelection {
                kind: Some(IdeationAnalysisBaseRefKind::LocalBranch),
                branch_mode: Some(AgentConversationWorkspaceBranchMode::Linked),
                base_ref: Some(linked_branch.to_string()),
                display_name: Some(linked_branch.to_string()),
                source_pull_request: None,
            },
        )
        .await
        .expect("linked workspace should prepare");
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .expect("linked workspace should persist");

        let execution_state = Arc::new(ExecutionState::new());
        execution_state.pause();
        let app = mock_builder()
            .manage(state)
            .manage(Arc::clone(&execution_state))
            .build(mock_context(noop_assets()))
            .expect("mock app should build");

        let error = start_agent_conversation(
            StartAgentConversationInput {
                project_id: Some(project_id.as_str().to_string()),
                content: "Start on linked branch".to_string(),
                persona_id: None,
                source_persona_id: None,
                conversation_id: None,
                parent_conversation_id: None,
                title: None,
                provider_harness: None,
                model_override: None,
                codex_fast_mode: None,
                logical_effort: Some(LogicalEffort::Medium),
                mode: Some("edit".to_string()),
                base_ref_kind: Some("local_branch".to_string()),
                base_branch_mode: Some("linked".to_string()),
                base_ref: Some(linked_branch.to_string()),
                base_display_name: Some(linked_branch.to_string()),
                base_source_pull_request: None,
                composer_project_references: Vec::new(),
                composer_integration_references: Vec::new(),
                composer_artifact_references: Vec::new(),
                composer_selection_snapshot: None,
                team_intent: None,
            },
            app.state::<AppState>(),
            app.state::<Arc<ExecutionState>>(),
            app.handle().clone(),
        )
        .await
        .expect_err("linked branch conflict should fail before creating a new chat");

        assert!(
            error.contains("[ralphx:linked_setup_failure]"),
            "unexpected error: {error}"
        );
        let existing_id = existing.id.as_str();
        assert!(
            error.contains(linked_branch) && error.contains(&existing_id),
            "error should explain the conflicting branch and conversation: {error}"
        );
        let conversations = app
            .state::<AppState>()
            .chat_conversation_repo
            .get_by_context(ChatContextType::Project, project_id.as_str())
            .await
            .expect("project conversations should load");
        assert_eq!(conversations.len(), 1);
        assert_eq!(conversations[0].id, existing.id);
    }

    #[tokio::test]
    async fn ipc_contract_start_agent_conversation_archives_seeded_draft_on_linked_setup_failure() {
        let _fake_claude = FakeCliOnPath::new("claude");
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let repo_path = temp.path().join("repo");
        let worktree_parent = temp.path().join("worktrees");
        super::setup_publish_repo(&repo_path);
        let linked_branch = "feature/primary-linked-start";
        super::git(&repo_path, &["checkout", "-b", linked_branch]);

        let state = AppState::new_test();
        let project_id =
            ProjectId::from_string("project-start-agent-linked-archive-ipc".to_string());
        let mut project = Project::new(
            "Start Agent Linked Archive".to_string(),
            repo_path.to_string_lossy().to_string(),
        );
        project.id = project_id.clone();
        project.base_branch = Some("main".to_string());
        project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
        state
            .project_repo
            .create(project)
            .await
            .expect("project should persist");
        let mut draft = ChatConversation::new_project(project_id.clone());
        draft.set_agent_mode(Some(AgentConversationWorkspaceMode::Edit));
        let draft = state
            .chat_conversation_repo
            .create(draft)
            .await
            .expect("draft conversation should persist");

        let execution_state = Arc::new(ExecutionState::new());
        execution_state.pause();
        let app = mock_builder()
            .manage(state)
            .manage(Arc::clone(&execution_state))
            .build(mock_context(noop_assets()))
            .expect("mock app should build");

        let error = start_agent_conversation(
            StartAgentConversationInput {
                project_id: Some(project_id.as_str().to_string()),
                content: "Start on checked-out linked branch".to_string(),
                persona_id: None,
                source_persona_id: None,
                conversation_id: Some(draft.id.as_str()),
                parent_conversation_id: None,
                title: None,
                provider_harness: None,
                model_override: None,
                codex_fast_mode: None,
                logical_effort: Some(LogicalEffort::Medium),
                mode: Some("edit".to_string()),
                base_ref_kind: Some("local_branch".to_string()),
                base_branch_mode: Some("linked".to_string()),
                base_ref: Some(linked_branch.to_string()),
                base_display_name: Some(linked_branch.to_string()),
                base_source_pull_request: None,
                composer_project_references: Vec::new(),
                composer_integration_references: Vec::new(),
                composer_artifact_references: Vec::new(),
                composer_selection_snapshot: None,
                team_intent: None,
            },
            app.state::<AppState>(),
            app.state::<Arc<ExecutionState>>(),
            app.handle().clone(),
        )
        .await
        .expect_err("linked primary checkout should fail setup");

        assert!(
            error.contains("[ralphx:linked_setup_failure]"),
            "unexpected error: {error}"
        );
        assert!(
            error.contains("checked out in the project root"),
            "error should explain the checkout conflict: {error}"
        );
        let stored_draft = app
            .state::<AppState>()
            .chat_conversation_repo
            .get_by_id(&draft.id)
            .await
            .expect("draft should load")
            .expect("draft should still exist");
        assert!(
            stored_draft.archived_at.is_some(),
            "failed seeded draft should be hidden from active conversations"
        );
        let workspace = app
            .state::<AppState>()
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&draft.id)
            .await
            .expect("workspace lookup should succeed");
        assert!(workspace.is_none());
    }

    #[tokio::test]
    async fn ipc_contract_start_agent_conversation_plan_mode_links_planning_session() {
        let _fake_claude = FakeCliOnPath::new("claude");
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let repo_path = temp.path().join("repo");
        let worktree_parent = temp.path().join("worktrees");
        super::setup_publish_repo(&repo_path);

        let state = AppState::new_test();
        let project_id = ProjectId::from_string("project-start-agent-plan-ipc".to_string());
        let mut project = Project::new(
            "Start Agent Plan".to_string(),
            repo_path.to_string_lossy().to_string(),
        );
        project.id = project_id.clone();
        project.base_branch = Some("main".to_string());
        project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
        state
            .project_repo
            .create(project)
            .await
            .expect("project should persist");

        let execution_state = Arc::new(ExecutionState::new());
        execution_state.pause();
        let app = mock_builder()
            .manage(state)
            .manage(Arc::clone(&execution_state))
            .build(mock_context(noop_assets()))
            .expect("mock app should build");

        let response = start_agent_conversation(
            StartAgentConversationInput {
                project_id: Some(project_id.as_str().to_string()),
                content: "Plan a small refactor".to_string(),
                persona_id: None,
                source_persona_id: None,
                conversation_id: None,
                parent_conversation_id: None,
                title: None,
                provider_harness: None,
                model_override: None,
                codex_fast_mode: None,
                logical_effort: Some(LogicalEffort::Medium),
                mode: Some("plan".to_string()),
                base_ref_kind: Some("project_default".to_string()),
                base_branch_mode: None,
                base_ref: None,
                base_display_name: None,
                base_source_pull_request: None,
                composer_project_references: Vec::new(),
                composer_integration_references: Vec::new(),
                composer_artifact_references: Vec::new(),
                composer_selection_snapshot: None,
                team_intent: None,
            },
            app.state::<AppState>(),
            app.state::<Arc<ExecutionState>>(),
            app.handle().clone(),
        )
        .await
        .expect("plan-mode start should succeed");

        let workspace = response.workspace.expect("workspace should be returned");
        assert_eq!(workspace.mode, "plan");
        assert!(response.send_result.was_queued);
        let session_id = IdeationSessionId::from_string(
            workspace
                .linked_ideation_session_id
                .as_ref()
                .expect("plan workspace should link a planning session")
                .clone(),
        );

        let persisted_workspace = app
            .state::<AppState>()
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&ChatConversationId::from_string(
                response.conversation.id.clone(),
            ))
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should persist");
        assert_eq!(
            persisted_workspace.mode,
            AgentConversationWorkspaceMode::Plan
        );
        assert_eq!(
            persisted_workspace
                .linked_ideation_session_id
                .as_ref()
                .map(IdeationSessionId::as_str),
            Some(session_id.as_str())
        );

        let session = app
            .state::<AppState>()
            .ideation_session_repo
            .get_by_id(&session_id)
            .await
            .expect("planning session lookup should succeed")
            .expect("planning session should persist");
        assert_eq!(session.session_flow, IdeationSessionFlow::Planning);
        assert_eq!(session.project_id.as_str(), project_id.as_str());
        assert_eq!(session.analysis.base_ref.as_deref(), Some("main"));
        assert_eq!(
            session.analysis.workspace_path.as_deref(),
            Some(workspace.worktree_path.as_str())
        );
        assert!(session.plan_artifact_id.is_none());

        let switched = switch_agent_conversation_mode_for_state(
            SwitchAgentConversationModeInput {
                conversation_id: response.conversation.id.clone(),
                mode: "ideation".to_string(),
                runtime_override: None,
                base_ref_kind: None,
                base_branch_mode: None,
                base_ref: None,
                base_display_name: None,
                base_source_pull_request: None,
            },
            app.state::<AppState>().inner(),
        )
        .await
        .expect("plan workspace should promote to ideation");
        let switched_workspace = switched
            .workspace
            .expect("workspace should still be returned after promotion");
        assert_eq!(switched_workspace.mode, "ideation");
        assert_eq!(
            switched_workspace.linked_ideation_session_id.as_deref(),
            Some(session_id.as_str())
        );

        let promoted_workspace = app
            .state::<AppState>()
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&ChatConversationId::from_string(response.conversation.id))
            .await
            .expect("workspace lookup after promotion should succeed")
            .expect("workspace should persist after promotion");
        assert_eq!(
            promoted_workspace.mode,
            AgentConversationWorkspaceMode::Ideation
        );
        assert_eq!(
            promoted_workspace
                .linked_ideation_session_id
                .as_ref()
                .map(IdeationSessionId::as_str),
            Some(session_id.as_str())
        );
    }

    struct FakeCliOnPath {
        _path_guard: crate::support::env::EnvVarGuard,
        _temp_dir: tempfile::TempDir,
    }

    impl FakeCliOnPath {
        fn new(command_name: &str) -> Self {
            let temp_dir = tempfile::tempdir().expect("fake CLI dir should be created");
            let cli_path = temp_dir.path().join(command_name);
            std::fs::write(&cli_path, "#!/bin/sh\nexit 0\n").expect("fake CLI should be written");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut permissions = std::fs::metadata(&cli_path)
                    .expect("fake CLI metadata should load")
                    .permissions();
                permissions.set_mode(0o755);
                std::fs::set_permissions(&cli_path, permissions)
                    .expect("fake CLI should be executable");
            }

            let path_guard = crate::support::env::prepend_to_path(temp_dir.path());

            Self {
                _path_guard: path_guard,
                _temp_dir: temp_dir,
            }
        }
    }

    #[test]
    fn switch_agent_conversation_mode_input_deserializes_camel_case() {
        let json = r#"{"conversationId":"conv-123","mode":"edit","baseRefKind":"project_default","baseRef":"main","baseDisplayName":"Project default (main)"}"#;
        let input: SwitchAgentConversationModeInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.conversation_id, "conv-123");
        assert_eq!(input.mode, "edit");
        assert_eq!(input.base_ref_kind.as_deref(), Some("project_default"));
        assert_eq!(input.base_ref.as_deref(), Some("main"));
    }

    #[tokio::test]
    async fn agent_model_commands_round_trip_through_ipc_state() {
        let app = agent_model_command_app();

        let initial = list_agent_models(app.state::<AppState>())
            .await
            .expect("models should list");
        assert!(initial.iter().any(|model| model.model_id == "gpt-5.5"));
        for model_id in ["claude-opus-4-7", "claude-opus-4-8", "claude-opus-5"] {
            assert!(
                initial
                    .iter()
                    .any(|model| model.model_id == model_id && model.source == "built_in"),
                "expected built-in model '{}' in command response",
                model_id
            );
        }
        assert!(initial
            .iter()
            .any(|model| model.default_effort == "max" || model.default_effort == "xhigh"));

        let saved = upsert_custom_agent_model(
            UpsertCustomAgentModelInput {
                provider: "codex".to_string(),
                model_id: "gpt-5.6".to_string(),
                label: "GPT-5.6".to_string(),
                menu_label: Some(String::new()),
                description: Some(" Future model ".to_string()),
                supported_efforts: vec!["high".to_string(), "low".to_string(), "high".to_string()],
                default_effort: "low".to_string(),
                enabled: true,
            },
            app.state::<AppState>(),
        )
        .await
        .expect("custom model should save");
        assert_eq!(saved.provider, "codex");
        assert_eq!(saved.model_id, "gpt-5.6");
        assert_eq!(saved.source, "custom");
        assert_eq!(saved.supported_efforts, vec!["low", "high"]);

        let after_save = list_agent_models(app.state::<AppState>())
            .await
            .expect("models should list after save");
        assert!(after_save
            .iter()
            .any(|model| model.model_id == "gpt-5.6" && model.default_effort == "low"));

        let opus_override = upsert_custom_agent_model(
            UpsertCustomAgentModelInput {
                provider: "claude".to_string(),
                model_id: "claude-opus-5".to_string(),
                label: "Private Opus 5".to_string(),
                menu_label: Some("Private Opus 5".to_string()),
                description: None,
                supported_efforts: vec!["low".to_string(), "high".to_string()],
                default_effort: "high".to_string(),
                enabled: true,
            },
            app.state::<AppState>(),
        )
        .await
        .expect("same-ID custom override should save");
        assert_eq!(opus_override.source, "custom");
        let overridden = list_agent_models(app.state::<AppState>())
            .await
            .expect("models should list custom override");
        assert!(overridden.iter().any(|model| {
            model.model_id == "claude-opus-5"
                && model.source == "custom"
                && model.label == "Private Opus 5"
        }));
        assert!(delete_custom_agent_model(
            "claude".to_string(),
            "claude-opus-5".to_string(),
            app.state::<AppState>(),
        )
        .await
        .expect("custom override should delete"));
        let restored = list_agent_models(app.state::<AppState>())
            .await
            .expect("models should list restored built-in");
        assert!(restored.iter().any(|model| {
            model.model_id == "claude-opus-5"
                && model.source == "built_in"
                && model.label == "Claude Opus 5"
        }));

        let deleted = delete_custom_agent_model(
            "codex".to_string(),
            "gpt-5.6".to_string(),
            app.state::<AppState>(),
        )
        .await
        .expect("custom model should delete");
        assert!(deleted);

        let missing = delete_custom_agent_model(
            "codex".to_string(),
            "gpt-5.6".to_string(),
            app.state::<AppState>(),
        )
        .await
        .expect("missing delete should return false");
        assert!(!missing);
    }

    #[test]
    fn agent_model_registry_ipc_contract_covers_provider_defaults() {
        let built_ins = built_in_agent_models();
        assert_eq!(built_ins.len(), 17);
        for (model_id, label) in [
            ("claude-opus-4-7", "Claude Opus 4.7"),
            ("claude-opus-4-8", "Claude Opus 4.8"),
            ("claude-opus-5", "Claude Opus 5"),
        ] {
            let model = built_ins
                .iter()
                .find(|model| {
                    model.provider == AgentHarnessKind::Claude && model.model_id == model_id
                })
                .expect("pinned Opus model should be exposed as a built-in Claude model");
            assert_eq!(model.source, AgentModelSource::BuiltIn);
            assert_eq!(model.label, label);
            assert_eq!(model.default_effort, LogicalEffort::High);
        }
        let sonnet_4_6 = built_ins
            .iter()
            .find(|model| {
                model.provider == AgentHarnessKind::Claude && model.model_id == "claude-sonnet-4-6"
            })
            .expect("Sonnet 4.6 should be exposed as a built-in Claude model");
        assert_eq!(sonnet_4_6.source, AgentModelSource::BuiltIn);
        assert_eq!(sonnet_4_6.default_effort, LogicalEffort::High);
        let sonnet_5 = built_ins
            .iter()
            .find(|model| {
                model.provider == AgentHarnessKind::Claude && model.model_id == "claude-sonnet-5"
            })
            .expect("Sonnet 5 should be exposed as a built-in Claude model");
        assert_eq!(sonnet_5.source, AgentModelSource::BuiltIn);
        assert_eq!(sonnet_5.default_effort, LogicalEffort::High);
        let fable = built_ins
            .iter()
            .find(|model| model.provider == AgentHarnessKind::Claude && model.model_id == "fable")
            .expect("Fable should be exposed as a built-in Claude model");
        assert_eq!(fable.source, AgentModelSource::BuiltIn);
        assert_eq!(fable.default_effort, LogicalEffort::High);
        assert_eq!(
            fable.supported_efforts,
            vec![
                LogicalEffort::Low,
                LogicalEffort::Medium,
                LogicalEffort::High,
                LogicalEffort::XHigh,
                LogicalEffort::Max
            ]
        );
        let codex_model_ids = built_ins
            .iter()
            .filter(|model| model.provider == AgentHarnessKind::Codex)
            .map(|model| model.model_id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            codex_model_ids,
            vec![
                "gpt-5.6-sol",
                "gpt-5.6-terra",
                "gpt-5.6-luna",
                "gpt-5.5",
                "gpt-5.4",
                "gpt-5.4-mini",
                "gpt-5.3-codex",
                "gpt-5.3-codex-spark",
            ]
        );
        let gpt_56_sol = built_ins
            .iter()
            .find(|model| {
                model.provider == AgentHarnessKind::Codex && model.model_id == "gpt-5.6-sol"
            })
            .expect("GPT-5.6 Sol should be exposed as a built-in Codex model");
        assert_eq!(gpt_56_sol.default_effort, LogicalEffort::Medium);
        assert_eq!(
            gpt_56_sol.supported_efforts,
            vec![
                LogicalEffort::Low,
                LogicalEffort::Medium,
                LogicalEffort::High,
                LogicalEffort::XHigh,
                LogicalEffort::Max,
                LogicalEffort::Ultra,
            ]
        );
        let gpt_56_terra = built_ins
            .iter()
            .find(|model| {
                model.provider == AgentHarnessKind::Codex && model.model_id == "gpt-5.6-terra"
            })
            .expect("GPT-5.6 Terra should be exposed as a built-in Codex model");
        assert_eq!(gpt_56_terra.default_effort, LogicalEffort::Medium);
        assert_eq!(gpt_56_terra.supported_efforts, gpt_56_sol.supported_efforts);
        let gpt_56_luna = built_ins
            .iter()
            .find(|model| {
                model.provider == AgentHarnessKind::Codex && model.model_id == "gpt-5.6-luna"
            })
            .expect("GPT-5.6 Luna should be exposed as a built-in Codex model");
        assert_eq!(gpt_56_luna.default_effort, LogicalEffort::Medium);
        assert_eq!(
            gpt_56_luna.supported_efforts,
            vec![
                LogicalEffort::Low,
                LogicalEffort::Medium,
                LogicalEffort::High,
                LogicalEffort::XHigh,
                LogicalEffort::Max,
            ]
        );
        assert_eq!(
            default_model_for_provider(AgentHarnessKind::Claude),
            "sonnet"
        );
        assert_eq!(
            default_model_for_provider(AgentHarnessKind::Codex),
            "gpt-5.5"
        );
        assert_eq!(
            lightweight_model_for_provider(AgentHarnessKind::Claude),
            "haiku"
        );
        assert_eq!(
            lightweight_model_for_provider(AgentHarnessKind::Codex),
            "gpt-5.4-mini"
        );
        assert_eq!(
            default_effort_for_provider(AgentHarnessKind::Claude),
            LogicalEffort::Medium
        );
        assert_eq!(
            default_effort_for_provider(AgentHarnessKind::Codex),
            LogicalEffort::XHigh
        );
        assert_eq!(
            default_efforts_for_provider(AgentHarnessKind::Claude),
            &[
                LogicalEffort::Low,
                LogicalEffort::Medium,
                LogicalEffort::High
            ]
        );

        let custom = AgentModelDefinition::custom(
            AgentHarnessKind::Codex,
            " gpt-5.6 ",
            "",
            "",
            Some(" next model ".to_string()),
            vec![
                LogicalEffort::XHigh,
                LogicalEffort::Low,
                LogicalEffort::XHigh,
            ],
            LogicalEffort::Max,
            true,
        );
        let disabled_default = AgentModelDefinition::custom(
            AgentHarnessKind::Codex,
            "gpt-5.5",
            "Disabled GPT-5.5",
            "Disabled GPT-5.5",
            None,
            vec![LogicalEffort::Low],
            LogicalEffort::Low,
            false,
        );
        let snapshot = AgentModelRegistrySnapshot::merged(vec![custom, disabled_default]);

        let custom = snapshot
            .find_enabled(AgentHarnessKind::Codex, "gpt-5.6")
            .expect("custom model should be enabled");
        assert_eq!(custom.label, "gpt-5.6");
        assert_eq!(custom.menu_label, "gpt-5.6");
        assert_eq!(custom.description.as_deref(), Some("next model"));
        assert_eq!(
            custom.supported_efforts,
            vec![LogicalEffort::Low, LogicalEffort::XHigh]
        );
        assert_eq!(custom.default_effort, LogicalEffort::XHigh);
        assert_eq!(custom.source, AgentModelSource::Custom);
        assert!(snapshot
            .find_enabled(AgentHarnessKind::Codex, "gpt-5.5")
            .is_none());
        assert_eq!(
            snapshot
                .default_for_provider(AgentHarnessKind::Codex)
                .map(|model| model.model_id.as_str()),
            Some("gpt-5.6-sol")
        );
    }

    #[tokio::test]
    async fn agent_model_memory_repository_ipc_contract_round_trips() {
        let repo = MemoryAgentModelRegistryRepository::new();
        let model = AgentModelDefinition::custom(
            AgentHarnessKind::Claude,
            "claude-opus-5",
            "Claude Opus 5",
            "Claude Opus 5",
            None,
            vec![
                LogicalEffort::High,
                LogicalEffort::XHigh,
                LogicalEffort::Max,
            ],
            LogicalEffort::Max,
            true,
        );

        let saved = repo.upsert_custom_model(&model).await.unwrap();
        let saved_again = repo.upsert_custom_model(&model).await.unwrap();
        let rows = repo.list_custom_models().await.unwrap();

        assert_eq!(saved.source, AgentModelSource::Custom);
        assert_eq!(saved.created_at, saved_again.created_at);
        assert!(saved_again.updated_at >= saved.updated_at);
        assert_eq!(rows.len(), 1);
        assert!(repo
            .delete_custom_model(AgentHarnessKind::Claude, "claude-opus-5")
            .await
            .unwrap());
        assert!(!repo
            .delete_custom_model(AgentHarnessKind::Claude, "claude-opus-5")
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn agent_model_sqlite_repository_ipc_contract_round_trips() {
        let state = AppState::new_sqlite_test();
        let model = AgentModelDefinition::custom(
            AgentHarnessKind::Codex,
            "gpt-5.6",
            "GPT-5.6",
            "GPT-5.6",
            Some("Future model".to_string()),
            vec![
                LogicalEffort::Low,
                LogicalEffort::Medium,
                LogicalEffort::High,
                LogicalEffort::XHigh,
            ],
            LogicalEffort::XHigh,
            true,
        );
        let saved = state
            .agent_model_registry_repo
            .upsert_custom_model(&model)
            .await
            .unwrap();
        let updated = AgentModelDefinition::custom(
            AgentHarnessKind::Codex,
            "gpt-5.6",
            "GPT-5.6 Preview",
            "GPT-5.6 Preview",
            None,
            vec![LogicalEffort::Low, LogicalEffort::Medium],
            LogicalEffort::Medium,
            false,
        );
        let saved_again = state
            .agent_model_registry_repo
            .upsert_custom_model(&updated)
            .await
            .unwrap();
        let rows = state
            .agent_model_registry_repo
            .list_custom_models()
            .await
            .unwrap();

        assert_eq!(saved.source, AgentModelSource::Custom);
        assert_eq!(saved_again.created_at, saved.created_at);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].label, "GPT-5.6 Preview");
        assert!(!rows[0].enabled);
        assert!(state
            .agent_model_registry_repo
            .delete_custom_model(AgentHarnessKind::Codex, "gpt-5.6")
            .await
            .unwrap());
        assert!(!state
            .agent_model_registry_repo
            .delete_custom_model(AgentHarnessKind::Codex, "gpt-5.6")
            .await
            .unwrap());
    }

    // -----------------------------------------------------------------------
    // Archive conversation: PR close + workspace status
    // -----------------------------------------------------------------------

    async fn set_archive_test_workspace_mode(
        state: &AppState,
        conversation_id: &ChatConversationId,
        mode: AgentConversationWorkspaceMode,
    ) -> AgentConversationWorkspace {
        let mut workspace = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(conversation_id)
            .await
            .expect("workspace read should succeed")
            .expect("workspace should exist");
        workspace.mode = mode;
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("workspace mode update should succeed");
        workspace
    }

    async fn create_archive_test_plan_branch(
        state: &AppState,
        conversation_id: &ChatConversationId,
        workspace: &AgentConversationWorkspace,
        session_suffix: &str,
        execution_plan: Option<&ExecutionPlan>,
    ) -> PlanBranch {
        let session_id = execution_plan
            .map(|plan| plan.session_id.clone())
            .unwrap_or_else(|| IdeationSessionId::from_string(format!("session-{session_suffix}")));
        let mut plan_branch = PlanBranch::new(
            ArtifactId::from_string(format!("artifact-{session_suffix}")),
            session_id.clone(),
            workspace.project_id.clone(),
            format!("plan/{session_suffix}"),
            "main".to_string(),
        );
        plan_branch.execution_plan_id = execution_plan.map(|plan| plan.id.clone());
        let plan_branch_id = plan_branch.id.clone();
        let created = state
            .plan_branch_repo
            .create(plan_branch)
            .await
            .expect("plan branch should be created");
        state
            .agent_conversation_workspace_repo
            .update_links(conversation_id, Some(&session_id), Some(&plan_branch_id))
            .await
            .expect("workspace links should be updated");
        created
    }

    #[tokio::test]
    async fn archive_conversation_sets_workspace_status_to_archived() {
        let github = Arc::new(crate::common::MockGithubService::new());
        let (_temp, state, conv_id, _github) =
            super::setup_ipc_workspace_state("archive-status", true, None, github).await;

        archive_agent_conversation_inner(&conv_id, false, &state)
            .await
            .expect("archive should succeed");

        let ws = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conv_id)
            .await
            .expect("repo call should succeed")
            .expect("workspace should exist");
        assert_eq!(ws.status, AgentConversationWorkspaceStatus::Archived);
    }

    #[tokio::test]
    async fn archive_conversation_stops_project_agent() {
        let github = Arc::new(crate::common::MockGithubService::new());
        let (_temp, state, conv_id, _github) =
            super::setup_ipc_workspace_state("archive-stop-agent", true, None, github).await;
        let key = RunningAgentKey::new(ChatContextType::Project.to_string(), conv_id.as_str());
        state
            .running_agent_registry
            .register(
                key.clone(),
                0,
                conv_id.as_str().to_string(),
                "run-archive-stop-agent".to_string(),
                None,
                None,
            )
            .await;
        assert!(state.running_agent_registry.is_running(&key).await);

        archive_agent_conversation_inner(&conv_id, false, &state)
            .await
            .expect("archive should succeed");

        assert!(!state.running_agent_registry.is_running(&key).await);
    }

    #[tokio::test]
    async fn archive_conversation_closes_open_pr() {
        let github = Arc::new(crate::common::MockGithubService::new());
        let (_temp, state, conv_id, github) =
            super::setup_ipc_workspace_state("archive-close-pr", true, Some(42), github.clone())
                .await;

        archive_agent_conversation_inner(&conv_id, true, &state)
            .await
            .expect("archive should succeed");

        assert_eq!(*github.close_pr_calls.lock().unwrap(), 1);

        let ws = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conv_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(ws.publication_pr_status.as_deref(), Some("closed"));
        assert_eq!(ws.status, AgentConversationWorkspaceStatus::Archived);
    }

    #[tokio::test]
    async fn archive_conversation_keeps_open_pr_without_close_request() {
        let github = Arc::new(crate::common::MockGithubService::new());
        let (_temp, state, conv_id, github) =
            super::setup_ipc_workspace_state("archive-keep-pr", true, Some(43), github.clone())
                .await;

        archive_agent_conversation_inner(&conv_id, false, &state)
            .await
            .expect("archive should succeed");

        assert_eq!(*github.close_pr_calls.lock().unwrap(), 0);
        let workspace = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conv_id)
            .await
            .unwrap()
            .expect("workspace should exist");
        assert_eq!(workspace.status, AgentConversationWorkspaceStatus::Archived);
        assert_eq!(workspace.publication_pr_status.as_deref(), Some("open"));
    }

    #[tokio::test]
    async fn archive_review_pr_keeps_open_pr_even_when_close_is_requested() {
        let github = Arc::new(crate::common::MockGithubService::new());
        let (_temp, state, conv_id, github) =
            super::setup_ipc_workspace_state("archive-review-pr", true, Some(44), github.clone())
                .await;
        set_archive_test_workspace_mode(&state, &conv_id, AgentConversationWorkspaceMode::ReviewPr)
            .await;

        archive_agent_conversation_inner(&conv_id, true, &state)
            .await
            .expect("archive should preserve the reviewed PR");

        assert_eq!(*github.close_pr_calls.lock().unwrap(), 0);
        let workspace = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conv_id)
            .await
            .unwrap()
            .expect("workspace should exist");
        assert_eq!(workspace.status, AgentConversationWorkspaceStatus::Archived);
        assert_eq!(workspace.publication_pr_status.as_deref(), Some("open"));
    }

    #[tokio::test]
    async fn archive_conversation_skips_close_when_pr_already_closed() {
        let github = Arc::new(crate::common::MockGithubService::new());
        let (_temp, state, conv_id, github) = super::setup_ipc_workspace_state(
            "archive-already-closed",
            true,
            Some(99),
            github.clone(),
        )
        .await;

        let _ = state
            .agent_conversation_workspace_repo
            .update_publication(&conv_id, Some(99), None, Some("closed"), None)
            .await;

        archive_agent_conversation_inner(&conv_id, true, &state)
            .await
            .expect("archive should succeed");

        assert_eq!(*github.close_pr_calls.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn archive_conversation_skips_close_when_pr_merged() {
        let github = Arc::new(crate::common::MockGithubService::new());
        let (_temp, state, conv_id, github) =
            super::setup_ipc_workspace_state("archive-merged", true, Some(77), github.clone())
                .await;

        let _ = state
            .agent_conversation_workspace_repo
            .update_publication(&conv_id, Some(77), None, Some("merged"), None)
            .await;

        archive_agent_conversation_inner(&conv_id, true, &state)
            .await
            .expect("archive should succeed");

        assert_eq!(*github.close_pr_calls.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn archive_conversation_closes_linked_plan_branch_pr() {
        let github = Arc::new(crate::common::MockGithubService::new());
        let (_temp, state, conv_id, github) =
            super::setup_ipc_workspace_state("archive-plan-branch", true, None, github.clone())
                .await;
        let workspace = set_archive_test_workspace_mode(
            &state,
            &conv_id,
            AgentConversationWorkspaceMode::Ideation,
        )
        .await;
        let plan_branch =
            create_archive_test_plan_branch(&state, &conv_id, &workspace, "plan-branch-pr", None)
                .await;
        let plan_branch_id = plan_branch.id.clone();
        state
            .plan_branch_repo
            .update_pr_info(
                &plan_branch_id,
                55,
                "https://github.com/mock/repo/pull/55".to_string(),
                DbPrStatus::Open,
                false,
            )
            .await
            .expect("pr info update should succeed");

        state
            .agent_conversation_workspace_repo
            .update_publication(&conv_id, None, None, None, None)
            .await
            .expect("workspace publication clear should succeed");

        archive_agent_conversation_inner(&conv_id, true, &state)
            .await
            .expect("archive should succeed");

        assert_eq!(*github.close_pr_calls.lock().unwrap(), 1);

        let updated_branch = state
            .plan_branch_repo
            .get_by_id(&plan_branch_id)
            .await
            .unwrap()
            .expect("plan branch should still exist");
        assert_eq!(updated_branch.pr_status, Some(DbPrStatus::Closed));
        let ws = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conv_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(ws.publication_pr_number, Some(55));
        assert_eq!(ws.publication_pr_status.as_deref(), Some("closed"));
    }

    #[tokio::test]
    async fn archive_ideation_workspace_archives_current_execution_tasks_only() {
        let github = Arc::new(crate::common::MockGithubService::new());
        let (_temp, state, conv_id, _github) =
            super::setup_ipc_workspace_state("archive-current-tasks", true, None, github).await;
        let workspace = set_archive_test_workspace_mode(
            &state,
            &conv_id,
            AgentConversationWorkspaceMode::Ideation,
        )
        .await;
        let session_id = IdeationSessionId::from_string("session-current-tasks".to_string());
        let current_plan = state
            .execution_plan_repo
            .create(ExecutionPlan::new(session_id.clone()))
            .await
            .expect("current execution plan should be created");
        let mut stale_plan = ExecutionPlan::new(session_id.clone());
        stale_plan.status = ExecutionPlanStatus::Superseded;
        let stale_plan = state
            .execution_plan_repo
            .create(stale_plan)
            .await
            .expect("stale execution plan should be created");
        let plan_branch = create_archive_test_plan_branch(
            &state,
            &conv_id,
            &workspace,
            "current-tasks",
            Some(&current_plan),
        )
        .await;

        let mut current_task = Task::new(workspace.project_id.clone(), "current task".to_string());
        current_task.ideation_session_id = Some(session_id.clone());
        current_task.execution_plan_id = Some(current_plan.id.clone());
        let current_task_id = current_task.id.clone();
        state
            .task_repo
            .create(current_task)
            .await
            .expect("current task should be created");

        let mut stale_task = Task::new(workspace.project_id.clone(), "stale task".to_string());
        stale_task.ideation_session_id = Some(session_id);
        stale_task.execution_plan_id = Some(stale_plan.id.clone());
        let stale_task_id = stale_task.id.clone();
        state
            .task_repo
            .create(stale_task)
            .await
            .expect("stale task should be created");

        archive_agent_conversation_inner(&conv_id, false, &state)
            .await
            .expect("archive should succeed");

        let current_task = state
            .task_repo
            .get_by_id(&current_task_id)
            .await
            .unwrap()
            .expect("current task should exist");
        assert!(current_task.archived_at.is_some());
        let stale_task = state
            .task_repo
            .get_by_id(&stale_task_id)
            .await
            .unwrap()
            .expect("stale task should exist");
        assert!(stale_task.archived_at.is_none());

        let current_plan = state
            .execution_plan_repo
            .get_by_id(&current_plan.id)
            .await
            .unwrap()
            .expect("current plan should exist");
        assert_eq!(current_plan.status, ExecutionPlanStatus::Superseded);
        let updated_branch = state
            .plan_branch_repo
            .get_by_id(&plan_branch.id)
            .await
            .unwrap()
            .expect("plan branch should exist");
        assert_eq!(updated_branch.status, PlanBranchStatus::Abandoned);
    }

    #[tokio::test]
    async fn archive_conversation_does_not_archive_tasks_for_direct_edit_workspace() {
        let github = Arc::new(crate::common::MockGithubService::new());
        let (_temp, state, conv_id, _github) =
            super::setup_ipc_workspace_state("archive-edit-task-scope", true, None, github).await;
        let workspace = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conv_id)
            .await
            .unwrap()
            .expect("workspace should exist");
        assert_eq!(workspace.mode, AgentConversationWorkspaceMode::Edit);
        let session_id = IdeationSessionId::from_string("session-edit-task-scope".to_string());
        let current_plan = state
            .execution_plan_repo
            .create(ExecutionPlan::new(session_id.clone()))
            .await
            .expect("execution plan should be created");
        let plan_branch = create_archive_test_plan_branch(
            &state,
            &conv_id,
            &workspace,
            "edit-task-scope",
            Some(&current_plan),
        )
        .await;

        let mut task = Task::new(workspace.project_id.clone(), "edit scoped task".to_string());
        task.ideation_session_id = Some(session_id);
        task.execution_plan_id = Some(current_plan.id.clone());
        let task_id = task.id.clone();
        state
            .task_repo
            .create(task)
            .await
            .expect("task should be created");

        archive_agent_conversation_inner(&conv_id, false, &state)
            .await
            .expect("archive should succeed");

        let task = state
            .task_repo
            .get_by_id(&task_id)
            .await
            .unwrap()
            .expect("task should exist");
        assert!(task.archived_at.is_none());
        let current_plan = state
            .execution_plan_repo
            .get_by_id(&current_plan.id)
            .await
            .unwrap()
            .expect("execution plan should exist");
        assert_eq!(current_plan.status, ExecutionPlanStatus::Active);
        let updated_branch = state
            .plan_branch_repo
            .get_by_id(&plan_branch.id)
            .await
            .unwrap()
            .expect("plan branch should exist");
        assert_eq!(updated_branch.status, PlanBranchStatus::Active);
    }

    #[tokio::test]
    async fn archive_conversation_without_workspace_still_succeeds() {
        let state = AppState::new_test();
        let conv_id = ChatConversationId::from_string("no-workspace-conv".to_string());
        let project = Project::new("NoWS".to_string(), "/tmp/nows".to_string());
        state.project_repo.create(project.clone()).await.unwrap();
        let mut conversation = ChatConversation::new_project(project.id.clone());
        conversation.id = conv_id;
        state
            .chat_conversation_repo
            .create(conversation)
            .await
            .unwrap();

        archive_agent_conversation_inner(&conv_id, false, &state)
            .await
            .expect("archive should succeed even without workspace");
    }
}
