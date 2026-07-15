use axum::extract::{Path, State};
use axum::http::StatusCode;
use chrono::Utc;
use ralphx_lib::application::chat_service::{CachedStreamingTask, CachedToolCall};
use ralphx_lib::application::{AppState, TeamService, TeamStateTracker};
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::agents::AgentHarnessKind;
use ralphx_lib::domain::entities::{
    AgentRun, ChatContextType, ChatConversation, DelegatedSession, IdeationSessionId, ProjectId,
    TaskId,
};
use ralphx_lib::domain::services::RunningAgentKey;
use ralphx_lib::http_server::handlers::*;
use ralphx_lib::http_server::types::{ActiveStreamingTask, HttpServerState};
use std::sync::Arc;

fn cached_streaming_task(tool_use_id: &str) -> CachedStreamingTask {
    CachedStreamingTask {
        tool_use_id: tool_use_id.to_string(),
        description: None,
        subagent_type: None,
        model: None,
        status: "running".to_string(),
        agent_id: None,
        teammate_name: None,
        delegated_job_id: None,
        delegated_session_id: None,
        delegated_conversation_id: None,
        delegated_agent_run_id: None,
        provider_harness: None,
        provider_session_id: None,
        upstream_provider: None,
        provider_profile: None,
        logical_model: None,
        effective_model_id: None,
        logical_effort: None,
        effective_effort: None,
        approval_policy: None,
        sandbox_mode: None,
        total_tokens: None,
        total_tool_uses: None,
        duration_ms: None,
        input_tokens: None,
        output_tokens: None,
        cache_creation_tokens: None,
        cache_read_tokens: None,
        estimated_usd: None,
        text_output: None,
    }
}

async fn setup_test_state() -> HttpServerState {
    let app_state = Arc::new(AppState::new_test());
    let execution_state = Arc::new(ExecutionState::new());
    let tracker = TeamStateTracker::new();
    let team_service = Arc::new(TeamService::new_without_events(Arc::new(tracker.clone())));

    HttpServerState {
        app_state,
        execution_state,
        team_tracker: tracker,
        team_service,
        delegation_service: Default::default(),
    }
}

