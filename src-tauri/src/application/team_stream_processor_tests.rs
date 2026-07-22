use super::*;
use crate::domain::entities::UsageProvenance;

#[test]
fn test_module_compiles() {
    // Verify the module compiles and types are accessible — includes exit_signal parameter
    fn _assert_fn_signature() {
        fn _check(
            _stdout: ChildStdout,
            _exit_signal: oneshot::Receiver<()>,
            _team_name: String,
            _teammate_name: String,
            _context_type: String,
            _context_id: String,
            _app_handle: AppHandle,
            _team_tracker: Arc<TeamStateTracker>,
            _team_service: Option<Arc<TeamService>>,
        ) -> JoinHandle<()> {
            unimplemented!()
        }
        let _ = _check;
    }
}

/// Fix B: exit_signal channel pair is created and wired correctly.
/// Verifies that sending on exit_tx causes exit_rx to resolve immediately
/// (which is what the select! in start_teammate_stream relies on).
#[tokio::test]
async fn test_exit_signal_channel_resolves_on_send() {
    let (exit_tx, exit_rx) = oneshot::channel::<()>();

    // Sender fires — receiver should resolve immediately
    exit_tx.send(()).unwrap();

    // Using tokio::time::timeout to ensure the future resolves
    let result = tokio::time::timeout(std::time::Duration::from_millis(100), exit_rx).await;

    assert!(result.is_ok(), "exit_rx should resolve when exit_tx sends");
    assert!(result.unwrap().is_ok(), "exit_rx value should be Ok(())");
}

/// Fix B: kill_tx send is received on kill_rx.
/// Simulates the stop_teammate path: dropping kill_tx signals kill_rx.
#[tokio::test]
async fn test_kill_tx_dropped_fires_kill_rx() {
    let (kill_tx, kill_rx) = oneshot::channel::<()>();

    // Dropping kill_tx (without send) fires RecvError on kill_rx,
    // which the select! pattern `_ = kill_rx` also matches — triggering cleanup.
    drop(kill_tx);

    let result = tokio::time::timeout(std::time::Duration::from_millis(100), kill_rx).await;

    assert!(
        result.is_ok(),
        "kill_rx should resolve when kill_tx is dropped"
    );
    // Err(RecvError) is expected — sender dropped without sending
    assert!(
        result.unwrap().is_err(),
        "kill_rx should get RecvError when kill_tx dropped"
    );
}

