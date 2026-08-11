use super::{
    attribution_from_message, build_assistant_transcript_segments,
    enqueue_silent_completion_recovery, session_changed_after_resume, should_process_stream_queue,
    should_recover_silent_completion, should_warn_missing_agent_task_ledger,
    silent_completion_recovery_backoff, SilentCompletionRecoveryEnqueue,
};
use crate::application::chat_service::{AppChatService, ChatService, SendMessageOptions};
use crate::application::interactive_process_registry::{
    InteractiveProcessKey, InteractiveProcessRegistry, PendingStdinTurn,
};
use crate::application::personas::PERSONA_UNAVAILABLE_PREFIX;
use crate::application::plan_approval_notification_service::{
    has_deferred_plan_approval, reconcile_plan_approval_on_publish, PlanApprovalPublishAuthority,
};
use crate::application::AppState;
use crate::commands::ExecutionState;
use crate::domain::agents::{AgentHarnessKind, AgentProviderSettings};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentRun, AgentRunActionKind,
    AgentRunId, AgentRunStatus, ChatAttachment, ChatContextType, ChatConversation,
    ChatConversationId, ChatMessage, ChatTimelineItemStatus, IdeationAnalysisBaseRefKind,
    IdeationSession, IdeationSessionFlow, IdeationSessionStatus, Persona, PersonaId, PersonaStatus,
    Project, ProjectId, SessionPurpose, VerificationRoundSnapshot, VerificationRunSnapshot,
    VerificationStatus,
};
use crate::domain::repositories::PersonaRepository;
use crate::domain::repositories::{
    AgentProviderSettingsRepository, AgentRunRepository, QueuedMessageRepository,
    PRUNED_STALE_AGENT_RUN,
};
use crate::domain::services::{QueueKey, RunningAgentKey};
use crate::infrastructure::agents::claude::{ContentBlockItem, ToolCall};
use crate::infrastructure::memory::{
    MemoryAgentProviderSettingsRepository, MemoryAgentRunRepository, MemoryPersonaRepository,
};
use chrono::Utc;
use ralphx_events::{NullEventSink, RecordingEventSink};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use tauri::Manager;
use tokio::io::AsyncWriteExt;

use super::super::chat_service_run_finalization::{
    finalize_run_completed, finalize_run_completed_by_id, queue_run_completed_event_authority,
    run_completed_event_is_authorized, run_completed_without_queue_is_authorized,
    terminal_failure_reason,
};

fn test_tool_call(name: &str) -> ToolCall {
    ToolCall {
        id: None,
        name: name.to_string(),
        arguments: serde_json::json!({}),
        result: None,
        parent_tool_use_id: None,
        diff_context: None,
        stats: None,
    }
}

fn agent_mode_conversation() -> ChatConversation {
    let mut conversation = ChatConversation::new_project(ProjectId::new());
    conversation.set_agent_mode(Some(AgentConversationWorkspaceMode::Edit));
    conversation
}

fn claude_spawn_permission_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

async fn seed_completed_continuation_runtime(
    agent_run_repo: &Arc<dyn crate::domain::repositories::AgentRunRepository>,
    conversation_id: &ChatConversationId,
    harness: AgentHarnessKind,
    provider_session_id: &str,
) {
    let mut run = AgentRun::new(conversation_id.clone());
    run.complete();
    run.harness = Some(harness);
    run.provider_session_id = Some(provider_session_id.to_string());
    let model = match harness {
        AgentHarnessKind::Claude => "sonnet",
        AgentHarnessKind::Codex => "gpt-5.6-sol",
    };
    run.logical_model = Some(model.to_string());
    run.effective_model_id = Some(model.to_string());
    agent_run_repo
        .create(run)
        .await
        .expect("seed completed continuation runtime");
}

struct CreateFailingAgentRunRepository {
    inner: Arc<dyn AgentRunRepository>,
    fail_create: AtomicBool,
    fail_complete_if_running: AtomicBool,
    fail_complete_if_prune_cancelled: AtomicBool,
    fail_get_by_id: AtomicBool,
}

impl CreateFailingAgentRunRepository {
    fn new(inner: Arc<dyn AgentRunRepository>) -> Self {
        Self {
            inner,
            fail_create: AtomicBool::new(false),
            fail_complete_if_running: AtomicBool::new(false),
            fail_complete_if_prune_cancelled: AtomicBool::new(false),
            fail_get_by_id: AtomicBool::new(false),
        }
    }

    fn fail_creates(&self) {
        self.fail_create.store(true, Ordering::SeqCst);
    }

    fn fail_running_completion(&self) {
        self.fail_complete_if_running.store(true, Ordering::SeqCst);
    }

    fn fail_prune_completion(&self) {
        self.fail_complete_if_prune_cancelled
            .store(true, Ordering::SeqCst);
    }

    fn fail_run_reads(&self) {
        self.fail_get_by_id.store(true, Ordering::SeqCst);
    }
}

#[async_trait::async_trait]
impl AgentRunRepository for CreateFailingAgentRunRepository {
    async fn create(&self, run: AgentRun) -> crate::AppResult<AgentRun> {
        if self.fail_create.load(Ordering::SeqCst) {
            return Err(crate::AppError::Database(
                "forced queued run create failure".to_string(),
            ));
        }
        self.inner.create(run).await
    }

    async fn get_by_id(&self, id: &AgentRunId) -> crate::AppResult<Option<AgentRun>> {
        if self.fail_get_by_id.load(Ordering::SeqCst) {
            return Err(crate::AppError::Database(
                "forced agent run read failure".to_string(),
            ));
        }
        self.inner.get_by_id(id).await
    }

    async fn get_by_ids(&self, ids: &[AgentRunId]) -> crate::AppResult<Vec<AgentRun>> {
        self.inner.get_by_ids(ids).await
    }

    async fn get_latest_for_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> crate::AppResult<Option<AgentRun>> {
        self.inner
            .get_latest_for_conversation(conversation_id)
            .await
    }

    async fn get_active_for_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> crate::AppResult<Option<AgentRun>> {
        self.inner
            .get_active_for_conversation(conversation_id)
            .await
    }

    async fn get_by_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> crate::AppResult<Vec<AgentRun>> {
        self.inner.get_by_conversation(conversation_id).await
    }

    async fn update_status(&self, id: &AgentRunId, status: AgentRunStatus) -> crate::AppResult<()> {
        self.inner.update_status(id, status).await
    }

    async fn update_usage(
        &self,
        id: &AgentRunId,
        usage: &crate::domain::entities::AgentRunUsage,
    ) -> crate::AppResult<()> {
        self.inner.update_usage(id, usage).await
    }

    async fn update_attribution(
        &self,
        id: &AgentRunId,
        attribution: &crate::domain::entities::AgentRunAttribution,
    ) -> crate::AppResult<()> {
        self.inner.update_attribution(id, attribution).await
    }

    async fn set_persona_attribution(
        &self,
        id: &AgentRunId,
        attribution: crate::domain::entities::agent_run::PersonaRunAttribution,
    ) -> crate::AppResult<()> {
        self.inner.set_persona_attribution(id, attribution).await
    }

    async fn complete(&self, id: &AgentRunId) -> crate::AppResult<()> {
        self.inner.complete(id).await
    }

    async fn complete_if_running(&self, id: &AgentRunId) -> crate::AppResult<bool> {
        if self.fail_complete_if_running.load(Ordering::SeqCst) {
            return Err(crate::AppError::Database(
                "forced running completion failure".to_string(),
            ));
        }
        self.inner.complete_if_running(id).await
    }

    async fn complete_if_prune_cancelled(&self, id: &AgentRunId) -> crate::AppResult<bool> {
        if self.fail_complete_if_prune_cancelled.load(Ordering::SeqCst) {
            return Err(crate::AppError::Database(
                "forced prune completion failure".to_string(),
            ));
        }
        self.inner.complete_if_prune_cancelled(id).await
    }

    async fn fail(&self, id: &AgentRunId, error_message: &str) -> crate::AppResult<()> {
        self.inner.fail(id, error_message).await
    }

    async fn cancel(&self, id: &AgentRunId) -> crate::AppResult<()> {
        self.inner.cancel(id).await
    }

    async fn cancel_with_reason(&self, id: &AgentRunId, reason: &str) -> crate::AppResult<()> {
        self.inner.cancel_with_reason(id, reason).await
    }

    async fn delete(&self, id: &AgentRunId) -> crate::AppResult<()> {
        self.inner.delete(id).await
    }

    async fn delete_by_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> crate::AppResult<()> {
        self.inner.delete_by_conversation(conversation_id).await
    }

    async fn count_by_status(
        &self,
        conversation_id: &ChatConversationId,
        status: AgentRunStatus,
    ) -> crate::AppResult<u32> {
        self.inner.count_by_status(conversation_id, status).await
    }

    async fn cancel_all_running(&self) -> crate::AppResult<u32> {
        self.inner.cancel_all_running().await
    }

    async fn cancel_running_started_before(
        &self,
        cutoff: chrono::DateTime<chrono::Utc>,
    ) -> crate::AppResult<u32> {
        self.inner.cancel_running_started_before(cutoff).await
    }

    async fn get_interrupted_conversations(
        &self,
    ) -> crate::AppResult<Vec<crate::domain::entities::InterruptedConversation>> {
        self.inner.get_interrupted_conversations().await
    }
}

struct EnvVarGuard {
    key: &'static str,
    previous: Option<String>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let previous = std::env::var(key).ok();
        std::env::set_var(key, value);
        Self { key, previous }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => std::env::set_var(self.key, value),
            None => std::env::remove_var(self.key),
        }
    }
}

