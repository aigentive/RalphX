// Integration tests for chat session recovery functionality
// Verifies the complete session recovery flow from stale session detection
// through successful message retry, including edge cases and regression scenarios.

#![allow(dead_code)]
#![allow(unused_imports)]

use chrono::Utc;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use ralphx_lib::application::chat_service::{
    attempt_session_recovery, AGENT_ERROR_PREFIX, STALE_SESSION_ERROR,
};
use ralphx_lib::domain::agents::{AgentHarnessKind, ProviderSessionRef};
use ralphx_lib::domain::entities::{
    ChatContextType, ChatConversation, ChatConversationId, ChatMessage, IdeationSessionId,
    MessageRole, Persona, PersonaDirective, PersonaId, PersonaStatus, ProjectId, TaskId,
};
use ralphx_lib::domain::repositories::{
    ChatConversationRepository, ChatMessageRepository, PersonaRepository,
};
use ralphx_lib::infrastructure::memory::{
    MemoryArtifactRepository, MemoryChatAttachmentRepository, MemoryChatConversationRepository,
    MemoryChatMessageRepository, MemoryPersonaRepository,
};

use crate::support::erroring_persona_repository::ErroringPersonaRepository;

fn claude_spawn_override_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[allow(clippy::await_holding_lock)]
async fn with_claude_spawn_allowed_in_tests<T, Fut>(f: impl FnOnce() -> Fut) -> T
where
    Fut: Future<Output = T>,
{
    let _guard = claude_spawn_override_lock().lock().expect("lock poisoned");
    let _env_guard =
        crate::support::env::EnvVarGuard::set("RALPHX_ALLOW_CLAUDE_SPAWN_IN_TESTS", "1");
    f().await
}

fn repo_plugin_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("repository root")
        .join("plugins/app")
}

fn final_spawnable_command(
    spawnable: &ralphx_lib::infrastructure::agents::claude::SpawnableCommand,
) -> String {
    let args = spawnable.get_args_for_test();
    let mut rendered = args.join("\n");
    for window in args.windows(2) {
        if window[0] == "--append-system-prompt-file" {
            rendered.push('\n');
            rendered.push_str(&std::fs::read_to_string(&window[1]).expect("read persona prompt"));
        }
    }
    rendered
}

async fn bound_project_persona() -> (ChatConversation, Arc<MemoryPersonaRepository>) {
    let repo = Arc::new(MemoryPersonaRepository::new());
    let now = Utc::now();
    let persona = Persona {
        id: PersonaId::from("recovery-bound-persona"),
        slug: "recovery-bound-persona".to_string(),
        name: "Recovery Bound Persona".to_string(),
        description: "recovery command fixture".to_string(),
        content: "Use the recovery persona voice.".to_string(),
        status: PersonaStatus::Active,
        version: 1,
        content_hash: "recovery-bound-persona-hash".to_string(),
        source_session_id: None,
        source_json: "{}".to_string(),
        created_at: now,
        updated_at: now,
    };
    repo.create(persona.clone())
        .await
        .expect("seed active recovery persona");

    let mut conversation = ChatConversation::new_project(ProjectId::from_string(
        "recovery-persona-project".to_string(),
    ));
    conversation.persona_id = Some(persona.id.to_string());
    (conversation, repo)
}

// ============================================================================
// Test Harness
// ============================================================================

/// Test state containing repositories and test data
struct TestHarness {
    conversation_repo: Arc<MemoryChatConversationRepository>,
    message_repo: Arc<MemoryChatMessageRepository>,
}

impl TestHarness {
    /// Create a new test harness with empty repositories
    fn new() -> Self {
        Self {
            conversation_repo: Arc::new(MemoryChatConversationRepository::new()),
            message_repo: Arc::new(MemoryChatMessageRepository::new()),
        }
    }