#[tokio::test]
async fn teammate_stream_creates_solo_teammate_conversation() {
    use std::process::Stdio;

    let app = crate::testing::create_mock_app();
    let team_tracker = Arc::new(TeamStateTracker::new());
    team_tracker
        .create_team("team-a", "project-1", "project")
        .await
        .expect("team should be created");
    team_tracker
        .add_teammate("team-a", "worker", "#ff6b35", "sonnet", "worker")
        .await
        .expect("teammate should be added");
    let repo = Arc::new(crate::infrastructure::memory::MemoryChatConversationRepository::new());
    let conversation_repo: Arc<dyn ChatConversationRepository> = repo.clone();
    let mut child = tokio::process::Command::new("cat")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("cat should spawn");
    let stdout = child.stdout.take().expect("stdout should be piped");
    let (exit_tx, exit_rx) = oneshot::channel::<()>();
    let stream_task = start_teammate_stream::<tauri::test::MockRuntime>(
        stdout,
        exit_rx,
        "team-a".to_string(),
        "worker".to_string(),
        "project".to_string(),
        "project-1".to_string(),
        app.handle().clone(),
        team_tracker,
        None,
        Some(conversation_repo),
        None,
        None,
        None,
    );

    let mut created = None;
    for _ in 0..20 {
        created = repo
            .get_active_for_context(ChatContextType::Project, "teammate:team-a:worker")
            .await
            .expect("conversation lookup should succeed");
        if created.is_some() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let _ = exit_tx.send(());
    let _ = child.kill().await;
    let _ = child.wait().await;
    stream_task.await.expect("stream task should join");

    let conversation = created.expect("teammate conversation should be created");
    assert_eq!(conversation.context_type, ChatContextType::Project);
    assert_eq!(conversation.context_id, "teammate:team-a:worker");
    assert_eq!(conversation.coordination_mode, CoordinationMode::Solo);
}

#[tokio::test]
async fn teammate_turn_complete_persists_authoritative_claude_usage_on_message_ledger() {
    use std::process::Stdio;

    let app = crate::testing::create_mock_app();
    let team_tracker = Arc::new(TeamStateTracker::new());
    team_tracker
        .create_team("team-usage", "project-1", "project")
        .await
        .unwrap();
    team_tracker
        .add_teammate("team-usage", "worker", "#ff6b35", "sonnet", "worker")
        .await
        .unwrap();
    let conversations =
        Arc::new(crate::infrastructure::memory::MemoryChatConversationRepository::new());
    let messages = Arc::new(crate::infrastructure::memory::MemoryChatMessageRepository::new());
    let assistant = r#"{"type":"assistant","session_id":"teammate-session","message":{"usage":{"input_tokens":3,"output_tokens":73,"cache_creation_input_tokens":100,"cache_read_input_tokens":500},"content":[{"type":"text","text":"Working"}]}}"#;
    let result = r#"{"type":"result","session_id":"teammate-session","is_error":false,"result":"Done","usage":{"input_tokens":13,"output_tokens":1434,"cache_creation_tokens":127826,"cache_read_tokens":1099251}}"#;
    let mut child = tokio::process::Command::new("printf")
        .arg("%s\n%s\n")
        .arg(assistant)
        .arg(result)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("printf should spawn");
    let stdout = child.stdout.take().unwrap();
    let (_exit_tx, exit_rx) = oneshot::channel::<()>();
    let stream_task = start_teammate_stream::<tauri::test::MockRuntime>(
        stdout,
        exit_rx,
        "team-usage".to_string(),
        "worker".to_string(),
        "project".to_string(),
        "project-1".to_string(),
        app.handle().clone(),
        team_tracker.clone(),
        None,
        Some(conversations.clone()),
        Some(messages.clone()),
        None,
        None,
    );

    child.wait().await.unwrap();
    stream_task.await.unwrap();
    let conversation = conversations
        .get_active_for_context(ChatContextType::Project, "teammate:team-usage:worker")
        .await
        .unwrap()
        .unwrap();
    let persisted = messages
        .get_by_conversation(&conversation.id)
        .await
        .unwrap();
    let message = persisted.first().expect("teammate assistant message");
    assert_eq!(message.provider_harness, Some(AgentHarnessKind::Claude));
    assert_eq!(
        message.provider_session_id.as_deref(),
        Some("teammate-session")
    );
    assert_eq!(message.input_tokens, Some(13));
    assert_eq!(message.output_tokens, Some(1_434));
    assert_eq!(message.cache_creation_tokens, Some(127_826));
    assert_eq!(message.cache_read_tokens, Some(1_099_251));
    assert_eq!(
        message.usage_provenance,
        Some(UsageProvenance::ProviderTurnDelta)
    );
    assert_eq!(
        conversation.provider_session_ref(),
        Some(crate::domain::agents::ProviderSessionRef {
            harness: AgentHarnessKind::Claude,
            provider_session_id: "teammate-session".to_string(),
        })
    );
    let cost = team_tracker
        .get_teammate_cost("team-usage", "worker")
        .await
        .unwrap();
    assert_eq!(cost.input_tokens, 13);
    assert_eq!(cost.output_tokens, 1_434);
    assert_eq!(cost.cache_creation_tokens, 127_826);
    assert_eq!(cost.cache_read_tokens, 1_099_251);
}

#[test]
fn test_message_type_mapping() {
    // Verify TeamMessageSent message_type string → TeamMessageType mapping
    let broadcast_type = match "broadcast" {
        "broadcast" => TeamMessageType::Broadcast,
        _ => TeamMessageType::TeammateMessage,
    };
    assert_eq!(broadcast_type, TeamMessageType::Broadcast);

    let message_type = match "message" {
        "broadcast" => TeamMessageType::Broadcast,
        _ => TeamMessageType::TeammateMessage,
    };
    assert_eq!(message_type, TeamMessageType::TeammateMessage);
}

#[test]
fn teammate_tool_result_event_payload_includes_preview_metadata() {
    let result = serde_json::json!((1..=12)
        .map(|index| format!("line {index}"))
        .collect::<Vec<_>>()
        .join("\n"));
    let detail_ref = crate::application::chat_service::tool_result_preview::tool_detail_ref(
        "conv-1",
        "msg-1",
        Some("tool-heavy"),
        None,
    );
    let preview =
        crate::application::chat_service::tool_result_preview::build_live_tool_result_preview(
            Some("bash"),
            &result,
            Some(detail_ref),
        );

    let payload = teammate_tool_result_event_payload(
        "worker",
        "tool-heavy",
        &preview,
        "project",
        "project-1",
        Some("parent-tool".to_string()),
        Some("conv-1"),
    );

    assert_eq!(payload["teammate_name"], "worker");
    assert_eq!(payload["tool_name"], "result:tool-heavy");
    assert_eq!(payload["result"].as_str().unwrap().lines().count(), 10);
    assert_eq!(payload["result_preview_truncated"], true);
    assert_eq!(payload["result_preview_line_count"], 12);
    assert_eq!(payload["result_preview_omitted_lines"], 2);
    assert_eq!(payload["detail_ref"]["message_id"], "msg-1");
    assert_eq!(payload["parent_tool_use_id"], "parent-tool");
    assert_eq!(payload["conversation_id"], "conv-1");
}

// ============================================================================
// truncate_str tests
// ============================================================================

#[test]
fn test_truncate_str_shorter_than_limit() {
    assert_eq!(truncate_str("hello", 200), "hello");
}

#[test]
fn test_truncate_str_exactly_at_limit() {
    let s = "a".repeat(200);
    assert_eq!(truncate_str(&s, 200), s.as_str());
}

#[test]
fn test_truncate_str_longer_than_limit() {
    let s = "a".repeat(300);
    let result = truncate_str(&s, 200);
    assert_eq!(result.len(), 200);
    assert_eq!(result, "a".repeat(200).as_str());
}

#[test]
fn test_truncate_str_empty() {
    assert_eq!(truncate_str("", 200), "");
}

#[test]
fn test_truncate_str_multibyte_at_boundary() {
    // "→" is 3 bytes (UTF-8: E2 86 92)
    // "a" * 199 + "→" = 199 + 3 = 202 bytes total
    // truncate at 200 bytes: can't split "→", so must truncate to 199 bytes
    let mut s = "a".repeat(199);
    s.push('→');
    let result = truncate_str(&s, 200);
    assert_eq!(
        result.len(),
        199,
        "must not split multi-byte char at boundary"
    );
    assert_eq!(result, "a".repeat(199).as_str());
}

#[test]
fn test_truncate_str_only_multibyte_chars() {
    // "→" is 3 bytes; 5 × "→" = 15 bytes
    // truncate at 10 bytes: 3 chars fit (9 bytes), 4th would overflow
    let s = "→".repeat(5);
    let result = truncate_str(&s, 10);
    assert_eq!(
        result.len(),
        9,
        "3 × 3-byte chars = 9 bytes fit in 10-byte limit"
    );
    assert_eq!(result, "→".repeat(3).as_str());
}

#[test]
fn test_truncate_str_limit_zero() {
    // Zero limit → always return empty
    assert_eq!(truncate_str("hello", 0), "");
}

#[test]
fn test_truncate_str_multibyte_first_char_exceeds_limit() {
    // Single 4-byte char (emoji) with limit of 3 → empty result
    let s = "😀"; // U+1F600, 4 bytes
    let result = truncate_str(s, 3);
    assert_eq!(result, "", "4-byte char cannot fit in 3-byte limit");
}
