use chrono::{Duration, Utc};
use ralphx_lib::commands::conversation_stats_commands::{
    build_conversation_stats_response, build_scope_stats_response,
};
use ralphx_lib::domain::agents::{AgentHarnessKind, LogicalEffort, ProviderSessionRef};
use ralphx_lib::domain::entities::{
    AgentRun, ChatContextType, ChatConversation, ChatMessage, IdeationSessionId, MessageRole,
    ProjectId, TaskId, UsageProvenance,
};

#[test]
fn test_conversation_stats_prefers_runs_when_usable_coverage_ties() {
    let session_id = IdeationSessionId::new();
    let mut conversation = ChatConversation::new_ideation(session_id.clone());
    conversation.set_provider_session_ref(ProviderSessionRef {
        harness: AgentHarnessKind::Codex,
        provider_session_id: "thread-1".to_string(),
    });
    conversation.set_provider_origin(Some("openai".to_string()), None);

    let mut message = ChatMessage::orchestrator_in_session(session_id.clone(), "done");
    message.conversation_id = Some(conversation.id);
    message.provider_harness = Some(AgentHarnessKind::Codex);
    message.provider_session_id = Some("thread-1".to_string());
    message.upstream_provider = Some("openai".to_string());
    message.effective_model_id = Some("gpt-5.4".to_string());
    message.effective_effort = Some("high".to_string());
    message.input_tokens = Some(120);
    message.output_tokens = Some(40);
    message.cache_creation_tokens = Some(5);
    message.cache_read_tokens = Some(8);
    message.estimated_usd = Some(0.42);

    let mut run = AgentRun::new(conversation.id);
    run.harness = Some(AgentHarnessKind::Codex);
    run.upstream_provider = Some("openai".to_string());
    run.effective_model_id = Some("gpt-5.4".to_string());
    run.logical_effort = Some(LogicalEffort::High);
    run.effective_effort = Some("high".to_string());
    run.input_tokens = Some(999);
    run.output_tokens = Some(111);
    run.estimated_usd = Some(1.25);

    let response = build_conversation_stats_response(&conversation, &[message], &[run]);

    assert_eq!(response.usage_coverage.effective_totals_source, "runs");
    assert_eq!(response.message_usage_totals.input_tokens, 120);
    assert_eq!(response.message_usage_totals.output_tokens, 40);
    assert_eq!(response.effective_usage_totals.input_tokens, 999);
    assert_eq!(response.effective_usage_totals.output_tokens, 111);
    assert_eq!(response.effective_usage_totals.estimated_usd, Some(1.25));
    assert_eq!(response.by_harness[0].key, "codex");
    assert_eq!(response.by_harness[0].usage.input_tokens, 999);
    assert_eq!(response.by_model[0].key, "gpt-5.4");
    assert_eq!(response.by_effort[0].key, "high");
}

#[test]
fn test_conversation_source_selection_prefers_more_usable_samples() {
    let session_id = IdeationSessionId::new();
    let conversation = ChatConversation::new_ideation(session_id.clone());
    let messages = vec![tagged_message(
        &conversation,
        &session_id,
        AgentHarnessKind::Claude,
        100,
    )];
    let runs = vec![
        tagged_run(&conversation, AgentHarnessKind::Claude, 200),
        tagged_run(&conversation, AgentHarnessKind::Claude, 300),
    ];

    let response = build_conversation_stats_response(&conversation, &messages, &runs);

    assert_eq!(response.usage_coverage.effective_totals_source, "runs");
    assert_eq!(response.effective_usage_totals.input_tokens, 500);
    assert_eq!(response.usage_coverage.effective_run_conversation_count, 1);
    assert_eq!(
        response.usage_coverage.effective_message_conversation_count,
        0
    );
}

#[test]
fn test_conversation_source_selection_ignores_uncountable_coverage() {
    let session_id = IdeationSessionId::new();
    let conversation = ChatConversation::new_ideation(session_id.clone());
    let mut first_unknown = ChatMessage::orchestrator_in_session(session_id.clone(), "unknown-1");
    first_unknown.conversation_id = Some(conversation.id);
    first_unknown.input_tokens = Some(1_000);
    first_unknown.output_tokens = Some(100);
    let mut second_unknown = ChatMessage::orchestrator_in_session(session_id, "unknown-2");
    second_unknown.conversation_id = Some(conversation.id);
    second_unknown.input_tokens = Some(2_000);
    second_unknown.output_tokens = Some(200);
    let run = tagged_run(&conversation, AgentHarnessKind::Codex, 300);

    let response =
        build_conversation_stats_response(&conversation, &[first_unknown, second_unknown], &[run]);

    assert_eq!(response.usage_coverage.effective_totals_source, "runs");
    assert_eq!(response.effective_usage_totals.input_tokens, 300);
    assert_eq!(response.effective_usage_totals.processed_tokens, Some(300));
}

