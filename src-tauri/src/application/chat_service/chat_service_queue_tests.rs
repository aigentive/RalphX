use super::*;
use crate::domain::agents::LogicalEffort;
use crate::domain::entities::ChatAttachmentId;
use crate::domain::services::{
    ComposerArtifactReference, ComposerIntegrationReference, ComposerProjectReference,
    ComposerProjectReferenceKind,
};
use crate::infrastructure::agents::claude::agent_names;

fn codex_continuation_runtime() -> super::super::continuation_runtime::ContinuationRuntime {
    super::super::continuation_runtime::ContinuationRuntime {
        harness: AgentHarnessKind::Codex,
        provider_session_id: "codex-session".to_string(),
        logical_model: Some("gpt-5.5".to_string()),
        effective_model_id: Some("gpt-5.5".to_string()),
        logical_effort: Some(LogicalEffort::XHigh),
        service_tier: Some("fast".to_string()),
        approval_policy: Some("never".to_string()),
        sandbox_mode: Some("danger-full-access".to_string()),
    }
}

#[test]
fn queued_agent_run_inherits_exact_continuation_runtime_attribution() {
    let message = crate::domain::services::QueuedMessage::new("follow up".to_string());

    let run = build_queued_agent_run(
        ChatConversationId::new(),
        AgentHarnessKind::Codex,
        "codex-session",
        None,
        None,
        None,
        &codex_continuation_runtime(),
        &message,
    );

    assert_eq!(run.logical_model.as_deref(), Some("gpt-5.5"));
    assert_eq!(run.effective_model_id.as_deref(), Some("gpt-5.5"));
    assert_eq!(run.logical_effort, Some(LogicalEffort::XHigh));
    assert_eq!(run.service_tier.as_deref(), Some("fast"));
    assert_eq!(run.approval_policy.as_deref(), Some("never"));
    assert_eq!(run.sandbox_mode.as_deref(), Some("danger-full-access"));
}

#[test]
fn queued_agent_run_records_explicit_runtime_overrides() {
    let mut message = crate::domain::services::QueuedMessage::new("follow up".to_string());
    message.model_override = Some("gpt-5.6".to_string());
    message.logical_effort_override = Some(LogicalEffort::High);
    message.service_tier_override = Some("standard".to_string());

    let run = build_queued_agent_run(
        ChatConversationId::new(),
        AgentHarnessKind::Codex,
        "codex-session",
        None,
        None,
        None,
        &codex_continuation_runtime(),
        &message,
    );

    assert_eq!(run.logical_model.as_deref(), Some("gpt-5.6"));
    assert_eq!(run.effective_model_id.as_deref(), Some("gpt-5.6"));
    assert_eq!(run.logical_effort, Some(LogicalEffort::High));
    assert_eq!(run.service_tier.as_deref(), Some("standard"));
}

