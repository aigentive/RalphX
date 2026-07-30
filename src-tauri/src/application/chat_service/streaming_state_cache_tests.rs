use super::*;

fn cached_streaming_task(tool_use_id: &str) -> CachedStreamingTask {
    CachedStreamingTask {
        tool_use_id: tool_use_id.to_string(),
        description: None,
        subagent_type: None,
        model: None,
        status: "running".to_string(),
        agent_id: None,
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
        started_at: None,
        completed_at: None,
        timestamp_provenance: None,
        seq: None,
    }
}

#[tokio::test]
async fn test_new_cache_is_empty() {
    let cache = StreamingStateCache::new();
    let state = cache.get("conv-123").await;
    assert!(state.is_none());
}

#[tokio::test]
async fn test_changing_run_id_discards_stale_transient_projection() {
    let cache = StreamingStateCache::new();
    cache
        .set_run_id("conv-123", Some("run-1".to_string()))
        .await;
    cache.append_text("conv-123", 0, "stale text").await;
    cache
        .upsert_tool_call(
            "conv-123",
            CachedToolCall {
                id: "toolu_stale".to_string(),
                name: "bash".to_string(),
                arguments: serde_json::json!({}),
                result: None,
                diff_context: None,
                parent_tool_use_id: None,
            },
        )
        .await;
    cache
        .add_task("conv-123", cached_streaming_task("task-stale"))
        .await;

    cache
        .set_run_id("conv-123", Some("run-2".to_string()))
        .await;

    let state = cache.get("conv-123").await.unwrap();
    assert_eq!(state.run_id.as_deref(), Some("run-2"));
    assert!(state.partial_text.is_empty());
    assert!(state.partial_text_segments.is_empty());
    assert!(state.tool_calls.is_empty());
    assert!(state.streaming_tasks.is_empty());
}

#[tokio::test]
async fn test_add_task_for_run_rejects_stale_run_and_accepts_current_run() {
    let cache = StreamingStateCache::new();
    cache
        .set_run_id("conv-123", Some("run-current".to_string()))
        .await;

    assert!(
        !cache
            .add_task_for_run("conv-123", "run-stale", cached_streaming_task("task-stale"),)
            .await
    );
    assert!(
        cache
            .add_task_for_run(
                "conv-123",
                "run-current",
                cached_streaming_task("task-current"),
            )
            .await
    );

    let state = cache.get("conv-123").await.unwrap();
    assert_eq!(state.streaming_tasks.len(), 1);
    assert_eq!(state.streaming_tasks[0].tool_use_id, "task-current");
}

#[tokio::test]
async fn test_upsert_tool_call_creates_state() {
    let cache = StreamingStateCache::new();
    let tool_call = CachedToolCall {
        id: "toolu_001".to_string(),
        name: "bash".to_string(),
        arguments: serde_json::json!({"command": "ls"}),
        result: None,
        diff_context: None,
        parent_tool_use_id: None,
    };

    cache.upsert_tool_call("conv-123", tool_call).await;

    let state = cache.get("conv-123").await;
    assert!(state.is_some());
    let state = state.unwrap();
    assert_eq!(state.tool_calls.len(), 1);
    assert_eq!(state.tool_calls[0].name, "bash");
}

#[tokio::test]
async fn test_upsert_tool_call_updates_existing() {
    let cache = StreamingStateCache::new();

    // Add initial tool call
    let tool_call = CachedToolCall {
        id: "toolu_001".to_string(),
        name: "bash".to_string(),
        arguments: serde_json::json!({"command": "ls"}),
        result: None,
        diff_context: None,
        parent_tool_use_id: None,
    };
    cache.upsert_tool_call("conv-123", tool_call).await;

    // Update with result
    let updated = CachedToolCall {
        id: "toolu_001".to_string(),
        name: "bash".to_string(),
        arguments: serde_json::json!({"command": "ls"}),
        result: Some(serde_json::json!({"output": "file1.txt\nfile2.txt"})),
        diff_context: None,
        parent_tool_use_id: None,
    };
    cache.upsert_tool_call("conv-123", updated).await;

    let state = cache.get("conv-123").await.unwrap();
    assert_eq!(state.tool_calls.len(), 1); // Still just one
    assert!(state.tool_calls[0].result.is_some());
}