#[test]
fn test_conversation_source_selection_counts_legacy_logical_samples_before_normalization() {
    let session_id = IdeationSessionId::new();
    let conversation = ChatConversation::new_ideation(session_id.clone());
    let now = Utc::now();
    let messages = [100, 200, 300]
        .into_iter()
        .enumerate()
        .map(|(index, input_tokens)| {
            let mut message = tagged_message(
                &conversation,
                &session_id,
                AgentHarnessKind::Codex,
                input_tokens,
            );
            message.usage_provenance = None;
            message.provider_session_id = Some("legacy-session".to_string());
            message.created_at = now + Duration::seconds(index as i64);
            message
        })
        .collect::<Vec<_>>();
    let run = tagged_run(&conversation, AgentHarnessKind::Codex, 999);

    let response = build_conversation_stats_response(&conversation, &messages, &[run]);

    assert_eq!(response.usage_coverage.effective_totals_source, "messages");
    assert_eq!(response.effective_usage_totals.input_tokens, 600);
    assert_eq!(
        response.usage_coverage.effective_message_conversation_count,
        1
    );
}

#[test]
fn test_all_uncounted_ledgers_do_not_claim_an_effective_source() {
    let session_id = IdeationSessionId::new();
    let conversation = ChatConversation::new_ideation(session_id.clone());
    let mut message = ChatMessage::orchestrator_in_session(session_id, "unknown message");
    message.conversation_id = Some(conversation.id);
    message.input_tokens = Some(100);
    let mut run = AgentRun::new(conversation.id);
    run.input_tokens = Some(200);

    let response = build_conversation_stats_response(&conversation, &[message], &[run]);

    assert_eq!(response.usage_coverage.effective_totals_source, "none");
    assert_eq!(response.usage_coverage.effective_run_conversation_count, 0);
    assert_eq!(
        response.usage_coverage.effective_message_conversation_count,
        0
    );
    assert_eq!(response.usage_coverage.uncounted_sample_count, 1);
    assert_eq!(response.effective_usage_totals.processed_tokens, None);
}

#[test]
fn test_scope_selection_can_mix_ledgers_without_bucket_drift() {
    let first_session = IdeationSessionId::new();
    let second_session = IdeationSessionId::new();
    let first = ChatConversation::new_ideation(first_session.clone());
    let second = ChatConversation::new_ideation(second_session.clone());
    let messages = vec![
        tagged_message(&first, &first_session, AgentHarnessKind::Claude, 100),
        tagged_message(&first, &first_session, AgentHarnessKind::Claude, 200),
        tagged_message(&second, &second_session, AgentHarnessKind::Codex, 900),
    ];
    let runs = vec![
        tagged_run(&first, AgentHarnessKind::Claude, 800),
        tagged_run(&second, AgentHarnessKind::Codex, 300),
        tagged_run(&second, AgentHarnessKind::Codex, 400),
    ];

    let response =
        build_scope_stats_response("project", "project-1", &[first, second], &messages, &runs);

    assert_eq!(response.usage_coverage.effective_totals_source, "mixed");
    assert_eq!(
        response.usage_coverage.effective_message_conversation_count,
        1
    );
    assert_eq!(response.usage_coverage.effective_run_conversation_count, 1);
    assert_eq!(response.effective_usage_totals.input_tokens, 1_000);
    assert_eq!(
        response
            .by_harness
            .iter()
            .map(|bucket| bucket.usage.input_tokens)
            .sum::<u64>(),
        response.effective_usage_totals.input_tokens
    );
}

#[test]
fn test_processed_tokens_follow_provider_semantics_and_capture_quality() {
    let claude_session = IdeationSessionId::new();
    let codex_session = IdeationSessionId::new();
    let claude_conversation = ChatConversation::new_ideation(claude_session.clone());
    let codex_conversation = ChatConversation::new_ideation(codex_session.clone());
    let mut claude = tagged_message(
        &claude_conversation,
        &claude_session,
        AgentHarnessKind::Claude,
        100,
    );
    claude.output_tokens = Some(10);
    claude.cache_creation_tokens = Some(20);
    claude.cache_read_tokens = Some(30);
    let mut codex = tagged_message(
        &codex_conversation,
        &codex_session,
        AgentHarnessKind::Codex,
        100,
    );
    codex.output_tokens = Some(10);
    codex.cache_creation_tokens = Some(20);
    codex.cache_read_tokens = Some(30);

    let response = build_scope_stats_response(
        "project",
        "project-1",
        &[claude_conversation, codex_conversation],
        &[claude, codex],
        &[],
    );

    assert_eq!(response.effective_usage_totals.processed_tokens, Some(270));
    assert_eq!(response.usage_coverage.fallback_estimated_sample_count, 0);
    assert_eq!(response.usage_coverage.legacy_estimated_sample_count, 0);
    assert_eq!(response.usage_coverage.uncounted_sample_count, 0);
}