#[tokio::test]
async fn test_get_active_state_returns_not_found_for_nonexistent_conversation() {
    let state = setup_test_state().await;

    let result = get_conversation_active_state(
        State(state),
        Path("nonexistent-conversation-id".to_string()),
    )
    .await;

    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_get_active_state_returns_empty_for_inactive_conversation() {
    let state = setup_test_state().await;

    // Create a conversation using the context-specific constructor
    let task_id = TaskId::new();
    let conversation = ChatConversation::new_task(task_id.clone());
    let conversation_id = conversation.id.as_str().to_string();
    state
        .app_state
        .chat_conversation_repo
        .create(conversation)
        .await
        .unwrap();

    let response = get_conversation_active_state(State(state), Path(conversation_id.clone()))
        .await
        .unwrap();

    assert!(!response.0.is_active);
    assert!(response.0.tool_calls.is_empty());
    assert!(response.0.streaming_tasks.is_empty());
    assert!(response.0.partial_text.is_empty());
}

#[tokio::test]
async fn test_get_active_state_returns_cached_tool_calls() {
    let state = setup_test_state().await;

    // Create a conversation
    let task_id = TaskId::new();
    let conversation = ChatConversation::new_task(task_id.clone());
    let conversation_id = conversation.id.as_str().to_string();
    state
        .app_state
        .chat_conversation_repo
        .create(conversation)
        .await
        .unwrap();

    // Add tool call to cache
    let tool_call = CachedToolCall {
        id: "toolu_001".to_string(),
        name: "bash".to_string(),
        arguments: serde_json::json!({"command": "ls -la"}),
        result: Some(serde_json::json!({"output": "file1.txt"})),
        diff_context: None,
        parent_tool_use_id: None,
    };
    state
        .app_state
        .streaming_state_cache
        .upsert_tool_call(&conversation_id, tool_call)
        .await;

    let response = get_conversation_active_state(State(state), Path(conversation_id))
        .await
        .unwrap();

    assert!(!response.0.is_active); // No agent registered
    assert_eq!(response.0.tool_calls.len(), 1);
    assert_eq!(response.0.tool_calls[0].id, "toolu_001");
    assert_eq!(response.0.tool_calls[0].name, "bash");
    assert!(response.0.tool_calls[0].result.is_some());
}

#[tokio::test]
async fn test_get_active_state_returns_cached_streaming_tasks() {
    let state = setup_test_state().await;

    // Create a conversation
    let task_id = TaskId::new();
    let conversation = ChatConversation::new_task(task_id.clone());
    let conversation_id = conversation.id.as_str().to_string();
    state
        .app_state
        .chat_conversation_repo
        .create(conversation)
        .await
        .unwrap();

    // Add streaming task to cache
    let task = CachedStreamingTask {
        description: Some("Running tests".to_string()),
        subagent_type: Some("ralphx:coder".to_string()),
        model: Some("sonnet".to_string()),
        ..cached_streaming_task("toolu_002")
    };
    state
        .app_state
        .streaming_state_cache
        .add_task(&conversation_id, task)
        .await;

    let response = get_conversation_active_state(State(state), Path(conversation_id))
        .await
        .unwrap();

    assert_eq!(response.0.streaming_tasks.len(), 1);
    assert_eq!(response.0.streaming_tasks[0].tool_use_id, "toolu_002");
    assert_eq!(response.0.streaming_tasks[0].status, "running");
}

#[tokio::test]
async fn test_get_active_state_returns_partial_text() {
    let state = setup_test_state().await;

    // Create a conversation
    let session_id = IdeationSessionId::new();
    let conversation = ChatConversation::new_ideation(session_id.clone());
    let conversation_id = conversation.id.as_str().to_string();
    state
        .app_state
        .chat_conversation_repo
        .create(conversation)
        .await
        .unwrap();

    // Append text to cache
    state
        .app_state
        .streaming_state_cache
        .append_text(&conversation_id, "Hello ")
        .await;
    state
        .app_state
        .streaming_state_cache
        .append_text(&conversation_id, "world!")
        .await;

    let response = get_conversation_active_state(State(state), Path(conversation_id))
        .await
        .unwrap();

    assert_eq!(response.0.partial_text, "Hello world!");
}

#[tokio::test]
async fn test_get_active_state_reflects_running_agent() {
    let state = setup_test_state().await;

    // Create a conversation
    let task_id = TaskId::new();
    let conversation = ChatConversation::new_task(task_id.clone());
    let conversation_id = conversation.id.as_str().to_string();
    let context_id = conversation.context_id.clone();
    state
        .app_state
        .chat_conversation_repo
        .create(conversation)
        .await
        .unwrap();

    // Register a running agent
    let key = RunningAgentKey::new(ChatContextType::Task.to_string(), context_id);
    state
        .app_state
        .running_agent_registry
        .register(
            key,
            12345,
            conversation_id.clone(),
            "run-001".to_string(),
            None,
            None,
        )
        .await;

    let response = get_conversation_active_state(State(state), Path(conversation_id))
        .await
        .unwrap();

    assert!(response.0.is_active);
}

#[tokio::test]
async fn test_get_active_state_combines_all_data() {
    let state = setup_test_state().await;

    // Create a conversation
    let task_id = TaskId::new();
    let conversation = ChatConversation::new_task_execution(task_id.clone());
    let conversation_id = conversation.id.as_str().to_string();
    let context_id = conversation.context_id.clone();
    state
        .app_state
        .chat_conversation_repo
        .create(conversation)
        .await
        .unwrap();

    // Register running agent
    let key = RunningAgentKey::new(ChatContextType::TaskExecution.to_string(), context_id);
    state
        .app_state
        .running_agent_registry
        .register(
            key,
            99999,
            conversation_id.clone(),
            "run-combined".to_string(),
            None,
            None,
        )
        .await;

    // Add tool call
    let tool_call = CachedToolCall {
        id: "toolu_combined".to_string(),
        name: "read".to_string(),
        arguments: serde_json::json!({"file_path": "/tmp/test.txt"}),
        result: None,
        diff_context: None,
        parent_tool_use_id: None,
    };
    state
        .app_state
        .streaming_state_cache
        .upsert_tool_call(&conversation_id, tool_call)
        .await;

    // Add streaming task
    let task = CachedStreamingTask {
        description: Some("Reading file".to_string()),
        status: "completed".to_string(),
        ..cached_streaming_task("toolu_task")
    };
    state
        .app_state
        .streaming_state_cache
        .add_task(&conversation_id, task)
        .await;

    // Add partial text
    state
        .app_state
        .streaming_state_cache
        .append_text(&conversation_id, "Analyzing...")
        .await;

    let response = get_conversation_active_state(State(state), Path(conversation_id))
        .await
        .unwrap();

    assert!(response.0.is_active);
    assert_eq!(response.0.tool_calls.len(), 1);
    assert_eq!(response.0.streaming_tasks.len(), 1);
    assert_eq!(response.0.partial_text, "Analyzing...");
}

#[tokio::test]
async fn test_active_streaming_task_from_impl_forwards_stats() {
    let cached = CachedStreamingTask {
        description: Some("Test task".to_string()),
        subagent_type: Some("ralphx:coder".to_string()),
        model: Some("sonnet".to_string()),
        status: "completed".to_string(),
        total_tokens: Some(9876),
        total_tool_uses: Some(42),
        duration_ms: Some(60000),
        ..cached_streaming_task("toolu_stats_test")
    };

    let active = ActiveStreamingTask::from(cached);

    assert_eq!(active.tool_use_id, "toolu_stats_test");
    assert_eq!(active.status, "completed");
    assert_eq!(active.total_tokens, Some(9876));
    assert_eq!(active.total_tool_uses, Some(42));
    assert_eq!(active.duration_ms, Some(60000));
}

#[tokio::test]
async fn test_active_streaming_task_from_impl_handles_none_stats() {
    let cached = cached_streaming_task("toolu_no_stats");

    let active = ActiveStreamingTask::from(cached);

    assert_eq!(active.tool_use_id, "toolu_no_stats");
    assert_eq!(active.status, "running");
    assert!(active.total_tokens.is_none());
    assert!(active.total_tool_uses.is_none());
    assert!(active.duration_ms.is_none());
}

#[tokio::test]
async fn test_get_active_state_returns_delegated_metadata() {
    let state = setup_test_state().await;

    let task_id = TaskId::new();
    let conversation = ChatConversation::new_task_execution(task_id.clone());
    let conversation_id = conversation.id.as_str().to_string();
    state
        .app_state
        .chat_conversation_repo
        .create(conversation)
        .await
        .unwrap();

    state
        .app_state
        .streaming_state_cache
        .add_task(
            &conversation_id,
            CachedStreamingTask {
                description: Some("execution-reviewer".to_string()),
                subagent_type: Some("delegated".to_string()),
                model: Some("gpt-5.4".to_string()),
                status: "completed".to_string(),
                agent_id: Some("run-parent-child".to_string()),
                delegated_job_id: Some("job-123".to_string()),
                delegated_session_id: Some("delegated-session-123".to_string()),
                delegated_conversation_id: Some("conv-child-123".to_string()),
                delegated_agent_run_id: Some("run-child-123".to_string()),
                provider_harness: Some("codex".to_string()),
                provider_session_id: Some("provider-session-123".to_string()),
                upstream_provider: Some("openai".to_string()),
                provider_profile: Some("prod".to_string()),
                logical_model: Some("gpt-5.4".to_string()),
                effective_model_id: Some("gpt-5.4-2026-04-01".to_string()),
                logical_effort: Some("high".to_string()),
                effective_effort: Some("high".to_string()),
                approval_policy: Some("never".to_string()),
                sandbox_mode: Some("danger-full-access".to_string()),
                total_tokens: Some(120),
                total_tool_uses: Some(3),
                duration_ms: Some(4500),
                input_tokens: Some(10),
                output_tokens: Some(20),
                cache_creation_tokens: Some(30),
                cache_read_tokens: Some(40),
                estimated_usd: Some(0.12),
                text_output: Some("delegate done".to_string()),
                ..cached_streaming_task("toolu_delegate")
            },
        )
        .await;

    let response = get_conversation_active_state(State(state), Path(conversation_id))
        .await
        .unwrap();

    let task = &response.0.streaming_tasks[0];
    assert_eq!(task.delegated_job_id.as_deref(), Some("job-123"));
    assert_eq!(task.delegated_session_id.as_deref(), Some("delegated-session-123"));
    assert_eq!(task.delegated_conversation_id.as_deref(), Some("conv-child-123"));
    assert_eq!(task.provider_harness.as_deref(), Some("codex"));
    assert_eq!(task.upstream_provider.as_deref(), Some("openai"));
    assert_eq!(task.logical_model.as_deref(), Some("gpt-5.4"));
    assert_eq!(task.input_tokens, Some(10));
    assert_eq!(task.estimated_usd, Some(0.12));
    assert_eq!(task.text_output.as_deref(), Some("delegate done"));
}

#[tokio::test]
async fn test_get_active_state_reconciles_stale_delegated_running_task() {
    let state = setup_test_state().await;

    let parent_task_id = TaskId::new();
    let parent_conversation = ChatConversation::new_task_execution(parent_task_id);
    let parent_conversation_id = parent_conversation.id.as_str().to_string();
    state
        .app_state
        .chat_conversation_repo
        .create(parent_conversation)
        .await
        .unwrap();

    let mut delegated_session = DelegatedSession::new(
        ProjectId::new(),
        ChatContextType::TaskExecution.to_string(),
        "parent-context-id",
        "verification-critic",
        AgentHarnessKind::Codex,
    );
    delegated_session.status = "running".to_string();
    delegated_session.provider_session_id = Some("delegated-provider-session".to_string());
    state
        .app_state
        .delegated_session_repo
        .create(delegated_session.clone())
        .await
        .unwrap();

    let delegated_conversation = ChatConversation::new_delegation(delegated_session.id.clone());
    let delegated_conversation_id = delegated_conversation.id.as_str().to_string();
    state
        .app_state
        .chat_conversation_repo
        .create(delegated_conversation.clone())
        .await
        .unwrap();

    let mut delegated_run = AgentRun::new(delegated_conversation.id);
    delegated_run.complete();
    delegated_run.harness = Some(AgentHarnessKind::Codex);
    delegated_run.provider_session_id = Some("run-provider-session".to_string());
    delegated_run.upstream_provider = Some("openai".to_string());
    delegated_run.provider_profile = Some("prod".to_string());
    delegated_run.logical_model = Some("gpt-5.4".to_string());
    delegated_run.effective_model_id = Some("gpt-5.4-2026-04-01".to_string());
    delegated_run.approval_policy = Some("never".to_string());
    delegated_run.sandbox_mode = Some("danger-full-access".to_string());
    delegated_run.input_tokens = Some(11);
    delegated_run.output_tokens = Some(29);
    delegated_run.cache_creation_tokens = Some(7);
    delegated_run.cache_read_tokens = Some(13);
    delegated_run.estimated_usd = Some(0.42);
    delegated_run.completed_at = Some(Utc::now());
    let delegated_run_id = delegated_run.id.as_str();
    state
        .app_state
        .agent_run_repo
        .create(delegated_run)
        .await
        .unwrap();

    state
        .app_state
        .streaming_state_cache
        .add_task(
            &parent_conversation_id,
            CachedStreamingTask {
                description: Some("verification-critic".to_string()),
                subagent_type: Some("delegated".to_string()),
                model: Some("gpt-5.4".to_string()),
                status: "running".to_string(),
                delegated_session_id: Some(delegated_session.id.as_str().to_string()),
                delegated_conversation_id: Some(delegated_conversation_id),
                delegated_agent_run_id: Some(delegated_run_id.to_string()),
                ..cached_streaming_task("toolu_delegate_stale")
            },
        )
        .await;

    let response = get_conversation_active_state(State(state), Path(parent_conversation_id))
        .await
        .unwrap();

    let task = &response.0.streaming_tasks[0];
    assert_eq!(task.status, "completed");
    assert_eq!(task.provider_harness.as_deref(), Some("codex"));
    assert_eq!(task.provider_session_id.as_deref(), Some("run-provider-session"));
    assert_eq!(task.upstream_provider.as_deref(), Some("openai"));
    assert_eq!(task.provider_profile.as_deref(), Some("prod"));
    assert_eq!(task.logical_model.as_deref(), Some("gpt-5.4"));
    assert_eq!(
        task.effective_model_id.as_deref(),
        Some("gpt-5.4-2026-04-01")
    );
    assert_eq!(task.approval_policy.as_deref(), Some("never"));
    assert_eq!(task.sandbox_mode.as_deref(), Some("danger-full-access"));
    assert_eq!(task.input_tokens, Some(11));
    assert_eq!(task.output_tokens, Some(29));
    assert_eq!(task.cache_creation_tokens, Some(7));
    assert_eq!(task.cache_read_tokens, Some(13));
    assert_eq!(task.total_tokens, Some(60));
    assert_eq!(task.estimated_usd, Some(0.42));
}
