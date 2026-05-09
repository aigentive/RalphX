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
    use ralphx_lib::commands::unified_chat_commands::{
        CreateAgentConversationInput, QueueAgentMessageInput, SendAgentMessageInput,
        StartAgentConversationInput, SwitchAgentConversationModeInput,
        UpdateAgentConversationTitleInput,
    };
    use ralphx_lib::commands::unified_chat_commands::{
        get_agent_conversation_messages_page_for_app_state, get_agent_conversation_workspace,
        get_agent_conversation_workspace_freshness, get_agent_message_tool_call_detail,
        publish_agent_conversation_workspace_for_app_state, CreateAgentConversationInput,
        QueueAgentMessageInput, SendAgentMessageInput, StartAgentConversationInput,
        SwitchAgentConversationModeInput, UpdateAgentConversationTitleInput,
    };
    use ralphx_lib::domain::agents::{
        built_in_agent_models, default_effort_for_provider, default_efforts_for_provider,
        default_model_for_provider, lightweight_model_for_provider, AgentHarnessKind,
        AgentModelDefinition, AgentModelRegistrySnapshot, AgentModelSource, LogicalEffort,
    };
    use ralphx_lib::domain::entities::{
        AgentConversationWorkspace, AgentConversationWorkspaceMode,
        AgentConversationWorkspaceStatus, AgentRun, ChatConversation, ChatConversationId,
        ChatMessage, IdeationAnalysisBaseRefKind, MessageRole, ProjectId,
    };
    use ralphx_lib::domain::repositories::{
        AgentConversationWorkspaceRepository, AgentModelRegistryRepository,
    };
    use ralphx_lib::infrastructure::memory::MemoryAgentModelRegistryRepository;
    use ralphx_lib::infrastructure::sqlite::sqlite_agent_conversation_workspace_repo::SqliteAgentConversationWorkspaceRepository;
    use ralphx_lib::testing::SqliteTestDb;
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
            std::sync::Arc::new(super::common::MockGithubService::new()),
        )
        .await;
        let app = mock_builder()
            .manage(state)
            .build(mock_context(noop_assets()))
            .expect("mock app should build");

        let response =
            get_agent_conversation_workspace_freshness(conversation_id.as_str(), app.state())
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
            std::sync::Arc::new(super::common::MockGithubService::new()),
        )
        .await;
        let app = mock_builder()
            .manage(state)
            .build(mock_context(noop_assets()))
            .expect("mock app should build");

        let response =
            get_agent_conversation_workspace_freshness(conversation_id.as_str(), app.state())
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
    async fn ipc_contract_workspace_response_recovers_stale_needs_agent_publish_lock() {
        let (_temp, state, conversation_id, _github) = super::setup_ipc_workspace_state(
            "workspace-stale-repair-response",
            true,
            Some(765),
            std::sync::Arc::new(super::common::MockGithubService::new()),
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

        assert_eq!(response.publication_push_status.as_deref(), Some("failed"));
    }

    #[tokio::test]
    async fn ipc_contract_workspace_freshness_recovers_stale_needs_agent_publish_lock() {
        let (_temp, state, conversation_id, _github) = super::setup_ipc_workspace_state(
            "workspace-stale-repair-freshness",
            true,
            Some(766),
            std::sync::Arc::new(super::common::MockGithubService::new()),
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

        get_agent_conversation_workspace_freshness(conversation_id.as_str(), app.state())
            .await
            .expect("freshness should load");
        let refreshed = app
            .state::<AppState>()
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");

        assert_eq!(refreshed.publication_push_status.as_deref(), Some("failed"));
    }

    #[tokio::test]
    async fn ipc_contract_startup_publish_recovery_clears_stale_lock() {
        let (_temp, state, conversation_id, _github) = super::setup_ipc_workspace_state(
            "workspace-stale-repair-startup",
            true,
            Some(767),
            std::sync::Arc::new(super::common::MockGithubService::new()),
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
        let github = std::sync::Arc::new(super::common::MockGithubService::new());
        github.will_fail_update_pr_base("denied");
        let (_temp, state, conversation_id, github) =
            super::setup_ipc_workspace_state("publish-retarget-fails", true, Some(654), github)
                .await;
        let execution_state = std::sync::Arc::new(super::ExecutionState::new());

        let error = publish_agent_conversation_workspace_for_app_state(
            &state,
            &execution_state,
            None,
            conversation_id.clone(),
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
        let conversation_id = conversation.id.clone();
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
        message.conversation_id = Some(conversation_id.clone());
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
            conversation_id.clone(),
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
        let json = r#"{"projectId":"project-1","content":"What changed?","mode":"chat","providerHarness":"codex","modelOverride":"gpt-5.4"}"#;
        let input: StartAgentConversationInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.project_id, "project-1");
        assert_eq!(input.mode.as_deref(), Some("chat"));
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
}