#[tokio::test]
async fn append_thinking_keeps_partial_text_isolated() {
    let cache = StreamingStateCache::new();

    cache.append_text("conv-123", 1, "answer").await;
    cache.append_thinking("conv-123", 0, "reasoning").await;

    let state = cache.get("conv-123").await.unwrap();
    assert_eq!(state.partial_text, "answer");
    assert_eq!(
        state.partial_text_segments,
        vec!["".to_string(), "answer".to_string()]
    );
    assert_eq!(
        state.partial_thinking_segments,
        vec!["reasoning".to_string()]
    );
}

#[tokio::test]
async fn test_add_task() {
    let cache = StreamingStateCache::new();
    let task = CachedStreamingTask {
        description: Some("Running tests".to_string()),
        subagent_type: Some("ralphx:coder".to_string()),
        model: Some("sonnet".to_string()),
        ..cached_streaming_task("toolu_002")
    };

    cache.add_task("conv-123", task).await;

    let state = cache.get("conv-123").await.unwrap();
    assert_eq!(state.streaming_tasks.len(), 1);
    assert_eq!(state.streaming_tasks[0].status, "running");
}

#[tokio::test]
async fn test_complete_task() {
    let cache = StreamingStateCache::new();
    let task = CachedStreamingTask {
        description: Some("Running tests".to_string()),
        subagent_type: Some("ralphx:coder".to_string()),
        model: Some("sonnet".to_string()),
        ..cached_streaming_task("toolu_002")
    };
    cache.add_task("conv-123", task).await;

    cache.complete_task("conv-123", "toolu_002", None).await;

    let state = cache.get("conv-123").await.unwrap();
    assert_eq!(state.streaming_tasks[0].status, "completed");
}

#[tokio::test]
async fn test_append_text() {
    let cache = StreamingStateCache::new();

    cache.append_text("conv-123", 0, "Hello ").await;
    cache.append_text("conv-123", 0, "world!").await;

    let state = cache.get("conv-123").await.unwrap();
    assert_eq!(state.partial_text, "Hello world!");
    assert_eq!(state.partial_text_segments, vec!["Hello world!"]);
    assert_eq!(
        state.partial_text,
        state.partial_text_segments.concat(),
        "joined partial text must remain compatible with segment recovery"
    );
}

#[tokio::test]
async fn test_append_text_keeps_text_blocks_ordered_and_fills_gaps() {
    let cache = StreamingStateCache::new();

    cache.append_text("conv-123", 0, "Before ").await;
    cache.append_text("conv-123", 2, "After").await;
    cache.append_text("conv-123", 0, "tool").await;

    let state = cache.get("conv-123").await.unwrap();
    assert_eq!(
        state.partial_text_segments,
        vec!["Before tool", "", "After"]
    );
    assert_eq!(state.partial_text, "Before toolAfter");
    assert_eq!(
        state.partial_text,
        state.partial_text_segments.concat(),
        "visible text must remain contiguous when the tool block position stays empty"
    );
}

#[tokio::test]
async fn test_clear() {
    let cache = StreamingStateCache::new();
    let tool_call = CachedToolCall {
        id: "toolu_001".to_string(),
        name: "bash".to_string(),
        arguments: serde_json::json!({}),
        result: None,
        diff_context: None,
        parent_tool_use_id: None,
    };
    cache.upsert_tool_call("conv-123", tool_call).await;

    cache.clear("conv-123").await;

    let state = cache.get("conv-123").await;
    assert!(state.is_none());
}

#[tokio::test]
async fn test_clear_nonexistent_is_noop() {
    let cache = StreamingStateCache::new();
    // Should not panic
    cache.clear("nonexistent").await;
}

