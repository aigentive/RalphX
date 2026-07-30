use super::*;

#[test]
fn test_processor_text_accumulation() {
    let mut processor = StreamProcessor::new();

    // Simulate text delta messages
    let msg1 = StreamMessage::ContentBlockDelta {
        index: Some(0),
        delta: ContentDelta {
            delta_type: "text_delta".to_string(),
            text: Some("Hello ".to_string()),
            partial_json: None,
        },
    };
    let msg2 = StreamMessage::ContentBlockDelta {
        index: Some(0),
        delta: ContentDelta {
            delta_type: "text_delta".to_string(),
            text: Some("world!".to_string()),
            partial_json: None,
        },
    };

    let events1 = processor.process_message(msg1);
    let events2 = processor.process_message(msg2);

    assert_eq!(events1.len(), 1);
    assert_eq!(events2.len(), 1);

    let result = processor.finish();
    assert_eq!(result.response_text, "Hello world!");
}

#[test]
fn test_processor_assistant_message() {
    let mut processor = StreamProcessor::new();

    let msg = StreamMessage::Assistant {
        message: AssistantMessage {
            content: vec![
                AssistantContent::Text {
                    text: "Here's my response".to_string(),
                },
                AssistantContent::ToolUse {
                    id: "toolu_456".to_string(),
                    name: "search".to_string(),
                    input: serde_json::json!({"query": "test"}),
                },
            ],
            stop_reason: Some("end_turn".to_string()),
            usage: None,
        },
        session_id: Some("session-abc".to_string()),
    };

    let events = processor.process_message(msg);

    // Should emit: TextChunk, ToolCallCompleted, SessionId
    assert_eq!(events.len(), 3);
    assert!(matches!(&events[0], StreamEvent::TextChunk(t) if t == "Here's my response"));
    assert!(
        matches!(&events[1], StreamEvent::ToolCallCompleted { ref tool_call, .. } if tool_call.name == "search")
    );
    assert!(matches!(&events[2], StreamEvent::SessionId(id) if id == "session-abc"));

    let result = processor.finish();
    assert_eq!(result.response_text, "Here's my response");
    assert_eq!(result.tool_calls.len(), 1);
    assert_eq!(result.session_id, Some("session-abc".to_string()));
}

#[test]
fn test_processor_session_id_from_result() {
    let mut processor = StreamProcessor::new();

    let msg = StreamMessage::Result {
        result: Some("Done".to_string()),
        session_id: Some("result-session".to_string()),
        is_error: false,
        errors: Vec::new(),
        subtype: None,
        cost_usd: 0.01,
        usage: None,
    };

    let events = processor.process_message(msg);

    // Result message emits both SessionId and TurnComplete for the lead
    assert_eq!(events.len(), 2);
    assert!(matches!(&events[0], StreamEvent::SessionId(id) if id == "result-session"));
    assert!(matches!(
        &events[1],
        StreamEvent::TurnComplete { session_id } if *session_id == Some("result-session".to_string())
    ));

    let result = processor.finish();
    assert_eq!(result.session_id, Some("result-session".to_string()));
}

#[test]
fn test_processor_collects_usage_from_assistant_and_result() {
    let mut processor = StreamProcessor::new();

    let assistant = StreamMessage::Assistant {
        message: AssistantMessage {
            content: vec![AssistantContent::Text {
                text: "Usage-bearing response".to_string(),
            }],
            stop_reason: Some("end_turn".to_string()),
            usage: Some(serde_json::json!({
                "input_tokens": 1200,
                "output_tokens": 340,
                "cache_creation_input_tokens": 50,
                "cache_read_input_tokens": 210
            })),
        },
        session_id: Some("usage-session".to_string()),
    };
    processor.process_message(assistant);

    processor.process_message(StreamMessage::Result {
        result: Some("done".to_string()),
        session_id: Some("usage-session".to_string()),
        is_error: false,
        errors: Vec::new(),
        subtype: None,
        cost_usd: 0.0125,
        usage: None,
    });

    let result = processor.finish();
    assert_eq!(result.usage.input_tokens, Some(1200));
    assert_eq!(result.usage.output_tokens, Some(340));
    assert_eq!(result.usage.cache_creation_tokens, Some(50));
    assert_eq!(result.usage.cache_read_tokens, Some(210));
    assert_eq!(result.usage.estimated_usd, Some(0.0125));
}