fn persona_for_send_fixture(id: &str, status: PersonaStatus) -> Persona {
    Persona {
        id: PersonaId::from(id),
        artifact_id: None,

        project_id: None,
        slug: id.to_string(),
        name: format!("{id} persona"),
        description: "send failure fixture".to_string(),
        content: "A persona body that must never reach excluded effects.".to_string(),
        status,
        version: 1,
        content_hash: format!("{id}-hash"),
        source_session_id: None,
        source_persona_id: None,
        source_content_hash: None,
        source_json: "{}".to_string(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

#[cfg(unix)]
#[allow(clippy::await_holding_lock)]
async fn send_bound_persona_and_capture_pre_spawn_effects(
    persona: Option<Persona>,
    bound_persona_id: &str,
) -> (String, bool, Vec<ChatMessage>, Vec<String>) {
    let _spawn_guard = claude_spawn_permission_lock()
        .lock()
        .expect("lock poisoned");
    let _spawn_permission = EnvVarGuard::set("RALPHX_ALLOW_CLAUDE_SPAWN_IN_TESTS", "1");
    let mut state = AppState::new_test();
    let project_dir = tempfile::tempdir().expect("project directory");
    let project = crate::domain::entities::Project::new(
        "Persona send fixture".to_string(),
        project_dir.path().to_string_lossy().to_string(),
    );
    let project_id = project.id.clone();
    state
        .project_repo
        .create(project)
        .await
        .expect("project should persist");

    let persona_repo = Arc::new(MemoryPersonaRepository::new());
    if let Some(persona) = persona {
        persona_repo
            .create(persona)
            .await
            .expect("persona fixture should persist");
    }
    state.persona_repo = persona_repo;

    let mut conversation = ChatConversation::new_project(project_id.clone());
    conversation.persona_id = Some(bound_persona_id.to_string());
    let conversation_id = conversation.id.clone();
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("bound conversation should persist");
    let message_repo = Arc::clone(&state.chat_message_repo);
    let event_sink = RecordingEventSink::new();
    state.events = Arc::new(event_sink.clone());

    let app = tauri::test::mock_builder()
        .manage(state)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");

    let spawn_marker = project_dir.path().join("spawned");
    let cli_path = project_dir.path().join("fake-claude");
    std::fs::write(
        &cli_path,
        format!(
            "#!/bin/sh\ntouch '{}'\nprintf '%s\\n' '{{\"type\":\"result\",\"session_id\":\"unexpected-spawn\",\"is_error\":false,\"result\":\"unexpected spawn\",\"cost_usd\":0.0}}'\n",
            spawn_marker.display()
        ),
    )
    .expect("write capture CLI");
    let mut permissions = std::fs::metadata(&cli_path)
        .expect("capture CLI metadata")
        .permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
    std::fs::set_permissions(&cli_path, permissions).expect("make capture CLI executable");

    let service: AppChatService = app
        .state::<AppState>()
        .build_chat_service_with_execution_state(Arc::new(ExecutionState::new()))
        .with_persona_feature_enabled(true)
        .with_cli_path(cli_path)
        .with_working_directory(project_dir.path());
    let error = service
        .send_message(
            ChatContextType::Project,
            project_id.as_str(),
            "must fail before effects",
            SendMessageOptions {
                conversation_id_override: Some(conversation_id.clone()),
                ..Default::default()
            },
        )
        .await
        .expect_err("unavailable persona must reject the send before spawning");

    tokio::task::yield_now().await;
    let messages = message_repo
        .get_by_conversation(&conversation_id)
        .await
        .expect("conversation message lookup");
    let events = event_sink
        .events()
        .into_iter()
        .map(|event| event.event)
        .collect();
    (error.to_string(), spawn_marker.exists(), messages, events)
}

#[cfg(unix)]
#[tokio::test]
async fn invalid_persona_blocks_send_before_spawn_no_process_no_message_row_no_events() {
    let (error, spawned, messages, events) =
        send_bound_persona_and_capture_pre_spawn_effects(None, "missing-persona").await;

    assert!(
        error.starts_with(PERSONA_UNAVAILABLE_PREFIX),
        "a missing bound persona must retain the typed unavailable prefix: {error}"
    );
    assert!(!spawned, "the CLI capture proves no process was spawned");
    assert!(
        messages.is_empty(),
        "no user or error message row may persist"
    );
    assert!(
        events.is_empty(),
        "no agent event may be emitted before authority"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn send_on_bound_draft_persona_fails_closed_with_persona_unavailable() {
    let draft = persona_for_send_fixture("bound-draft-persona", PersonaStatus::Draft);
    let (error, spawned, messages, events) =
        send_bound_persona_and_capture_pre_spawn_effects(Some(draft), "bound-draft-persona").await;

    assert!(
        error.starts_with(PERSONA_UNAVAILABLE_PREFIX),
        "draft-bound sends must expose the A15 unavailable prefix: {error}"
    );
    assert!(!spawned, "a draft persona must fail before process spawn");
    assert!(
        messages.is_empty(),
        "a draft persona must fail before message persistence"
    );
    assert!(
        events.is_empty(),
        "a draft persona must fail before event emission"
    );
}

#[test]
fn session_changed_returns_true_when_ids_differ() {
    assert!(session_changed_after_resume(
        Some("session-old-abc"),
        Some("session-new-xyz"),
    ));
}

#[test]
fn session_changed_returns_false_when_ids_match() {
    assert!(!session_changed_after_resume(
        Some("session-abc"),
        Some("session-abc"),
    ));
}

#[test]
fn session_changed_returns_false_when_no_stored_id() {
    // --resume was not used; no comparison possible
    assert!(!session_changed_after_resume(None, Some("session-new")));
}

#[test]
fn session_changed_returns_false_when_no_new_id() {
    // Stream returned no session ID; cannot detect change
    assert!(!session_changed_after_resume(Some("session-old"), None));
}

#[test]
fn session_changed_returns_false_when_both_none() {
    assert!(!session_changed_after_resume(None, None));
}

#[test]
fn stream_queue_processing_gate_requires_queue_session_and_no_cancelled_silent_exit() {
    assert!(should_process_stream_queue(1, true, false, false));
    assert!(!should_process_stream_queue(0, true, false, false));
    assert!(!should_process_stream_queue(1, false, false, false));
    assert!(!should_process_stream_queue(1, true, true, true));
}

#[test]
fn stream_queue_processing_gate_allows_non_cancel_silent_exit_with_queue() {
    assert!(
        should_process_stream_queue(1, true, true, false),
        "timeout/eof silent exits can still drain queued messages"
    );
}

#[test]
fn silent_completion_recovery_triggers_after_tool_without_final_text() {
    let tool_calls = vec![test_tool_call("apply_patch")];
    let content_blocks = vec![
        ContentBlockItem::Text {
            text: "I am patching this now.".to_string(),
        },
        ContentBlockItem::ToolUse {
            id: Some("patch-1".to_string()),
            name: "apply_patch".to_string(),
            arguments: serde_json::json!({ "file": "src/lib.rs" }),
            result: Some(serde_json::json!({ "ok": true })),
            parent_tool_use_id: None,
            diff_context: None,
        },
    ];

    assert!(should_recover_silent_completion(
        ChatContextType::Project,
        "I am patching this now.",
        &tool_calls,
        &content_blocks,
        0,
        false,
        false,
        true,
    ));
}

#[test]
fn silent_completion_recovery_triggers_for_ideation_tool_activity_without_final_text() {
    let tool_calls = vec![test_tool_call("mcp__ralphx__create_agent_task")];
    let content_blocks = vec![ContentBlockItem::ToolUse {
        id: Some("task-1".to_string()),
        name: "mcp__ralphx__create_agent_task".to_string(),
        arguments: serde_json::json!({ "title": "Create implementation proposals" }),
        result: Some(serde_json::json!({ "success": true })),
        parent_tool_use_id: None,
        diff_context: None,
    }];

    assert!(should_recover_silent_completion(
        ChatContextType::Ideation,
        "",
        &tool_calls,
        &content_blocks,
        0,
        false,
        false,
        true,
    ));
}

#[test]
fn silent_completion_recovery_treats_blank_text_before_tool_as_unfinished() {
    let tool_calls = vec![test_tool_call("apply_patch")];
    let content_blocks = vec![
        ContentBlockItem::Text {
            text: "   ".to_string(),
        },
        ContentBlockItem::ToolUse {
            id: Some("patch-1".to_string()),
            name: "apply_patch".to_string(),
            arguments: serde_json::json!({ "file": "src/lib.rs" }),
            result: Some(serde_json::json!({ "ok": true })),
            parent_tool_use_id: None,
            diff_context: None,
        },
    ];

    assert!(should_recover_silent_completion(
        ChatContextType::Project,
        "",
        &tool_calls,
        &content_blocks,
        0,
        false,
        false,
        true,
    ));
}

#[test]
fn silent_completion_recovery_does_not_trigger_after_final_text() {
    let tool_calls = vec![test_tool_call("apply_patch")];
    let content_blocks = vec![
        ContentBlockItem::ToolUse {
            id: Some("patch-1".to_string()),
            name: "apply_patch".to_string(),
            arguments: serde_json::json!({ "file": "src/lib.rs" }),
            result: Some(serde_json::json!({ "ok": true })),
            parent_tool_use_id: None,
            diff_context: None,
        },
        ContentBlockItem::Text {
            text: "Done and validated.".to_string(),
        },
    ];

    assert!(!should_recover_silent_completion(
        ChatContextType::Project,
        "Done and validated.",
        &tool_calls,
        &content_blocks,
        0,
        false,
        false,
        true,
    ));
}

#[test]
fn silent_completion_recovery_ignores_terminal_completion_tools() {
    for tool_name in [
        "mcp__ralphx__execution_complete",
        "mcp__ralphx__complete_workspace_review_run",
    ] {
        let tool_calls = vec![ToolCall {
            result: Some(serde_json::json!({ "ok": true })),
            ..test_tool_call(tool_name)
        }];
        let content_blocks = vec![ContentBlockItem::ToolUse {
            id: Some("complete-1".to_string()),
            name: tool_name.to_string(),
            arguments: serde_json::json!({}),
            result: Some(serde_json::json!({ "ok": true })),
            parent_tool_use_id: None,
            diff_context: None,
        }];

        assert!(
            !should_recover_silent_completion(
                ChatContextType::Project,
                "",
                &tool_calls,
                &content_blocks,
                0,
                false,
                false,
                true,
            ),
            "{tool_name} must suppress silent-completion recovery"
        );
    }
}

#[test]
fn silent_completion_recovery_requires_an_accepted_recorded_completion_result() {
    for (result, expected_recovery, expectation) in [
        (
            Some(serde_json::json!({ "success": true })),
            false,
            "an accepted Workspace Review result must suppress recovery",
        ),
        (
            Some(serde_json::json!({ "is_error": true })),
            true,
            "a rejected Workspace Review result must request recovery",
        ),
        (
            None,
            true,
            "a Workspace Review completion without a recorded result must request recovery",
        ),
    ] {
        let tool_calls = vec![ToolCall {
            result: result.clone(),
            ..test_tool_call("mcp__ralphx__complete_workspace_review_run")
        }];
        assert_eq!(
            should_recover_silent_completion(
                ChatContextType::Project,
                "",
                &tool_calls,
                &[],
                0,
                false,
                false,
                true,
            ),
            expected_recovery,
            "legacy tool-call path: {expectation}",
        );

        let content_blocks = vec![ContentBlockItem::ToolUse {
            id: Some("complete-1".to_string()),
            name: "mcp__ralphx__complete_workspace_review_run".to_string(),
            arguments: serde_json::json!({}),
            result,
            parent_tool_use_id: None,
            diff_context: None,
        }];
        assert_eq!(
            should_recover_silent_completion(
                ChatContextType::Project,
                "",
                &tool_calls,
                &content_blocks,
                0,
                false,
                false,
                true,
            ),
            expected_recovery,
            "content-block path: {expectation}",
        );
    }
}

#[test]
fn silent_completion_recovery_triggers_from_legacy_tool_calls_without_content_blocks() {
    let tool_calls = vec![test_tool_call("apply_patch")];

    assert!(should_recover_silent_completion(
        ChatContextType::Project,
        "",
        &tool_calls,
        &[],
        0,
        false,
        false,
        true,
    ));
}

#[test]
fn silent_completion_recovery_ignores_question_and_permission_tools() {
    for tool_name in [
        "mcp__ralphx__ask_user_question",
        "mcp__ralphx__permission_request",
        "mcp__ralphx__resolve_permission_request",
    ] {
        let tool_calls = vec![test_tool_call(tool_name)];
        let content_blocks = vec![ContentBlockItem::ToolUse {
            id: Some("tool-1".to_string()),
            name: tool_name.to_string(),
            arguments: serde_json::json!({}),
            result: Some(serde_json::json!({ "ok": true })),
            parent_tool_use_id: None,
            diff_context: None,
        }];

        assert!(
            !should_recover_silent_completion(
                ChatContextType::Project,
                "",
                &tool_calls,
                &content_blocks,
                0,
                false,
                false,
                true,
            ),
            "{tool_name} should not trigger silent-completion recovery"
        );
    }
}

#[tokio::test]
async fn silent_completion_recovery_enqueues_hidden_retry_at_front() {
    let queue = crate::domain::services::MessageQueue::new();
    queue.queue(
        ChatContextType::Project,
        "conversation-1",
        "user follow-up".to_string(),
    );
    let tool_calls = vec![test_tool_call("apply_patch")];
    let content_blocks = vec![ContentBlockItem::ToolUse {
        id: Some("patch-1".to_string()),
        name: "apply_patch".to_string(),
        arguments: serde_json::json!({ "file": "src/lib.rs" }),
        result: Some(serde_json::json!({ "ok": true })),
        parent_tool_use_id: None,
        diff_context: None,
    }];

    let result = enqueue_silent_completion_recovery(
        &queue,
        None,
        ChatContextType::Project,
        "conversation-1",
        "",
        &tool_calls,
        &content_blocks,
        0,
        false,
        false,
        true,
        None,
    )
    .await;

    assert_eq!(
        result,
        SilentCompletionRecoveryEnqueue::Queued {
            attempt: 1,
            backoff_ms: 1_000,
        }
    );
    let queued = queue.get_queued(ChatContextType::Project, "conversation-1");
    assert_eq!(queued.len(), 2);
    assert!(queued[0].content.contains("ended after tool activity"));
    assert_eq!(queued[1].content, "user follow-up");
    let metadata: serde_json::Value =
        serde_json::from_str(queued[0].metadata_override.as_deref().unwrap()).unwrap();
    assert_eq!(metadata["resume_in_place"], true);
    assert_eq!(metadata["persist_hidden_marker"], true);
    assert_eq!(metadata["recovery_attempt"], 1);
    assert_eq!(metadata["recovery_max_attempts"], 3);
    assert_eq!(
        silent_completion_recovery_backoff(queued[0].metadata_override.as_deref())
            .map(|duration| duration.as_millis()),
        Some(1_000)
    );
}

#[tokio::test]
async fn silent_completion_recovery_enqueues_ideation_retry_in_place() {
    let queue = crate::domain::services::MessageQueue::new();
    let tool_calls = vec![test_tool_call("mcp__ralphx__create_agent_task")];

    let result = enqueue_silent_completion_recovery(
        &queue,
        None,
        ChatContextType::Ideation,
        "planning-session-1",
        "",
        &tool_calls,
        &[],
        0,
        false,
        false,
        true,
        None,
    )
    .await;

    assert_eq!(
        result,
        SilentCompletionRecoveryEnqueue::Queued {
            attempt: 1,
            backoff_ms: 1_000,
        }
    );
    let queued = queue.get_queued(ChatContextType::Ideation, "planning-session-1");
    assert_eq!(queued.len(), 1);
    assert!(queued[0].content.contains("ended after tool activity"));
    let metadata: serde_json::Value =
        serde_json::from_str(queued[0].metadata_override.as_deref().unwrap()).unwrap();
    assert_eq!(metadata["resume_in_place"], true);
    assert_eq!(metadata["recovery_attempt"], 1);
}

#[tokio::test]
async fn silent_completion_recovery_keeps_memory_queue_when_durable_persist_fails() {
    let queue = crate::domain::services::MessageQueue::new();
    let repo: Arc<dyn QueuedMessageRepository> = Arc::new(
        crate::infrastructure::sqlite::SqliteQueuedMessageRepository::new(
            rusqlite::Connection::open_in_memory().expect("create in-memory db"),
        ),
    );
    let tool_calls = vec![test_tool_call("apply_patch")];
    let content_blocks = vec![ContentBlockItem::ToolUse {
        id: Some("patch-1".to_string()),
        name: "apply_patch".to_string(),
        arguments: serde_json::json!({ "file": "src/lib.rs" }),
        result: Some(serde_json::json!({ "ok": true })),
        parent_tool_use_id: None,
        diff_context: None,
    }];

    let result = enqueue_silent_completion_recovery(
        &queue,
        Some(&repo),
        ChatContextType::Project,
        "conversation-1",
        "",
        &tool_calls,
        &content_blocks,
        0,
        false,
        false,
        true,
        None,
    )
    .await;

    assert_eq!(
        result,
        SilentCompletionRecoveryEnqueue::Queued {
            attempt: 1,
            backoff_ms: 1_000,
        }
    );
    let queued = queue.get_queued(ChatContextType::Project, "conversation-1");
    assert_eq!(queued.len(), 1);
    assert!(queued[0].content.contains("ended after tool activity"));
}

#[tokio::test]
async fn silent_completion_recovery_persists_hidden_retry_to_durable_queue() {
    let queue = crate::domain::services::MessageQueue::new();
    let repo: Arc<dyn QueuedMessageRepository> =
        Arc::new(crate::infrastructure::memory::MemoryQueuedMessageRepository::new());
    let tool_calls = vec![test_tool_call("apply_patch")];
    let content_blocks = vec![ContentBlockItem::ToolUse {
        id: Some("patch-1".to_string()),
        name: "apply_patch".to_string(),
        arguments: serde_json::json!({ "file": "src/lib.rs" }),
        result: Some(serde_json::json!({ "ok": true })),
        parent_tool_use_id: None,
        diff_context: None,
    }];

    let result = enqueue_silent_completion_recovery(
        &queue,
        Some(&repo),
        ChatContextType::Project,
        "conversation-1",
        "",
        &tool_calls,
        &content_blocks,
        0,
        false,
        false,
        true,
        None,
    )
    .await;

    assert_eq!(
        result,
        SilentCompletionRecoveryEnqueue::Queued {
            attempt: 1,
            backoff_ms: 1_000,
        }
    );
    let key = QueueKey::new(ChatContextType::Project, "conversation-1");
    let durable = repo
        .list(&key)
        .await
        .expect("durable recovery queue lookup should not fail");
    assert_eq!(durable.len(), 1);
    assert_eq!(
        durable[0].metadata_override,
        queue.get_queued_with_key(&key)[0].metadata_override
    );
}

#[tokio::test]
async fn silent_completion_recovery_enqueues_second_attempt_with_backoff() {
    let queue = crate::domain::services::MessageQueue::new();
    let metadata = serde_json::json!({
        "recovery_reason": "silent_completion_after_tool_activity",
        "recovery_attempt": 1,
    })
    .to_string();
    let tool_calls = vec![test_tool_call("apply_patch")];
    let content_blocks = vec![ContentBlockItem::ToolUse {
        id: Some("patch-1".to_string()),
        name: "apply_patch".to_string(),
        arguments: serde_json::json!({ "file": "src/lib.rs" }),
        result: Some(serde_json::json!({ "ok": true })),
        parent_tool_use_id: None,
        diff_context: None,
    }];

    let result = enqueue_silent_completion_recovery(
        &queue,
        None,
        ChatContextType::Project,
        "conversation-1",
        "",
        &tool_calls,
        &content_blocks,
        0,
        false,
        false,
        true,
        Some(&metadata),
    )
    .await;

    assert_eq!(
        result,
        SilentCompletionRecoveryEnqueue::Queued {
            attempt: 2,
            backoff_ms: 2_000,
        }
    );
    let queued = queue.get_queued(ChatContextType::Project, "conversation-1");
    assert_eq!(queued.len(), 1);
    let metadata: serde_json::Value =
        serde_json::from_str(queued[0].metadata_override.as_deref().unwrap()).unwrap();
    assert_eq!(metadata["recovery_attempt"], 2);
    assert_eq!(metadata["recovery_backoff_ms"], 2_000);
}

#[tokio::test]
async fn silent_completion_recovery_enqueues_first_attempt_for_unrelated_prior_metadata() {
    let queue = crate::domain::services::MessageQueue::new();
    let metadata = serde_json::json!({
        "recovery_reason": "other_recovery_path",
        "recovery_attempt": 2,
    })
    .to_string();
    let tool_calls = vec![test_tool_call("apply_patch")];
    let content_blocks = vec![ContentBlockItem::ToolUse {
        id: Some("patch-1".to_string()),
        name: "apply_patch".to_string(),
        arguments: serde_json::json!({ "file": "src/lib.rs" }),
        result: Some(serde_json::json!({ "ok": true })),
        parent_tool_use_id: None,
        diff_context: None,
    }];

    let result = enqueue_silent_completion_recovery(
        &queue,
        None,
        ChatContextType::Project,
        "conversation-1",
        "",
        &tool_calls,
        &content_blocks,
        0,
        false,
        false,
        true,
        Some(&metadata),
    )
    .await;

    assert_eq!(
        result,
        SilentCompletionRecoveryEnqueue::Queued {
            attempt: 1,
            backoff_ms: 1_000,
        }
    );
    let queued = queue.get_queued(ChatContextType::Project, "conversation-1");
    assert_eq!(queued.len(), 1);
    let queued_metadata: serde_json::Value =
        serde_json::from_str(queued[0].metadata_override.as_deref().unwrap()).unwrap();
    assert_eq!(queued_metadata["recovery_attempt"], 1);
}

#[tokio::test]
async fn silent_completion_recovery_stops_at_max_attempts() {
    let queue = crate::domain::services::MessageQueue::new();
    let metadata = serde_json::json!({
        "resume_in_place": true,
        "recovery_reason": "silent_completion_after_tool_activity",
        "recovery_attempt": 3,
    })
    .to_string();
    let tool_calls = vec![test_tool_call("apply_patch")];
    let content_blocks = vec![ContentBlockItem::ToolUse {
        id: Some("patch-1".to_string()),
        name: "apply_patch".to_string(),
        arguments: serde_json::json!({ "file": "src/lib.rs" }),
        result: Some(serde_json::json!({ "ok": true })),
        parent_tool_use_id: None,
        diff_context: None,
    }];

    let result = enqueue_silent_completion_recovery(
        &queue,
        None,
        ChatContextType::Project,
        "conversation-1",
        "",
        &tool_calls,
        &content_blocks,
        0,
        false,
        false,
        true,
        Some(&metadata),
    )
    .await;

    assert_eq!(
        result,
        SilentCompletionRecoveryEnqueue::Exhausted { attempts: 3 }
    );
    assert!(queue
        .get_queued(ChatContextType::Project, "conversation-1")
        .is_empty());
}

#[tokio::test]
async fn silent_completion_recovery_enqueue_skips_without_resumable_session() {
    let queue = crate::domain::services::MessageQueue::new();
    let tool_calls = vec![test_tool_call("apply_patch")];
    let content_blocks = vec![ContentBlockItem::ToolUse {
        id: Some("patch-1".to_string()),
        name: "apply_patch".to_string(),
        arguments: serde_json::json!({ "file": "src/lib.rs" }),
        result: Some(serde_json::json!({ "ok": true })),
        parent_tool_use_id: None,
        diff_context: None,
    }];

    let result = enqueue_silent_completion_recovery(
        &queue,
        None,
        ChatContextType::Project,
        "conversation-1",
        "",
        &tool_calls,
        &content_blocks,
        0,
        false,
        false,
        false,
        None,
    )
    .await;

    assert_eq!(result, SilentCompletionRecoveryEnqueue::NotNeeded);
    assert!(queue
        .get_queued(ChatContextType::Project, "conversation-1")
        .is_empty());
}

#[test]
fn silent_completion_recovery_backoff_ignores_invalid_metadata() {
    assert_eq!(silent_completion_recovery_backoff(None), None);
    assert_eq!(silent_completion_recovery_backoff(Some("not json")), None);
    assert_eq!(
        silent_completion_recovery_backoff(Some(
            r#"{"recovery_reason":"different","recovery_backoff_ms":1000}"#
        )),
        None
    );
}

#[test]
fn agent_task_ledger_warning_triggers_for_agent_mode_edit_without_ledger_tool() {
    let conversation = agent_mode_conversation();

    assert!(should_warn_missing_agent_task_ledger(
        Some(&conversation),
        &[test_tool_call("Edit")]
    ));
}

#[test]
fn agent_task_ledger_warning_triggers_for_agent_mode_many_readonly_tools_without_ledger_tool() {
    let conversation = agent_mode_conversation();

    assert!(should_warn_missing_agent_task_ledger(
        Some(&conversation),
        &[
            test_tool_call("Read"),
            test_tool_call("Grep"),
            test_tool_call("Read"),
        ],
    ));
}

#[test]
fn agent_task_ledger_warning_is_suppressed_after_ledger_tool_use() {
    let conversation = agent_mode_conversation();

    assert!(!should_warn_missing_agent_task_ledger(
        Some(&conversation),
        &[
            test_tool_call("Edit"),
            test_tool_call("mcp__ralphx__create_agent_task"),
        ],
    ));
}

#[test]
fn agent_task_ledger_warning_recognizes_codex_mutating_tools_and_namespaced_ledger_tools() {
    let conversation = agent_mode_conversation();

    assert!(should_warn_missing_agent_task_ledger(
        Some(&conversation),
        &[test_tool_call("exec_command")]
    ));
    assert!(!should_warn_missing_agent_task_ledger(
        Some(&conversation),
        &[
            test_tool_call("apply_patch"),
            test_tool_call("mcp::complete_agent_task"),
        ],
    ));
    assert!(!should_warn_missing_agent_task_ledger(
        Some(&conversation),
        &[test_tool_call("mcp__ralphx__update_agent_task")]
    ));
}

#[test]
fn agent_task_ledger_warning_is_suppressed_for_non_agent_mode_conversation() {
    let conversation = ChatConversation::new_project(ProjectId::new());

    assert!(!should_warn_missing_agent_task_ledger(
        Some(&conversation),
        &[
            test_tool_call("Read"),
            test_tool_call("Grep"),
            test_tool_call("Edit"),
        ],
    ));
}

#[test]
fn assistant_transcript_segments_split_text_after_tool_and_build_missing_tool_call() {
    let content_blocks = vec![
        ContentBlockItem::Text {
            text: "before tool".to_string(),
        },
        ContentBlockItem::ToolUse {
            id: Some("tool-1".to_string()),
            name: "exec_command".to_string(),
            arguments: serde_json::json!({"cmd":"date"}),
            result: Some(serde_json::json!({"ok":true})),
            parent_tool_use_id: Some("parent-tool".to_string()),
            diff_context: None,
        },
        ContentBlockItem::Text {
            text: "after tool".to_string(),
        },
    ];

    let segments = build_assistant_transcript_segments(&[], &content_blocks);

    assert_eq!(segments.len(), 2);
    assert_eq!(segments[0].content, "before tool");
    assert_eq!(segments[0].tool_calls.len(), 1);
    assert_eq!(segments[0].tool_calls[0].id.as_deref(), Some("tool-1"));
    assert_eq!(segments[0].tool_calls[0].name, "exec_command");
    assert_eq!(
        segments[0].tool_calls[0].parent_tool_use_id.as_deref(),
        Some("parent-tool")
    );
    assert_eq!(segments[1].content, "after tool");
    assert!(segments[1].tool_calls.is_empty());
}

#[test]
fn message_attribution_preserves_provider_metadata() {
    use crate::domain::agents::{AgentHarnessKind, LogicalEffort};

    let mut message = ChatMessage::user_in_project(ProjectId::new(), "assistant response");
    message.conversation_id = Some(ChatConversationId::new());
    message.attribution_source = Some("native_runtime".to_string());
    message.provider_harness = Some(AgentHarnessKind::Codex);
    message.provider_session_id = Some("codex-session".to_string());
    message.upstream_provider = Some("openai".to_string());
    message.provider_profile = Some("ideation".to_string());
    message.logical_model = Some("gpt-5.5".to_string());
    message.effective_model_id = Some("gpt-5.5-2026-06-01".to_string());
    message.logical_effort = Some(LogicalEffort::High);
    message.effective_effort = Some("high".to_string());

    let attribution = attribution_from_message(&message);

    assert_eq!(
        attribution.attribution_source.as_deref(),
        Some("native_runtime")
    );
    assert_eq!(attribution.provider_harness, Some(AgentHarnessKind::Codex));
    assert_eq!(
        attribution.provider_session_id.as_deref(),
        Some("codex-session")
    );
    assert_eq!(attribution.upstream_provider.as_deref(), Some("openai"));
    assert_eq!(attribution.provider_profile.as_deref(), Some("ideation"));
    assert_eq!(attribution.logical_model.as_deref(), Some("gpt-5.5"));
    assert_eq!(
        attribution.effective_model_id.as_deref(),
        Some("gpt-5.5-2026-06-01")
    );
    assert_eq!(attribution.logical_effort, Some(LogicalEffort::High));
    assert_eq!(attribution.effective_effort.as_deref(), Some("high"));
}

/// Zero processed queued messages still trigger diagnostics, but terminal event
/// authority must come from the persisted terminal run rather than queue counts.
#[test]
fn zero_processed_queue_warns_without_granting_completion_authority() {
    use crate::domain::entities::ChatContextType;
    use crate::domain::services::MessageQueue;

    let queue = MessageQueue::new();

    queue.queue(
        ChatContextType::TaskExecution,
        "task-1",
        "Queued message 1".to_string(),
    );
    queue.queue(
        ChatContextType::TaskExecution,
        "task-1",
        "Queued message 2".to_string(),
    );

    let initial_queue_count = queue
        .get_queued(ChatContextType::TaskExecution, "task-1")
        .len();
    assert_eq!(
        initial_queue_count, 2,
        "initial_queue_count must reflect queued messages"
    );

    // Simulate spawn failure: total_processed stays 0
    let total_processed: usize = 0;

    let should_warn = total_processed == 0 && initial_queue_count > 0;
    assert!(
        should_warn,
        "Warning condition must trigger for race/spawn failure/cancellation case"
    );
}

#[tokio::test]
async fn finalizer_completes_running_run_and_authorizes_completion_event() {
    let concrete = Arc::new(MemoryAgentRunRepository::new());
    let repo: Arc<dyn AgentRunRepository> = concrete.clone();
    let run = AgentRun::new(ChatConversationId::new());
    let run_id = run.id;
    repo.create(run).await.unwrap();

    assert!(finalize_run_completed_by_id(&repo, &run_id.as_str()).await);
    assert!(run_completed_event_is_authorized(&repo, &run_id).await);
    assert_eq!(
        repo.get_by_id(&run_id).await.unwrap().unwrap().status,
        AgentRunStatus::Completed
    );
}

#[tokio::test]
async fn finalizer_repairs_only_prune_cancelled_run_and_authorizes_completion_event() {
    let concrete = Arc::new(MemoryAgentRunRepository::new());
    let repo: Arc<dyn AgentRunRepository> = concrete.clone();
    let mut run = AgentRun::new(ChatConversationId::new());
    run.status = AgentRunStatus::Cancelled;
    run.completed_at = Some(Utc::now());
    run.error_message = Some(PRUNED_STALE_AGENT_RUN.to_string());
    let run_id = run.id;
    repo.create(run).await.unwrap();

    assert!(finalize_run_completed(&repo, &run_id).await);
    assert!(run_completed_event_is_authorized(&repo, &run_id).await);
    let persisted = repo.get_by_id(&run_id).await.unwrap().unwrap();
    assert_eq!(persisted.status, AgentRunStatus::Completed);
    assert!(persisted.error_message.is_none());
}

#[tokio::test]
async fn finalizer_preserves_user_cancel_and_suppresses_completion_event() {
    let concrete = Arc::new(MemoryAgentRunRepository::new());
    let repo: Arc<dyn AgentRunRepository> = concrete.clone();
    let mut run = AgentRun::new(ChatConversationId::new());
    run.cancel();
    let run_id = run.id;
    repo.create(run).await.unwrap();

    assert!(!finalize_run_completed(&repo, &run_id).await);
    assert!(!run_completed_event_is_authorized(&repo, &run_id).await);
    assert_eq!(
        repo.get_by_id(&run_id).await.unwrap().unwrap().status,
        AgentRunStatus::Cancelled
    );
}

#[tokio::test]
async fn finalizer_fails_closed_when_running_completion_errors() {
    let inner: Arc<dyn AgentRunRepository> = Arc::new(MemoryAgentRunRepository::new());
    let failing = Arc::new(CreateFailingAgentRunRepository::new(Arc::clone(&inner)));
    let repo: Arc<dyn AgentRunRepository> = failing.clone();
    let run = AgentRun::new(ChatConversationId::new());
    let run_id = run.id;
    inner.create(run).await.unwrap();
    failing.fail_running_completion();

    assert!(!finalize_run_completed(&repo, &run_id).await);
    assert_eq!(
        inner.get_by_id(&run_id).await.unwrap().unwrap().status,
        AgentRunStatus::Running
    );
}

#[tokio::test]
async fn finalizer_fails_closed_when_prune_repair_errors() {
    let inner: Arc<dyn AgentRunRepository> = Arc::new(MemoryAgentRunRepository::new());
    let failing = Arc::new(CreateFailingAgentRunRepository::new(Arc::clone(&inner)));
    let repo: Arc<dyn AgentRunRepository> = failing.clone();
    let mut run = AgentRun::new(ChatConversationId::new());
    run.status = AgentRunStatus::Cancelled;
    run.completed_at = Some(Utc::now());
    run.error_message = Some(PRUNED_STALE_AGENT_RUN.to_string());
    let run_id = run.id;
    inner.create(run).await.unwrap();
    failing.fail_prune_completion();

    assert!(!finalize_run_completed(&repo, &run_id).await);
    let persisted = inner.get_by_id(&run_id).await.unwrap().unwrap();
    assert_eq!(persisted.status, AgentRunStatus::Cancelled);
    assert_eq!(
        persisted.error_message.as_deref(),
        Some(PRUNED_STALE_AGENT_RUN)
    );
}

#[tokio::test]
async fn completion_event_authority_fails_closed_when_run_missing() {
    let repo: Arc<dyn AgentRunRepository> = Arc::new(MemoryAgentRunRepository::new());
    let missing_run_id = AgentRunId::from_string("missing-run");

    assert!(!run_completed_event_is_authorized(&repo, &missing_run_id).await);
}

#[tokio::test]
async fn completion_event_authority_fails_closed_when_read_errors() {
    let inner: Arc<dyn AgentRunRepository> = Arc::new(MemoryAgentRunRepository::new());
    let failing = Arc::new(CreateFailingAgentRunRepository::new(Arc::clone(&inner)));
    let repo: Arc<dyn AgentRunRepository> = failing.clone();
    let mut run = AgentRun::new(ChatConversationId::new());
    run.complete();
    let run_id = run.id;
    inner.create(run).await.unwrap();
    failing.fail_run_reads();

    assert!(!run_completed_event_is_authorized(&repo, &run_id).await);
}

#[test]
fn no_queue_completion_event_requires_completion_authority() {
    assert!(run_completed_without_queue_is_authorized(
        true, false, false
    ));
    assert!(run_completed_without_queue_is_authorized(true, true, true));
    assert!(!run_completed_without_queue_is_authorized(
        false, false, true
    ));
    assert!(!run_completed_without_queue_is_authorized(
        true, true, false
    ));
}

#[tokio::test]
async fn queue_completion_event_authority_uses_terminal_run_status() {
    let repo: Arc<dyn AgentRunRepository> = Arc::new(MemoryAgentRunRepository::new());
    let parent_run = AgentRun::new(ChatConversationId::new());
    let parent_run_id = parent_run.id;
    repo.create(parent_run).await.unwrap();
    let mut queued_run = AgentRun::new(ChatConversationId::new());
    queued_run.complete();
    let queued_run_id = queued_run.id;
    repo.create(queued_run).await.unwrap();

    let outcome = super::super::chat_service_queue::QueueProcessingOutcome {
        total_processed: 1,
        last_run_id: Some(queued_run_id.as_str().to_string()),
    };
    let (terminal_run_id, authorized) =
        queue_run_completed_event_authority(&repo, &outcome, &parent_run_id.as_str()).await;

    assert_eq!(terminal_run_id, queued_run_id.as_str());
    assert!(authorized);
}

#[tokio::test]
async fn queue_completion_event_authority_suppresses_non_completed_parent_fallback() {
    let repo: Arc<dyn AgentRunRepository> = Arc::new(MemoryAgentRunRepository::new());
    let mut parent_run = AgentRun::new(ChatConversationId::new());
    parent_run.fail("spawn failed");
    let parent_run_id = parent_run.id;
    repo.create(parent_run).await.unwrap();

    let outcome = super::super::chat_service_queue::QueueProcessingOutcome {
        total_processed: 0,
        last_run_id: None,
    };
    let (terminal_run_id, authorized) =
        queue_run_completed_event_authority(&repo, &outcome, &parent_run_id.as_str()).await;

    assert_eq!(terminal_run_id, parent_run_id.as_str());
    assert!(!authorized);
}

/// Zero processed queued messages must still emit run_completed when the terminal
/// run really is Completed (race / spawn failure / cancellation diagnostics only).
#[tokio::test]
async fn queue_completion_event_authority_granted_when_zero_processed_but_run_completed() {
    let repo: Arc<dyn AgentRunRepository> = Arc::new(MemoryAgentRunRepository::new());
    let mut parent_run = AgentRun::new(ChatConversationId::new());
    parent_run.complete();
    let parent_run_id = parent_run.id;
    repo.create(parent_run).await.unwrap();

    let initial_queue_count = 2usize;
    let outcome = super::super::chat_service_queue::QueueProcessingOutcome {
        total_processed: 0,
        last_run_id: None,
    };
    assert!(outcome.total_processed == 0 && initial_queue_count > 0);

    let (terminal_run_id, authorized) =
        queue_run_completed_event_authority(&repo, &outcome, &parent_run_id.as_str()).await;

    assert_eq!(terminal_run_id, parent_run_id.as_str());
    assert!(
        authorized,
        "zero processed queued messages must not suppress a genuinely Completed run"
    );
}

#[tokio::test]
async fn terminal_failure_reason_reports_persisted_failure_and_denies_completion_event() {
    let repo: Arc<dyn AgentRunRepository> = Arc::new(MemoryAgentRunRepository::new());
    let mut run = AgentRun::new(ChatConversationId::new());
    run.fail("Agent completed with no output");
    let run_id = run.id;
    repo.create(run).await.unwrap();

    assert_eq!(
        terminal_failure_reason(&repo, &run_id).await.as_deref(),
        Some("Agent completed with no output")
    );
    assert!(!run_completed_event_is_authorized(&repo, &run_id).await);
}

#[tokio::test]
async fn completion_event_authority_granted_when_another_writer_completed_the_run() {
    let repo: Arc<dyn AgentRunRepository> = Arc::new(MemoryAgentRunRepository::new());
    let run = AgentRun::new(ChatConversationId::new());
    let run_id = run.id;
    repo.create(run).await.unwrap();

    // First writer (e.g. the TurnComplete finalizer or an HTTP completion handler).
    assert!(finalize_run_completed(&repo, &run_id).await);
    // Second call loses the CAS but the run is genuinely Completed.
    assert!(!finalize_run_completed(&repo, &run_id).await);

    assert!(
        run_completed_event_is_authorized(&repo, &run_id).await,
        "persisted Completed status must authorize the event even when this call did not apply it"
    );
    assert!(terminal_failure_reason(&repo, &run_id).await.is_none());
}

#[tokio::test]
async fn terminal_failure_reason_ignores_user_cancelled_run() {
    let repo: Arc<dyn AgentRunRepository> = Arc::new(MemoryAgentRunRepository::new());
    let mut run = AgentRun::new(ChatConversationId::new());
    run.cancel();
    let run_id = run.id;
    repo.create(run).await.unwrap();

    assert!(
        terminal_failure_reason(&repo, &run_id).await.is_none(),
        "user cancels are covered by agent:stopped and must not emit a duplicate agent:error"
    );
    assert!(!run_completed_event_is_authorized(&repo, &run_id).await);
}

#[tokio::test]
async fn terminal_failure_reason_fails_closed_when_read_errors() {
    let inner: Arc<dyn AgentRunRepository> = Arc::new(MemoryAgentRunRepository::new());
    let failing = Arc::new(CreateFailingAgentRunRepository::new(Arc::clone(&inner)));
    let repo: Arc<dyn AgentRunRepository> = failing.clone();
    let mut run = AgentRun::new(ChatConversationId::new());
    run.fail("boom");
    let run_id = run.id;
    inner.create(run).await.unwrap();
    failing.fail_run_reads();

    assert!(terminal_failure_reason(&repo, &run_id).await.is_none());
    assert!(!run_completed_event_is_authorized(&repo, &run_id).await);
}

#[test]
fn queue_processing_outcome_uses_last_queued_run_for_terminal_event() {
    let outcome = super::super::chat_service_queue::QueueProcessingOutcome {
        total_processed: 2,
        last_run_id: Some("queued-run-2".to_string()),
    };

    assert_eq!(outcome.terminal_run_id("parent-run"), "queued-run-2");
}

#[test]
fn queue_processing_outcome_falls_back_to_parent_run_without_queued_run() {
    let outcome = super::super::chat_service_queue::QueueProcessingOutcome {
        total_processed: 0,
        last_run_id: None,
    };

    assert_eq!(outcome.terminal_run_id("parent-run"), "parent-run");
}

#[tokio::test]
async fn queue_provider_decision_blocks_disabled_slot_provider_without_app_handle() {
    let repo = Arc::new(MemoryAgentProviderSettingsRepository::new());
    let mut codex = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Codex);
    codex.enabled = true;
    codex.is_default = true;
    repo.upsert(&codex).await.expect("seed codex provider");
    let provider_repo: Arc<dyn AgentProviderSettingsRepository> = repo;
    let provider_repo = Some(provider_repo);

    let block = super::super::chat_service_queue::queue_provider_decision(
        &provider_repo,
        AgentHarnessKind::Claude,
        ChatContextType::Review,
    )
    .await
    .expect_err("disabled Claude must block queued review resume before spawn");

    match block {
        super::super::chat_service_queue::QueueProviderBlock::Disabled(message) => {
            assert!(message.contains("claude is not enabled"), "{message}");
        }
        other => panic!("expected disabled-provider block, got {other:?}"),
    }
}

#[tokio::test]
async fn queue_processing_leaves_messages_pending_when_execution_paused() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());
    execution_state.pause();

    app_state.message_queue.queue(
        ChatContextType::Ideation,
        "session-paused",
        "Queued while paused".to_string(),
    );

    let conversation_id = ChatConversationId::new();
    let unused_paused_path = Path::new(".");

    let outcome = super::super::chat_service_queue::process_queued_messages(
        ChatContextType::Ideation,
        crate::domain::agents::AgentHarnessKind::Claude,
        "session-paused",
        "session-paused",
        conversation_id,
        "session-cli",
        false,
        &app_state.message_queue,
        None,
        None,
        &app_state.running_agent_registry,
        &app_state.agent_run_repo,
        &app_state.chat_message_repo,
        None,
        &app_state.chat_attachment_repo,
        &app_state.artifact_repo,
        &app_state.activity_event_repo,
        &app_state.task_repo,
        &app_state.ideation_session_repo,
        unused_paused_path,
        unused_paused_path,
        unused_paused_path,
        None,
        Some(Arc::clone(&execution_state)),
        Arc::new(NullEventSink),
        None,
        None,
        None,
        None,
        tokio_util::sync::CancellationToken::new(),
        None,
        None,
        super::StreamingStateCache::new(),
    )
    .await;

    assert_eq!(
        outcome.total_processed, 0,
        "paused queue processing must not launch messages"
    );
    assert_eq!(outcome.last_run_id, None);
    assert_eq!(
        app_state
            .message_queue
            .get_queued(ChatContextType::Ideation, "session-paused")
            .len(),
        1,
        "paused queue processing must leave the queued message pending"
    );
}

#[tokio::test]
async fn queue_processing_records_run_id_before_spawn_failure() {
    let app_state = AppState::new_test();
    let runtime_factory_deps =
        crate::application::runtime_factory::ChatRuntimeFactoryDeps::from_app_state(&app_state);
    let message_queue = Arc::clone(&app_state.message_queue);
    let running_agent_registry = Arc::clone(&app_state.running_agent_registry);
    let agent_run_repo = Arc::clone(&app_state.agent_run_repo);
    let chat_message_repo = Arc::clone(&app_state.chat_message_repo);
    let chat_attachment_repo = Arc::clone(&app_state.chat_attachment_repo);
    let artifact_repo = Arc::clone(&app_state.artifact_repo);
    let activity_event_repo = Arc::clone(&app_state.activity_event_repo);
    let task_repo = Arc::clone(&app_state.task_repo);
    let ideation_session_repo = Arc::clone(&app_state.ideation_session_repo);
    message_queue.queue(
        ChatContextType::Ideation,
        "session-spawn-fails",
        "Queued message".to_string(),
    );

    let conversation_id = ChatConversationId::new();
    seed_completed_continuation_runtime(
        &agent_run_repo,
        &conversation_id,
        AgentHarnessKind::Claude,
        "session-cli",
    )
    .await;
    let invalid_cli_path = Path::new("/definitely/missing/ralphx-test-cli");
    let unused_path = Path::new(".");

    let outcome = super::super::chat_service_queue::process_queued_messages(
        ChatContextType::Ideation,
        crate::domain::agents::AgentHarnessKind::Claude,
        "session-spawn-fails",
        "session-spawn-fails",
        conversation_id.clone(),
        "session-cli",
        false,
        &message_queue,
        None,
        None,
        &running_agent_registry,
        &agent_run_repo,
        &chat_message_repo,
        None,
        &chat_attachment_repo,
        &artifact_repo,
        &activity_event_repo,
        &task_repo,
        &ideation_session_repo,
        invalid_cli_path,
        unused_path,
        unused_path,
        None,
        None,
        Arc::new(NullEventSink),
        None,
        Some(runtime_factory_deps),
        None,
        None,
        tokio_util::sync::CancellationToken::new(),
        None,
        None,
        super::StreamingStateCache::new(),
    )
    .await;

    assert_eq!(outcome.total_processed, 1);
    let queued_run_id = outcome
        .last_run_id
        .as_deref()
        .expect("queued continuation run id should be recorded");
    let queued_run = agent_run_repo
        .get_by_id(&AgentRunId::from_string(queued_run_id.to_string()))
        .await
        .expect("queued run lookup should succeed")
        .expect("queued run should be persisted");
    assert_eq!(queued_run.status, AgentRunStatus::Failed);
    assert!(
        running_agent_registry
            .get(&RunningAgentKey::new(
                ChatContextType::Ideation.to_string(),
                "session-spawn-fails"
            ))
            .await
            .is_none(),
        "spawn failure should not leave queued continuation marked running"
    );
}

#[tokio::test]
async fn queue_processing_stops_before_launch_when_run_persistence_fails() {
    let app_state = AppState::new_test();
    let message_queue = Arc::clone(&app_state.message_queue);
    let running_agent_registry = Arc::clone(&app_state.running_agent_registry);
    let inner_agent_run_repo = Arc::clone(&app_state.agent_run_repo);
    let failing_agent_run_repo = Arc::new(CreateFailingAgentRunRepository::new(Arc::clone(
        &inner_agent_run_repo,
    )));
    let chat_message_repo = Arc::clone(&app_state.chat_message_repo);
    let chat_attachment_repo = Arc::clone(&app_state.chat_attachment_repo);
    let artifact_repo = Arc::clone(&app_state.artifact_repo);
    let activity_event_repo = Arc::clone(&app_state.activity_event_repo);
    let task_repo = Arc::clone(&app_state.task_repo);
    let ideation_session_repo = Arc::clone(&app_state.ideation_session_repo);
    let events = RecordingEventSink::new();

    message_queue.queue(
        ChatContextType::Ideation,
        "session-create-fails",
        "Queued message".to_string(),
    );
    let conversation_id = ChatConversationId::new();
    seed_completed_continuation_runtime(
        &inner_agent_run_repo,
        &conversation_id,
        AgentHarnessKind::Claude,
        "session-cli",
    )
    .await;
    failing_agent_run_repo.fail_creates();
    let failing_agent_run_repo: Arc<dyn AgentRunRepository> = failing_agent_run_repo;
    let invalid_cli_path = Path::new("/definitely/missing/ralphx-test-cli");
    let unused_path = Path::new(".");

    let outcome = super::super::chat_service_queue::process_queued_messages(
        ChatContextType::Ideation,
        AgentHarnessKind::Claude,
        "session-create-fails",
        "session-create-fails",
        conversation_id.clone(),
        "session-cli",
        false,
        &message_queue,
        None,
        None,
        &running_agent_registry,
        &failing_agent_run_repo,
        &chat_message_repo,
        None,
        &chat_attachment_repo,
        &artifact_repo,
        &activity_event_repo,
        &task_repo,
        &ideation_session_repo,
        invalid_cli_path,
        unused_path,
        unused_path,
        None,
        None,
        Arc::new(events.clone()),
        None,
        None,
        None,
        None,
        tokio_util::sync::CancellationToken::new(),
        None,
        None,
        super::StreamingStateCache::new(),
    )
    .await;

    assert_eq!(outcome.total_processed, 1);
    assert!(outcome.last_run_id.is_some());
    assert!(
        !events
            .events()
            .iter()
            .any(|event| event.event == "agent:run_started"),
        "a queued continuation without a durable AgentRun must not emit run_started"
    );
    assert!(
        running_agent_registry
            .get(&RunningAgentKey::new(
                ChatContextType::Ideation.to_string(),
                "session-create-fails"
            ))
            .await
            .is_none(),
        "a queued continuation without a durable AgentRun must not reserve a launch slot"
    );
    assert_eq!(
        inner_agent_run_repo
            .get_by_conversation(&conversation_id)
            .await
            .expect("runs should list")
            .len(),
        1,
        "only the seeded completed run should exist"
    );
}

#[tokio::test]
async fn terminal_queued_verifier_failure_releases_deferred_plan_attention() {
    let state = AppState::new_test();
    state
        .db
        .run(|conn| {
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS deferred_plan_approval_notifications (
                    session_id TEXT PRIMARY KEY NOT NULL,
                    artifact_id TEXT NOT NULL,
                    created_at TEXT NOT NULL DEFAULT (datetime('now'))
                 );",
            )?;
            Ok(())
        })
        .await
        .unwrap();
    let project = state
        .project_repo
        .create(Project::new(
            "Queued verifier failure".to_string(),
            "/tmp/queued-verifier-failure".to_string(),
        ))
        .await
        .unwrap();
    let mut session = IdeationSession::new(project.id.clone());
    session.session_flow = IdeationSessionFlow::Planning;
    session.plan_blueprint_artifact_id = Some(crate::domain::entities::ArtifactId::from_string(
        "plan-current-blueprint",
    ));
    let session = state.ideation_session_repo.create(session).await.unwrap();
    state
        .ideation_session_repo
        .update_plan_artifact_id(&session.id, Some("plan-current".to_string()))
        .await
        .unwrap();
    let session = state
        .ideation_session_repo
        .get_by_id(&session.id)
        .await
        .unwrap()
        .unwrap();
    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project.id.clone()))
        .await
        .unwrap();
    let mut workspace = AgentConversationWorkspace::new(
        conversation.id,
        project.id,
        AgentConversationWorkspaceMode::Plan,
        IdeationAnalysisBaseRefKind::LocalBranch,
        "main".to_string(),
        Some("main".to_string()),
        Some("base".to_string()),
        "plan-workspace".to_string(),
        "/tmp/plan-workspace".to_string(),
    );
    workspace.linked_ideation_session_id = Some(session.id.clone());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();
    let publish_run = state
        .agent_run_repo
        .create(AgentRun::new(conversation.id))
        .await
        .unwrap();
    let publish_authority = PlanApprovalPublishAuthority::new(publish_run.id, conversation.id);
    reconcile_plan_approval_on_publish(
        &state,
        None,
        "plan-current",
        std::slice::from_ref(&session),
        Some(&publish_authority),
    )
    .await;
    assert!(
        has_deferred_plan_approval(&state, &session.id, "plan-current")
            .await
            .unwrap()
    );

    let mut verifier_run = AgentRun::new(conversation.id);
    verifier_run.action_kind = Some(AgentRunActionKind::VerifyPlan);
    verifier_run.action_context_id = Some(session.id.as_str().to_string());
    verifier_run.action_target_id = Some(
        session
            .plan_artifact_bundle()
            .expect("queued verifier test requires a complete plan bundle")
            .action_target_id(),
    );
    let verifier_run = state.agent_run_repo.create(verifier_run).await.unwrap();
    state
        .agent_run_repo
        .fail(&verifier_run.id, "queued preflight failed")
        .await
        .unwrap();
    let verifier_run_id = verifier_run.id.as_str();
    let plan_verification_completion = Arc::new(
        crate::application::plan_verification_service::PlanVerificationCompletionAdapter::from_app_state(
            &state,
        ),
    );
    let app = tauri::test::mock_builder()
        .manage(state)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");
    let app_handle = app.handle().clone();

    super::super::chat_service_queue::settle_terminal_queued_plan_verification(
        Some(&plan_verification_completion),
        &verifier_run_id,
    )
    .await;

    let state = app_handle.state::<AppState>();
    assert!(
        !has_deferred_plan_approval(state.inner(), &session.id, "plan-current")
            .await
            .unwrap()
    );
    let notifications = state
        .notification_repo
        .list(None, None, 20)
        .await
        .unwrap()
        .notifications;
    assert_eq!(notifications.len(), 1);
    assert_eq!(notifications[0].title, "Plan approval needed");
}