#[tokio::test]
async fn test_multiple_conversations_independent() {
    let cache = StreamingStateCache::new();

    let tool1 = CachedToolCall {
        id: "toolu_001".to_string(),
        name: "bash".to_string(),
        arguments: serde_json::json!({}),
        result: None,
        diff_context: None,
        parent_tool_use_id: None,
    };
    let tool2 = CachedToolCall {
        id: "toolu_002".to_string(),
        name: "read".to_string(),
        arguments: serde_json::json!({"file_path": "/tmp/test.txt"}),
        result: None,
        diff_context: None,
        parent_tool_use_id: None,
    };

    cache.upsert_tool_call("conv-1", tool1).await;
    cache.upsert_tool_call("conv-2", tool2).await;

    let state1 = cache.get("conv-1").await.unwrap();
    let state2 = cache.get("conv-2").await.unwrap();

    assert_eq!(state1.tool_calls.len(), 1);
    assert_eq!(state1.tool_calls[0].name, "bash");
    assert_eq!(state2.tool_calls.len(), 1);
    assert_eq!(state2.tool_calls[0].name, "read");

    // Clear one doesn't affect the other
    cache.clear("conv-1").await;
    assert!(cache.get("conv-1").await.is_none());
    assert!(cache.get("conv-2").await.is_some());
}

#[tokio::test]
async fn test_updated_at_changes_on_modification() {
    let cache = StreamingStateCache::new();

    cache.append_text("conv-123", 0, "test").await;
    let first_update = cache.get("conv-123").await.unwrap().updated_at;

    // Small delay to ensure timestamp difference
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    cache.append_text("conv-123", 0, " more").await;
    let second_update = cache.get("conv-123").await.unwrap().updated_at;

    assert!(second_update > first_update);
}

#[tokio::test]
async fn test_serialize_produces_expected_json() {
    let state = ConversationStreamingState {
        run_id: Some("run-1".to_string()),
        tool_calls: vec![CachedToolCall {
            id: "toolu_001".to_string(),
            name: "bash".to_string(),
            arguments: serde_json::json!({"command": "ls"}),
            result: None,
            diff_context: None,
            parent_tool_use_id: None,
        }],
        streaming_tasks: vec![CachedStreamingTask {
            description: Some("Test task".to_string()),
            ..cached_streaming_task("toolu_002")
        }],
        partial_text: "Hello".to_string(),
        partial_text_segments: vec!["Hello".to_string()],
        partial_thinking_segments: vec![],
        updated_at: Utc::now(),
    };

    let json = serde_json::to_string(&state).unwrap();
    assert!(json.contains("\"tool_calls\""));
    assert!(json.contains("\"streaming_tasks\""));
    assert!(json.contains("\"partial_text\""));
    assert!(json.contains("\"partial_text_segments\""));
    assert!(json.contains("\"toolu_001\""));
    assert!(json.contains("\"running\""));
    assert!(json.contains("\"Hello\""));
}

#[tokio::test]
async fn test_serialize_skips_none_fields() {
    let tool_call = CachedToolCall {
        id: "toolu_001".to_string(),
        name: "bash".to_string(),
        arguments: serde_json::json!({}),
        result: None,
        diff_context: None,
        parent_tool_use_id: None,
    };

    let json = serde_json::to_string(&tool_call).unwrap();
    assert!(!json.contains("\"result\""));
    assert!(!json.contains("\"diff_context\""));
    assert!(!json.contains("\"parent_tool_use_id\""));
}

#[tokio::test]
async fn test_complete_task_with_stats() {
    let cache = StreamingStateCache::new();
    let task = CachedStreamingTask {
        description: Some("Running tests".to_string()),
        ..cached_streaming_task("toolu_002")
    };
    cache.add_task("conv-123", task).await;

    use crate::infrastructure::agents::claude::ToolCallStats;
    let stats = ToolCallStats {
        model: Some("sonnet".to_string()),
        total_tokens: Some(1234),
        total_tool_uses: Some(5),
        duration_ms: Some(30000),
    };
    cache
        .complete_task("conv-123", "toolu_002", Some(stats))
        .await;

    let state = cache.get("conv-123").await.unwrap();
    assert_eq!(state.streaming_tasks[0].status, "completed");
    assert_eq!(state.streaming_tasks[0].total_tokens, Some(1234));
    assert_eq!(state.streaming_tasks[0].total_tool_uses, Some(5));
    assert_eq!(state.streaming_tasks[0].duration_ms, Some(30000));
}