#[test]
fn terminal_result_usage_replaces_provisional_assistant_maxima() {
    let mut processor = StreamProcessor::new();
    processor.process_message(StreamMessage::Assistant {
        message: AssistantMessage {
            content: vec![],
            stop_reason: Some("tool_use".to_string()),
            usage: Some(serde_json::json!({
                "input_tokens": 3,
                "output_tokens": 73,
                "cache_creation_input_tokens": 105479,
                "cache_read_input_tokens": 142307
            })),
        },
        session_id: Some("usage-session".to_string()),
    });
    let parsed = StreamProcessor::parse_line(
        r#"{"type":"result","session_id":"usage-session","is_error":false,"result":"Done","usage":{"input_tokens":13,"output_tokens":1434,"cache_creation_tokens":127826,"cache_read_tokens":1099251}}"#,
    )
    .expect("result should parse");
    processor.process_parsed_line(parsed);

    let capture = processor.current_turn_capture().expect("typed capture");
    assert_eq!(capture.normalized.input_tokens, Some(13));
    assert_eq!(capture.normalized.output_tokens, Some(1_434));
    assert_eq!(capture.normalized.cache_creation_tokens, Some(127_826));
    assert_eq!(capture.normalized.cache_read_tokens, Some(1_099_251));
    assert_eq!(capture.provenance, UsageProvenance::ProviderTurnDelta);
}

#[test]
fn empty_terminal_result_usage_preserves_provisional_assistant_usage() {
    let mut processor = StreamProcessor::new();
    let assistant = StreamProcessor::parse_line(
        r#"{"type":"assistant","message":{"usage":{"input_tokens":41,"output_tokens":7,"cache_read_input_tokens":30},"content":[]}}"#,
    )
    .expect("assistant usage should parse");
    processor.process_parsed_line(assistant);
    let result = StreamProcessor::parse_line(
        r#"{"type":"result","session_id":"session-1","is_error":false,"result":"done","usage":{}}"#,
    )
    .expect("empty terminal usage should parse");
    processor.process_parsed_line(result);

    let capture = processor
        .current_turn_capture()
        .expect("assistant usage should remain available");
    assert_eq!(capture.normalized.input_tokens, Some(41));
    assert_eq!(capture.normalized.output_tokens, Some(7));
    assert_eq!(capture.normalized.cache_read_tokens, Some(30));
    assert_eq!(
        capture.provenance,
        UsageProvenance::ProviderSnapshotFallback
    );
}

#[test]
fn nested_terminal_result_usage_does_not_replace_lead_usage() {
    let mut processor = StreamProcessor::new();
    processor.process_message(StreamMessage::Assistant {
        message: AssistantMessage {
            content: vec![],
            stop_reason: Some("end_turn".to_string()),
            usage: Some(serde_json::json!({"input_tokens": 10, "output_tokens": 2})),
        },
        session_id: Some("lead-session".to_string()),
    });
    let mut parsed = StreamProcessor::parse_line(
        r#"{"type":"result","session_id":"child-session","is_error":false,"result":"Done","usage":{"input_tokens":999,"output_tokens":888}}"#,
    )
    .expect("nested result should parse");
    parsed.parent_tool_use_id = Some("tool-child".to_string());
    processor.process_parsed_line(parsed);

    let capture = processor.current_turn_capture().expect("fallback capture");
    assert_eq!(capture.normalized.input_tokens, Some(10));
    assert_eq!(capture.normalized.output_tokens, Some(2));
    assert_eq!(
        capture.provenance,
        UsageProvenance::ProviderSnapshotFallback
    );
}

#[test]
fn nested_assistant_and_delta_usage_do_not_seed_lead_fallback() {
    let mut processor = StreamProcessor::new();
    processor.process_parsed_line(ParsedLine {
        message: StreamMessage::Assistant {
            message: AssistantMessage {
                content: vec![],
                stop_reason: Some("end_turn".to_string()),
                usage: Some(serde_json::json!({
                    "input_tokens": 999,
                    "output_tokens": 888
                })),
            },
            session_id: Some("child-session".to_string()),
        },
        parent_tool_use_id: Some("tool-child".to_string()),
        is_synthetic: false,
        tool_use_result: None,
    });
    processor.process_parsed_line(ParsedLine {
        message: StreamMessage::MessageDelta {
            delta: None,
            usage: Some(serde_json::json!({
                "input_tokens": 1_111,
                "output_tokens": 999
            })),
        },
        parent_tool_use_id: Some("tool-child".to_string()),
        is_synthetic: false,
        tool_use_result: None,
    });

    assert_eq!(processor.current_turn_capture(), None);
}

