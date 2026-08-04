// Regression coverage for the queue-drain user-message delivery invariant.

use async_trait::async_trait;
use chrono::{Duration, Utc};
use ralphx_events::RecordingEventSink;
use ralphx_lib::application::chat_service::{process_queued_messages_for_test, ChatService};
use ralphx_lib::application::interactive_process_registry::{
    InteractiveProcessKey, InteractiveProcessMetadata, PendingStdinTurn,
};
use ralphx_lib::application::AppState;
use ralphx_lib::domain::agents::{AgentHarnessKind, AgentProviderSettings};
use ralphx_lib::domain::entities::{
    AgentRun, AgentRunAttribution, AgentRunId, AgentRunStatus, AgentRunUsage, ChatContextType,
    ChatConversation, ChatConversationId, ChatMessage, ChatMessageId, InterruptedConversation,
    MessageRole, Project,
};
use ralphx_lib::domain::repositories::AgentRunRepository;
use ralphx_lib::domain::services::{MessageQueue, QueuedMessage};
use ralphx_lib::error::{AppError, AppResult};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::Manager;
use tokio::process::Command;

const STALE_SECS: u64 = 300;

fn old_message(id: &str, content: &str, metadata: Option<&str>) -> QueuedMessage {
    let mut message = QueuedMessage::with_id(id.to_string(), content.to_string());
    message.created_at = (Utc::now() - Duration::seconds(STALE_SECS as i64 + 1)).to_rfc3339();
    message.metadata_override = metadata.map(str::to_string);
    message
}

fn fixture(temp: &Path) -> (PathBuf, PathBuf) {
    let cli = temp.join("delivery-claude");
    let calls = temp.join("calls.log");
    fs::write(&cli, format!("#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo '2.1.0 (Claude Code)'; exit; fi\nif [ \"$1\" = \"--help\" ]; then echo '--resume --output-format'; exit; fi\nif [ \"$1\" = \"--thinking-display\" ]; then echo '2.1.0 (Claude Code)'; exit; fi\necho call >> '{}'\ncat >/dev/null\necho '{{\"type\":\"result\",\"session_id\":\"delivery-session\",\"is_error\":false,\"result\":\"ok\",\"cost_usd\":0.0}}'\n", calls.display())).expect("write fixture");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o755))
            .expect("make fixture executable");
    }
    (cli, calls)
}

/// The drain hands delivery to a background spawn; poll for the fixture's
/// call marker instead of asserting on it immediately.
async fn wait_for_file(path: &Path) {
    for _ in 0..200 {
        if path.is_file() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("timed out waiting for {}", path.display());
}

async fn seed(state: &AppState, label: &str) -> (Project, ChatConversation) {
    // The fresh-session replay path resolves the project root for the real
    // spawn pipeline, which rejects relative roots.
    let project_root = std::env::current_dir()
        .expect("absolute project root")
        .to_string_lossy()
        .into_owned();
    let project = state
        .project_repo
        .create(Project::new(label.to_string(), project_root))
        .await
        .expect("project");
    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project.id.clone()))
        .await
        .expect("conversation");
    (project, conversation)
}

async fn completed_owner(state: &AppState, conversation: ChatConversationId, session: &str) {
    let mut run = AgentRun::new(conversation);
    run.complete();
    run.harness = Some(AgentHarnessKind::Claude);
    run.provider_session_id = Some(session.to_string());
    state
        .agent_run_repo
        .create(run)
        .await
        .expect("completed owner");
}

async fn enable_fixture(state: &AppState, cli: &Path) {
    let mut settings = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Claude);
    settings.enabled = true;
    settings.is_default = true;
    settings.custom_binary_enabled = true;
    settings.custom_binary_path = Some(cli.to_string_lossy().into_owned());
    state
        .agent_provider_settings_repo
        .upsert(&settings)
        .await
        .expect("provider fixture");
}