#[tokio::test]
async fn test_complete_task_with_none_stats_clears_nothing() {
    let cache = StreamingStateCache::new();
    let task = cached_streaming_task("toolu_003");
    cache.add_task("conv-abc", task).await;

    cache.complete_task("conv-abc", "toolu_003", None).await;

    let state = cache.get("conv-abc").await.unwrap();
    assert_eq!(state.streaming_tasks[0].status, "completed");
    assert_eq!(state.streaming_tasks[0].total_tokens, None);
    assert_eq!(state.streaming_tasks[0].total_tool_uses, None);
    assert_eq!(state.streaming_tasks[0].duration_ms, None);
}

#[tokio::test]
async fn test_add_task_replaces_existing_tool_use_id() {
    let cache = StreamingStateCache::new();

    cache
        .add_task(
            "conv-123",
            CachedStreamingTask {
                description: Some("Initial".to_string()),
                subagent_type: Some("delegated".to_string()),
                model: Some("sonnet".to_string()),
                delegated_job_id: Some("job-1".to_string()),
                ..cached_streaming_task("toolu_002")
            },
        )
        .await;

    cache
        .add_task(
            "conv-123",
            CachedStreamingTask {
                description: Some("Updated".to_string()),
                subagent_type: Some("delegated".to_string()),
                model: Some("gpt-5.4".to_string()),
                status: "completed".to_string(),
                agent_id: Some("run-1".to_string()),
                delegated_job_id: Some("job-1".to_string()),
                delegated_session_id: Some("delegated-session-1".to_string()),
                delegated_conversation_id: Some("conv-child".to_string()),
                delegated_agent_run_id: Some("run-1".to_string()),
                provider_harness: Some("codex".to_string()),
                provider_session_id: Some("provider-session-1".to_string()),
                upstream_provider: Some("openai".to_string()),
                provider_profile: Some("prod".to_string()),
                logical_model: Some("gpt-5.4".to_string()),
                effective_model_id: Some("gpt-5.4-2026-04-01".to_string()),
                logical_effort: Some("high".to_string()),
                effective_effort: Some("high".to_string()),
                approval_policy: Some("never".to_string()),
                sandbox_mode: Some("danger-full-access".to_string()),
                total_tokens: Some(111),
                total_tool_uses: Some(2),
                duration_ms: Some(3000),
                input_tokens: Some(11),
                output_tokens: Some(22),
                cache_creation_tokens: Some(33),
                cache_read_tokens: Some(44),
                estimated_usd: Some(0.55),
                text_output: Some("done".to_string()),
                ..cached_streaming_task("toolu_002")
            },
        )
        .await;

    let state = cache.get("conv-123").await.unwrap();
    assert_eq!(state.streaming_tasks.len(), 1);
    let task = &state.streaming_tasks[0];
    assert_eq!(task.description.as_deref(), Some("Updated"));
    assert_eq!(task.status, "completed");
    assert_eq!(task.provider_harness.as_deref(), Some("codex"));
    assert_eq!(task.input_tokens, Some(11));
    assert_eq!(task.text_output.as_deref(), Some("done"));
}

#[tokio::test]
async fn test_delegate_completion_preserves_start_metadata_and_clock_by_job_id() {
    let cache = StreamingStateCache::new();
    cache
        .add_task(
            "conv-123",
            CachedStreamingTask {
                description: Some("Investigate cache".to_string()),
                delegated_job_id: Some("job-1".to_string()),
                started_at: Some("2026-07-23T00:00:00Z".to_string()),
                timestamp_provenance: Some("delegation_job".to_string()),
                seq: Some(10),
                ..cached_streaming_task("provider-tool-1")
            },
        )
        .await;
    cache
        .add_task(
            "conv-123",
            CachedStreamingTask {
                status: "completed".to_string(),
                delegated_job_id: Some("job-1".to_string()),
                completed_at: Some("2026-07-23T00:00:05Z".to_string()),
                timestamp_provenance: Some("delegated_run".to_string()),
                seq: Some(11),
                ..cached_streaming_task("delegate-job:job-1")
            },
        )
        .await;

    let state = cache.get("conv-123").await.unwrap();
    assert_eq!(state.streaming_tasks.len(), 1);
    let task = &state.streaming_tasks[0];
    assert_eq!(task.tool_use_id, "provider-tool-1");
    assert_eq!(task.description.as_deref(), Some("Investigate cache"));
    assert_eq!(task.started_at.as_deref(), Some("2026-07-23T00:00:00Z"));
    assert_eq!(task.completed_at.as_deref(), Some("2026-07-23T00:00:05Z"));
    assert_eq!(task.status, "completed");
}