#[test]
fn test_processor_usage_accumulates_across_turns() {
    let mut processor = StreamProcessor::new();

    processor.process_message(StreamMessage::MessageDelta {
        delta: None,
        usage: Some(serde_json::json!({
            "input_tokens": 100,
            "output_tokens": 25
        })),
    });
    processor.process_message(StreamMessage::Result {
        result: Some("turn 1".to_string()),
        session_id: Some("session-1".to_string()),
        is_error: false,
        errors: Vec::new(),
        subtype: None,
        cost_usd: 0.001,
        usage: None,
    });
    processor.reset_for_next_turn();

    processor.process_message(StreamMessage::Assistant {
        message: AssistantMessage {
            content: vec![AssistantContent::Text {
                text: "turn 2".to_string(),
            }],
            stop_reason: Some("end_turn".to_string()),
            usage: Some(serde_json::json!({
                "input_tokens": 50,
                "output_tokens": 10,
                "cached_input_tokens": 30
            })),
        },
        session_id: Some("session-1".to_string()),
    });
    processor.process_message(StreamMessage::Result {
        result: Some("turn 2".to_string()),
        session_id: Some("session-1".to_string()),
        is_error: false,
        errors: Vec::new(),
        subtype: None,
        cost_usd: 0.002,
        usage: None,
    });

    let result = processor.finish();
    assert_eq!(result.usage.input_tokens, Some(150));
    assert_eq!(result.usage.output_tokens, Some(35));
    assert_eq!(result.usage.cache_read_tokens, Some(30));
    assert_eq!(result.usage.estimated_usd, Some(0.003));
}

#[test]
fn test_processor_usage_accumulation_does_not_wrap_or_panic() {
    let mut processor = StreamProcessor::new();
    processor.process_message(StreamMessage::MessageDelta {
        delta: None,
        usage: Some(serde_json::json!({ "input_tokens": u64::MAX })),
    });
    processor.reset_for_next_turn();
    processor.process_message(StreamMessage::MessageDelta {
        delta: None,
        usage: Some(serde_json::json!({ "input_tokens": 1 })),
    });

    assert_eq!(processor.finish().usage.input_tokens, Some(u64::MAX));
}

#[test]
fn test_processor_thinking_block_streaming() {
    let mut processor = StreamProcessor::new();

    // Thinking block start
    let start = StreamMessage::ContentBlockStart {
        index: Some(0),
        content_block: ContentBlock {
            block_type: "thinking".to_string(),
            id: None,
            name: None,
            text: None,
            input: None,
        },
    };

    // Thinking content delta
    let delta1 = StreamMessage::ContentBlockDelta {
        index: Some(0),
        delta: ContentDelta {
            delta_type: "thinking_delta".to_string(),
            text: Some("Let me analyze ".to_string()),
            partial_json: None,
        },
    };

    let delta2 = StreamMessage::ContentBlockDelta {
        index: Some(0),
        delta: ContentDelta {
            delta_type: "thinking_delta".to_string(),
            text: Some("this problem.".to_string()),
            partial_json: None,
        },
    };

    // Thinking block stop
    let stop = StreamMessage::ContentBlockStop { index: Some(0) };

    let events1 = processor.process_message(start);
    assert!(events1.is_empty()); // start doesn't emit event

    let events2 = processor.process_message(delta1);
    assert_eq!(events2.len(), 1);
    assert!(matches!(&events2[0], StreamEvent::Thinking(t) if t == "Let me analyze "));

    let events3 = processor.process_message(delta2);
    assert_eq!(events3.len(), 1);
    assert!(matches!(&events3[0], StreamEvent::Thinking(t) if t == "this problem."));

    let events4 = processor.process_message(stop);
    assert!(events4.is_empty()); // stop doesn't emit event for thinking
}