#[tokio::test]
async fn old_queued_user_message_survives_exit_cleanup_and_drains() {
    let _allow_spawn =
        crate::support::env::EnvVarGuard::set("RALPHX_ALLOW_CLAUDE_SPAWN_IN_TESTS", "1");
    let state = AppState::new_test();
    let (project, conversation) = seed(&state, "old queue user").await;
    let temp = tempfile::tempdir().expect("fixture dir");
    let (cli, calls) = fixture(temp.path());
    enable_fixture(&state, &cli).await;
    let message = old_message("old-user", "deliver me", None);
    state.message_queue.queue_front_existing(
        ChatContextType::Project,
        conversation.id.as_str(),
        message,
    );
    completed_owner(&state, conversation.id, "completed-session").await;
    assert!(
        state
            .message_queue
            .remove_stale(
                ChatContextType::Project,
                &conversation.id.as_str(),
                STALE_SECS
            )
            .is_empty(),
        "exit cleanup must retain user messages"
    );
    let app = tauri::test::mock_builder()
        .manage(state)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("app");
    let state = app.state::<AppState>();
    let (processed, _) = process_queued_messages_for_test(
        state.inner(),
        None,
        Arc::clone(&state.events),
        ChatContextType::Project,
        AgentHarnessKind::Claude,
        project.id.as_str(),
        conversation.id,
        "completed-session",
        &cli,
    )
    .await;
    assert_eq!(processed, 1);
    wait_for_file(&calls).await;
    assert!(app
        .state::<AppState>()
        .message_queue
        .get_queued(ChatContextType::Project, &conversation.id.as_str())
        .is_empty());
}

#[tokio::test]
async fn missing_completed_owner_replays_fresh_without_queued_preflight_failure() {
    let _allow_spawn =
        crate::support::env::EnvVarGuard::set("RALPHX_ALLOW_CLAUDE_SPAWN_IN_TESTS", "1");
    let mut state = AppState::new_test();
    let events = RecordingEventSink::new();
    state.events = Arc::new(events.clone());
    let (project, conversation) = seed(&state, "fresh replay").await;
    let temp = tempfile::tempdir().expect("fixture dir");
    let (cli, calls) = fixture(temp.path());
    enable_fixture(&state, &cli).await;
    state.message_queue.queue_front_existing(
        ChatContextType::Project,
        conversation.id.as_str(),
        old_message("fresh-replay", "replay once", None),
    );
    let app = tauri::test::mock_builder()
        .manage(state)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("app");
    let state = app.state::<AppState>();
    let (processed, run_id) = process_queued_messages_for_test(
        state.inner(),
        None,
        Arc::clone(&state.events),
        ChatContextType::Project,
        AgentHarnessKind::Claude,
        project.id.as_str(),
        conversation.id,
        "no-completed-owner",
        &cli,
    )
    .await;
    assert_eq!(processed, 1);
    assert!(run_id.is_some());
    wait_for_file(&calls).await;
    assert!(
        app.state::<AppState>()
            .message_queue
            .get_queued(ChatContextType::Project, &conversation.id.as_str())
            .is_empty(),
        "fresh replay cannot lose the message"
    );
    assert!(
        events
            .events()
            .iter()
            .filter(|event| event.event == "agent:error")
            .all(|event| !event.payload.to_string().contains("queued_preflight")),
        "no queued_preflight error event"
    );
    assert!(
        app.state::<AppState>()
            .agent_run_repo
            .get_by_conversation(&conversation.id)
            .await
            .expect("runs")
            .iter()
            .all(|run| run.error_message.as_deref() != Some("queued_preflight")),
        "no queued_preflight failure run"
    );
}