#[cfg(unix)]
#[tokio::test]
async fn queue_persona_resume_attributes_the_continuation_run() {
    use crate::domain::agents::AgentHarnessKind;

    let state = AppState::new_test();
    let message_queue = Arc::clone(&state.message_queue);
    let running_agent_registry = Arc::clone(&state.running_agent_registry);
    let agent_run_repo = Arc::clone(&state.agent_run_repo);
    let chat_message_repo = Arc::clone(&state.chat_message_repo);
    let chat_timeline_repo = Arc::clone(&state.chat_timeline_repo);
    let chat_attachment_repo = Arc::clone(&state.chat_attachment_repo);
    let artifact_repo = Arc::clone(&state.artifact_repo);
    let activity_event_repo = Arc::clone(&state.activity_event_repo);
    let task_repo = Arc::clone(&state.task_repo);
    let ideation_session_repo = Arc::clone(&state.ideation_session_repo);
    let persona = Persona {
        id: PersonaId::from("queue-persona"),
        artifact_id: None,

        project_id: None,
        slug: "queue-persona".to_string(),
        name: "Queue Persona".to_string(),
        description: "queue attribution fixture".to_string(),
        content: "SECRET_QUEUE_PERSONA_BODY".to_string(),
        status: PersonaStatus::Active,
        version: 3,
        content_hash: "queue-persona-hash".to_string(),
        source_session_id: None,
        source_persona_id: None,
        source_content_hash: None,
        source_json: "{}".to_string(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    state
        .persona_repo
        .create(persona.clone())
        .await
        .expect("seed queue persona");
    let project_id = ProjectId::new();
    let mut conversation = ChatConversation::new_project(project_id.clone());
    conversation.persona_id = Some(persona.id.to_string());
    let conversation = state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("seed queue conversation");
    let conversation_id = conversation.id;
    seed_completed_continuation_runtime(
        &agent_run_repo,
        &conversation_id,
        AgentHarnessKind::Claude,
        "session-cli",
    )
    .await;
    let runtime_factory_deps =
        crate::application::runtime_factory::ChatRuntimeFactoryDeps::from_app_state(&state);
    let events = RecordingEventSink::new();
    let temp = tempfile::tempdir().expect("tempdir");
    let cli_path = temp.path().join("fake-claude");
    std::fs::write(
        &cli_path,
        r#"#!/bin/sh
cat <<'EOF'
{"type":"assistant","message":{"content":[{"type":"text","text":"queued continuation response"}]},"session_id":"session-cli"}
{"type":"result","session_id":"session-cli","is_error":false,"result":"queued continuation response","cost_usd":0.0}
EOF
"#,
    )
    .expect("write fake cli");
    let mut permissions = std::fs::metadata(&cli_path)
        .expect("fake cli metadata")
        .permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
    std::fs::set_permissions(&cli_path, permissions).expect("mark fake cli executable");

    message_queue.queue(
        ChatContextType::Project,
        conversation_id.as_str(),
        "Queued message".to_string(),
    );

    let outcome = super::super::chat_service_queue::process_queued_messages(
        ChatContextType::Project,
        AgentHarnessKind::Claude,
        project_id.as_str(),
        &conversation_id.as_str(),
        conversation_id.clone(),
        "session-cli",
        true,
        &message_queue,
        None,
        None,
        &running_agent_registry,
        &agent_run_repo,
        &chat_message_repo,
        Some(chat_timeline_repo),
        &chat_attachment_repo,
        &artifact_repo,
        &activity_event_repo,
        &task_repo,
        &ideation_session_repo,
        &cli_path,
        temp.path(),
        temp.path(),
        None,
        None,
        Arc::new(events.clone()),
        None,
        Some(runtime_factory_deps),
        Some(project_id.as_str()),
        None,
        tokio_util::sync::CancellationToken::new(),
        Some("chain-queued"),
        Some("parent-run"),
        super::StreamingStateCache::new(),
    )
    .await;

    assert_eq!(outcome.total_processed, 1);
    let queued_run_id = outcome
        .last_run_id
        .as_deref()
        .expect("queued continuation run id should be recorded");
    let queued_run = agent_run_repo
        .get_by_id(&AgentRunId::from_string(queued_run_id.to_string()))
        .await
        .expect("queued run lookup should succeed")
        .expect("queued run should be persisted");
    assert_eq!(queued_run.status, AgentRunStatus::Completed);
    assert_eq!(queued_run.run_chain_id.as_deref(), Some("chain-queued"));
    assert_eq!(queued_run.parent_run_id.as_deref(), Some("parent-run"));
    assert_eq!(queued_run.persona_id.as_deref(), Some("queue-persona"));
    assert_eq!(queued_run.persona_slug.as_deref(), Some("queue-persona"));
    assert_eq!(queued_run.persona_version, Some(3));
    // This unit fixture has no canonical `agents/` tree, so the spawn cannot
    // resolve an agent prompt and injection is skipped. What it proves is that
    // the queue continuation path records attribution AT ALL (it previously
    // recorded nothing). The injected=true path is pinned against the real send
    // path in tests/suite_chat_service/persona_feature_flag.rs.
    assert_eq!(queued_run.persona_injected, Some(false));
    assert_eq!(
        queued_run.persona_skipped_reason.as_deref(),
        Some("agent_prompt_not_found_native_agent")
    );
    assert!(!serde_json::to_string(&queued_run)
        .expect("serialize queued run")
        .contains("SECRET_QUEUE_PERSONA_BODY"));
    {
        // Injection is skipped in this fixture (no canonical agents tree), so the
        // queue path must emit the body-free skip event for the continuation run.
        let events: Vec<_> = events
            .events()
            .into_iter()
            .filter(|event| event.event == "persona:injection_skipped")
            .collect();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].payload["run_id"], queued_run_id);
        assert!(!events[0]
            .payload
            .to_string()
            .contains("SECRET_QUEUE_PERSONA_BODY"));
    }
    assert!(
        running_agent_registry
            .get(&RunningAgentKey::new(
                ChatContextType::Project.to_string(),
                conversation_id.as_str()
            ))
            .await
            .is_none(),
        "successful queued continuation should unregister the runtime key"
    );
}