#[test]
fn test_processor_thinking_block_verbose() {
    let mut processor = StreamProcessor::new();

    let msg = StreamMessage::Assistant {
        message: AssistantMessage {
            content: vec![
                AssistantContent::Thinking {
                    thinking: "Deep analysis of the problem...".to_string(),
                },
                AssistantContent::Text {
                    text: "Here's my answer.".to_string(),
                },
            ],
            stop_reason: Some("end_turn".to_string()),
            usage: None,
        },
        session_id: Some("sess-456".to_string()),
    };

    let events = processor.process_message(msg);

    // Should emit: Thinking, TextChunk, SessionId
    assert_eq!(events.len(), 3);
    assert!(
        matches!(&events[0], StreamEvent::Thinking(t) if t == "Deep analysis of the problem...")
    );
    assert!(matches!(&events[1], StreamEvent::TextChunk(t) if t == "Here's my answer."));
    assert!(matches!(&events[2], StreamEvent::SessionId(id) if id == "sess-456"));
}

/// When Claude CLI emits both streaming delta events AND a verbose `assistant` summary,
/// the `TextChunk` must not be emitted twice and `response_text` must contain the text once.
#[test]
fn wrapped_deltas_then_verbose_summary_emits_text_once() {
    let mut processor = StreamProcessor::new();
    let first = r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello "}}}"#;
    let second = r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"world"}}}"#;
    let summary = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Hello world"}]},"session_id":"sess-1"}"#;

    let first_events = processor.process_parsed_line(
        StreamProcessor::parse_line(first).expect("first wrapped delta should parse"),
    );
    let second_events = processor.process_parsed_line(
        StreamProcessor::parse_line(second).expect("second wrapped delta should parse"),
    );
    let summary_events = processor.process_parsed_line(
        StreamProcessor::parse_line(summary).expect("verbose summary should parse"),
    );

    assert!(matches!(&first_events[..], [StreamEvent::TextChunk(text)] if text == "Hello "));
    assert!(matches!(&second_events[..], [StreamEvent::TextChunk(text)] if text == "world"));
    assert!(
        !summary_events
            .iter()
            .any(|event| matches!(event, StreamEvent::TextChunk(_))),
        "verbose summary must not duplicate text emitted by wrapped deltas"
    );
    assert_eq!(processor.finish().response_text, "Hello world");
}

#[test]
fn test_verbose_mode_no_double_emission() {
    let mut processor = StreamProcessor::new();

    // Step 1: streaming deltas arrive first (the normal live-stream path)
    let delta1 = StreamMessage::ContentBlockDelta {
        index: Some(0),
        delta: ContentDelta {
            delta_type: "text_delta".to_string(),
            text: Some("Hello ".to_string()),
            partial_json: None,
        },
    };
    let delta2 = StreamMessage::ContentBlockDelta {
        index: Some(0),
        delta: ContentDelta {
            delta_type: "text_delta".to_string(),
            text: Some("world".to_string()),
            partial_json: None,
        },
    };

    let delta_events1 = processor.process_message(delta1);
    let delta_events2 = processor.process_message(delta2);

    assert_eq!(delta_events1.len(), 1);
    assert!(matches!(&delta_events1[0], StreamEvent::TextChunk(t) if t == "Hello "));
    assert_eq!(delta_events2.len(), 1);
    assert!(matches!(&delta_events2[0], StreamEvent::TextChunk(t) if t == "world"));

    // Step 2: verbose assistant message arrives with the same full text
    let verbose_msg = StreamMessage::Assistant {
        message: AssistantMessage {
            content: vec![AssistantContent::Text {
                text: "Hello world".to_string(),
            }],
            stop_reason: Some("end_turn".to_string()),
            usage: None,
        },
        session_id: None,
    };

    let verbose_events = processor.process_message(verbose_msg);

    // The verbose message should emit NO TextChunk (deltas already did)
    let text_chunk_count = verbose_events
        .iter()
        .filter(|e| matches!(e, StreamEvent::TextChunk(_)))
        .count();
    assert_eq!(
        text_chunk_count, 0,
        "TextChunk must not be emitted again when deltas already streamed the text"
    );

    // response_text must contain the text exactly once
    let result = processor.finish();
    assert_eq!(
        result.response_text, "Hello world",
        "response_text must contain the text exactly once"
    );
}

