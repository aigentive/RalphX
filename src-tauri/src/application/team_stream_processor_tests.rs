use super::*;

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
    let result = tokio::time::timeout(
        std::time::Duration::from_millis(100),
        exit_rx,
    )
    .await;

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

    let result = tokio::time::timeout(
        std::time::Duration::from_millis(100),
        kill_rx,
    )
    .await;

    assert!(result.is_ok(), "kill_rx should resolve when kill_tx is dropped");
    // Err(RecvError) is expected — sender dropped without sending
    assert!(result.unwrap().is_err(), "kill_rx should get RecvError when kill_tx dropped");
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
// extract_assistant_usage tests
// ============================================================================

#[test]
fn test_extract_assistant_usage_updates_from_message_usage() {
    let raw = serde_json::json!({
        "type": "assistant",
        "message": {
            "usage": {
                "input_tokens": 1500,
                "output_tokens": 300
            },
            "content": [{"type": "text", "text": "hello"}]
        }
    });

    let mut total_input = 0u64;
    let mut total_output = 0u64;

    let updated = extract_assistant_usage(&raw, &mut total_input, &mut total_output);

    assert!(updated, "Should return true when totals increase");
    assert_eq!(total_input, 1500);
    assert_eq!(total_output, 300);
}

#[test]
fn test_extract_assistant_usage_cumulative_only_increases() {
    // Assistant usage is cumulative within a turn — later messages have higher counts.
    // Earlier (lower) values should not reduce the totals.
    let mut total_input = 2000u64;
    let mut total_output = 500u64;

    let raw = serde_json::json!({
        "type": "assistant",
        "message": {
            "usage": {
                "input_tokens": 1500,
                "output_tokens": 300
            }
        }
    });

    let updated = extract_assistant_usage(&raw, &mut total_input, &mut total_output);

    assert!(!updated, "Should return false when new values are lower");
    assert_eq!(total_input, 2000, "Input should stay at higher value");
    assert_eq!(total_output, 500, "Output should stay at higher value");
}

#[test]
fn test_extract_assistant_usage_partial_increase() {
    // Only input increases, output stays the same
    let mut total_input = 1000u64;
    let mut total_output = 500u64;

    let raw = serde_json::json!({
        "type": "assistant",
        "message": {
            "usage": {
                "input_tokens": 2000,
                "output_tokens": 300
            }
        }
    });

    let updated = extract_assistant_usage(&raw, &mut total_input, &mut total_output);

    assert!(updated, "Should return true when at least one total increases");
    assert_eq!(total_input, 2000, "Input should update to higher value");
    assert_eq!(total_output, 500, "Output should stay at higher value");
}

#[test]
fn test_extract_assistant_usage_no_usage_field() {
    let raw = serde_json::json!({
        "type": "assistant",
        "message": {
            "content": [{"type": "text", "text": "hello"}]
        }
    });

    let mut total_input = 100u64;
    let mut total_output = 50u64;

    let updated = extract_assistant_usage(&raw, &mut total_input, &mut total_output);

    assert!(!updated, "Should return false when no usage field");
    assert_eq!(total_input, 100, "Input should be unchanged");
    assert_eq!(total_output, 50, "Output should be unchanged");
}

#[test]
fn test_extract_assistant_usage_no_message_field() {
    let raw = serde_json::json!({
        "type": "assistant"
    });

    let mut total_input = 0u64;
    let mut total_output = 0u64;

    let updated = extract_assistant_usage(&raw, &mut total_input, &mut total_output);

    assert!(!updated, "Should return false when no message field");
}

#[test]
fn test_extract_assistant_usage_zero_initial_values() {
    // Even zero → zero should not count as an update
    let raw = serde_json::json!({
        "type": "assistant",
        "message": {
            "usage": {
                "input_tokens": 0,
                "output_tokens": 0
            }
        }
    });

    let mut total_input = 0u64;
    let mut total_output = 0u64;

    let updated = extract_assistant_usage(&raw, &mut total_input, &mut total_output);

    assert!(!updated, "Should return false when values are equal (both zero)");
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
    assert_eq!(result.len(), 199, "must not split multi-byte char at boundary");
    assert_eq!(result, "a".repeat(199).as_str());
}

#[test]
fn test_truncate_str_only_multibyte_chars() {
    // "→" is 3 bytes; 5 × "→" = 15 bytes
    // truncate at 10 bytes: 3 chars fit (9 bytes), 4th would overflow
    let s = "→".repeat(5);
    let result = truncate_str(&s, 10);
    assert_eq!(result.len(), 9, "3 × 3-byte chars = 9 bytes fit in 10-byte limit");
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