#[tokio::test]
async fn continuation_resolution_error_restores_queue_front_and_emits_error() {
    let mut state = AppState::new_test();
    let events = RecordingEventSink::new();
    state.events = Arc::new(events.clone());
    let (project, conversation) = seed(&state, "continuation error").await;
    state.agent_run_repo = Arc::new(FailingContinuationRepo);
    let message = old_message("restore-front", "retain me", None);
    state.message_queue.queue_front_existing(
        ChatContextType::Project,
        conversation.id.as_str(),
        message.clone(),
    );
    let app = tauri::test::mock_builder()
        .manage(state)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("app");
    let state = app.state::<AppState>();
    let (processed, run_id) = process_queued_messages_for_test(
        state.inner(),
        None,
        Arc::clone(&state.events),
        ChatContextType::Project,
        AgentHarnessKind::Claude,
        project.id.as_str(),
        conversation.id,
        "repo-error",
        Path::new("/definitely/missing/cli"),
    )
    .await;
    assert_eq!(processed, 0);
    assert!(run_id.is_none());
    assert_eq!(
        app.state::<AppState>()
            .message_queue
            .get_queued(ChatContextType::Project, &conversation.id.as_str()),
        vec![message],
        "failed resolution restores the original message at queue front"
    );
    let recorded_events = events.events();
    assert!(recorded_events
        .iter()
        .filter(|event| event.event == "agent:error")
        .any(|event| event
            .payload
            .to_string()
            .contains("injected continuation resolution failure")));
    assert!(
        recorded_events
            .iter()
            .all(|event| event.event != "agent:queue_sent"),
        "restored queue truth must not be followed by a terminal queue_sent event"
    );
}

