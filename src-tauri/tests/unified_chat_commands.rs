use ralphx_lib::application::{AppState, MockChatService, SendResult};
use ralphx_lib::commands::unified_chat_commands::{
    mark_agent_workspace_publish_failure, parse_context_type,
    send_agent_workspace_publish_repair_message, AgentRunStatusResponse,
    AgentWorkspaceRepairRuntimeOverrides, QueuedMessageResponse, SendAgentMessageResponse,
};
use ralphx_lib::domain::agents::{AgentHarnessKind, LogicalEffort, ProviderSessionRef};
use ralphx_lib::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentRun, ChatContextType,
    ChatConversation, ChatConversationId, IdeationAnalysisBaseRefKind, ProjectId,
};
use ralphx_lib::domain::services::QueuedMessage;
use ralphx_lib::infrastructure::agents::claude::agent_names::AGENT_WORKSPACE_REPAIR;

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
    assert_eq!(response.queued_message_id.as_deref(), Some("queued-msg-123"));
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
    let workspace = test_agent_workspace();

    mark_agent_workspace_publish_failure(
        &state,
        &workspace,
        "Failed to commit workspace changes: typecheck failed",
        None,
        &service,
    )
    .await;

    assert_eq!(service.call_count(), 1);
    let messages = service.get_sent_messages().await;
    assert_eq!(messages.len(), 1);
    assert!(messages[0].contains("typecheck failed"));
}

#[tokio::test]
async fn workspace_publish_repair_inherits_workspace_runtime_but_starts_fresh_session() {
    let state = AppState::new_test();
    let service = MockChatService::new();
    let workspace = test_agent_workspace();

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

    mark_agent_workspace_publish_failure(
        &state,
        &workspace,
        "Failed to commit workspace changes: merge conflict",
        None,
        &service,
    )
    .await;

    let options = service.get_sent_options().await;
    assert_eq!(options.len(), 1);
    assert_eq!(options[0].harness_override, Some(AgentHarnessKind::Codex));
    assert_eq!(options[0].model_override.as_deref(), Some("gpt-5.4"));
    assert_eq!(
        options[0].logical_effort_override,
        Some(LogicalEffort::High)
    );
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
    };
    let json = serde_json::to_string(&response).unwrap();
    assert!(json.contains(r#""model_id":null"#));
    assert!(json.contains(r#""model_label":null"#));
}

// ── IPC contract tests ─────────────────────────────────────────────────────────
// Verify camelCase deserialization for unified chat command input structs.

#[cfg(test)]
mod ipc_contract {
    use ralphx_lib::application::AppState;
    use ralphx_lib::commands::agent_model_commands::{
        delete_custom_agent_model, list_agent_models, upsert_custom_agent_model,
        UpsertCustomAgentModelInput,
    };
    use ralphx_lib::commands::unified_chat_commands::{
        CreateAgentConversationInput, QueueAgentMessageInput, SendAgentMessageInput,
        StartAgentConversationInput, SwitchAgentConversationModeInput,
        UpdateAgentConversationTitleInput,
    };
    use ralphx_lib::domain::agents::{
        built_in_agent_models, default_effort_for_provider, default_efforts_for_provider,
        default_model_for_provider, lightweight_model_for_provider, AgentHarnessKind,
        AgentModelDefinition, AgentModelRegistrySnapshot, AgentModelSource, LogicalEffort,
    };
    use ralphx_lib::domain::repositories::AgentModelRegistryRepository;
    use ralphx_lib::infrastructure::memory::MemoryAgentModelRegistryRepository;
    use tauri::test::{mock_builder, mock_context, noop_assets};
    use tauri::Manager;

    fn agent_model_command_app() -> tauri::App<tauri::test::MockRuntime> {
        mock_builder()
            .manage(AppState::new_test())
            .build(mock_context(noop_assets()))
            .expect("mock app should build")
    }

    // ── SendAgentMessageInput ───────────────────────────────────────────────

    #[test]
    fn send_agent_message_input_deserializes_camel_case() {
        let json = r#"{"contextType":"task_execution","contextId":"task-123","content":"Hello agent","modelOverride":"gpt-5.5","logicalEffort":"xhigh","target":null}"#;
        let input: SendAgentMessageInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.context_type, "task_execution");
        assert_eq!(input.context_id, "task-123");
        assert_eq!(input.content, "Hello agent");
        assert_eq!(input.model_override.as_deref(), Some("gpt-5.5"));
        assert_eq!(input.logical_effort, Some(LogicalEffort::XHigh));
        assert!(input.target.is_none());
    }

    #[test]
    fn send_agent_message_input_with_target() {
        let json = r#"{"contextType":"ideation","contextId":"session-456","content":"Plan this","target":"orchestrator"}"#;
        let input: SendAgentMessageInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.context_type, "ideation");
        assert_eq!(input.context_id, "session-456");
        assert_eq!(input.target, Some("orchestrator".to_string()));
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
        let json = r#"{"contextType":"task","contextId":"task-789","content":"Queued msg","clientId":"client-abc","target":null}"#;
        let input: QueueAgentMessageInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.context_type, "task");
        assert_eq!(input.context_id, "task-789");
        assert_eq!(input.content, "Queued msg");
        assert_eq!(input.client_id, Some("client-abc".to_string()));
        assert!(input.target.is_none());
    }

    #[test]
    fn queue_agent_message_input_optional_fields_absent() {
        let json = r#"{"contextType":"project","contextId":"proj-1","content":"Hello"}"#;
        let input: QueueAgentMessageInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.context_type, "project");
        assert!(input.client_id.is_none());
        assert!(input.target.is_none());
    }

    // ── CreateAgentConversationInput ────────────────────────────────────────

    #[test]
    fn create_agent_conversation_input_deserializes_camel_case() {
        let json = r#"{"contextType":"review","contextId":"task-review-123"}"#;
        let input: CreateAgentConversationInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.context_type, "review");
        assert_eq!(input.context_id, "task-review-123");
    }

    #[test]
    fn create_agent_conversation_input_rejects_missing_fields() {
        let json = r#"{"contextType":"ideation"}"#;
        let result: Result<CreateAgentConversationInput, _> = serde_json::from_str(json);
        assert!(
            result.is_err(),
            "missing contextId must cause deserialization failure"
        );
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
        let json = r#"{"projectId":"project-1","content":"What changed?","mode":"chat","providerHarness":"codex","modelOverride":"gpt-5.5","logicalEffort":"xhigh"}"#;
        let input: StartAgentConversationInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.project_id, "project-1");
        assert_eq!(input.mode.as_deref(), Some("chat"));
        assert_eq!(input.model_override.as_deref(), Some("gpt-5.5"));
        assert_eq!(input.logical_effort, Some(LogicalEffort::XHigh));
        assert!(input.base_ref_kind.is_none());
        assert!(input.base_ref.is_none());
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
                supported_efforts: vec![
                    "high".to_string(),
                    "low".to_string(),
                    "high".to_string(),
                ],
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
        assert_eq!(built_ins.len(), 8);
        assert_eq!(default_model_for_provider(AgentHarnessKind::Claude), "sonnet");
        assert_eq!(default_model_for_provider(AgentHarnessKind::Codex), "gpt-5.5");
        assert_eq!(lightweight_model_for_provider(AgentHarnessKind::Claude), "haiku");
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
            Some("gpt-5.4")
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
            vec![LogicalEffort::High, LogicalEffort::XHigh, LogicalEffort::Max],
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
}