#[test]
fn queued_persisted_metadata_embeds_composer_references() {
    let mut message = crate::domain::services::QueuedMessage::new("follow up".to_string());
    message.metadata_override = Some(r#"{"source":"queue"}"#.to_string());
    message.composer_project_references = vec![ComposerProjectReference {
        path: "src/main.rs".to_string(),
        kind: Some(ComposerProjectReferenceKind::File),
    }];

    let metadata = queued_persisted_metadata(&message).expect("metadata");
    let value: serde_json::Value = serde_json::from_str(&metadata).expect("json");

    assert_eq!(value["source"], "queue");
    assert_eq!(
        value["composer_project_references"][0]["path"],
        "src/main.rs"
    );
    assert_eq!(value["composer_project_references"][0]["kind"], "file");
}

#[test]
fn queued_persisted_metadata_preserves_raw_metadata_when_references_exist() {
    let mut message = crate::domain::services::QueuedMessage::new("follow up".to_string());
    message.metadata_override = Some("not-json".to_string());
    message.composer_project_references = vec![ComposerProjectReference {
        path: "README.md".to_string(),
        kind: None,
    }];

    let metadata = queued_persisted_metadata(&message).expect("metadata");
    let value: serde_json::Value = serde_json::from_str(&metadata).expect("json");

    assert_eq!(value["raw_metadata"], "not-json");
    assert_eq!(value["composer_project_references"][0]["path"], "README.md");
}

#[test]
fn queued_message_requires_fresh_provider_session_on_harness_mismatch() {
    let mut message = crate::domain::services::QueuedMessage::new("switch".to_string());
    message.harness_override = Some(AgentHarnessKind::Codex);

    assert!(queued_message_requires_fresh_provider_session(
        &message,
        AgentHarnessKind::Claude
    ));
    assert!(!queued_message_requires_fresh_provider_session(
        &message,
        AgentHarnessKind::Codex
    ));
}

#[test]
fn queued_message_requires_fresh_provider_session_on_explicit_flag() {
    let mut message = crate::domain::services::QueuedMessage::new("switch".to_string());
    message.force_new_provider_session = true;

    assert!(queued_message_requires_fresh_provider_session(
        &message,
        AgentHarnessKind::Claude
    ));
}

#[test]
fn queued_created_at_override_parses_valid_timestamps_only() {
    let mut message = crate::domain::services::QueuedMessage::new("timed".to_string());
    message.created_at_override = Some("2026-06-12T12:00:00+02:00".to_string());

    let parsed = queued_created_at_override(&message).expect("valid timestamp should be parsed");
    assert_eq!(parsed.to_rfc3339(), "2026-06-12T10:00:00+00:00");

    message.created_at_override = Some("not-a-timestamp".to_string());
    assert!(queued_created_at_override(&message).is_none());
}

#[test]
fn queued_persisted_created_at_falls_back_to_queue_entry_time() {
    let mut message = crate::domain::services::QueuedMessage::new("timed".to_string());
    message.created_at = "2026-06-12T12:00:00Z".to_string();

    let parsed = queued_persisted_created_at(&message).expect("queue timestamp should be parsed");

    assert_eq!(parsed.to_rfc3339(), "2026-06-12T12:00:00+00:00");
}

#[test]
fn provider_switch_send_options_for_queued_message_preserve_payload() {
    let conversation_id = ChatConversationId::new();
    let attachment_id = ChatAttachmentId::new();
    let mut message = crate::domain::services::QueuedMessage::new("switch".to_string());
    message.metadata_override = Some(r#"{"source":"queue"}"#.to_string());
    message.created_at_override = Some("2026-06-12T12:00:00Z".to_string());
    message.harness_override = Some(AgentHarnessKind::Codex);
    message.agent_name_override = Some("ralphx-queued-agent".to_string());
    message.persona_directive = crate::domain::entities::PersonaDirective::Suppress;
    message.model_override = Some("gpt-5.5".to_string());
    message.logical_effort_override = Some(LogicalEffort::High);
    message.composer_project_references = vec![ComposerProjectReference {
        path: "src/main.rs".to_string(),
        kind: Some(ComposerProjectReferenceKind::File),
    }];
    message.composer_integration_references = vec![ComposerIntegrationReference {
        provider: "atlassian".to_string(),
        kind: "jira".to_string(),
        id: "RX-42".to_string(),
        key: Some("RX-42".to_string()),
        title: Some("Fix queue replay".to_string()),
        url: None,
        summary_excerpt: None,
        include_transcript: None,
    }];
    message.composer_artifact_references = vec![ComposerArtifactReference {
        artifact_id: "artifact-1".to_string(),
        kind: "plan".to_string(),
        title: Some("Implementation Plan".to_string()),
        session_id: Some("session-1".to_string()),
        version: Some(1),
        status: Some("approved".to_string()),
    }];
    message.attachment_ids = vec![attachment_id];

    let options = provider_switch_send_options_for_queued_message(
        &message,
        conversation_id.clone(),
        true,
        Some(TeamIntent::rx_native(None)),
    );

    assert_eq!(options.metadata.as_deref(), Some(r#"{"source":"queue"}"#));
    assert_eq!(
        options
            .created_at
            .map(|timestamp| timestamp.to_rfc3339())
            .as_deref(),
        Some("2026-06-12T12:00:00+00:00")
    );
    assert_eq!(options.harness_override, Some(AgentHarnessKind::Codex));
    assert_eq!(
        options.agent_name_override.as_deref(),
        Some("ralphx-queued-agent")
    );
    assert_eq!(
        options.persona_directive,
        crate::domain::entities::PersonaDirective::Suppress
    );
    assert_eq!(options.model_override.as_deref(), Some("gpt-5.5"));
    assert_eq!(options.conversation_id_override, Some(conversation_id));
    assert_eq!(options.logical_effort_override, Some(LogicalEffort::High));
    assert_eq!(
        options.composer_project_references,
        message.composer_project_references
    );
    assert_eq!(
        options.composer_integration_references,
        message.composer_integration_references
    );
    assert_eq!(
        options.composer_artifact_references,
        message.composer_artifact_references
    );
    assert_eq!(options.attachment_ids, message.attachment_ids);
    assert_eq!(options.team_intent, Some(TeamIntent::rx_native(None)));
    assert!(options.force_new_provider_session);
}

#[test]
fn provider_switch_send_options_can_reuse_fresh_provider_run() {
    let conversation_id = ChatConversationId::new();
    let mut message = QueuedMessage::new("second queued provider message".to_string());
    message.harness_override = Some(AgentHarnessKind::Codex);
    message.force_new_provider_session = true;

    let options = provider_switch_send_options_for_queued_message(
        &message,
        conversation_id,
        false,
        Some(TeamIntent::rx_native(None)),
    );

    assert_eq!(options.harness_override, Some(AgentHarnessKind::Codex));
    assert_eq!(options.team_intent, Some(TeamIntent::rx_native(None)));
    assert!(
        !options.force_new_provider_session,
        "same-harness queued follow-ups should reuse the freshly started provider run"
    );
}

#[test]
fn effective_queue_team_mode_uses_persisted_coordination_mode() {
    assert!(effective_queue_team_mode(
        false,
        Some(CoordinationMode::RxNativeTeam)
    ));
    assert!(effective_queue_team_mode(
        false,
        Some(CoordinationMode::LegacyClaudeTeam)
    ));
    assert!(!effective_queue_team_mode(
        false,
        Some(CoordinationMode::Solo)
    ));
    assert!(effective_queue_team_mode(
        true,
        Some(CoordinationMode::Solo)
    ));
}

#[test]
fn fresh_provider_run_reuse_requires_matching_queued_harness() {
    let mut same_harness = QueuedMessage::new("same harness".to_string());
    same_harness.harness_override = Some(AgentHarnessKind::Codex);
    same_harness.force_new_provider_session = true;

    let mut no_harness = QueuedMessage::new("explicit fresh session".to_string());
    no_harness.force_new_provider_session = true;

    let mut different_harness = QueuedMessage::new("different harness".to_string());
    different_harness.harness_override = Some(AgentHarnessKind::Claude);
    different_harness.force_new_provider_session = true;

    assert!(can_reuse_fresh_provider_run(
        &same_harness,
        Some(AgentHarnessKind::Codex)
    ));
    assert!(!can_reuse_fresh_provider_run(
        &no_harness,
        Some(AgentHarnessKind::Codex)
    ));
    assert!(!can_reuse_fresh_provider_run(
        &different_harness,
        Some(AgentHarnessKind::Codex)
    ));
}

#[tokio::test]
async fn provider_switch_queue_without_app_handle_requeues_instead_of_resuming() {
    let app_state = AppState::new_test();
    let message_queue = Arc::clone(&app_state.message_queue);
    let running_agent_registry = Arc::clone(&app_state.running_agent_registry);
    let agent_run_repo = Arc::clone(&app_state.agent_run_repo);
    let chat_message_repo = Arc::clone(&app_state.chat_message_repo);
    let chat_attachment_repo = Arc::clone(&app_state.chat_attachment_repo);
    let artifact_repo = Arc::clone(&app_state.artifact_repo);
    let activity_event_repo = Arc::clone(&app_state.activity_event_repo);
    let task_repo = Arc::clone(&app_state.task_repo);
    let ideation_session_repo = Arc::clone(&app_state.ideation_session_repo);

    message_queue.queue_with_runtime_overrides_and_project_references(
        ChatContextType::Ideation,
        "session-queued-switch",
        "queued provider switch".to_string(),
        None,
        None,
        Some(AgentHarnessKind::Codex),
        None,
        crate::domain::entities::PersonaDirective::Inherit,
        Some("gpt-5.5".to_string()),
        Some(crate::domain::agents::LogicalEffort::High),
        Some("fast".to_string()),
        true,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        None,
        Vec::new(),
    );

    let outcome = process_queued_messages::<tauri::test::MockRuntime>(
        ChatContextType::Ideation,
        AgentHarnessKind::Claude,
        "session-queued-switch",
        "session-queued-switch",
        ChatConversationId::new(),
        "claude-session-old",
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
        std::path::Path::new("/definitely/missing/ralphx-test-cli"),
        std::path::Path::new("."),
        std::path::Path::new("."),
        None,
        None,
        None,
        None,
        None,
        false,
        tokio_util::sync::CancellationToken::new(),
        None,
        None,
        crate::application::chat_service::StreamingStateCache::new(),
    )
    .await;

    assert_eq!(outcome.total_processed, 0);
    let queued = message_queue.get_queued(ChatContextType::Ideation, "session-queued-switch");
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].harness_override, Some(AgentHarnessKind::Codex));
    assert_eq!(queued[0].model_override.as_deref(), Some("gpt-5.5"));
    assert_eq!(
        queued[0].logical_effort_override,
        Some(crate::domain::agents::LogicalEffort::High)
    );
    assert_eq!(queued[0].service_tier_override.as_deref(), Some("fast"));
    assert!(queued[0].force_new_provider_session);
}

#[tokio::test]
async fn missing_continuation_authority_records_failed_action_run() {
    let app_state = AppState::new_test();
    let message_queue = Arc::clone(&app_state.message_queue);
    let running_agent_registry = Arc::clone(&app_state.running_agent_registry);
    let agent_run_repo = Arc::clone(&app_state.agent_run_repo);
    let chat_message_repo = Arc::clone(&app_state.chat_message_repo);
    let chat_attachment_repo = Arc::clone(&app_state.chat_attachment_repo);
    let artifact_repo = Arc::clone(&app_state.artifact_repo);
    let activity_event_repo = Arc::clone(&app_state.activity_event_repo);
    let task_repo = Arc::clone(&app_state.task_repo);
    let ideation_session_repo = Arc::clone(&app_state.ideation_session_repo);
    let conversation_id = ChatConversationId::new();

    message_queue.queue_with_runtime_overrides_and_project_references(
        ChatContextType::Ideation,
        "plan-session",
        "verify plan".to_string(),
        Some(
            r#"{"ralphx_action_kind":"verify_plan","ralphx_action_context_id":"plan-session","ralphx_action_target_id":"plan-artifact"}"#
                .to_string(),
        ),
        None,
        None,
        None,
        crate::domain::entities::PersonaDirective::Inherit,
        None,
        None,
        None,
        false,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        None,
        Vec::new(),
    );

    let outcome = process_queued_messages::<tauri::test::MockRuntime>(
        ChatContextType::Ideation,
        AgentHarnessKind::Codex,
        "plan-session",
        "plan-session",
        conversation_id.clone(),
        "missing-codex-session",
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
        std::path::Path::new("/definitely/missing/ralphx-test-cli"),
        std::path::Path::new("."),
        std::path::Path::new("."),
        None,
        None,
        None,
        None,
        None,
        false,
        tokio_util::sync::CancellationToken::new(),
        None,
        None,
        crate::application::chat_service::StreamingStateCache::new(),
    )
    .await;

    assert_eq!(outcome.total_processed, 1);
    assert!(
        message_queue
            .get_queued(ChatContextType::Ideation, "plan-session")
            .is_empty(),
        "permanent authority failures must not be retried forever"
    );
    let runs = agent_run_repo
        .get_by_conversation(&conversation_id)
        .await
        .expect("load failed action run");
    assert_eq!(runs.len(), 1);
    assert_eq!(
        runs[0].status,
        crate::domain::entities::AgentRunStatus::Failed
    );
    assert_eq!(
        runs[0].action_kind,
        Some(crate::domain::entities::AgentRunActionKind::VerifyPlan)
    );
    assert_eq!(runs[0].action_context_id.as_deref(), Some("plan-session"));
    assert_eq!(runs[0].action_target_id.as_deref(), Some("plan-artifact"));
    assert_eq!(runs[0].harness, Some(AgentHarnessKind::Codex));
    assert_eq!(
        runs[0].provider_session_id.as_deref(),
        Some("missing-codex-session")
    );
}

#[test]
fn hidden_resume_marker_metadata_strips_transient_flags() {
    let metadata = hidden_resume_in_place_marker_metadata(Some(
        r#"{"resume_in_place":true,"persist_hidden_marker":true,"reason":"verify"}"#,
    ))
    .expect("marker metadata");
    let value: serde_json::Value = serde_json::from_str(&metadata).expect("json");

    assert_eq!(value.get("resume_in_place"), None);
    assert_eq!(value.get("persist_hidden_marker"), None);
    assert_eq!(value["hidden_from_ui"], true);
    assert_eq!(value["recovery_context"], true);
    assert_eq!(value["reason"], "verify");
    assert!(hidden_resume_in_place_marker_metadata(Some(r#"{"resume_in_place":true}"#)).is_none());
}

#[test]
fn queued_agent_identity_for_plan_uses_ideation_agent_plan_profile() {
    let identity = queued_agent_identity_for_mode(Some(AgentConversationWorkspaceMode::Plan));

    assert_eq!(
        identity.agent_name,
        Some(agent_names::AGENT_ORCHESTRATOR_IDEATION.to_string())
    );
    assert_eq!(identity.agent_profile, Some("plan"));
}