#[test]
fn old_hidden_recovery_message_still_drops() {
    let queue = MessageQueue::new();
    let hidden = old_message("hidden", "internal", Some(r#"{"hidden_from_ui":true}"#));
    queue.queue_front_existing(ChatContextType::Project, "hidden-project", hidden.clone());
    assert_eq!(
        queue.remove_stale(ChatContextType::Project, "hidden-project", STALE_SECS),
        vec![hidden]
    );
}

#[tokio::test]
async fn replayed_message_is_delivered_exactly_once_on_reentry() {
    let _allow_spawn =
        crate::support::env::EnvVarGuard::set("RALPHX_ALLOW_CLAUDE_SPAWN_IN_TESTS", "1");
    let mut state = AppState::new_test();
    let events = RecordingEventSink::new();
    state.events = Arc::new(events.clone());
    let (project, conversation) = seed(&state, "exactly once").await;
    let temp = tempfile::tempdir().expect("fixture dir");
    let (cli, calls) = fixture(temp.path());
    enable_fixture(&state, &cli).await;
    state.message_queue.queue_front_existing(
        ChatContextType::Project,
        conversation.id.as_str(),
        old_message("once", "one replay", None),
    );
    let app = tauri::test::mock_builder()
        .manage(state)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("app");
    let state = app.state::<AppState>();
    let first = process_queued_messages_for_test(
        state.inner(),
        None,
        Arc::clone(&state.events),
        ChatContextType::Project,
        AgentHarnessKind::Claude,
        project.id.as_str(),
        conversation.id,
        "unowned",
        &cli,
    )
    .await;
    let second = process_queued_messages_for_test(
        state.inner(),
        None,
        Arc::clone(&state.events),
        ChatContextType::Project,
        AgentHarnessKind::Claude,
        project.id.as_str(),
        conversation.id,
        "unowned",
        &cli,
    )
    .await;
    assert_eq!(first.0, 1);
    assert_eq!(second.0, 0);
    wait_for_file(&calls).await;
    // Give a wrongly-started duplicate spawn time to land before counting.
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    assert_eq!(fs::read_to_string(calls).expect("calls").lines().count(), 1);
    assert_eq!(
        events
            .events()
            .iter()
            .filter(|event| event.event == "agent:queue_sent")
            .count(),
        1,
        "re-entry must not emit a duplicate delivery event"
    );
}

#[tokio::test]
async fn recovered_stdin_turn_drains_without_duplicate_user_message_row() {
    let _allow_spawn =
        crate::support::env::EnvVarGuard::set("RALPHX_ALLOW_CLAUDE_SPAWN_IN_TESTS", "1");
    let state = AppState::new_test();
    let (project, conversation) = seed(&state, "stdin cancellation recovery").await;
    let temp = tempfile::tempdir().expect("fixture dir");
    let (cli, calls) = fixture(temp.path());
    enable_fixture(&state, &cli).await;
    let persisted_message_id = ChatMessageId::from_string("stdin-user-row".to_string());
    let mut persisted = ChatMessage::user_in_project(project.id.clone(), "deliver after cancel");
    persisted.id = persisted_message_id.clone();
    persisted.conversation_id = Some(conversation.id);
    state
        .chat_message_repo
        .create(persisted)
        .await
        .expect("persist Gate 1 user row");
    // This is the durable queue state created by the stream-exit cancellation
    // path after it drains the registry's pending stdin ledger.
    let mut recovered = QueuedMessage::new("deliver after cancel".to_string());
    recovered.persisted_message_id = Some(persisted_message_id.as_str().to_string());
    state.message_queue.queue_front_existing(
        ChatContextType::Project,
        conversation.id.as_str(),
        recovered,
    );
    completed_owner(&state, conversation.id, "completed-session").await;
    let app = tauri::test::mock_builder()
        .manage(state)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("app");

    let state = app.state::<AppState>();
    let (processed, _) = process_queued_messages_for_test(
        state.inner(),
        None,
        Arc::clone(&state.events),
        ChatContextType::Project,
        AgentHarnessKind::Claude,
        project.id.as_str(),
        conversation.id,
        "completed-session",
        &cli,
    )
    .await;

    assert_eq!(processed, 1);
    wait_for_file(&calls).await;
    let user_rows = app
        .state::<AppState>()
        .chat_message_repo
        .get_by_conversation(&conversation.id)
        .await
        .expect("conversation messages")
        .into_iter()
        .filter(|message| message.role == MessageRole::User)
        .collect::<Vec<_>>();
    assert_eq!(user_rows.len(), 1, "recovery must reuse the Gate 1 row");
    assert_eq!(user_rows[0].id, persisted_message_id);
}

#[tokio::test]
async fn stopping_exact_interactive_owner_requeues_pending_turn_and_publishes_backend_truth() {
    let mut state = AppState::new_test();
    let events = RecordingEventSink::new();
    state.events = Arc::new(events.clone());
    let (project, conversation) = seed(&state, "stdin stop recovery").await;
    let persisted_message_id = ChatMessageId::from_string("stdin-stop-user-row".to_string());
    let mut persisted = ChatMessage::user_in_project(project.id.clone(), "recover after stop");
    persisted.id = persisted_message_id.clone();
    persisted.conversation_id = Some(conversation.id);
    state
        .chat_message_repo
        .create(persisted)
        .await
        .expect("persist Gate 1 user row");
    let second_message_id = ChatMessageId::from_string("stdin-stop-second-row".to_string());
    let mut second = ChatMessage::user_in_project(project.id.clone(), "recover second after stop");
    second.id = second_message_id.clone();
    second.conversation_id = Some(conversation.id);
    state
        .chat_message_repo
        .create(second)
        .await
        .expect("persist second Gate 1 user row");

    let mut child = Command::new("/bin/cat")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .expect("spawn stdin observer");
    let key = InteractiveProcessKey::new(
        ChatContextType::Project.to_string(),
        conversation.id.as_str(),
    );
    let token = state
        .interactive_process_registry
        .register_with_metadata(
            key.clone(),
            child.stdin.take().expect("observer stdin"),
            InteractiveProcessMetadata {
                agent_run_id: Some("stdin-stop-run".to_string()),
                ..Default::default()
            },
        )
        .await;
    state
        .interactive_process_registry
        .write_message_if_owner_with_pending_turn(
            &key,
            token,
            "stdin-stop-run",
            "recover after stop",
            PendingStdinTurn {
                persisted_message_id: persisted_message_id.as_str().to_string(),
                content: "recover after stop".to_string(),
                metadata_override: None,
                queued_at: Utc::now().to_rfc3339(),
            },
        )
        .await
        .expect("atomic stdin delivery");
    state
        .interactive_process_registry
        .write_message_if_owner_with_pending_turn(
            &key,
            token,
            "stdin-stop-run",
            "recover second after stop",
            PendingStdinTurn {
                persisted_message_id: second_message_id.as_str().to_string(),
                content: "recover second after stop".to_string(),
                metadata_override: None,
                queued_at: Utc::now().to_rfc3339(),
            },
        )
        .await
        .expect("second atomic stdin delivery");

    let app = tauri::test::mock_builder()
        .manage(state)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("app");
    let state = app.state::<AppState>();
    let service = state.build_chat_service();

    assert!(!service
        .stop_agent(ChatContextType::Project, &conversation.id.as_str())
        .await
        .expect("stop without running-registry row"));

    let recovered = state
        .queued_message_repo
        .list(&ralphx_lib::domain::services::QueueKey::new(
            ChatContextType::Project,
            conversation.id.as_str(),
        ))
        .await
        .expect("durable recovered queue");
    assert_eq!(recovered.len(), 2);
    assert_eq!(
        recovered[0].persisted_message_id.as_deref(),
        Some(persisted_message_id.as_str())
    );
    assert_eq!(
        recovered[1].persisted_message_id.as_deref(),
        Some(second_message_id.as_str())
    );
    let queued_events: Vec<_> = events
        .events()
        .into_iter()
        .filter(|event| event.event == "agent:message_queued")
        .collect();
    assert_eq!(queued_events.len(), 2);
    assert!(queued_events[0]
        .payload
        .to_string()
        .contains("recover after stop"));
    assert!(queued_events[1]
        .payload
        .to_string()
        .contains("recover second after stop"));

    let _ = child.kill().await;
    let _ = child.wait().await;
}

struct FailingContinuationRepo;
#[async_trait]
impl AgentRunRepository for FailingContinuationRepo {
    async fn create(&self, _: AgentRun) -> AppResult<AgentRun> {
        unreachable!()
    }
    async fn get_by_id(&self, _: &AgentRunId) -> AppResult<Option<AgentRun>> {
        unreachable!()
    }
    async fn get_latest_for_conversation(
        &self,
        _: &ChatConversationId,
    ) -> AppResult<Option<AgentRun>> {
        unreachable!()
    }
    async fn get_latest_completed_for_provider_session(
        &self,
        _: &ChatConversationId,
        _: AgentHarnessKind,
        _: &str,
    ) -> AppResult<Option<AgentRun>> {
        Err(AppError::Infrastructure(
            "injected continuation resolution failure".to_string(),
        ))
    }
    async fn get_active_for_conversation(
        &self,
        _: &ChatConversationId,
    ) -> AppResult<Option<AgentRun>> {
        unreachable!()
    }
    async fn get_by_conversation(&self, _: &ChatConversationId) -> AppResult<Vec<AgentRun>> {
        unreachable!()
    }
    async fn update_status(&self, _: &AgentRunId, _: AgentRunStatus) -> AppResult<()> {
        unreachable!()
    }
    async fn update_usage(&self, _: &AgentRunId, _: &AgentRunUsage) -> AppResult<()> {
        unreachable!()
    }
    async fn update_attribution(&self, _: &AgentRunId, _: &AgentRunAttribution) -> AppResult<()> {
        unreachable!()
    }
    async fn complete(&self, _: &AgentRunId) -> AppResult<()> {
        unreachable!()
    }
    async fn complete_if_prune_cancelled(&self, _: &AgentRunId) -> AppResult<bool> {
        unreachable!()
    }
    async fn fail(&self, _: &AgentRunId, _: &str) -> AppResult<()> {
        unreachable!()
    }
    async fn cancel(&self, _: &AgentRunId) -> AppResult<()> {
        unreachable!()
    }
    async fn cancel_with_reason(&self, _: &AgentRunId, _: &str) -> AppResult<()> {
        unreachable!()
    }
    async fn delete(&self, _: &AgentRunId) -> AppResult<()> {
        unreachable!()
    }
    async fn delete_by_conversation(&self, _: &ChatConversationId) -> AppResult<()> {
        unreachable!()
    }
    async fn count_by_status(&self, _: &ChatConversationId, _: AgentRunStatus) -> AppResult<u32> {
        unreachable!()
    }
    async fn cancel_all_running(&self) -> AppResult<u32> {
        unreachable!()
    }
    async fn cancel_running_started_before(&self, _: chrono::DateTime<Utc>) -> AppResult<u32> {
        unreachable!()
    }
    async fn get_interrupted_conversations(&self) -> AppResult<Vec<InterruptedConversation>> {
        unreachable!()
    }
}
