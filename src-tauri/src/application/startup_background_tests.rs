use super::automation::plan_gate::{AutomationRunResumer, ResumeDelivery};
use super::chat_service::{MockChatService, SendCallerContext};
use super::startup_background::{
    automation_run_resumer_for_test, automation_run_starter_for_test, external_mcp_startup_timeout,
    resume_automation_run_with_prompt_via_chat_service, try_start_recurring_service,
};
use super::AppState;
use crate::application::automation::provisioning::{
    AutomationRunStartRequest, AutomationRunStarter,
};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AutomationRunId, ChatContextType,
    ChatConversation, ChatConversationId, IdeationAnalysisBaseRefKind, ProjectId,
};
use crate::error::AppError;

#[test]
fn recurring_startup_services_are_claimed_exactly_once() {
    const SERVICE: &str = "startup_background_tests_unique_service";

    assert!(try_start_recurring_service(SERVICE));
    assert!(!try_start_recurring_service(SERVICE));
}

#[test]
fn external_mcp_startup_uses_the_configured_timeout() {
    let config = crate::infrastructure::agents::claude::ExternalMcpConfig {
        startup_timeout_secs: 17,
        ..Default::default()
    };

    assert_eq!(
        external_mcp_startup_timeout(&config),
        std::time::Duration::from_secs(17)
    );
}

#[tokio::test]
async fn automation_run_starter_validates_request_before_runtime_start() {
    let starter = automation_run_starter_for_test(AppState::new_test());
    let request = AutomationRunStartRequest {
        project_id: "project-1".to_string(),
        conversation_id: ChatConversationId::from_string("11111111-1111-4111-8111-111111111111"),
        run_prompt: "Run the automation".to_string(),
        provider_harness: "codex".to_string(),
        model_id: "gpt-5.4".to_string(),
        logical_effort: Some("impossible".to_string()),
        run_mode: "edit".to_string(),
        base_ref_kind: "local_branch".to_string(),
        base_ref: "main".to_string(),
        base_display_name: Some("main".to_string()),
        base_source_pull_request_json: None,
        composer_project_references: Vec::new(),
        composer_integration_references: Vec::new(),
        composer_artifact_references: Vec::new(),
        automation_context: None,
    };

    let error = starter.start_run(request).await.unwrap_err();

    assert!(matches!(error, AppError::Validation(message) if message.contains("impossible")));
}

#[tokio::test]
async fn automation_run_resumer_purges_queued_prompt_then_returns_queued_and_purged() {
    let (state, conversation_id) = seed_project_conversation().await;
    let chat_service = MockChatService::new();
    chat_service.set_already_running_after(0).await;

    let outcome = resume_automation_run_with_prompt_via_chat_service(
        &state,
        &chat_service,
        &conversation_id,
        "approved plan prompt",
    )
    .await
    .unwrap();

    assert_eq!(outcome, ResumeDelivery::QueuedAndPurged);
    let delete_calls = chat_service.get_delete_queued_message_calls().await;
    assert_eq!(
        delete_calls,
        vec![(
            ChatContextType::Project,
            conversation_id.as_str(),
            "mock-queued-id".to_string()
        )]
    );
    let sent_options = chat_service.get_sent_options().await;
    assert_eq!(sent_options.len(), 1);
    assert_eq!(
        sent_options[0].conversation_id_override.as_ref(),
        Some(&conversation_id)
    );
    assert_eq!(
        sent_options[0].caller_context,
        SendCallerContext::StartupResumption
    );
}

#[tokio::test]
async fn automation_run_resumer_returns_queued_and_purged_when_queued_prompt_purge_fails() {
    let (state, conversation_id) = seed_project_conversation().await;
    let chat_service = MockChatService::new();
    chat_service.set_already_running_after(0).await;
    chat_service.fail_next_delete_queued_message().await;

    let outcome = resume_automation_run_with_prompt_via_chat_service(
        &state,
        &chat_service,
        &conversation_id,
        "approved plan prompt",
    )
    .await
    .unwrap();

    assert_eq!(outcome, ResumeDelivery::QueuedAndPurged);
    let delete_calls = chat_service.get_delete_queued_message_calls().await;
    assert_eq!(delete_calls.len(), 1);
    assert_eq!(delete_calls[0].2, "mock-queued-id");
}

#[tokio::test]
async fn automation_run_resumer_switch_to_edit_uses_system_mode_switch() {
    let (state, conversation_id) = seed_automation_plan_conversation().await;
    let resumer = automation_run_resumer_for_test(state.clone());

    resumer
        .switch_to_edit(&conversation_id)
        .await
        .expect("system switch should allow automation conversation");

    let conversation = state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .expect("load conversation")
        .expect("conversation");
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load workspace")
        .expect("workspace");
    assert_eq!(
        conversation.agent_mode,
        Some(AgentConversationWorkspaceMode::Edit)
    );
    assert_eq!(workspace.mode, AgentConversationWorkspaceMode::Edit);
}

async fn seed_project_conversation() -> (AppState, ChatConversationId) {
    let state = AppState::new_test();
    let project_id = ProjectId::from_string("project-resumer".to_string());
    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project_id))
        .await
        .expect("create conversation");
    (state, conversation.id)
}

async fn seed_automation_plan_conversation() -> (AppState, ChatConversationId) {
    let state = AppState::new_test();
    let project_id = ProjectId::from_string("project-resumer-switch".to_string());
    let mut conversation = ChatConversation::new_project(project_id.clone());
    conversation.automation_run_id = Some(AutomationRunId::new());
    conversation.set_agent_mode(Some(AgentConversationWorkspaceMode::Plan));
    let conversation = state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("create conversation");
    let workspace = AgentConversationWorkspace::new(
        conversation.id.clone(),
        project_id,
        AgentConversationWorkspaceMode::Plan,
        IdeationAnalysisBaseRefKind::LocalBranch,
        "main".to_string(),
        Some("main".to_string()),
        Some("abc123".to_string()),
        "run-branch".to_string(),
        "/tmp/run-branch".to_string(),
    );
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("create workspace");
    (state, conversation.id)
}