#[tokio::test]
async fn test_stale_running_delegate_update_cannot_revive_terminal_task() {
    let cache = StreamingStateCache::new();
    cache
        .add_task(
            "conv-123",
            CachedStreamingTask {
                status: "completed".to_string(),
                delegated_job_id: Some("job-1".to_string()),
                completed_at: Some("2026-07-23T00:00:05Z".to_string()),
                seq: Some(20),
                ..cached_streaming_task("delegate-job:job-1")
            },
        )
        .await;
    cache
        .add_task(
            "conv-123",
            CachedStreamingTask {
                delegated_job_id: Some("job-1".to_string()),
                seq: Some(20),
                ..cached_streaming_task("provider-tool-1")
            },
        )
        .await;

    let state = cache.get("conv-123").await.unwrap();
    assert_eq!(state.streaming_tasks.len(), 1);
    assert_eq!(state.streaming_tasks[0].status, "completed");
    assert_eq!(
        state.streaming_tasks[0].completed_at.as_deref(),
        Some("2026-07-23T00:00:05Z")
    );
}

#[tokio::test]
async fn test_started_at_uses_earliest_rfc3339_instant_and_deterministic_legacy_fallback() {
    let cache = StreamingStateCache::new();
    cache
        .add_task(
            "conv-123",
            CachedStreamingTask {
                delegated_job_id: Some("job-1".to_string()),
                started_at: Some("2026-07-23T03:00:00+03:00".to_string()),
                seq: Some(1),
                ..cached_streaming_task("delegate-job:job-1")
            },
        )
        .await;
    cache
        .add_task(
            "conv-123",
            CachedStreamingTask {
                delegated_job_id: Some("job-1".to_string()),
                started_at: Some("2026-07-23T00:30:00Z".to_string()),
                seq: Some(2),
                ..cached_streaming_task("delegate-job:job-1")
            },
        )
        .await;

    let state = cache.get("conv-123").await.unwrap();
    assert_eq!(
        state.streaming_tasks[0].started_at.as_deref(),
        Some("2026-07-23T03:00:00+03:00")
    );

    cache
        .add_task(
            "conv-legacy",
            CachedStreamingTask {
                delegated_job_id: Some("job-legacy".to_string()),
                started_at: Some("not-a-date-z".to_string()),
                seq: Some(1),
                ..cached_streaming_task("delegate-job:job-legacy")
            },
        )
        .await;
    cache
        .add_task(
            "conv-legacy",
            CachedStreamingTask {
                delegated_job_id: Some("job-legacy".to_string()),
                started_at: Some("not-a-date-a".to_string()),
                seq: Some(2),
                ..cached_streaming_task("delegate-job:job-legacy")
            },
        )
        .await;

    let legacy_state = cache.get("conv-legacy").await.unwrap();
    assert_eq!(
        legacy_state.streaming_tasks[0].started_at.as_deref(),
        Some("not-a-date-a")
    );
}

#[tokio::test]
async fn test_stale_terminal_delegate_update_cannot_replace_newer_running_attempt_metadata() {
    let cache = StreamingStateCache::new();
    cache
        .add_task(
            "conv-123",
            CachedStreamingTask {
                status: "running".to_string(),
                delegated_job_id: Some("job-1".to_string()),
                delegated_agent_run_id: Some("run-1".to_string()),
                description: Some("current attempt".to_string()),
                started_at: Some("2026-07-23T00:10:00Z".to_string()),
                seq: Some(20),
                ..cached_streaming_task("delegate-job:job-1")
            },
        )
        .await;
    cache
        .add_task(
            "conv-123",
            CachedStreamingTask {
                status: "completed".to_string(),
                delegated_job_id: Some("job-1".to_string()),
                delegated_agent_run_id: Some("run-1".to_string()),
                description: Some("stale attempt".to_string()),
                completed_at: Some("2026-07-23T00:05:00Z".to_string()),
                timestamp_provenance: Some("delegated_run".to_string()),
                seq: Some(10),
                ..cached_streaming_task("delegate-job:job-1")
            },
        )
        .await;

    let state = cache.get("conv-123").await.unwrap();
    let task = &state.streaming_tasks[0];
    assert_eq!(task.status, "running");
    assert_eq!(task.description.as_deref(), Some("current attempt"));
    assert_eq!(task.completed_at, None);
    assert_eq!(task.timestamp_provenance, None);
    assert_eq!(task.seq, Some(20));
    assert_eq!(task.delegated_job_id.as_deref(), Some("job-1"));
    assert_eq!(task.delegated_agent_run_id.as_deref(), Some("run-1"));
}

