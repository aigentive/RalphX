use std::fs;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ralphx_lib::application::chat_service::{
    build_initial_prompt, process_stream_background, ChatService, SendMessageOptions,
    StreamingStateCache,
};
use ralphx_lib::application::interactive_process_registry::{
    InteractiveProcessKey, InteractiveProcessMetadata,
};
use ralphx_lib::application::AppState;
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::agents::AgentHarnessKind;
use ralphx_lib::domain::entities::{
    AgentRun, AgentRunId, AgentRunStatus, ChatContextType, ChatConversation, Project, ProjectId,
};
use ralphx_lib::domain::services::QueueKey;
use ralphx_lib::infrastructure::agents::claude::format_stream_json_input;
use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime};
use tauri::{Listener, Manager};
use tokio::process::Command;
use tokio::time::{sleep, timeout};
use tokio_util::sync::CancellationToken;

async fn wait_for_run_status(
    state: &AppState,
    run_id: &str,
    expected: AgentRunStatus,
) -> ralphx_lib::domain::entities::AgentRun {
    for _ in 0..100 {
        let run = state
            .agent_run_repo
            .get_by_id(&AgentRunId::from_string(run_id.to_string()))
            .await
            .expect("load scripted run")
            .expect("scripted run should remain persisted");
        if run.status == expected {
            return run;
        }
        sleep(Duration::from_millis(10)).await;
    }

    panic!("scripted run {run_id} did not reach {expected:?}");
}