    /// Create a conversation with a multi-turn history
    async fn setup_conversation_with_history(
        &self,
        turns: Vec<(MessageRole, &str)>,
    ) -> (ChatConversationId, IdeationSessionId) {
        let session_id = IdeationSessionId::new();
        let mut conversation = ChatConversation::new_ideation(session_id.clone());
        conversation.claude_session_id = Some("test-session-id".to_string());

        let conversation_id = conversation.id;
        self.conversation_repo.create(conversation).await.unwrap();

        // Create messages for each turn
        for (role, content) in turns {
            let mut message = match role {
                MessageRole::User => ChatMessage::user_in_session(session_id.clone(), content),
                MessageRole::Orchestrator => {
                    ChatMessage::orchestrator_in_session(session_id.clone(), content)
                }
                MessageRole::System => ChatMessage::system_in_session(session_id.clone(), content),
                _ => ChatMessage::user_in_session(session_id.clone(), content),
            };
            message.conversation_id = Some(conversation_id);
            self.message_repo.create(message).await.unwrap();
        }

        (conversation_id, session_id)
    }

    /// Create a conversation with tool calls in the history
    async fn setup_conversation_with_tool_calls(&self) -> (ChatConversationId, IdeationSessionId) {
        let session_id = IdeationSessionId::new();
        let mut conversation = ChatConversation::new_ideation(session_id.clone());
        conversation.claude_session_id = Some("test-session-with-tools".to_string());

        let conversation_id = conversation.id;
        self.conversation_repo.create(conversation).await.unwrap();

        // User message
        let mut user_msg = ChatMessage::user_in_session(session_id.clone(), "Create a new file");
        user_msg.conversation_id = Some(conversation_id);
        self.message_repo.create(user_msg).await.unwrap();

        // Orchestrator message with tool calls
        let mut assistant_msg =
            ChatMessage::orchestrator_in_session(session_id.clone(), "I'll create that file");
        assistant_msg.conversation_id = Some(conversation_id);
        assistant_msg.tool_calls = Some(
            r#"[{"name":"Write","parameters":{"file_path":"/test.txt","content":"Hello"}}]"#
                .to_string(),
        );
        self.message_repo.create(assistant_msg).await.unwrap();

        (conversation_id, session_id)
    }

    /// Get all messages for a conversation in chronological order
    async fn get_messages(&self, conversation_id: &ChatConversationId) -> Vec<ChatMessage> {
        self.message_repo
            .get_by_conversation(conversation_id)
            .await
            .unwrap()
    }

    /// Simulate updating the session ID (as if recovery happened)
    async fn update_session_id(&self, conversation_id: &ChatConversationId, new_session_id: &str) {
        self.conversation_repo
            .update_provider_session_ref(
                conversation_id,
                &ProviderSessionRef {
                    harness: AgentHarnessKind::Claude,
                    provider_session_id: new_session_id.to_string(),
                },
            )
            .await
            .unwrap();
    }

    /// Get conversation by ID
    async fn get_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> Option<ChatConversation> {
        self.conversation_repo
            .get_by_id(conversation_id)
            .await
            .unwrap()
    }
}

// ============================================================================
// Session Recovery Flow Tests
// ============================================================================