/// When Claude CLI runs in verbose-only mode (no streaming deltas, only an `assistant` message),
/// the `TextChunk` event must be emitted and `response_text` must be populated correctly.
#[test]
fn test_verbose_only_text_emission() {
    let mut processor = StreamProcessor::new();

    let msg = StreamMessage::Assistant {
        message: AssistantMessage {
            content: vec![AssistantContent::Text {
                text: "Verbose response".to_string(),
            }],
            stop_reason: Some("end_turn".to_string()),
            usage: None,
        },
        session_id: None,
    };

    let events = processor.process_message(msg);

    // Must emit exactly one TextChunk
    let text_chunks: Vec<_> = events
        .iter()
        .filter_map(|e| {
            if let StreamEvent::TextChunk(t) = e {
                Some(t.as_str())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(text_chunks, vec!["Verbose response"]);

    // response_text must be populated
    let result = processor.finish();
    assert_eq!(result.response_text, "Verbose response");
}

/// In stream-json mode, two API calls in one turn produce two sequential Assistant messages
/// with no ContentBlockDelta events. The second (synthesis) text must NOT be dropped.
#[test]
fn test_task_continuation_synthesis_text_not_dropped() {
    let mut processor = StreamProcessor::new();

    // API Call 1: subagent spawns, agent produces first text
    let msg1 = StreamMessage::Assistant {
        message: AssistantMessage {
            content: vec![AssistantContent::Text {
                text: "Good call, spawning subagent...".to_string(),
            }],
            stop_reason: Some("tool_use".to_string()),
            usage: None,
        },
        session_id: None,
    };

    // API Call 2: after subagent completes, agent synthesizes result
    let msg2 = StreamMessage::Assistant {
        message: AssistantMessage {
            content: vec![AssistantContent::Text {
                text: "Confirmed: dead code removed successfully.".to_string(),
            }],
            stop_reason: Some("end_turn".to_string()),
            usage: None,
        },
        session_id: None,
    };

    processor.process_message(msg1);
    processor.process_message(msg2);

    let result = processor.finish();

    assert_eq!(
        result.response_text,
        "Good call, spawning subagent...Confirmed: dead code removed successfully.",
        "Synthesis text from second API call must not be dropped"
    );

    let text_blocks: Vec<_> = result
        .content_blocks
        .iter()
        .filter_map(|b| {
            if let ContentBlockItem::Text { text } = b {
                Some(text.as_str())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        text_blocks.len(),
        2,
        "Both text blocks must appear in content_blocks"
    );
    assert_eq!(text_blocks[0], "Good call, spawning subagent...");
    assert_eq!(text_blocks[1], "Confirmed: dead code removed successfully.");
}

/// When streaming deltas ARE received, the dedup guard must still prevent double-emission
/// when the verbose Assistant summary arrives for the same API call.
#[test]
fn test_streaming_deltas_dedup_still_works() {
    let mut processor = StreamProcessor::new();

    // Streaming deltas arrive first
    let delta = StreamMessage::ContentBlockDelta {
        index: Some(0),
        delta: ContentDelta {
            delta_type: "text_delta".to_string(),
            text: Some("Hello world".to_string()),
            partial_json: None,
        },
    };
    processor.process_message(delta);

    // Verbose Assistant summary with identical text arrives after
    let verbose_msg = StreamMessage::Assistant {
        message: AssistantMessage {
            content: vec![AssistantContent::Text {
                text: "Hello world".to_string(),
            }],
            stop_reason: Some("end_turn".to_string()),
            usage: None,
        },
        session_id: None,
    };
    let verbose_events = processor.process_message(verbose_msg);

    // No TextChunk must be emitted by the verbose message
    let text_chunk_count = verbose_events
        .iter()
        .filter(|e| matches!(e, StreamEvent::TextChunk(_)))
        .count();
    assert_eq!(
        text_chunk_count, 0,
        "Dedup guard must suppress TextChunk when streaming deltas already emitted it"
    );

    // response_text must contain the text exactly once
    let result = processor.finish();
    assert_eq!(
        result.response_text, "Hello world",
        "response_text must not be duplicated"
    );
}

/// 3 sequential API calls in one stream-json turn (each as an Assistant message with no
/// ContentBlockDelta events) must all accumulate into `response_text` and `content_blocks`.
#[test]
fn test_three_api_calls_all_text_accumulated() {
    let mut processor = StreamProcessor::new();

    // API Call 1: stop_reason tool_use → more calls follow
    let msg1 = StreamMessage::Assistant {
        message: AssistantMessage {
            content: vec![AssistantContent::Text {
                text: "Step one: planning.".to_string(),
            }],
            stop_reason: Some("tool_use".to_string()),
            usage: None,
        },
        session_id: None,
    };

    // API Call 2: another tool_use
    let msg2 = StreamMessage::Assistant {
        message: AssistantMessage {
            content: vec![AssistantContent::Text {
                text: "Step two: executing.".to_string(),
            }],
            stop_reason: Some("tool_use".to_string()),
            usage: None,
        },
        session_id: None,
    };

    // API Call 3: final end_turn
    let msg3 = StreamMessage::Assistant {
        message: AssistantMessage {
            content: vec![AssistantContent::Text {
                text: "Step three: done.".to_string(),
            }],
            stop_reason: Some("end_turn".to_string()),
            usage: None,
        },
        session_id: None,
    };

    processor.process_message(msg1);
    processor.process_message(msg2);
    processor.process_message(msg3);

    let result = processor.finish();

    assert_eq!(
        result.response_text, "Step one: planning.Step two: executing.Step three: done.",
        "All 3 API call texts must appear in response_text"
    );

    let text_blocks: Vec<_> = result
        .content_blocks
        .iter()
        .filter_map(|b| {
            if let ContentBlockItem::Text { text } = b {
                Some(text.as_str())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        text_blocks.len(),
        3,
        "All 3 text blocks must appear in content_blocks"
    );
    assert_eq!(text_blocks[0], "Step one: planning.");
    assert_eq!(text_blocks[1], "Step two: executing.");
    assert_eq!(text_blocks[2], "Step three: done.");
}

/// Exact ideation-agent scenario that triggered the original bug:
/// API Call 1 has BOTH a Text block AND a ToolUse block (stop_reason: tool_use).
/// API Call 2 is the synthesis text only (stop_reason: end_turn).
/// Both texts must appear in `response_text` and `content_blocks`.
#[test]
fn test_text_plus_tool_use_then_synthesis_text_not_dropped() {
    let mut processor = StreamProcessor::new();

    // API Call 1: text + tool_use (no ContentBlockDelta events — stream-json mode)
    let msg1 = StreamMessage::Assistant {
        message: AssistantMessage {
            content: vec![
                AssistantContent::Text {
                    text: "I'll search for that.".to_string(),
                },
                AssistantContent::ToolUse {
                    id: "toolu_search1".to_string(),
                    name: "Task".to_string(),
                    input: serde_json::json!({
                        "description": "Find unused functions",
                        "subagent_type": "Explore"
                    }),
                },
            ],
            usage: None,
            stop_reason: Some("tool_use".to_string()),
        },
        session_id: None,
    };

    // API Call 2: synthesis text after subagent completed
    let msg2 = StreamMessage::Assistant {
        message: AssistantMessage {
            content: vec![AssistantContent::Text {
                text: "Found 3 unused functions.".to_string(),
            }],
            stop_reason: Some("end_turn".to_string()),
            usage: None,
        },
        session_id: None,
    };

    processor.process_message(msg1);
    processor.process_message(msg2);

    let result = processor.finish();

    assert_eq!(
        result.response_text, "I'll search for that.Found 3 unused functions.",
        "Both texts must appear in response_text; synthesis must not be dropped"
    );

    let text_blocks: Vec<_> = result
        .content_blocks
        .iter()
        .filter_map(|b| {
            if let ContentBlockItem::Text { text } = b {
                Some(text.as_str())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        text_blocks.len(),
        2,
        "Both text content blocks must be present"
    );
    assert_eq!(text_blocks[0], "I'll search for that.");
    assert_eq!(text_blocks[1], "Found 3 unused functions.");
}

/// `reset_for_next_turn` must clear `had_streaming_text_deltas` so that verbose-only
/// Assistant messages in the NEXT turn are allowed through the dedup guard.
#[test]
fn test_had_streaming_text_deltas_resets_across_turns() {
    let mut processor = StreamProcessor::new();

    // Turn 1: streaming delta (sets had_streaming_text_deltas = true)
    let delta = StreamMessage::ContentBlockDelta {
        index: Some(0),
        delta: ContentDelta {
            delta_type: "text_delta".to_string(),
            text: Some("streamed text".to_string()),
            partial_json: None,
        },
    };
    processor.process_message(delta);

    // Turn 1: verbose Assistant summary — must be suppressed
    let verbose_turn1 = StreamMessage::Assistant {
        message: AssistantMessage {
            content: vec![AssistantContent::Text {
                text: "streamed text".to_string(),
            }],
            stop_reason: Some("end_turn".to_string()),
            usage: None,
        },
        session_id: None,
    };
    let turn1_events = processor.process_message(verbose_turn1);
    let turn1_text_chunks = turn1_events
        .iter()
        .filter(|e| matches!(e, StreamEvent::TextChunk(_)))
        .count();
    assert_eq!(
        turn1_text_chunks, 0,
        "Turn 1 verbose summary must be suppressed by dedup guard"
    );

    // Reset for next turn — clears had_streaming_text_deltas
    processor.reset_for_next_turn();

    // Turn 2: verbose-only Assistant message (no deltas) — must emit TextChunk and accumulate
    let verbose_turn2 = StreamMessage::Assistant {
        message: AssistantMessage {
            content: vec![AssistantContent::Text {
                text: "Turn two result.".to_string(),
            }],
            stop_reason: Some("end_turn".to_string()),
            usage: None,
        },
        session_id: None,
    };
    let turn2_events = processor.process_message(verbose_turn2);

    let turn2_text_chunks: Vec<_> = turn2_events
        .iter()
        .filter_map(|e| {
            if let StreamEvent::TextChunk(t) = e {
                Some(t.as_str())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        turn2_text_chunks,
        vec!["Turn two result."],
        "Turn 2 verbose text must emit TextChunk after flag was reset"
    );

    // response_text must contain turn 2 text
    let result = processor.finish();
    assert_eq!(
        result.response_text, "Turn two result.",
        "Turn 2 response_text must be populated after reset"
    );
}

/// An empty string in a subsequent API call must not corrupt `response_text`
/// and must not panic.
#[test]
fn test_empty_text_in_second_api_call_handled_gracefully() {
    let mut processor = StreamProcessor::new();

    // API Call 1: normal text
    let msg1 = StreamMessage::Assistant {
        message: AssistantMessage {
            content: vec![AssistantContent::Text {
                text: "First call.".to_string(),
            }],
            stop_reason: Some("tool_use".to_string()),
            usage: None,
        },
        session_id: None,
    };

    // API Call 2: empty text (degenerate synthesis)
    let msg2 = StreamMessage::Assistant {
        message: AssistantMessage {
            content: vec![AssistantContent::Text {
                text: String::new(),
            }],
            stop_reason: Some("end_turn".to_string()),
            usage: None,
        },
        session_id: None,
    };

    processor.process_message(msg1);
    processor.process_message(msg2); // must not panic

    let result = processor.finish();

    assert_eq!(
        result.response_text, "First call.",
        "Empty second-call text must not corrupt response_text"
    );
}

/// Multi-API-call stream-json turn fed via `parse_line` + `process_parsed_line`
/// (the production path) must accumulate all text correctly.
#[test]
fn test_multi_api_call_via_parse_line_accumulates_text() {
    let mut processor = StreamProcessor::new();

    // In stream-json mode the CLI emits full assistant JSON objects as lines.
    // Feed two sequential assistant lines (no ContentBlockDelta events).
    let line1 = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Preparing analysis."}],"stop_reason":"tool_use","model":"claude-opus-4-5"},"session_id":null}"#;
    let line2 = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Analysis complete."}],"stop_reason":"end_turn","model":"claude-opus-4-5"},"session_id":null}"#;

    if let Some(parsed) = StreamProcessor::parse_line(line1) {
        processor.process_parsed_line(parsed);
    }
    if let Some(parsed) = StreamProcessor::parse_line(line2) {
        processor.process_parsed_line(parsed);
    }

    let result = processor.finish();

    assert_eq!(
        result.response_text, "Preparing analysis.Analysis complete.",
        "Both assistant lines must be accumulated via parse_line + process_parsed_line"
    );

    let text_blocks: Vec<_> = result
        .content_blocks
        .iter()
        .filter_map(|b| {
            if let ContentBlockItem::Text { text } = b {
                Some(text.as_str())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        text_blocks.len(),
        2,
        "Both text content blocks must be present after parse_line path"
    );
}

/// Once streaming mode is active in a turn (ContentBlockDelta text events received),
/// ALL subsequent verbose Assistant text in that same turn is suppressed.
/// This documents the invariant explicitly.
///
/// Invariant: Once `had_streaming_text_deltas = true`, verbose text in the SAME turn
/// is always suppressed — regardless of content.
#[test]
fn test_streaming_mode_assumption_documented() {
    let mut processor = StreamProcessor::new();

    // Streaming delta arrives — activates streaming mode for this turn
    let delta = StreamMessage::ContentBlockDelta {
        index: Some(0),
        delta: ContentDelta {
            delta_type: "text_delta".to_string(),
            text: Some("streamed".to_string()),
            partial_json: None,
        },
    };
    processor.process_message(delta);

    // Verbose Assistant summary with identical content arrives (Claude CLI always sends this)
    let verbose_msg = StreamMessage::Assistant {
        message: AssistantMessage {
            content: vec![AssistantContent::Text {
                text: "streamed".to_string(),
            }],
            stop_reason: Some("end_turn".to_string()),
            usage: None,
        },
        session_id: None,
    };
    let verbose_events = processor.process_message(verbose_msg);

    // Once streaming mode is active in a turn, ALL verbose text is suppressed.
    // This prevents the duplicate-text bug where the same content would appear twice.
    let text_chunk_count = verbose_events
        .iter()
        .filter(|e| matches!(e, StreamEvent::TextChunk(_)))
        .count();
    assert_eq!(
        text_chunk_count, 0,
        "Once streaming mode is active in a turn, ALL verbose text must be suppressed"
    );

    // The streamed text must appear exactly once
    let result = processor.finish();
    assert_eq!(
        result.response_text, "streamed",
        "response_text must contain streamed text exactly once"
    );
}

#[test]
fn test_system_without_subtype_still_works() {
    let mut processor = StreamProcessor::new();

    // Regular system message (no subtype) should still emit SessionId
    let msg = StreamMessage::System {
        message: Some("Init".to_string()),
        session_id: Some("sess-regular".to_string()),
        subtype: None,
        hook_id: None,
        hook_name: None,
        hook_event: None,
        output: None,
        exit_code: None,
        outcome: None,
    };

    let events = processor.process_message(msg);
    assert_eq!(events.len(), 1);
    assert!(matches!(&events[0], StreamEvent::SessionId(id) if id == "sess-regular"));
}

/// With `--include-partial-messages`, text arrives only as deltas and the dedup
/// guard suppresses the verbose Assistant summary — including its
/// `content_blocks` push. `content_block_stop` must therefore seal the streamed
/// text into `content_blocks`, or a delta-streamed turn persists no timeline
/// text at all (the chat UI renders the turn as unanswered).
#[test]
fn test_streamed_text_reaches_content_blocks_via_content_block_stop() {
    let mut processor = StreamProcessor::new();

    for piece in ["Streamed ", "answer ", "text."] {
        processor.process_message(StreamMessage::ContentBlockDelta {
            index: Some(0),
            delta: ContentDelta {
                delta_type: "text_delta".to_string(),
                text: Some(piece.to_string()),
                partial_json: None,
            },
        });
    }

    processor.process_message(StreamMessage::ContentBlockStop { index: Some(0) });

    // The verbose summary for the same message is deduped away.
    processor.process_message(StreamMessage::Assistant {
        message: AssistantMessage {
            content: vec![AssistantContent::Text {
                text: "Streamed answer text.".to_string(),
            }],
            stop_reason: Some("end_turn".to_string()),
            usage: None,
        },
        session_id: None,
    });

    let result = processor.finish();
    let text_blocks: Vec<&String> = result
        .content_blocks
        .iter()
        .filter_map(|block| match block {
            ContentBlockItem::Text { text } => Some(text),
            _ => None,
        })
        .collect();

    assert_eq!(
        text_blocks.len(),
        1,
        "streamed text must land in content_blocks exactly once, got {:?}",
        result.content_blocks
    );
    assert_eq!(text_blocks[0], "Streamed answer text.");
    assert_eq!(
        result.response_text, "Streamed answer text.",
        "the dedup guard must still keep response_text single-copy"
    );
}