#[tokio::test]
async fn test_conflicting_identity_and_missing_sequence_updates_are_rejected() {
    let cache = StreamingStateCache::new();
    cache
        .add_task(
            "conv-123",
            CachedStreamingTask {
                delegated_job_id: Some("job-1".to_string()),
                delegated_agent_run_id: Some("run-1".to_string()),
                description: Some("current task".to_string()),
                seq: Some(4),
                ..cached_streaming_task("provider-tool-1")
            },
        )
        .await;

    cache
        .add_task(
            "conv-123",
            CachedStreamingTask {
                delegated_job_id: Some("job-2".to_string()),
                delegated_agent_run_id: Some("run-2".to_string()),
                description: Some("conflicting task".to_string()),
                seq: Some(5),
                ..cached_streaming_task("provider-tool-1")
            },
        )
        .await;
    cache
        .add_task(
            "conv-123",
            CachedStreamingTask {
                delegated_job_id: Some("job-1".to_string()),
                delegated_agent_run_id: Some("run-1".to_string()),
                description: Some("unsequenced update".to_string()),
                ..cached_streaming_task("delegate-job:job-1")
            },
        )
        .await;

    let task = &cache.get("conv-123").await.unwrap().streaming_tasks[0];
    assert_eq!(task.description.as_deref(), Some("current task"));
    assert_eq!(task.delegated_job_id.as_deref(), Some("job-1"));
    assert_eq!(task.delegated_agent_run_id.as_deref(), Some("run-1"));
    assert_eq!(task.seq, Some(4));
}

#[tokio::test]
async fn test_started_at_prefers_parseable_timestamp_when_only_one_value_is_valid() {
    let cache = StreamingStateCache::new();
    cache
        .add_task(
            "conv-valid-existing",
            CachedStreamingTask {
                delegated_job_id: Some("job-1".to_string()),
                started_at: Some("2026-07-23T00:00:00Z".to_string()),
                seq: Some(1),
                ..cached_streaming_task("delegate-job:job-1")
            },
        )
        .await;
    cache
        .add_task(
            "conv-valid-existing",
            CachedStreamingTask {
                delegated_job_id: Some("job-1".to_string()),
                started_at: Some("not-a-date".to_string()),
                seq: Some(2),
                ..cached_streaming_task("delegate-job:job-1")
            },
        )
        .await;
    cache
        .add_task(
            "conv-valid-incoming",
            CachedStreamingTask {
                delegated_job_id: Some("job-2".to_string()),
                started_at: Some("not-a-date".to_string()),
                seq: Some(1),
                ..cached_streaming_task("delegate-job:job-2")
            },
        )
        .await;
    cache
        .add_task(
            "conv-valid-incoming",
            CachedStreamingTask {
                delegated_job_id: Some("job-2".to_string()),
                started_at: Some("2026-07-23T00:00:00Z".to_string()),
                seq: Some(2),
                ..cached_streaming_task("delegate-job:job-2")
            },
        )
        .await;

    assert_eq!(
        cache
            .get("conv-valid-existing")
            .await
            .unwrap()
            .streaming_tasks[0]
            .started_at
            .as_deref(),
        Some("2026-07-23T00:00:00Z")
    );
    assert_eq!(
        cache
            .get("conv-valid-incoming")
            .await
            .unwrap()
            .streaming_tasks[0]
            .started_at
            .as_deref(),
        Some("2026-07-23T00:00:00Z")
    );
}