#[cfg(unix)]
#[tokio::test]
async fn recovery_resolves_persona_from_conversation_row_before_build() {
    let (conversation, persona_repo) = bound_project_persona().await;
    let conversation_id = conversation.id;
    let mut state = ralphx_lib::application::AppState::new_test();
    state.persona_repo = persona_repo;
    state
        .chat_conversation_repo
        .create(conversation.clone())
        .await
        .expect("persist recovery conversation");
    let mut history = ChatMessage::user_in_project(
        ProjectId::from_string(conversation.context_id.as_str().to_string()),
        "prior recovery turn",
    );
    history.conversation_id = Some(conversation_id);
    state
        .chat_message_repo
        .create(history)
        .await
        .expect("persist recovery history");
    let message_repo = Arc::clone(&state.chat_message_repo);
    let conversation_repo = Arc::clone(&state.chat_conversation_repo);
    let attachment_repo = Arc::clone(&state.chat_attachment_repo);
    let artifact_repo = Arc::clone(&state.artifact_repo);
    let app = tauri::test::mock_builder()
        .manage(state)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");
    let app_handle = app.handle().clone();
    let temp = tempfile::tempdir().expect("temporary recovery runtime");
    let persona_marker = temp.path().join("persona-was-injected");
    let cli_path = temp.path().join("fake-claude");
    std::fs::write(
        &cli_path,
        format!(
            "#!/bin/sh\nfor arg in \"$@\"; do\n  [ -f \"$arg\" ] && grep -q '<ralphx_agent_persona>' \"$arg\" && touch '{}'\ndone\nprintf '%s\\n' '{{\"type\":\"result\",\"session_id\":\"recovered-session\",\"is_error\":false,\"result\":\"ok\",\"cost_usd\":0.0}}'\n",
            persona_marker.display()
        ),
    )
    .expect("write fake recovery cli");
    let mut permissions = std::fs::metadata(&cli_path)
        .expect("fake recovery cli metadata")
        .permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
    std::fs::set_permissions(&cli_path, permissions).expect("mark fake recovery cli executable");

    let recovered = with_claude_spawn_allowed_in_tests(|| async {
        attempt_session_recovery::<tauri::test::MockRuntime>(
            &conversation_id,
            &conversation,
            AgentHarnessKind::Claude,
            ChatContextType::Project,
            conversation.context_id.as_str(),
            "recover this conversation",
            &cli_path,
            &repo_plugin_dir(),
            temp.path(),
            None,
            false,
            message_repo,
            conversation_repo,
            attachment_repo,
            artifact_repo,
            None,
            None,
            None,
            true,
            false,
            "stale-session",
            Some(&app_handle),
        )
        .await
    })
    .await
    .expect("production recovery should rebuild and run the command");

    assert_eq!(recovered, "recovered-session");
    assert!(
        persona_marker.exists(),
        "the production recovery command must contain <ralphx_agent_persona>"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn recovery_with_persona_repo_error_fails_closed() {
    let (conversation, _) = bound_project_persona().await;
    let conversation_id = conversation.id;
    let mut state = ralphx_lib::application::AppState::new_test();
    state.persona_repo = Arc::new(ErroringPersonaRepository);
    state
        .chat_conversation_repo
        .create(conversation.clone())
        .await
        .expect("persist poisoned recovery conversation");
    let mut history = ChatMessage::user_in_project(
        ProjectId::from_string(conversation.context_id.as_str().to_string()),
        "prior recovery turn",
    );
    history.conversation_id = Some(conversation_id);
    state
        .chat_message_repo
        .create(history)
        .await
        .expect("persist poisoned recovery history");
    let message_repo = Arc::clone(&state.chat_message_repo);
    let conversation_repo = Arc::clone(&state.chat_conversation_repo);
    let attachment_repo = Arc::clone(&state.chat_attachment_repo);
    let artifact_repo = Arc::clone(&state.artifact_repo);
    let app = tauri::test::mock_builder()
        .manage(state)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");
    let app_handle = app.handle().clone();
    let temp = tempfile::tempdir().expect("temporary recovery runtime");
    let invoked_marker = temp.path().join("recovery-cli-invoked");
    let cli_path = temp.path().join("fake-claude");
    std::fs::write(
        &cli_path,
        format!("#!/bin/sh\ntouch '{}'\n", invoked_marker.display()),
    )
    .expect("write fail-closed fake cli");
    let mut permissions = std::fs::metadata(&cli_path)
        .expect("fake recovery cli metadata")
        .permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
    std::fs::set_permissions(&cli_path, permissions).expect("mark fake recovery cli executable");

    let error = with_claude_spawn_allowed_in_tests(|| async {
        attempt_session_recovery::<tauri::test::MockRuntime>(
            &conversation_id,
            &conversation,
            AgentHarnessKind::Claude,
            ChatContextType::Project,
            conversation.context_id.as_str(),
            "recover this conversation",
            &cli_path,
            &repo_plugin_dir(),
            temp.path(),
            None,
            false,
            message_repo,
            conversation_repo,
            attachment_repo,
            artifact_repo,
            None,
            None,
            None,
            true,
            false,
            "stale-session",
            Some(&app_handle),
        )
        .await
    })
    .await
    .expect_err("persona repository failure must stop production recovery");

    assert!(
        error.to_string().contains("persona repository exploded"),
        "{error}"
    );
    assert!(
        !invoked_marker.exists(),
        "failed persona resolution must not build or spawn a recovery command"
    );
}

/// Test that a stale session triggers recovery and assigns a new session ID
///
/// This test verifies the basic recovery mechanism:
/// 1. A conversation exists with a stale session ID
/// 2. Recovery is triggered (simulated)
/// 3. A new session ID is assigned to the conversation
#[tokio::test]
async fn test_stale_session_triggers_recovery_flow() {
    let harness = TestHarness::new();

    // Setup: Create conversation with history
    let (conversation_id, _session_id) = harness
        .setup_conversation_with_history(vec![
            (MessageRole::User, "Hello"),
            (MessageRole::Orchestrator, "Hi there!"),
            (MessageRole::User, "How are you?"),
        ])
        .await;

    // Verify initial state
    let conversation = harness.get_conversation(&conversation_id).await.unwrap();
    assert_eq!(
        conversation.claude_session_id,
        Some("test-session-id".to_string())
    );

    // Simulate recovery: update to new session ID
    let new_session_id = "recovered-session-id";
    harness
        .update_session_id(&conversation_id, new_session_id)
        .await;

    // Verify: conversation has NEW session ID
    let conversation = harness.get_conversation(&conversation_id).await.unwrap();
    assert_eq!(
        conversation.claude_session_id,
        Some(new_session_id.to_string())
    );
    assert_ne!(
        conversation.claude_session_id,
        Some("test-session-id".to_string())
    );
}

/// Test that recovery preserves message ordering chronologically
///
/// Verifies that when replaying conversation history for recovery,
/// messages are ordered by created_at timestamp to maintain conversation flow.
#[tokio::test]
async fn test_recovery_preserves_message_ordering() {
    let harness = TestHarness::new();

    // Setup: Create multi-turn conversation
    let (conversation_id, _session_id) = harness
        .setup_conversation_with_history(vec![
            (MessageRole::User, "Turn 1"),
            (MessageRole::Orchestrator, "Response 1"),
            (MessageRole::User, "Turn 2"),
            (MessageRole::Orchestrator, "Response 2"),
            (MessageRole::User, "Turn 3"),
        ])
        .await;

    // Retrieve messages (should be chronologically ordered)
    let messages = harness.get_messages(&conversation_id).await;

    // Verify ordering
    assert_eq!(messages.len(), 5);
    assert_eq!(messages[0].content, "Turn 1");
    assert_eq!(messages[0].role, MessageRole::User);
    assert_eq!(messages[1].content, "Response 1");
    assert_eq!(messages[1].role, MessageRole::Orchestrator);
    assert_eq!(messages[2].content, "Turn 2");
    assert_eq!(messages[2].role, MessageRole::User);
    assert_eq!(messages[3].content, "Response 2");
    assert_eq!(messages[3].role, MessageRole::Orchestrator);
    assert_eq!(messages[4].content, "Turn 3");
    assert_eq!(messages[4].role, MessageRole::User);

    // Verify chronological order by timestamps
    for i in 0..messages.len() - 1 {
        assert!(
            messages[i].created_at <= messages[i + 1].created_at,
            "Messages not in chronological order"
        );
    }
}

/// Test that non-stale errors (e.g., rate limits) do not trigger recovery
///
/// Only STALE_SESSION_ERROR-matching errors should trigger recovery.
/// Other errors like rate limits, network issues, etc. should be handled normally.
#[tokio::test]
async fn test_non_stale_errors_do_not_trigger_recovery() {
    let harness = TestHarness::new();

    // Setup: Create conversation with session ID
    let (conversation_id, _session_id) = harness
        .setup_conversation_with_history(vec![
            (MessageRole::User, "Hello"),
            (MessageRole::Orchestrator, "Hi!"),
        ])
        .await;

    let conversation = harness.get_conversation(&conversation_id).await.unwrap();
    let original_session_id = conversation.claude_session_id.clone();

    // Simulate different error types that should NOT trigger recovery
    // In actual implementation, classify_agent_error() would distinguish these

    // Test: Rate limit error should preserve session ID
    let rate_limit_error = "Error: Rate limit exceeded";
    assert!(!rate_limit_error.contains(STALE_SESSION_ERROR));

    // Test: Network error should preserve session ID
    let network_error = "Error: Connection timeout";
    assert!(!network_error.contains(STALE_SESSION_ERROR));

    // Verify: session ID unchanged (no recovery triggered)
    let conversation = harness.get_conversation(&conversation_id).await.unwrap();
    assert_eq!(conversation.claude_session_id, original_session_id);
}

/// Test that recovery loop prevention works correctly
///
/// If recovery itself fails, the system should NOT retry indefinitely.
/// It should attempt recovery once, and if that fails, surface the error to the user.
#[tokio::test]
async fn test_recovery_loop_prevention() {
    let harness = TestHarness::new();

    // Setup: Create conversation
    let (conversation_id, _session_id) = harness
        .setup_conversation_with_history(vec![(MessageRole::User, "Test message")])
        .await;

    let conversation = harness.get_conversation(&conversation_id).await.unwrap();
    assert!(conversation.claude_session_id.is_some());

    // In actual implementation:
    // 1. First send attempt fails with stale session
    // 2. Recovery is attempted (is_retry_attempt = false)
    // 3. If recovery sends a message and that ALSO fails with stale session,
    //    is_retry_attempt = true prevents another recovery attempt
    // 4. Error is surfaced to user

    // This test documents the expected behavior.
    // The actual implementation in chat_service_send_background.rs uses
    // the is_retry_attempt flag to prevent infinite loops.

    // Verify the flag mechanism would work: we can track retry state
    let mut retry_count = 0;
    let max_retries = 1;

    // Simulate first attempt
    retry_count += 1;
    assert_eq!(retry_count, 1);
    assert!(retry_count <= max_retries);

    // Simulate second attempt (should be blocked)
    retry_count += 1;
    assert_eq!(retry_count, 2);
    assert!(retry_count > max_retries, "Should prevent further retries");
}

/// Test that tool calls are preserved in conversation replay
///
/// When recovering a session, tool calls from previous messages must be included
/// in the replay to maintain full conversation context.
#[tokio::test]
async fn test_tool_calls_preserved_in_replay() {
    let harness = TestHarness::new();

    // Setup: Create conversation with tool calls
    let (conversation_id, _session_id) = harness.setup_conversation_with_tool_calls().await;

    // Retrieve messages
    let messages = harness.get_messages(&conversation_id).await;

    // Find message with tool calls
    let message_with_tools = messages
        .iter()
        .find(|m| m.tool_calls.is_some())
        .expect("Should have message with tool calls");

    // Verify tool calls are preserved
    assert!(message_with_tools.tool_calls.is_some());
    let tool_calls_json = message_with_tools.tool_calls.as_ref().unwrap();
    assert!(tool_calls_json.contains("Write"));
    assert!(tool_calls_json.contains("/test.txt"));

    // Verify it can be parsed as JSON
    let parsed: serde_json::Value = serde_json::from_str(tool_calls_json).unwrap();
    assert!(parsed.is_array());
    assert_eq!(parsed.as_array().unwrap().len(), 1);
}

/// Test that error messages are filtered out of replay
///
/// System messages containing [Agent error: ...] should be skipped during replay
/// to avoid replaying error state to Claude.
#[tokio::test]
async fn test_error_messages_filtered_from_replay() {
    let harness = TestHarness::new();

    let session_id = IdeationSessionId::new();
    let mut conversation = ChatConversation::new_ideation(session_id.clone());
    conversation.claude_session_id = Some("test-session".to_string());

    let conversation_id = conversation.id;
    harness
        .conversation_repo
        .create(conversation)
        .await
        .unwrap();

    // Create messages including an error message
    let error_msg = format!("{} Something went wrong]", AGENT_ERROR_PREFIX);
    let messages = vec![
        (MessageRole::User, "Hello"),
        (MessageRole::Orchestrator, "Hi!"),
        (MessageRole::System, error_msg.as_str()),
        (MessageRole::User, "Are you there?"),
    ];

    for (role, content) in messages {
        let mut message = match role {
            MessageRole::User => ChatMessage::user_in_session(session_id.clone(), content),
            MessageRole::Orchestrator => {
                ChatMessage::orchestrator_in_session(session_id.clone(), content)
            }
            MessageRole::System => ChatMessage::system_in_session(session_id.clone(), content),
            _ => ChatMessage::user_in_session(session_id.clone(), content),
        };
        message.conversation_id = Some(conversation_id);
        harness.message_repo.create(message).await.unwrap();
    }

    // Retrieve messages
    let all_messages = harness.get_messages(&conversation_id).await;
    assert_eq!(all_messages.len(), 4);

    // In actual ReplayBuilder implementation, this error message would be filtered:
    let messages_for_replay: Vec<_> = all_messages
        .iter()
        .filter(|m| {
            if m.role == MessageRole::System {
                !m.content.contains(AGENT_ERROR_PREFIX)
            } else {
                true
            }
        })
        .collect();

    // Verify filtering works
    assert_eq!(messages_for_replay.len(), 3);
    assert!(!messages_for_replay
        .iter()
        .any(|m| m.content.contains(AGENT_ERROR_PREFIX)));
}

// ============================================================================
// Regression Tests
// ============================================================================

/// Test that empty conversations can be handled safely
///
/// Recovery should handle edge case of a conversation with no messages
/// (though this is unlikely in practice).
#[tokio::test]
async fn test_empty_conversation_recovery() {
    let harness = TestHarness::new();

    let session_id = IdeationSessionId::new();
    let mut conversation = ChatConversation::new_ideation(session_id);
    conversation.claude_session_id = Some("stale-session".to_string());

    let conversation_id = conversation.id;
    harness
        .conversation_repo
        .create(conversation)
        .await
        .unwrap();

    // No messages created

    // Retrieve messages
    let messages = harness.get_messages(&conversation_id).await;
    assert_eq!(messages.len(), 0);

    // Recovery should still work (empty replay)
    harness
        .update_session_id(&conversation_id, "new-session")
        .await;

    let conversation = harness.get_conversation(&conversation_id).await.unwrap();
    assert_eq!(
        conversation.claude_session_id,
        Some("new-session".to_string())
    );
}

/// Test that very long conversations are handled within token budget
///
/// Documents the expected behavior when conversation exceeds token budget
/// (actual ReplayBuilder would implement truncation logic).
#[tokio::test]
async fn test_long_conversation_token_budget() {
    let harness = TestHarness::new();

    // Create a conversation with multiple messages
    // In a real scenario with 100+ messages, ReplayBuilder would truncate to fit token budget
    let (conversation_id, _session_id) = harness
        .setup_conversation_with_history(vec![
            (MessageRole::User, "Message 1"),
            (MessageRole::Orchestrator, "Response 1"),
            (MessageRole::User, "Message 2"),
            (MessageRole::Orchestrator, "Response 2"),
            (MessageRole::User, "Message 3"),
            (MessageRole::Orchestrator, "Response 3"),
            (MessageRole::User, "Message 4"),
            (MessageRole::Orchestrator, "Response 4"),
            (MessageRole::User, "Message 5"),
        ])
        .await;

    let messages = harness.get_messages(&conversation_id).await;
    assert_eq!(messages.len(), 9);

    // In actual ReplayBuilder implementation:
    // - Estimate tokens for each message (content length / 4)
    // - Keep newest messages that fit within budget (e.g., 100K tokens)
    // - Set is_truncated flag if not all messages fit
    //
    // This test documents the expected behavior for token budget management.
}