#[tokio::test]
async fn scripted_claude_process_round_trips_second_turn_through_gate1_and_streaming() {
    let app = mock_builder()
        .manage(AppState::new_test())
        .build(mock_context(noop_assets()))
        .expect("build mock app");
    let handle = app.handle().clone();
    let state = app.state::<AppState>();
    let execution_state = Arc::new(ExecutionState::new());
    let context_id = "scripted-claude-second-turn-project";
    let project_dir = tempfile::tempdir().expect("create scripted project dir");
    let mut project = Project::new(
        "Scripted Claude second-turn project".to_string(),
        project_dir.path().to_string_lossy().to_string(),
    );
    project.id = ProjectId::from_string(context_id.to_string());
    state
        .project_repo
        .create(project)
        .await
        .expect("seed project context");

    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(ProjectId::from_string(
            context_id.to_string(),
        )))
        .await
        .expect("persist live conversation");
    let mut run = AgentRun::new(conversation.id);
    run.harness = Some(AgentHarnessKind::Claude);
    run.provider_session_id = Some("scripted-claude-session".to_string());
    let run = state
        .agent_run_repo
        .create(run)
        .await
        .expect("persist authoritative run");
    let run_id = run.id.as_str().to_string();

    let streamed_chunks = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let streamed_chunks_for_listener = Arc::clone(&streamed_chunks);
    handle.listen("agent:chunk", move |event| {
        let payload = serde_json::from_str(event.payload()).expect("chunk payload JSON");
        streamed_chunks_for_listener
            .lock()
            .expect("chunk event lock")
            .push(payload);
    });
    let finalized_messages = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let finalized_messages_for_listener = Arc::clone(&finalized_messages);
    handle.listen("agent:message_created", move |event| {
        let payload = serde_json::from_str(event.payload()).expect("message payload JSON");
        finalized_messages_for_listener
            .lock()
            .expect("message event lock")
            .push(payload);
    });

    let capture_dir = tempfile::tempdir().expect("create stdin capture dir");
    let stdin_capture = capture_dir.path().join("second-turn-stdin.json");
    let first_assistant = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"first turn response"}]},"session_id":"scripted-claude-session"}"#;
    let first_result = r#"{"type":"result","session_id":"scripted-claude-session","is_error":false,"result":"first turn response","cost_usd":0.0}"#;
    let second_assistant = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"second turn response"}]},"session_id":"scripted-claude-session"}"#;
    let second_result = r#"{"type":"result","session_id":"scripted-claude-session","is_error":false,"result":"second turn response","cost_usd":0.0}"#;
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(
            "printf '%s\\n' \"$RALPHX_FIRST_ASSISTANT\"; \\
             printf '%s\\n' \"$RALPHX_FIRST_RESULT\"; \\
             IFS= read -r second_turn; \\
             printf '%s' \"$second_turn\" > \"$RALPHX_STDIN_CAPTURE\"; \\
             printf '%s\\n' \"$RALPHX_SECOND_ASSISTANT\"; \\
             printf '%s\\n' \"$RALPHX_SECOND_RESULT\"",
        )
        .env("RALPHX_FIRST_ASSISTANT", first_assistant)
        .env("RALPHX_FIRST_RESULT", first_result)
        .env("RALPHX_SECOND_ASSISTANT", second_assistant)
        .env("RALPHX_SECOND_RESULT", second_result)
        .env("RALPHX_STDIN_CAPTURE", &stdin_capture)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .expect("spawn scripted Claude-like process");
    let interactive_key = InteractiveProcessKey::new(
        ChatContextType::Project.to_string(),
        conversation.id.as_str(),
    );
    let token = state
        .interactive_process_registry
        .register_with_metadata(
            interactive_key.clone(),
            child.stdin.take().expect("scripted Claude stdin"),
            InteractiveProcessMetadata {
                agent_run_id: Some(run_id.clone()),
                harness: Some(AgentHarnessKind::Claude),
                provider_session_id: Some("scripted-claude-session".to_string()),
                persona_id: None,
                persona_content_hash: None,
                agent_name: None,
                agent_profile: None,
            },
        )
        .await;

    let stream_conversation_id = conversation.id;
    let stream_message_repo = Arc::clone(&state.chat_message_repo);
    let stream_timeline_repo = Arc::clone(&state.chat_timeline_repo);
    let stream_run_repo = Arc::clone(&state.agent_run_repo);
    let stream_conversation_repo = Arc::clone(&state.chat_conversation_repo);
    let stream_registry = Arc::clone(&state.interactive_process_registry);
    let stream_execution_state = Arc::clone(&execution_state);
    let stream_run_id = run_id.clone();
    let stream_key = interactive_key.clone();
    let stream_handle = handle.clone();
    let stream_task = tokio::spawn(async move {
        process_stream_background::<MockRuntime>(
            child,
            AgentHarnessKind::Claude,
            ChatContextType::Project,
            context_id,
            &stream_conversation_id,
            Some(stream_handle),
            None,
            None,
            Some(stream_message_repo),
            Some(stream_timeline_repo),
            None,
            None,
            CancellationToken::new(),
            StreamingStateCache::new(),
            None,
            Some(stream_run_repo),
            Some(stream_run_id),
            Some(stream_execution_state),
            Some(stream_conversation_repo),
            false,
            true,
            Some(stream_registry),
            Some(stream_key),
            Some(token),
        )
        .await
    });

    let first_turn = wait_for_run_status(&state, &run_id, AgentRunStatus::Completed).await;
    assert_eq!(first_turn.conversation_id, conversation.id);
    assert!(
        state
            .interactive_process_registry
            .has_process(&interactive_key)
            .await,
        "first TurnComplete must retain the live interactive process for the follow-up"
    );

    let service = state
        .build_chat_service_for_runtime(Some(Arc::clone(&execution_state)), Some(handle.clone()));
    let follow_up = "continue exactly this scripted Claude conversation";
    let send = service
        .send_message(
            ChatContextType::Project,
            context_id,
            follow_up,
            SendMessageOptions {
                conversation_id_override: Some(conversation.id),
                ..Default::default()
            },
        )
        .await
        .expect("Gate 1 should send the second turn to the live Claude process");

    assert!(!send.was_queued, "interactive second turn must not queue");
    assert!(send.queued_message_id.is_none());
    assert_eq!(send.conversation_id, conversation.id.as_str());
    assert_eq!(send.agent_run_id, run_id);
    assert!(
        state
            .queued_message_repo
            .list(&QueueKey::new(
                ChatContextType::Project,
                conversation.id.as_str()
            ))
            .await
            .expect("read durable queue")
            .is_empty(),
        "a live Gate-1 continuation must leave no durable queue row"
    );

    let outcome = timeout(Duration::from_secs(5), stream_task)
        .await
        .expect("second-turn stream should finish")
        .expect("stream task should not panic")
        .expect("scripted Claude stream should complete");
    assert_eq!(outcome.turns_finalized, 2);
    assert!(outcome.completion_applied);

    let captured_envelope: serde_json::Value = serde_json::from_str(
        fs::read_to_string(&stdin_capture)
            .expect("scripted child should capture second-turn stdin")
            .trim(),
    )
    .expect("second-turn stdin should be JSON");
    let expected_envelope: serde_json::Value = serde_json::from_str(&format_stream_json_input(
        &build_initial_prompt(ChatContextType::Project, context_id, follow_up, &[], 0),
    ))
    .expect("expected Gate-1 stream-json envelope");
    assert_eq!(
        captured_envelope, expected_envelope,
        "Gate 1 must write exact stream-json"
    );

    let final_run = wait_for_run_status(&state, &run_id, AgentRunStatus::Completed).await;
    assert_eq!(final_run.conversation_id, conversation.id);
    let messages = state
        .chat_message_repo
        .get_by_conversation(&conversation.id)
        .await
        .expect("load persisted conversation messages");
    assert!(
        messages
            .iter()
            .any(|message| message.content == "second turn response"),
        "production stream processing must persist the second assistant response"
    );

    let expected_conversation_id = conversation.id.as_str();
    let chunks = streamed_chunks.lock().expect("chunk event lock");
    assert!(chunks.iter().any(|payload| {
        payload["text"].as_str() == Some("second turn response")
            && payload["run_id"].as_str() == Some(run_id.as_str())
            && payload["conversation_id"].as_str() == Some(expected_conversation_id.as_str())
    }));
    let assistant_events = finalized_messages.lock().expect("message event lock");
    assert!(assistant_events.iter().any(|payload| {
        payload["role"].as_str() == Some("orchestrator")
            && payload["content"].as_str() == Some("second turn response")
            && payload["conversation_id"].as_str() == Some(expected_conversation_id.as_str())
    }));
}