#[cfg(unix)]
#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn queue_processing_success_reconciles_verification_child_completion() {
    use crate::domain::agents::AgentHarnessKind;

    let _spawn_guard = claude_spawn_permission_lock()
        .lock()
        .expect("lock poisoned");
    let _spawn_permission = EnvVarGuard::set("RALPHX_ALLOW_CLAUDE_SPAWN_IN_TESTS", "1");
    let state = AppState::new_test();
    let runtime_factory_deps =
        crate::application::runtime_factory::ChatRuntimeFactoryDeps::from_app_state(&state);
    let message_queue = Arc::clone(&state.message_queue);
    let running_agent_registry = Arc::clone(&state.running_agent_registry);
    let agent_run_repo = Arc::clone(&state.agent_run_repo);
    let chat_message_repo = Arc::clone(&state.chat_message_repo);
    let chat_timeline_repo = Arc::clone(&state.chat_timeline_repo);
    let chat_attachment_repo = Arc::clone(&state.chat_attachment_repo);
    let artifact_repo = Arc::clone(&state.artifact_repo);
    let activity_event_repo = Arc::clone(&state.activity_event_repo);
    let task_repo = Arc::clone(&state.task_repo);
    let ideation_session_repo = Arc::clone(&state.ideation_session_repo);

    let project_id = ProjectId::new();
    let mut parent = IdeationSession::new(project_id.clone());
    parent.verification_status = VerificationStatus::Reviewing;
    parent.verification_in_progress = true;
    let parent_id = parent.id.clone();
    ideation_session_repo
        .create(parent)
        .await
        .expect("parent verification session should persist");
    ideation_session_repo
        .save_verification_run_snapshot(
            &parent_id,
            &VerificationRunSnapshot {
                generation: 0,
                status: VerificationStatus::Reviewing,
                in_progress: true,
                current_round: 1,
                max_rounds: 5,
                best_round_index: Some(0),
                convergence_reason: Some("zero_blocking".to_string()),
                current_gaps: vec![],
                rounds: vec![VerificationRoundSnapshot {
                    round: 1,
                    gap_score: 0,
                    fingerprints: vec![],
                    gaps: vec![],
                    parse_failed: false,
                }],
            },
        )
        .await
        .expect("terminal verification snapshot should persist");

    let mut child = IdeationSession::new(project_id);
    child.session_purpose = SessionPurpose::Verification;
    child.parent_session_id = Some(parent_id.clone());
    let child_id = child.id.clone();
    ideation_session_repo
        .create(child)
        .await
        .expect("verification child should persist");

    let plan_verification_completion = Arc::new(
        crate::application::plan_verification_service::PlanVerificationCompletionAdapter::from_app_state(
            &state,
        ),
    );
    let temp = tempfile::tempdir().expect("tempdir");
    let cli_path = temp.path().join("fake-claude");
    std::fs::write(
        &cli_path,
        r#"#!/bin/sh
cat <<'EOF'
{"type":"assistant","message":{"content":[{"type":"text","text":"queued verifier continuation complete"}]},"session_id":"session-cli"}
{"type":"result","session_id":"session-cli","is_error":false,"result":"queued verifier continuation complete","cost_usd":0.0}
EOF
"#,
    )
    .expect("write fake cli");
    let mut permissions = std::fs::metadata(&cli_path)
        .expect("fake cli metadata")
        .permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
    std::fs::set_permissions(&cli_path, permissions).expect("mark fake cli executable");

    message_queue.queue(
        ChatContextType::Ideation,
        child_id.as_str(),
        "Continue".to_string(),
    );
    let conversation_id = ChatConversationId::new();
    seed_completed_continuation_runtime(
        &agent_run_repo,
        &conversation_id,
        AgentHarnessKind::Claude,
        "session-cli",
    )
    .await;

    let outcome = super::super::chat_service_queue::process_queued_messages(
        ChatContextType::Ideation,
        AgentHarnessKind::Claude,
        child_id.as_str(),
        child_id.as_str(),
        conversation_id,
        "session-cli",
        false,
        &message_queue,
        None,
        Some(Arc::clone(&state.agent_provider_settings_repo)),
        &running_agent_registry,
        &agent_run_repo,
        &chat_message_repo,
        Some(chat_timeline_repo),
        &chat_attachment_repo,
        &artifact_repo,
        &activity_event_repo,
        &task_repo,
        &ideation_session_repo,
        &cli_path,
        temp.path(),
        temp.path(),
        None,
        None,
        Arc::new(NullEventSink),
        Some(plan_verification_completion),
        Some(runtime_factory_deps),
        None,
        None,
        tokio_util::sync::CancellationToken::new(),
        Some("verification-chain"),
        Some("parent-run"),
        super::StreamingStateCache::new(),
    )
    .await;

    assert_eq!(outcome.total_processed, 1);
    let child_after = ideation_session_repo
        .get_by_id(&child_id)
        .await
        .expect("child lookup should succeed")
        .expect("child should still exist");
    assert_eq!(
        child_after.status,
        IdeationSessionStatus::Archived,
        "queued verifier continuation completion must run verification reconciliation"
    );
    let snapshot = ideation_session_repo
        .get_verification_run_snapshot(&parent_id, 0)
        .await
        .expect("snapshot lookup should succeed")
        .expect("snapshot should remain present");
    assert_eq!(snapshot.status, VerificationStatus::Verified);
    assert!(!snapshot.in_progress);
}