#[test]
fn test_baseline_and_fallback_quality_do_not_count_raw_baseline() {
    let session_id = IdeationSessionId::new();
    let conversation = ChatConversation::new_ideation(session_id.clone());
    let mut baseline = tagged_message(&conversation, &session_id, AgentHarnessKind::Codex, 0);
    baseline.input_tokens = None;
    baseline.output_tokens = None;
    baseline.usage_provenance = Some(UsageProvenance::CumulativeBaselineOnly);
    let mut fallback = tagged_message(&conversation, &session_id, AgentHarnessKind::Codex, 400);
    fallback.usage_provenance = Some(UsageProvenance::ProviderSnapshotFallback);

    let response = build_conversation_stats_response(&conversation, &[baseline, fallback], &[]);

    assert_eq!(response.effective_usage_totals.input_tokens, 400);
    assert_eq!(response.effective_usage_totals.processed_tokens, None);
    assert_eq!(response.usage_coverage.fallback_estimated_sample_count, 1);
    assert_eq!(response.usage_coverage.uncounted_sample_count, 1);
    assert_eq!(response.by_harness[0].count, 2);
    assert_eq!(response.by_harness[0].usage.input_tokens, 400);
    assert_eq!(response.by_harness[0].usage.processed_tokens, None);
}

#[test]
fn test_legacy_codex_overflow_fails_closed() {
    let session_id = IdeationSessionId::new();
    let conversation = ChatConversation::new_ideation(session_id.clone());
    let now = Utc::now();
    let messages = [u64::MAX, 1]
        .into_iter()
        .enumerate()
        .map(|(index, input_tokens)| {
            let mut message = tagged_message(
                &conversation,
                &session_id,
                AgentHarnessKind::Codex,
                input_tokens,
            );
            message.output_tokens = None;
            message.usage_provenance = None;
            message.provider_session_id = Some("legacy-overflow".to_string());
            message.created_at = now + Duration::seconds(index as i64);
            message
        })
        .collect::<Vec<_>>();

    let response = build_conversation_stats_response(&conversation, &messages, &[]);

    assert_eq!(response.usage_coverage.effective_totals_source, "messages");
    assert_eq!(response.effective_usage_totals.input_tokens, 0);
    assert_eq!(response.effective_usage_totals.processed_tokens, None);
    assert_eq!(response.usage_coverage.uncounted_sample_count, 1);
}

#[test]
fn test_conversation_stats_collapses_codex_cumulative_message_usage() {
    let session_id = IdeationSessionId::new();
    let mut conversation = ChatConversation::new_ideation(session_id.clone());
    conversation.set_provider_session_ref(ProviderSessionRef {
        harness: AgentHarnessKind::Codex,
        provider_session_id: "thread-cumulative".to_string(),
    });

    let now = Utc::now();
    let messages: Vec<ChatMessage> = [1_000_000, 2_000_000, 3_000_000, 4_000_000, 5_000_000]
        .into_iter()
        .enumerate()
        .map(|(index, input_tokens)| {
            let mut message = ChatMessage::orchestrator_in_session(session_id.clone(), "done");
            message.conversation_id = Some(conversation.id);
            message.provider_harness = Some(AgentHarnessKind::Codex);
            message.provider_session_id = Some("thread-cumulative".to_string());
            message.input_tokens = Some(input_tokens);
            message.output_tokens = Some(input_tokens / 100);
            message.cache_read_tokens = Some(input_tokens - 1_000);
            message.created_at = now + Duration::seconds(index as i64);
            message
        })
        .collect();

    let response = build_conversation_stats_response(&conversation, &messages, &[]);

    assert_eq!(response.usage_coverage.effective_totals_source, "messages");
    assert_eq!(response.message_usage_totals.input_tokens, 5_000_000);
    assert_eq!(response.message_usage_totals.output_tokens, 50_000);
    assert_eq!(response.message_usage_totals.cache_read_tokens, 4_999_000);
    assert_eq!(response.effective_usage_totals.input_tokens, 5_000_000);
    assert_eq!(response.by_harness[0].usage.input_tokens, 5_000_000);
}