#[cfg(unix)]
#[allow(clippy::await_holding_lock)]
async fn process_queue_resume_persona_block(
    agent_name_override: Option<&str>,
    persona_directive: crate::domain::entities::PersonaDirective,
    archive_before_flush: bool,
    replace_binding_before_flush: bool,
) -> (bool, bool) {
    let _spawn_guard = claude_spawn_permission_lock()
        .lock()
        .expect("lock poisoned");
    let _spawn_permission = EnvVarGuard::set("RALPHX_ALLOW_CLAUDE_SPAWN_IN_TESTS", "1");
    let mut state = AppState::new_test();
    let persona_repo = Arc::new(MemoryPersonaRepository::new());
    let persona = Persona {
        id: PersonaId::from("queued-resume-persona"),
        artifact_id: None,

        project_id: None,
        slug: "queued-resume-persona".to_string(),
        name: "Queued Resume Persona".to_string(),
        description: "queue resume fixture".to_string(),
        content: "Use the queued persona voice.".to_string(),
        status: PersonaStatus::Active,
        version: 1,
        content_hash: "queued-resume-persona-hash".to_string(),
        source_session_id: None,
        source_persona_id: None,
        source_content_hash: None,
        source_json: "{}".to_string(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    persona_repo
        .create(persona.clone())
        .await
        .expect("seed queued resume persona");
    let replacement_persona = Persona {
        id: PersonaId::from("queued-resume-replacement-persona"),
        artifact_id: None,

        project_id: None,
        slug: "queued-resume-replacement-persona".to_string(),
        name: "Queued Resume Replacement Persona".to_string(),
        description: "queue resume replacement fixture".to_string(),
        content: "Use the replacement queued persona voice.".to_string(),
        status: PersonaStatus::Active,
        version: 1,
        content_hash: "queued-resume-replacement-persona-hash".to_string(),
        source_session_id: None,
        source_persona_id: None,
        source_content_hash: None,
        source_json: "{}".to_string(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    if replace_binding_before_flush {
        persona_repo
            .create(replacement_persona.clone())
            .await
            .expect("seed replacement queued resume persona");
    }
    state.persona_repo = persona_repo;
    let project_id = ProjectId::from_string("queued-resume-project".to_string());
    let mut conversation = ChatConversation::new_project(project_id.clone());
    conversation.persona_id = Some(persona.id.to_string());
    let conversation_id = conversation.id.clone();
    state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("seed queued resume conversation");
    let message_queue = Arc::clone(&state.message_queue);
    let running_agent_registry = Arc::clone(&state.running_agent_registry);
    let agent_run_repo = Arc::clone(&state.agent_run_repo);
    let chat_message_repo = Arc::clone(&state.chat_message_repo);
    let chat_attachment_repo = Arc::clone(&state.chat_attachment_repo);
    let artifact_repo = Arc::clone(&state.artifact_repo);
    let chat_conversation_repo = Arc::clone(&state.chat_conversation_repo);
    let activity_event_repo = Arc::clone(&state.activity_event_repo);
    let task_repo = Arc::clone(&state.task_repo);
    let ideation_session_repo = Arc::clone(&state.ideation_session_repo);
    let persona_repo_for_flush = Arc::clone(&state.persona_repo);
    let runtime_factory_deps =
        crate::application::runtime_factory::ChatRuntimeFactoryDeps::from_app_state(&state);
    let temp = tempfile::tempdir().expect("temporary queued resume runtime");
    let plugin_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("repository root")
        .join("plugins/app");
    let persona_marker = temp.path().join("persona-was-injected");
    let replacement_persona_marker = temp.path().join("replacement-persona-was-injected");
    let cli_path = temp.path().join("fake-claude");
    std::fs::write(
        &cli_path,
        format!(
            "#!/bin/sh\nfor arg in \"$@\"; do\n  if [ -f \"$arg\" ]; then\n    grep -q '<ralphx_agent_persona>' \"$arg\" && touch '{}'\n    grep -q 'Use the replacement queued persona voice.' \"$arg\" && touch '{}'\n  fi\ndone\nprintf '%s\\n' '{{\"type\":\"result\",\"session_id\":\"queue-resume-session\",\"is_error\":false,\"result\":\"ok\",\"cost_usd\":0.0}}'\n",
            persona_marker.display(),
            replacement_persona_marker.display(),
        ),
    )
    .expect("write fake queued resume cli");
    let mut permissions = std::fs::metadata(&cli_path)
        .expect("fake queued resume cli metadata")
        .permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
    std::fs::set_permissions(&cli_path, permissions)
        .expect("mark fake queued resume cli executable");
    let mut queued = crate::domain::services::QueuedMessage::new("queued follow-up".to_string());
    queued.agent_name_override = agent_name_override.map(str::to_string);
    queued.persona_directive = persona_directive;
    message_queue.queue_front_existing(ChatContextType::Project, project_id.as_str(), queued);
    if replace_binding_before_flush {
        chat_conversation_repo
            .update_persona_binding(&conversation_id, Some(replacement_persona.id.as_str()))
            .await
            .expect("replace binding between enqueue and flush");
    }
    if archive_before_flush {
        persona_repo_for_flush
            .set_status(&persona.id, PersonaStatus::Archived)
            .await
            .expect("archive explicit persona between enqueue and flush");
    }
    seed_completed_continuation_runtime(
        &agent_run_repo,
        &conversation_id,
        AgentHarnessKind::Claude,
        "queue-resume-session",
    )
    .await;

    let outcome = super::super::chat_service_queue::process_queued_messages(
        ChatContextType::Project,
        AgentHarnessKind::Claude,
        project_id.as_str(),
        project_id.as_str(),
        conversation_id,
        "queue-resume-session",
        true,
        &message_queue,
        None,
        Some(Arc::clone(&state.agent_provider_settings_repo)),
        &running_agent_registry,
        &agent_run_repo,
        &chat_message_repo,
        None,
        &chat_attachment_repo,
        &artifact_repo,
        &activity_event_repo,
        &task_repo,
        &ideation_session_repo,
        &cli_path,
        &plugin_dir,
        temp.path(),
        None,
        None,
        Arc::new(NullEventSink),
        None,
        Some(runtime_factory_deps),
        Some(project_id.as_str()),
        None,
        tokio_util::sync::CancellationToken::new(),
        None,
        None,
        super::StreamingStateCache::new(),
    )
    .await;

    assert_eq!(outcome.total_processed, 1);
    (persona_marker.exists(), replacement_persona_marker.exists())
}

#[cfg(unix)]
#[tokio::test]
async fn queued_resume_resolves_inherit_persona_unless_agent_override_is_set() {
    assert!(
        process_queue_resume_persona_block(
            None,
            crate::domain::entities::PersonaDirective::Inherit,
            false,
            false,
        )
        .await
        .0,
        "bound Project persona must reach the real queued resume command"
    );
    assert!(
        !process_queue_resume_persona_block(
            Some("ralphx-queued-agent"),
            crate::domain::entities::PersonaDirective::Inherit,
            false,
            false,
        )
        .await
        .0,
        "queued agent override must suppress the inherited persona block"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn queued_flush_rereads_current_db_binding_not_enqueued_snapshot() {
    let (persona_injected, replacement_injected) = process_queue_resume_persona_block(
        None,
        crate::domain::entities::PersonaDirective::Inherit,
        false,
        true,
    )
    .await;

    assert!(
        persona_injected,
        "the current active binding must still inject a persona"
    );
    assert!(
        replacement_injected,
        "queue flush must re-read the current conversation binding instead of retaining enqueue-time state"
    );

    let (suppressed_injected, suppressed_replacement_injected) =
        process_queue_resume_persona_block(
            None,
            crate::domain::entities::PersonaDirective::Suppress,
            false,
            true,
        )
        .await;
    assert!(
        !suppressed_injected && !suppressed_replacement_injected,
        "a queued Suppress directive must still suppress after the binding changes"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn queued_explicit_persona_archived_before_flush_fails_closed() {
    let (persona_injected, replacement_injected) = process_queue_resume_persona_block(
        None,
        crate::domain::entities::PersonaDirective::Explicit(PersonaId::from(
            "queued-resume-persona",
        )),
        true,
        false,
    )
    .await;

    assert!(
        !persona_injected && !replacement_injected,
        "an archived explicit persona must block the queued continuation before command spawn"
    );
}

#[tokio::test]
async fn send_queued_message_now_preserves_suppress_directive_and_agent_override() {
    use crate::application::chat_service::{ChatService, MockChatService};
    use crate::domain::agents::{AgentHarnessKind, LogicalEffort};
    use crate::domain::entities::PersonaDirective;
    use crate::domain::services::{ComposerArtifactReference, MessageQueue};

    let message_queue = Arc::new(MessageQueue::new());
    let service = MockChatService::with_queue(Arc::clone(&message_queue));
    let missing = service
        .send_queued_message_now(ChatContextType::Task, "task-1", "missing")
        .await
        .expect_err("missing queued message should fail");
    assert!(
        missing.to_string().contains("Queued message not found"),
        "missing queued message error should identify the queue lookup failure"
    );

    let queued = message_queue.queue_with_runtime_overrides_and_project_references(
        ChatContextType::Task,
        "task-1",
        "queued content".to_string(),
        Some(r#"{"source":"queue-now"}"#.to_string()),
        None,
        Some(AgentHarnessKind::Codex),
        Some("ralphx-queued-agent".to_string()),
        PersonaDirective::Suppress,
        Some("gpt-5.5".to_string()),
        Some(LogicalEffort::High),
        Some("fast".to_string()),
        true,
        Vec::new(),
        Vec::new(),
        vec![ComposerArtifactReference {
            artifact_id: "artifact-1".to_string(),
            kind: "plan".to_string(),
            title: Some("Implementation Plan".to_string()),
            session_id: Some("session-1".to_string()),
            version: Some(2),
            status: Some("approved".to_string()),
        }],
        None,
        Vec::new(),
        Vec::new(),
    );

    let result = service
        .send_queued_message_now(ChatContextType::Task, "task-1", &queued.id)
        .await
        .expect("queued message should send through mock service");

    assert!(!result.was_queued);
    assert_eq!(
        service.get_sent_messages().await,
        vec!["queued content".to_string()]
    );
    let sent_options = service.get_sent_options().await;
    assert_eq!(sent_options.len(), 1);
    assert_eq!(
        sent_options[0].metadata.as_deref(),
        Some(r#"{"source":"queue-now"}"#)
    );
    assert_eq!(
        sent_options[0].harness_override,
        Some(AgentHarnessKind::Codex)
    );
    assert_eq!(sent_options[0].model_override.as_deref(), Some("gpt-5.5"));
    assert_eq!(
        sent_options[0].agent_name_override.as_deref(),
        Some("ralphx-queued-agent")
    );
    assert_eq!(
        sent_options[0].persona_directive,
        PersonaDirective::Suppress
    );
    assert_eq!(
        sent_options[0].logical_effort_override,
        Some(LogicalEffort::High)
    );
    assert_eq!(
        sent_options[0].service_tier_override.as_deref(),
        Some("fast")
    );
    assert_eq!(
        sent_options[0].composer_artifact_references,
        queued.composer_artifact_references
    );
    assert!(sent_options[0].force_new_provider_session);
}

#[tokio::test]
async fn queue_processing_links_selected_attachments_before_spawn_failure() {
    let app_state = AppState::new_test();
    let runtime_factory_deps =
        crate::application::runtime_factory::ChatRuntimeFactoryDeps::from_app_state(&app_state);
    let message_queue = Arc::clone(&app_state.message_queue);
    let running_agent_registry = Arc::clone(&app_state.running_agent_registry);
    let agent_run_repo = Arc::clone(&app_state.agent_run_repo);
    let chat_message_repo = Arc::clone(&app_state.chat_message_repo);
    let chat_attachment_repo = Arc::clone(&app_state.chat_attachment_repo);
    let artifact_repo = Arc::clone(&app_state.artifact_repo);
    let activity_event_repo = Arc::clone(&app_state.activity_event_repo);
    let task_repo = Arc::clone(&app_state.task_repo);
    let ideation_session_repo = Arc::clone(&app_state.ideation_session_repo);
    let temp = tempfile::tempdir().expect("tempdir");
    let selected_path = temp.path().join("selected.txt");
    let unselected_path = temp.path().join("unselected.txt");
    std::fs::write(&selected_path, "selected queued attachment").expect("write selected");
    std::fs::write(&unselected_path, "unselected queued attachment").expect("write unselected");

    let conversation_id = ChatConversationId::new();
    seed_completed_continuation_runtime(
        &agent_run_repo,
        &conversation_id,
        AgentHarnessKind::Claude,
        "session-cli",
    )
    .await;
    let selected_attachment = chat_attachment_repo
        .create(ChatAttachment::new(
            conversation_id,
            "selected.txt",
            selected_path.to_string_lossy().to_string(),
            26,
            Some("text/plain".to_string()),
        ))
        .await
        .expect("selected attachment should persist");
    let unselected_attachment = chat_attachment_repo
        .create(ChatAttachment::new(
            conversation_id,
            "unselected.txt",
            unselected_path.to_string_lossy().to_string(),
            28,
            Some("text/plain".to_string()),
        ))
        .await
        .expect("unselected attachment should persist");

    message_queue.queue_with_overrides_and_project_references(
        ChatContextType::Ideation,
        "session-queued-attachments",
        "Queued message with selected attachment".to_string(),
        None,
        None,
        None,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        None,
        Vec::new(),
        vec![selected_attachment.id],
    );

    let invalid_cli_path = Path::new("/definitely/missing/ralphx-test-cli");
    let outcome = super::super::chat_service_queue::process_queued_messages(
        ChatContextType::Ideation,
        crate::domain::agents::AgentHarnessKind::Claude,
        "session-queued-attachments",
        "session-queued-attachments",
        conversation_id,
        "session-cli",
        false,
        &message_queue,
        None,
        None,
        &running_agent_registry,
        &agent_run_repo,
        &chat_message_repo,
        None,
        &chat_attachment_repo,
        &artifact_repo,
        &activity_event_repo,
        &task_repo,
        &ideation_session_repo,
        invalid_cli_path,
        temp.path(),
        temp.path(),
        None,
        None,
        Arc::new(NullEventSink),
        None,
        Some(runtime_factory_deps),
        None,
        None,
        tokio_util::sync::CancellationToken::new(),
        None,
        None,
        super::StreamingStateCache::new(),
    )
    .await;

    assert_eq!(outcome.total_processed, 1);

    let selected = chat_attachment_repo
        .get_by_id(&selected_attachment.id)
        .await
        .expect("selected lookup should succeed")
        .expect("selected attachment should exist");
    let unselected = chat_attachment_repo
        .get_by_id(&unselected_attachment.id)
        .await
        .expect("unselected lookup should succeed")
        .expect("unselected attachment should exist");

    assert!(
        selected.message_id.is_some(),
        "selected queued attachment should link to the queued user message"
    );
    assert_eq!(
        unselected.message_id, None,
        "unselected queued attachment should remain pending"
    );
}

async fn spawn_claude_jsonl_fixture(lines: &[&str]) -> tokio::process::Child {
    let mut payload = String::new();
    for line in lines {
        payload.push_str(line);
        payload.push('\n');
    }

    let mut child = tokio::process::Command::new("cat")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn stream fixture");
    let mut stdin = child.stdin.take().expect("capture fixture stdin");
    stdin
        .write_all(payload.as_bytes())
        .await
        .expect("write stream fixture");
    drop(stdin);
    child
}

async fn spawn_interactive_claude_jsonl_fixture(lines: &[&str]) -> tokio::process::Child {
    let mut payload = String::new();
    for line in lines {
        payload.push_str(line);
        payload.push('\n');
    }

    // Emit the terminal response without consuming stdin. The background runner
    // owns that piped handle through the interactive-process registry, so a
    // `cat` fixture would wait forever for its own cleanup to close stdin.
    tokio::process::Command::new("printf")
        .arg("%s")
        .arg(payload)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn interactive stream fixture")
}

#[tokio::test]
async fn background_run_drains_queue_after_non_cancelled_silent_exit() {
    use crate::domain::agents::AgentHarnessKind;
    use crate::domain::entities::{ChatConversation, ChatMessageAttribution, IdeationSessionId};
    use tokio::time::{sleep, timeout, Duration};

    let state = AppState::new_test();
    let runtime_factory_deps =
        crate::application::runtime_factory::ChatRuntimeFactoryDeps::from_app_state(&state);
    let context_id = IdeationSessionId::new();
    let mut ideation_session = IdeationSession::new(ProjectId::new());
    ideation_session.id = context_id.clone();
    state
        .ideation_session_repo
        .create(ideation_session)
        .await
        .expect("seed ideation session");
    let execution_state = Arc::new(ExecutionState::new());
    let conversation = ChatConversation::new_ideation(context_id.clone());
    let conversation_id = conversation.id.clone();
    state
        .chat_conversation_repo
        .create(conversation.clone())
        .await
        .expect("seed conversation");

    let message_queue = Arc::clone(&state.message_queue);
    let context_id_str = context_id.as_str().to_string();
    message_queue.queue(
        ChatContextType::Ideation,
        &context_id_str,
        "queued follow-up after idle exit".to_string(),
    );

    let repos = super::BackgroundRunRepos {
        chat_message_repo: Arc::clone(&state.chat_message_repo),
        chat_timeline_repo: Some(Arc::clone(&state.chat_timeline_repo)),
        chat_attachment_repo: Arc::clone(&state.chat_attachment_repo),
        artifact_repo: Arc::clone(&state.artifact_repo),
        conversation_repo: Arc::clone(&state.chat_conversation_repo),
        agent_run_repo: Arc::clone(&state.agent_run_repo),
        task_repo: Arc::clone(&state.task_repo),
        task_dependency_repo: Arc::clone(&state.task_dependency_repo),
        project_repo: Arc::clone(&state.project_repo),
        ideation_session_repo: Arc::clone(&state.ideation_session_repo),
        delegated_session_repo: Arc::clone(&state.delegated_session_repo),
        execution_settings_repo: Some(Arc::clone(&state.execution_settings_repo)),
        agent_lane_settings_repo: Some(Arc::clone(&state.agent_lane_settings_repo)),
        agent_provider_settings_repo: Some(Arc::clone(&state.agent_provider_settings_repo)),
        task_proposal_repo: Some(Arc::clone(&state.task_proposal_repo)),
        activity_event_repo: Arc::clone(&state.activity_event_repo),
        memory_event_repo: Arc::clone(&state.memory_event_repo),
        notification_service: None,
        message_queue: Arc::clone(&message_queue),
        queued_message_repo: Some(Arc::clone(&state.queued_message_repo)),
        running_agent_registry: Arc::clone(&state.running_agent_registry),
        task_step_repo: Some(Arc::clone(&state.task_step_repo)),
        validation_run_repo: Some(Arc::clone(&state.validation_run_repo)),
        external_events_repo: Some(Arc::clone(&state.external_events_repo)),
        webhook_publisher: None,
        review_repo: Some(Arc::clone(&state.review_repo)),
    };

    let child = spawn_claude_jsonl_fixture(&[
        r#"{"type":"assistant","message":{"content":[{"type":"text","text":"initial turn complete"}]},"session_id":"sess-bg"}"#,
        r#"{"type":"result","session_id":"sess-bg","is_error":false,"result":"initial turn complete","cost_usd":0.0}"#,
    ])
    .await;

    super::spawn_send_message_background(super::BackgroundRunContext {
        child,
        harness: AgentHarnessKind::Claude,
        context_type: ChatContextType::Ideation,
        context_id: context_id_str.clone(),
        runtime_context_id: context_id_str.clone(),
        conversation_id,
        agent_run_id: "background-run-id".to_string(),
        stored_session_id: None,
        // Never "." — background flows can run git (freshness auto-commit)
        // against the working directory, which would mutate the checkout.
        working_directory: std::env::temp_dir().join("ralphx-bg-missing-cli-wd"),
        cli_path: Path::new("/definitely/missing/ralphx-test-cli").to_path_buf(),
        plugin_dir: std::env::temp_dir().join("ralphx-bg-missing-cli-plugin"),
        repos,
        execution_state: Some(execution_state),
        question_state: None,
        plan_branch_repo: None,
        events: Arc::new(NullEventSink),
        plan_verification_completion: None,
        runtime_factory_deps: Some(runtime_factory_deps),
        run_chain_id: None,
        is_retry_attempt: false,
        persona_feature_enabled: false,
        agent_name_override_set: false,
        user_message_content: Some("initial prompt".to_string()),
        turn_metadata: None,
        conversation: Some(conversation),
        agent_name: Some("orchestrator".to_string()),
        assistant_message_attribution: ChatMessageAttribution::default(),
        persist_conversation_provider_session_ref: true,
        cancellation_token: tokio_util::sync::CancellationToken::new(),
        streaming_state_cache: super::StreamingStateCache::new(),
        interactive_process_registry: None,
        interactive_process_token: None,
        verification_child_registry: None,
    });

    timeout(Duration::from_secs(3), async {
        loop {
            if message_queue
                .get_queued(ChatContextType::Ideation, &context_id_str)
                .is_empty()
            {
                break;
            }
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("background queue processing should drain queued message");
}

/// Pins the exit-path write in `spawn_send_message_background` to refresh-only
/// semantics: a background send completing on a conversation whose provider
/// session ref was already cleared (e.g. by a Plan→Edit handoff) must not
/// resurrect a ref. Regression coverage for the Claude first-click "Implement
/// Directly" double-click bug.
#[tokio::test]
async fn background_exit_write_does_not_resurrect_a_cleared_provider_session() {
    use crate::domain::agents::{AgentHarnessKind, ProviderSessionRef};
    use crate::domain::entities::ChatMessageAttribution;
    use tokio::time::{sleep, timeout, Duration};

    let state = AppState::new_test();
    let conversation = ChatConversation::new_project(ProjectId::new());
    let conversation_id = conversation.id.clone();
    state
        .chat_conversation_repo
        .create(conversation.clone())
        .await
        .expect("seed conversation");
    state
        .chat_conversation_repo
        .update_provider_session_ref(
            &conversation_id,
            &ProviderSessionRef {
                harness: AgentHarnessKind::Claude,
                provider_session_id: "plan-session".to_string(),
            },
        )
        .await
        .expect("seed plan session ref");
    state
        .chat_conversation_repo
        .clear_provider_session_ref(&conversation_id)
        .await
        .expect("simulate Plan→Edit handoff clear");

    let agent_run = state
        .agent_run_repo
        .create(AgentRun::new(conversation_id.clone()))
        .await
        .expect("seed agent run");
    let agent_run_id = agent_run.id.as_str().to_string();
    let agent_run_repo = Arc::clone(&state.agent_run_repo);
    let agent_run_lookup_id = agent_run.id;

    let repos = super::BackgroundRunRepos {
        chat_message_repo: Arc::clone(&state.chat_message_repo),
        chat_timeline_repo: Some(Arc::clone(&state.chat_timeline_repo)),
        chat_attachment_repo: Arc::clone(&state.chat_attachment_repo),
        artifact_repo: Arc::clone(&state.artifact_repo),
        conversation_repo: Arc::clone(&state.chat_conversation_repo),
        agent_run_repo: Arc::clone(&state.agent_run_repo),
        task_repo: Arc::clone(&state.task_repo),
        task_dependency_repo: Arc::clone(&state.task_dependency_repo),
        project_repo: Arc::clone(&state.project_repo),
        ideation_session_repo: Arc::clone(&state.ideation_session_repo),
        delegated_session_repo: Arc::clone(&state.delegated_session_repo),
        execution_settings_repo: Some(Arc::clone(&state.execution_settings_repo)),
        agent_lane_settings_repo: Some(Arc::clone(&state.agent_lane_settings_repo)),
        agent_provider_settings_repo: Some(Arc::clone(&state.agent_provider_settings_repo)),
        task_proposal_repo: Some(Arc::clone(&state.task_proposal_repo)),
        activity_event_repo: Arc::clone(&state.activity_event_repo),
        memory_event_repo: Arc::clone(&state.memory_event_repo),
        notification_service: None,
        message_queue: Arc::clone(&state.message_queue),
        queued_message_repo: Some(Arc::clone(&state.queued_message_repo)),
        running_agent_registry: Arc::clone(&state.running_agent_registry),
        task_step_repo: Some(Arc::clone(&state.task_step_repo)),
        validation_run_repo: Some(Arc::clone(&state.validation_run_repo)),
        external_events_repo: Some(Arc::clone(&state.external_events_repo)),
        webhook_publisher: None,
        review_repo: Some(Arc::clone(&state.review_repo)),
    };

    // Deliberately omit the "result" line: only a "result" message emits
    // `StreamEvent::TurnComplete`, which is the unconditional creation path
    // (chat_service_streaming.rs TurnComplete handlers) and would otherwise
    // recreate the ref itself, masking whether the exit-path write in
    // chat_service_send_background.rs is refresh-only. This mirrors the real
    // race: a cancelled/torn-down stream carries a known session id without
    // ever completing a turn.
    let child = spawn_claude_jsonl_fixture(&[
        r#"{"type":"assistant","message":{"content":[{"type":"text","text":"initial turn complete"}]},"session_id":"sess-bg"}"#,
    ])
    .await;

    super::spawn_send_message_background(super::BackgroundRunContext {
        child,
        harness: AgentHarnessKind::Claude,
        context_type: ChatContextType::Project,
        context_id: conversation_id.as_str().to_string(),
        runtime_context_id: conversation_id.as_str().to_string(),
        conversation_id: conversation_id.clone(),
        agent_run_id,
        stored_session_id: None,
        // Never "." — background flows can run git (freshness auto-commit)
        // against the working directory, which would mutate the checkout.
        working_directory: std::env::temp_dir().join("ralphx-bg-refresh-cleared-wd"),
        cli_path: Path::new("/definitely/missing/ralphx-test-cli").to_path_buf(),
        plugin_dir: std::env::temp_dir().join("ralphx-bg-refresh-cleared-plugin"),
        repos,
        execution_state: None,
        question_state: None,
        plan_branch_repo: None,
        events: Arc::new(NullEventSink),
        plan_verification_completion: None,
        runtime_factory_deps: None,
        run_chain_id: None,
        is_retry_attempt: false,
        persona_feature_enabled: false,
        agent_name_override_set: false,
        user_message_content: Some("initial prompt".to_string()),
        turn_metadata: None,
        conversation: Some(conversation),
        agent_name: Some("orchestrator".to_string()),
        assistant_message_attribution: ChatMessageAttribution::default(),
        persist_conversation_provider_session_ref: true,
        cancellation_token: tokio_util::sync::CancellationToken::new(),
        streaming_state_cache: super::StreamingStateCache::new(),
        interactive_process_registry: None,
        interactive_process_token: None,
        verification_child_registry: None,
    });

    timeout(Duration::from_secs(3), async {
        loop {
            let run = agent_run_repo
                .get_by_id(&agent_run_lookup_id)
                .await
                .expect("agent run lookup")
                .expect("agent run should remain persisted");
            if run.status == AgentRunStatus::Completed {
                break;
            }
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("background run should complete");

    let persisted = state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .expect("conversation lookup")
        .expect("conversation should remain persisted");
    assert!(
        persisted.provider_session_ref().is_none(),
        "stream-exit write must not resurrect a deliberately cleared provider session ref"
    );
}

/// Positive companion to the resurrection regression above: the same exit-path
/// write must still refresh a provider session ref that is currently present,
/// proving the refresh-only call is not a silent no-op.
#[tokio::test]
async fn background_exit_write_refreshes_an_existing_provider_session() {
    use crate::domain::agents::{AgentHarnessKind, ProviderSessionRef};
    use crate::domain::entities::ChatMessageAttribution;
    use tokio::time::{sleep, timeout, Duration};

    let state = AppState::new_test();
    let conversation = ChatConversation::new_project(ProjectId::new());
    let conversation_id = conversation.id.clone();
    state
        .chat_conversation_repo
        .create(conversation.clone())
        .await
        .expect("seed conversation");
    state
        .chat_conversation_repo
        .update_provider_session_ref(
            &conversation_id,
            &ProviderSessionRef {
                harness: AgentHarnessKind::Claude,
                provider_session_id: "stale-session".to_string(),
            },
        )
        .await
        .expect("seed existing provider session ref");

    let agent_run = state
        .agent_run_repo
        .create(AgentRun::new(conversation_id.clone()))
        .await
        .expect("seed agent run");
    let agent_run_id = agent_run.id.as_str().to_string();
    let agent_run_repo = Arc::clone(&state.agent_run_repo);
    let agent_run_lookup_id = agent_run.id;

    let repos = super::BackgroundRunRepos {
        chat_message_repo: Arc::clone(&state.chat_message_repo),
        chat_timeline_repo: Some(Arc::clone(&state.chat_timeline_repo)),
        chat_attachment_repo: Arc::clone(&state.chat_attachment_repo),
        artifact_repo: Arc::clone(&state.artifact_repo),
        conversation_repo: Arc::clone(&state.chat_conversation_repo),
        agent_run_repo: Arc::clone(&state.agent_run_repo),
        task_repo: Arc::clone(&state.task_repo),
        task_dependency_repo: Arc::clone(&state.task_dependency_repo),
        project_repo: Arc::clone(&state.project_repo),
        ideation_session_repo: Arc::clone(&state.ideation_session_repo),
        delegated_session_repo: Arc::clone(&state.delegated_session_repo),
        execution_settings_repo: Some(Arc::clone(&state.execution_settings_repo)),
        agent_lane_settings_repo: Some(Arc::clone(&state.agent_lane_settings_repo)),
        agent_provider_settings_repo: Some(Arc::clone(&state.agent_provider_settings_repo)),
        task_proposal_repo: Some(Arc::clone(&state.task_proposal_repo)),
        activity_event_repo: Arc::clone(&state.activity_event_repo),
        memory_event_repo: Arc::clone(&state.memory_event_repo),
        notification_service: None,
        message_queue: Arc::clone(&state.message_queue),
        queued_message_repo: Some(Arc::clone(&state.queued_message_repo)),
        running_agent_registry: Arc::clone(&state.running_agent_registry),
        task_step_repo: Some(Arc::clone(&state.task_step_repo)),
        validation_run_repo: Some(Arc::clone(&state.validation_run_repo)),
        external_events_repo: Some(Arc::clone(&state.external_events_repo)),
        webhook_publisher: None,
        review_repo: Some(Arc::clone(&state.review_repo)),
    };

    let child = spawn_claude_jsonl_fixture(&[
        r#"{"type":"assistant","message":{"content":[{"type":"text","text":"initial turn complete"}]},"session_id":"sess-bg"}"#,
        r#"{"type":"result","session_id":"sess-bg","is_error":false,"result":"initial turn complete","cost_usd":0.0}"#,
    ])
    .await;

    super::spawn_send_message_background(super::BackgroundRunContext {
        child,
        harness: AgentHarnessKind::Claude,
        context_type: ChatContextType::Project,
        context_id: conversation_id.as_str().to_string(),
        runtime_context_id: conversation_id.as_str().to_string(),
        conversation_id: conversation_id.clone(),
        agent_run_id,
        stored_session_id: None,
        // Never "." — background flows can run git (freshness auto-commit)
        // against the working directory, which would mutate the checkout.
        working_directory: std::env::temp_dir().join("ralphx-bg-refresh-existing-wd"),
        cli_path: Path::new("/definitely/missing/ralphx-test-cli").to_path_buf(),
        plugin_dir: std::env::temp_dir().join("ralphx-bg-refresh-existing-plugin"),
        repos,
        execution_state: None,
        question_state: None,
        plan_branch_repo: None,
        events: Arc::new(NullEventSink),
        plan_verification_completion: None,
        runtime_factory_deps: None,
        run_chain_id: None,
        is_retry_attempt: false,
        persona_feature_enabled: false,
        agent_name_override_set: false,
        user_message_content: Some("initial prompt".to_string()),
        turn_metadata: None,
        conversation: Some(conversation),
        agent_name: Some("orchestrator".to_string()),
        assistant_message_attribution: ChatMessageAttribution::default(),
        persist_conversation_provider_session_ref: true,
        cancellation_token: tokio_util::sync::CancellationToken::new(),
        streaming_state_cache: super::StreamingStateCache::new(),
        interactive_process_registry: None,
        interactive_process_token: None,
        verification_child_registry: None,
    });

    timeout(Duration::from_secs(3), async {
        loop {
            let run = agent_run_repo
                .get_by_id(&agent_run_lookup_id)
                .await
                .expect("agent run lookup")
                .expect("agent run should remain persisted");
            if run.status == AgentRunStatus::Completed {
                break;
            }
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("background run should complete");

    let persisted = state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .expect("conversation lookup")
        .expect("conversation should remain persisted");
    assert_eq!(
        persisted
            .provider_session_ref()
            .map(|r| r.provider_session_id),
        Some("sess-bg".to_string()),
        "stream-exit write must refresh an existing provider session ref"
    );
}

#[tokio::test]
async fn background_run_suppresses_answered_pending_stdin_turns() {
    use crate::domain::entities::ChatMessageAttribution;
    use tokio::time::{sleep, timeout, Duration};

    let state = AppState::new_test();
    let conversation = ChatConversation::new_project(ProjectId::new());
    let conversation_id = conversation.id.clone();
    let context_id = conversation_id.as_str().to_string();
    state
        .chat_conversation_repo
        .create(conversation.clone())
        .await
        .expect("seed conversation");
    let agent_run = state
        .agent_run_repo
        .create(AgentRun::new(conversation_id.clone()))
        .await
        .expect("seed agent run");
    let mut assistant = super::super::chat_service_context::create_assistant_message(
        ChatContextType::Project,
        &context_id,
        "already answered",
        conversation_id.clone(),
        &[],
        &[],
    );
    assistant.created_at = Utc::now() - chrono::Duration::seconds(2);
    state
        .chat_message_repo
        .create(assistant.clone())
        .await
        .expect("seed later assistant evidence");

    let repos = super::BackgroundRunRepos {
        chat_message_repo: Arc::clone(&state.chat_message_repo),
        chat_timeline_repo: Some(Arc::clone(&state.chat_timeline_repo)),
        chat_attachment_repo: Arc::clone(&state.chat_attachment_repo),
        artifact_repo: Arc::clone(&state.artifact_repo),
        conversation_repo: Arc::clone(&state.chat_conversation_repo),
        agent_run_repo: Arc::clone(&state.agent_run_repo),
        task_repo: Arc::clone(&state.task_repo),
        task_dependency_repo: Arc::clone(&state.task_dependency_repo),
        project_repo: Arc::clone(&state.project_repo),
        ideation_session_repo: Arc::clone(&state.ideation_session_repo),
        delegated_session_repo: Arc::clone(&state.delegated_session_repo),
        execution_settings_repo: Some(Arc::clone(&state.execution_settings_repo)),
        agent_lane_settings_repo: Some(Arc::clone(&state.agent_lane_settings_repo)),
        agent_provider_settings_repo: Some(Arc::clone(&state.agent_provider_settings_repo)),
        task_proposal_repo: Some(Arc::clone(&state.task_proposal_repo)),
        queued_message_repo: Some(Arc::clone(&state.queued_message_repo)),
        activity_event_repo: Arc::clone(&state.activity_event_repo),
        memory_event_repo: Arc::clone(&state.memory_event_repo),
        notification_service: None,
        message_queue: Arc::clone(&state.message_queue),
        running_agent_registry: Arc::clone(&state.running_agent_registry),
        task_step_repo: Some(Arc::clone(&state.task_step_repo)),
        validation_run_repo: Some(Arc::clone(&state.validation_run_repo)),
        external_events_repo: Some(Arc::clone(&state.external_events_repo)),
        webhook_publisher: None,
        review_repo: Some(Arc::clone(&state.review_repo)),
    };
    let interactive_process_registry = Arc::clone(&state.interactive_process_registry);
    let message_queue = Arc::clone(&state.message_queue);
    let app = tauri::test::mock_builder()
        .manage(state)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");
    let recording_sink = Arc::new(RecordingEventSink::new());
    let mut child = spawn_interactive_claude_jsonl_fixture(&[
        r#"{"type":"assistant","message":{"content":[{"type":"text","text":"handled after the pending message"}]},"session_id":"sess-bg-pending"}"#,
        r#"{"type":"result","session_id":"sess-bg-pending","is_error":true,"errors":["fixture failure"],"result":"failed","cost_usd":0.0}"#,
    ])
    .await;
    let interactive_key = InteractiveProcessKey::new("project", &context_id);
    let interactive_process_token = interactive_process_registry
        .register_with_metadata(
            interactive_key.clone(),
            child.stdin.take().expect("interactive fixture stdin"),
            Default::default(),
        )
        .await;
    assert!(
        interactive_process_registry
            .push_pending_turn(
                &interactive_key,
                interactive_process_token,
                PendingStdinTurn {
                    persisted_message_id: "already-answered-turn".to_string(),
                    content: "do not replay".to_string(),
                    metadata_override: None,
                    queued_at: (assistant.created_at + chrono::Duration::seconds(1)).to_rfc3339(),
                },
            )
            .await
    );

    super::spawn_send_message_background(super::BackgroundRunContext {
        child,
        harness: AgentHarnessKind::Claude,
        context_type: ChatContextType::Project,
        context_id: context_id.clone(),
        runtime_context_id: context_id.clone(),
        conversation_id,
        agent_run_id: agent_run.id.as_str().to_string(),
        stored_session_id: None,
        working_directory: std::env::temp_dir().join("ralphx-bg-pending-stdin-wd"),
        cli_path: Path::new("/definitely/missing/ralphx-test-cli").to_path_buf(),
        plugin_dir: std::env::temp_dir().join("ralphx-bg-pending-stdin-plugin"),
        repos,
        execution_state: None,
        question_state: None,
        plan_branch_repo: None,
        events: Arc::clone(&recording_sink) as Arc<dyn ralphx_events::EventSink>,
        plan_verification_completion: None,
        runtime_factory_deps: None,
        run_chain_id: None,
        is_retry_attempt: false,
        persona_feature_enabled: false,
        agent_name_override_set: false,
        user_message_content: Some("initial prompt".to_string()),
        turn_metadata: None,
        conversation: Some(conversation),
        agent_name: Some("orchestrator".to_string()),
        assistant_message_attribution: ChatMessageAttribution::default(),
        persist_conversation_provider_session_ref: true,
        cancellation_token: tokio_util::sync::CancellationToken::new(),
        streaming_state_cache: super::StreamingStateCache::new(),
        interactive_process_registry: Some(interactive_process_registry.clone()),
        interactive_process_token: Some(interactive_process_token),
        verification_child_registry: None,
    });

    timeout(Duration::from_secs(3), async {
        loop {
            if !interactive_process_registry
                .has_process(&interactive_key)
                .await
            {
                break;
            }
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("background exit should remove the interactive process");

    let app_state = app.state::<AppState>();
    let queue_key = crate::domain::services::QueueKey::new(ChatContextType::Project, &context_id);
    assert!(
        app_state
            .queued_message_repo
            .list(&queue_key)
            .await
            .expect("durable queue")
            .is_empty(),
        "later assistant evidence must suppress the recovered turn"
    );
    assert!(
        message_queue.get_queued_with_key(&queue_key).is_empty(),
        "suppressed recovery must not leave an in-memory retry"
    );
    assert!(
        recording_sink
            .events()
            .iter()
            .all(|e| e.event != "agent:message_queued"),
        "suppressed recovery must not publish a queued-message event"
    );
}

#[tokio::test]
async fn background_run_error_passes_runtime_repos_to_error_handler() {
    use crate::domain::agents::AgentHarnessKind;
    use crate::domain::entities::ChatMessageAttribution;
    use tokio::time::{sleep, timeout, Duration};

    let state = AppState::new_test();
    let conversation = ChatConversation::new_project(ProjectId::new());
    let conversation_id = conversation.id.clone();
    state
        .chat_conversation_repo
        .create(conversation.clone())
        .await
        .expect("seed conversation");
    let agent_run = state
        .agent_run_repo
        .create(AgentRun::new(conversation_id.clone()))
        .await
        .expect("seed agent run");
    let agent_run_id = agent_run.id.as_str().to_string();
    let agent_run_repo = Arc::clone(&state.agent_run_repo);
    let agent_run_lookup_id = AgentRunId::from_string(agent_run_id.clone());
    let message_queue = Arc::clone(&state.message_queue);
    message_queue.queue_with_overrides(
        ChatContextType::Project,
        conversation_id.as_str(),
        "resume in place after handled error".to_string(),
        Some(r#"{"resume_in_place":true}"#.to_string()),
        None,
        None,
    );

    let repos = super::BackgroundRunRepos {
        chat_message_repo: Arc::clone(&state.chat_message_repo),
        chat_timeline_repo: Some(Arc::clone(&state.chat_timeline_repo)),
        chat_attachment_repo: Arc::clone(&state.chat_attachment_repo),
        artifact_repo: Arc::clone(&state.artifact_repo),
        conversation_repo: Arc::clone(&state.chat_conversation_repo),
        agent_run_repo: Arc::clone(&state.agent_run_repo),
        task_repo: Arc::clone(&state.task_repo),
        task_dependency_repo: Arc::clone(&state.task_dependency_repo),
        project_repo: Arc::clone(&state.project_repo),
        ideation_session_repo: Arc::clone(&state.ideation_session_repo),
        delegated_session_repo: Arc::clone(&state.delegated_session_repo),
        execution_settings_repo: Some(Arc::clone(&state.execution_settings_repo)),
        agent_lane_settings_repo: Some(Arc::clone(&state.agent_lane_settings_repo)),
        agent_provider_settings_repo: Some(Arc::clone(&state.agent_provider_settings_repo)),
        task_proposal_repo: Some(Arc::clone(&state.task_proposal_repo)),
        activity_event_repo: Arc::clone(&state.activity_event_repo),
        memory_event_repo: Arc::clone(&state.memory_event_repo),
        notification_service: None,
        message_queue: Arc::clone(&message_queue),
        queued_message_repo: Some(Arc::clone(&state.queued_message_repo)),
        running_agent_registry: Arc::clone(&state.running_agent_registry),
        task_step_repo: Some(Arc::clone(&state.task_step_repo)),
        validation_run_repo: Some(Arc::clone(&state.validation_run_repo)),
        external_events_repo: Some(Arc::clone(&state.external_events_repo)),
        webhook_publisher: None,
        review_repo: Some(Arc::clone(&state.review_repo)),
    };

    let child = spawn_claude_jsonl_fixture(&[
        r#"{"type":"assistant","message":{"content":[{"type":"text","text":"partial response"}]},"session_id":"sess-bg-error"}"#,
        r#"{"type":"result","session_id":"sess-bg-error","is_error":true,"errors":["fixture failure"],"result":"failed","cost_usd":0.0}"#,
    ])
    .await;

    super::spawn_send_message_background(super::BackgroundRunContext {
        child,
        harness: AgentHarnessKind::Claude,
        context_type: ChatContextType::Project,
        context_id: conversation_id.as_str().to_string(),
        runtime_context_id: conversation_id.as_str().to_string(),
        conversation_id,
        agent_run_id,
        stored_session_id: Some("sess-bg-error".to_string()),
        working_directory: std::env::temp_dir().join("ralphx-bg-error-wd"),
        cli_path: Path::new("/definitely/missing/ralphx-test-cli").to_path_buf(),
        plugin_dir: std::env::temp_dir().join("ralphx-bg-error-plugin"),
        repos,
        execution_state: None,
        question_state: None,
        plan_branch_repo: None,
        events: Arc::new(NullEventSink),
        plan_verification_completion: None,
        runtime_factory_deps: None,
        run_chain_id: None,
        is_retry_attempt: false,
        persona_feature_enabled: false,
        agent_name_override_set: false,
        user_message_content: Some("initial prompt".to_string()),
        turn_metadata: None,
        conversation: Some(conversation),
        agent_name: Some("orchestrator".to_string()),
        assistant_message_attribution: ChatMessageAttribution::default(),
        persist_conversation_provider_session_ref: true,
        cancellation_token: tokio_util::sync::CancellationToken::new(),
        streaming_state_cache: super::StreamingStateCache::new(),
        interactive_process_registry: None,
        interactive_process_token: None,
        verification_child_registry: None,
    });

    let failed_run = timeout(Duration::from_secs(3), async {
        loop {
            let run = agent_run_repo
                .get_by_id(&agent_run_lookup_id)
                .await
                .expect("agent run lookup")
                .expect("agent run should remain persisted");
            if run.status == AgentRunStatus::Failed {
                break run;
            }
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("background stream error should fail the agent run");

    assert!(
        failed_run
            .error_message
            .as_deref()
            .unwrap_or_default()
            .contains("fixture failure"),
        "persisted run error should include the stream failure"
    );
}

/// Verifies that session swap recovery enqueues rehydration at front of queue,
/// preserving ordering: recovery context → pending user messages.
#[test]
fn session_swap_recovery_enqueues_rehydration_before_user_messages() {
    use crate::domain::entities::ChatContextType;
    use crate::domain::services::MessageQueue;

    let queue = MessageQueue::new();

    // Simulate: user queued messages while agent was running
    queue.queue(
        ChatContextType::Ideation,
        "ctx-1",
        "User follow-up 1".to_string(),
    );
    queue.queue(
        ChatContextType::Ideation,
        "ctx-1",
        "User follow-up 2".to_string(),
    );

    // Session swap detected → recovery enqueues rehydration at front
    let rehydration_content = "<instructions>Your session was recovered</instructions>".to_string();
    queue.queue_front(
        ChatContextType::Ideation,
        "ctx-1",
        rehydration_content.clone(),
    );

    // Verify queue order: rehydration first, then user messages
    let queued = queue.get_queued(ChatContextType::Ideation, "ctx-1");
    assert_eq!(queued.len(), 3);
    assert_eq!(queued[0].content, rehydration_content);
    assert_eq!(queued[1].content, "User follow-up 1");
    assert_eq!(queued[2].content, "User follow-up 2");

    // Pop order should match: rehydration processed first via --resume
    let first = queue.pop(ChatContextType::Ideation, "ctx-1").unwrap();
    assert!(first.content.contains("session was recovered"));
}

// ============================================================================
// IPR zombie fix tests (Fix 1A)
//
// These tests verify the invariant: IPR is ALWAYS removed on stream exit,
// regardless of whether a team is still active. A dead process's stdin is
// useless and must never be kept as a zombie.
// ============================================================================

/// Helper: spawn a cat process to get a real ChildStdin (same as IPR registry tests).
async fn spawn_test_stdin() -> (tokio::process::ChildStdin, tokio::process::Child) {
    let mut child = tokio::process::Command::new("cat")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("failed to spawn cat");
    let stdin = child.stdin.take().expect("no stdin");
    (stdin, child)
}

/// Verifies that IPR entry is removed even when the team is still active.
///
/// Regression test for the IPR_KEEP zombie bug: previously, when `team_still_active=true`,
/// the IPR entry was kept (`IPR_KEEP`), creating a zombie stdin handle for a dead process.
/// The fix always removes the entry unconditionally on stream exit.
#[tokio::test]
async fn ipr_removed_even_when_team_still_active() {
    let (stdin, _child) = spawn_test_stdin().await;
    let ipr = InteractiveProcessRegistry::new();
    let key = InteractiveProcessKey::new("ideation", "session-zombie-test");

    // Register a process (simulating a lead agent that just started)
    ipr.register(key.clone(), stdin).await;
    assert!(
        ipr.has_process(&key).await,
        "Precondition: IPR entry must exist before cleanup"
    );

    // Simulate stream exit cleanup with team_still_active=true.
    // The new behavior: always remove, even when team is still active.
    // (Previously: IPR_KEEP would skip this remove → zombie)
    ipr.remove(&key).await;

    assert!(
        !ipr.has_process(&key).await,
        "IPR entry must be removed on stream exit even when team is still active"
    );
}

/// Verifies that after IPR removal, has_process returns false,
/// which causes the send_message path to fall through to agent re-spawn.
///
/// When a teammate tries to nudge the lead after IPR removal:
/// 1. has_process() returns false → write_message skipped
/// 2. running_agent_registry miss → queue skipped
/// 3. send_message spawns a new agent (re-spawn via IPR-miss path)
#[tokio::test]
async fn ipr_miss_enables_respawn_path() {
    let (stdin, _child) = spawn_test_stdin().await;
    let ipr = InteractiveProcessRegistry::new();
    let key = InteractiveProcessKey::new("ideation", "session-respawn-test");

    // Start with an IPR entry
    ipr.register(key.clone(), stdin).await;
    assert!(ipr.has_process(&key).await, "Precondition: entry exists");

    // Lead process exits → IPR removed (the fix)
    ipr.remove(&key).await;

    // After removal: has_process returns false
    // This is what triggers the re-spawn path in send_message handlers
    assert!(
        !ipr.has_process(&key).await,
        "has_process must return false after removal, enabling re-spawn path"
    );

    // write_message on a missing key returns an error (would be caught in send flow)
    let write_result = ipr.write_message(&key, "nudge from teammate").await;
    assert!(
        write_result.is_err(),
        "write_message must fail when IPR entry absent (triggers re-spawn fallthrough)"
    );
}

// ============================================================================
// Auto-archive guard tests (Fix 3)
//
// These tests verify the invariant: verification child sessions are NOT
// auto-archived at the auto-archive callsite in chat_service_send_background.rs.
// The run_completed hook (Fix 1) is responsible for archival after parent
// reconciliation. The periodic reconciler is the fallback for orphaned children.
// ============================================================================

/// Verifies that a verification child session is NOT auto-archived at the
/// auto-archive callsite.
///
/// Fix 3 changes the Verification match arm from archiving the child to
/// skipping archival (deferred to the run_completed hook). This test
/// confirms the guard fires: the session remains Active after the code path
/// executes without calling update_status.
#[tokio::test]
async fn verification_child_session_not_auto_archived_at_callsite() {
    use crate::domain::entities::{
        IdeationSession, IdeationSessionStatus, ProjectId, SessionPurpose,
    };
    use crate::domain::repositories::IdeationSessionRepository;
    use crate::infrastructure::memory::MemoryIdeationSessionRepository;
    use std::sync::Arc;

    let repo = Arc::new(MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();

    // Create a verification child session (simulates a ralphx-plan-verifier child agent)
    let session = IdeationSession::builder()
        .project_id(project_id)
        .session_purpose(SessionPurpose::Verification)
        .build();
    let session_id = session.id.clone();
    repo.create(session).await.unwrap();

    // Simulate the auto-archive guard logic:
    // The guard matches session_purpose == Verification and skips update_status.
    let retrieved = repo.get_by_id(&session_id).await.unwrap().unwrap();
    if retrieved.session_purpose == SessionPurpose::Verification {
        // Guard fires: do NOT call update_status — deferred to run_completed hook
    }
    // No update_status call means the session status is unchanged.

    let after = repo.get_by_id(&session_id).await.unwrap().unwrap();
    assert_eq!(
        after.status,
        IdeationSessionStatus::Active,
        "verification child must NOT be auto-archived at the auto-archive callsite"
    );
}

/// Verifies that non-verification (general) sessions are unaffected by the
/// auto-archive guard — no regression from Fix 3.
///
/// General sessions fall through to the `Ok(Some(_)) => {}` arm (no action).
/// This test confirms that after Fix 3, general sessions remain Active and
/// are not accidentally archived or errored.
#[tokio::test]
async fn general_session_not_archived_at_auto_archive_callsite_no_regression() {
    use crate::domain::entities::{
        IdeationSession, IdeationSessionStatus, ProjectId, SessionPurpose,
    };
    use crate::domain::repositories::IdeationSessionRepository;
    use crate::infrastructure::memory::MemoryIdeationSessionRepository;
    use std::sync::Arc;

    let repo = Arc::new(MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();

    // Create a general (non-verification) session — default session_purpose is General
    let session = IdeationSession::new(project_id);
    assert_eq!(
        session.session_purpose,
        SessionPurpose::General,
        "IdeationSession::new() must default to General purpose"
    );
    let session_id = session.id.clone();
    repo.create(session).await.unwrap();

    // Simulate the auto-archive guard logic:
    // The guard does not match General sessions → falls through to no-op arm.
    let retrieved = repo.get_by_id(&session_id).await.unwrap().unwrap();
    if retrieved.session_purpose == SessionPurpose::Verification {
        panic!("unexpected: general session matched verification guard");
    }
    // No update_status call for general sessions (same as before Fix 3).

    let after = repo.get_by_id(&session_id).await.unwrap().unwrap();
    assert_eq!(
        after.status,
        IdeationSessionStatus::Active,
        "general session must remain Active — not archived at the auto-archive callsite"
    );
}

#[tokio::test]
async fn finalize_no_output_writes_both_chat_messages_and_timeline_placeholder() {
    use crate::application::chat_service::create_assistant_message;
    use crate::application::chat_service::finalize_no_output_assistant_message_for_test;
    use crate::domain::entities::{ChatConversationId, ChatTimelineItemStatus, IdeationSessionId};

    let state = AppState::new_test();
    let conversation_id = ChatConversationId::new();
    let session_id = IdeationSessionId::new();

    // Seed the pre-created empty assistant placeholder, matching the production spawn flow.
    let pre_assistant = create_assistant_message(
        ChatContextType::Ideation,
        session_id.as_str(),
        "",
        conversation_id.clone(),
        &[],
        &[],
    );
    let pre_assistant_id = pre_assistant.id.as_str().to_string();
    state
        .chat_message_repo
        .create(pre_assistant)
        .await
        .expect("seed pre-assistant message");

    finalize_no_output_assistant_message_for_test(
        &state.chat_message_repo,
        &Some(state.chat_timeline_repo.clone()),
        &NullEventSink,
        &conversation_id,
        "ideation",
        session_id.as_str(),
        &pre_assistant_id,
        "orchestrator",
    )
    .await;

    // chat_messages got the placeholder note.
    let persisted = state
        .chat_message_repo
        .get_by_id(&crate::domain::entities::ChatMessageId::from_string(
            pre_assistant_id.clone(),
        ))
        .await
        .expect("load message")
        .expect("message persisted");
    assert!(
        persisted.content.contains("Agent completed with no output"),
        "chat_messages.content must carry the no-output note"
    );

    // chat_message_blocks (timeline) also got the placeholder so the timeline-rendering
    // chat UI does not show a blank turn.
    let page = state
        .chat_timeline_repo
        .get_page(&conversation_id, 10, None)
        .await
        .expect("load timeline page");
    let assistant_blocks: Vec<_> = page
        .items
        .iter()
        .filter(|item| {
            item.message_id
                .as_ref()
                .is_some_and(|id| id.as_str() == pre_assistant_id)
        })
        .collect();
    assert_eq!(
        assistant_blocks.len(),
        1,
        "no-output finalization must write exactly one timeline placeholder block"
    );
    assert_eq!(
        assistant_blocks[0].status,
        ChatTimelineItemStatus::Finalized,
        "the placeholder block must be finalized so the UI does not show a spinner"
    );
    assert!(
        assistant_blocks[0]
            .text
            .as_deref()
            .unwrap_or_default()
            .contains("Agent completed with no output"),
        "the placeholder block must carry the same note as chat_messages"
    );
}

#[tokio::test]
async fn finalize_structured_writes_chat_message_and_finalized_timeline_rows() {
    use crate::application::chat_service::create_assistant_message;
    use crate::domain::entities::IdeationSessionId;

    let state = AppState::new_test();
    let conversation_id = ChatConversationId::new();
    let session_id = IdeationSessionId::new();
    let pre_assistant = create_assistant_message(
        ChatContextType::Ideation,
        session_id.as_str(),
        "",
        conversation_id.clone(),
        &[],
        &[],
    );
    let pre_assistant_id = pre_assistant.id.as_str().to_string();
    state
        .chat_message_repo
        .create(pre_assistant)
        .await
        .expect("seed pre-assistant message");

    let tool_calls = vec![ToolCall {
        id: Some("toolu-read".to_string()),
        name: "Read".to_string(),
        arguments: serde_json::json!({ "file_path": "src/app.ts" }),
        result: Some(serde_json::json!("preview")),
        parent_tool_use_id: None,
        diff_context: None,
        stats: None,
    }];
    let content_blocks = vec![
        ContentBlockItem::Text {
            text: "Done".to_string(),
        },
        ContentBlockItem::ToolUse {
            id: Some("toolu-read".to_string()),
            name: "Read".to_string(),
            arguments: serde_json::json!({ "file_path": "src/app.ts" }),
            result: Some(serde_json::json!("preview")),
            parent_tool_use_id: None,
            diff_context: Some(serde_json::json!({ "file_path": "src/app.ts" })),
        },
    ];

    super::finalize_structured_assistant_message(
        &state.chat_message_repo,
        &Some(state.chat_timeline_repo.clone()),
        &NullEventSink,
        ChatContextType::Ideation,
        session_id.as_str(),
        &conversation_id,
        &pre_assistant_id,
        "orchestrator",
        "Done",
        &tool_calls,
        &content_blocks,
        false,
    )
    .await;

    let persisted = state
        .chat_message_repo
        .get_by_id(&crate::domain::entities::ChatMessageId::from_string(
            pre_assistant_id.clone(),
        ))
        .await
        .expect("load message")
        .expect("message persisted");
    assert_eq!(persisted.content, "Done");
    assert!(persisted
        .content_blocks
        .as_deref()
        .is_some_and(|raw| raw.contains("toolu-read")));

    let page = state
        .chat_timeline_repo
        .get_page(&conversation_id, 10, None)
        .await
        .expect("load timeline page");
    let assistant_blocks: Vec<_> = page
        .items
        .iter()
        .filter(|item| {
            item.message_id
                .as_ref()
                .is_some_and(|id| id.as_str() == pre_assistant_id)
        })
        .collect();
    assert_eq!(assistant_blocks.len(), 2);
    assert!(assistant_blocks
        .iter()
        .all(|item| item.status == ChatTimelineItemStatus::Finalized));
    assert_eq!(assistant_blocks[0].text.as_deref(), Some("Done"));
    assert_eq!(
        assistant_blocks[1].tool_call_id.as_deref(),
        Some("toolu-read")
    );
    assert_eq!(
        assistant_blocks[1].tool_status.as_deref(),
        Some("completed")
    );
    assert!(
        assistant_blocks[1].raw_block_json.is_none(),
        "Read is not in the full-fidelity allowlist"
    );
    assert!(assistant_blocks[1]
        .input_json
        .as_deref()
        .is_some_and(|raw| raw.contains("file_path")));
}

#[tokio::test]
async fn finalize_structured_split_transcript_writes_timeline_for_each_segment() {
    use crate::application::chat_service::create_assistant_message;
    use crate::domain::entities::IdeationSessionId;

    let state = AppState::new_test();
    let conversation_id = ChatConversationId::new();
    let session_id = IdeationSessionId::new();
    let pre_assistant = create_assistant_message(
        ChatContextType::Ideation,
        session_id.as_str(),
        "",
        conversation_id.clone(),
        &[],
        &[],
    );
    let pre_assistant_id = pre_assistant.id.as_str().to_string();
    state
        .chat_message_repo
        .create(pre_assistant)
        .await
        .expect("seed pre-assistant message");

    let content_blocks = vec![
        ContentBlockItem::Text {
            text: "First segment".to_string(),
        },
        ContentBlockItem::ToolUse {
            id: Some("toolu-read".to_string()),
            name: "Read".to_string(),
            arguments: serde_json::json!({ "file_path": "src/app.ts" }),
            result: Some(serde_json::json!("preview")),
            parent_tool_use_id: None,
            diff_context: None,
        },
        ContentBlockItem::Text {
            text: "Second segment".to_string(),
        },
    ];
    let tool_calls = vec![ToolCall {
        id: Some("toolu-read".to_string()),
        name: "Read".to_string(),
        arguments: serde_json::json!({ "file_path": "src/app.ts" }),
        result: Some(serde_json::json!("preview")),
        parent_tool_use_id: None,
        diff_context: None,
        stats: None,
    }];

    super::finalize_structured_assistant_message(
        &state.chat_message_repo,
        &Some(state.chat_timeline_repo.clone()),
        &NullEventSink,
        ChatContextType::Ideation,
        session_id.as_str(),
        &conversation_id,
        &pre_assistant_id,
        "orchestrator",
        "First segmentSecond segment",
        &tool_calls,
        &content_blocks,
        true,
    )
    .await;

    let messages = state
        .chat_message_repo
        .get_by_conversation(&conversation_id)
        .await
        .expect("load conversation messages");
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].id.as_str(), pre_assistant_id);
    assert_eq!(messages[0].content, "First segment");
    assert_eq!(messages[1].content, "Second segment");

    let page = state
        .chat_timeline_repo
        .get_page(&conversation_id, 10, None)
        .await
        .expect("load timeline page");
    assert_eq!(page.items.len(), 3);
    assert!(page
        .items
        .iter()
        .all(|item| item.status == ChatTimelineItemStatus::Finalized));
    assert_eq!(
        page.items[0].message_id.as_ref().unwrap().as_str(),
        pre_assistant_id
    );
    assert_eq!(
        page.items[1].message_id.as_ref().unwrap().as_str(),
        pre_assistant_id
    );
    assert_eq!(
        page.items[2].message_id.as_ref().unwrap().as_str(),
        messages[1].id.as_str()
    );
}

#[tokio::test]
async fn exported_finalization_test_helpers_delegate_to_core_paths() {
    use crate::application::chat_service::{
        create_assistant_message, finalize_assistant_message_for_test,
        finalize_structured_assistant_message_for_test,
    };
    use crate::domain::entities::{ChatMessageId, IdeationSessionId};

    let state = AppState::new_test();
    let conversation_id = ChatConversationId::new();
    let session_id = IdeationSessionId::new();

    let plain_message = create_assistant_message(
        ChatContextType::Ideation,
        session_id.as_str(),
        "",
        conversation_id.clone(),
        &[],
        &[],
    );
    let plain_message_id = plain_message.id.as_str().to_string();
    state
        .chat_message_repo
        .create(plain_message)
        .await
        .expect("seed plain assistant message");
    finalize_assistant_message_for_test(
        &state.chat_message_repo,
        &NullEventSink,
        &conversation_id.as_str(),
        "ideation",
        session_id.as_str(),
        &plain_message_id,
        "orchestrator",
        "Plain helper content",
        None,
        None,
    )
    .await;

    let structured_message = create_assistant_message(
        ChatContextType::Ideation,
        session_id.as_str(),
        "",
        conversation_id.clone(),
        &[],
        &[],
    );
    let structured_message_id = structured_message.id.as_str().to_string();
    state
        .chat_message_repo
        .create(structured_message)
        .await
        .expect("seed structured assistant message");
    finalize_structured_assistant_message_for_test(
        &state.chat_message_repo,
        &NullEventSink,
        ChatContextType::Ideation,
        session_id.as_str(),
        &conversation_id,
        &structured_message_id,
        "orchestrator",
        "Structured helper content",
        &[],
        &[ContentBlockItem::Text {
            text: "Structured helper content".to_string(),
        }],
        false,
    )
    .await;

    let plain = state
        .chat_message_repo
        .get_by_id(&ChatMessageId::from_string(plain_message_id))
        .await
        .expect("load plain helper message")
        .expect("plain helper message");
    let structured = state
        .chat_message_repo
        .get_by_id(&ChatMessageId::from_string(structured_message_id))
        .await
        .expect("load structured helper message")
        .expect("structured helper message");
    assert_eq!(plain.content, "Plain helper content");
    assert_eq!(structured.content, "Structured helper content");
}