#[test]
fn test_conversation_stats_falls_back_to_run_usage_when_messages_lack_usage() {
    let session_id = IdeationSessionId::new();
    let mut conversation = ChatConversation::new_ideation(session_id.clone());
    conversation.set_provider_session_ref(ProviderSessionRef {
        harness: AgentHarnessKind::Claude,
        provider_session_id: "session-1".to_string(),
    });
    conversation.set_provider_origin(Some("z_ai".to_string()), Some("z_ai".to_string()));

    let mut message = ChatMessage::orchestrator_in_session(session_id, "hello");
    message.conversation_id = Some(conversation.id);
    message.provider_harness = Some(AgentHarnessKind::Claude);

    let mut run = AgentRun::new(conversation.id);
    run.harness = Some(AgentHarnessKind::Claude);
    run.upstream_provider = Some("z_ai".to_string());
    run.provider_profile = Some("z_ai".to_string());
    run.effective_model_id = Some("glm-4.7".to_string());
    run.effective_effort = Some("medium".to_string());
    run.input_tokens = Some(300);
    run.output_tokens = Some(120);
    run.cache_creation_tokens = Some(30);
    run.cache_read_tokens = Some(12);

    let response = build_conversation_stats_response(&conversation, &[message], &[run]);

    assert_eq!(response.usage_coverage.effective_totals_source, "runs");
    assert_eq!(response.message_usage_totals.input_tokens, 0);
    assert_eq!(response.run_usage_totals.input_tokens, 300);
    assert_eq!(response.effective_usage_totals.input_tokens, 300);
    assert_eq!(response.by_upstream_provider[0].key, "z_ai");
    assert_eq!(response.by_model[0].key, "glm-4.7");
    assert_eq!(
        response
            .attribution_coverage
            .provider_messages_with_attribution,
        1
    );
}

#[test]
fn test_scope_stats_include_context_breakdown_and_conversation_count() {
    let project_id = ProjectId::from_string("project-1".to_string());
    let task_id = TaskId::from_string("task-1".to_string());
    let session_id = IdeationSessionId::new();

    let project_conversation = ChatConversation::new_project(project_id.clone());
    let mut task_conversation = ChatConversation::new_task(task_id);
    task_conversation.context_type = ChatContextType::TaskExecution;

    let mut project_message = ChatMessage::user_in_project(project_id.clone(), "project");
    project_message.role = MessageRole::Orchestrator;
    project_message.conversation_id = Some(project_conversation.id);
    project_message.provider_harness = Some(AgentHarnessKind::Codex);
    project_message.effective_model_id = Some("gpt-5.4".to_string());
    project_message.effective_effort = Some("high".to_string());
    project_message.input_tokens = Some(100);
    project_message.output_tokens = Some(20);

    let mut task_message = ChatMessage::orchestrator_in_session(session_id, "task");
    task_message.conversation_id = Some(task_conversation.id);
    task_message.provider_harness = Some(AgentHarnessKind::Codex);
    task_message.effective_model_id = Some("gpt-5.4".to_string());
    task_message.effective_effort = Some("high".to_string());
    task_message.input_tokens = Some(30);
    task_message.output_tokens = Some(10);

    let response = build_scope_stats_response(
        "project",
        project_id.as_str(),
        &[project_conversation, task_conversation],
        &[project_message, task_message],
        &[],
    );

    assert_eq!(response.scope_type, "project");
    assert_eq!(response.conversation_count, 2);
    assert_eq!(response.usage_coverage.effective_totals_source, "messages");
    assert_eq!(response.effective_usage_totals.input_tokens, 130);
    assert_eq!(response.by_context_type.len(), 2);
    assert_eq!(response.by_context_type[0].key, "project");
    assert_eq!(response.by_context_type[1].key, "task_execution");
}

fn tagged_message(
    conversation: &ChatConversation,
    session_id: &IdeationSessionId,
    harness: AgentHarnessKind,
    input_tokens: u64,
) -> ChatMessage {
    let mut message = ChatMessage::orchestrator_in_session(session_id.clone(), "done");
    message.conversation_id = Some(conversation.id);
    message.provider_harness = Some(harness);
    message.provider_session_id = Some(format!("{harness}-session"));
    message.input_tokens = Some(input_tokens);
    message.output_tokens = Some(0);
    message.usage_provenance = Some(UsageProvenance::ProviderTurnDelta);
    message
}

fn tagged_run(
    conversation: &ChatConversation,
    harness: AgentHarnessKind,
    input_tokens: u64,
) -> AgentRun {
    let mut run = AgentRun::new(conversation.id);
    run.harness = Some(harness);
    run.provider_session_id = Some(format!("{harness}-session"));
    run.input_tokens = Some(input_tokens);
    run.output_tokens = Some(0);
    run.usage_provenance = Some(UsageProvenance::ProviderTurnDelta);
    run
}
