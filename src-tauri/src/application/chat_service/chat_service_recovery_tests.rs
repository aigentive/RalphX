use super::{
    attempt_session_recovery, build_ideation_recovery_metadata, provider_env_for_harness,
    session_recovery_provider_block_error, session_recovery_provider_decision,
    SessionRecoveryProviderBlock, SessionRecoveryProviderDecision,
};

use std::path::Path;
use std::sync::Arc;

use ralphx_events::NullEventSink;

use crate::application::runtime_factory::ChatRuntimeFactoryDeps;
use crate::application::AppState;
use crate::domain::agents::{AgentHarnessKind, AgentProviderSettings};
use crate::domain::entities::{
    AgentConversationWorkspaceMode, ChatContextType, ChatConversation, ChatMessage,
    IdeationSession, ProjectId, TaskId, VerificationStatus,
};
use crate::domain::repositories::{
    AgentProviderSettingsRepository, IdeationSessionRepository, TaskProposalRepository,
};
use crate::infrastructure::memory::{
    MemoryAgentProviderSettingsRepository, MemoryIdeationSessionRepository,
    MemoryTaskProposalRepository,
};

fn make_repos() -> (
    Arc<MemoryIdeationSessionRepository>,
    Arc<MemoryTaskProposalRepository>,
) {
    (
        Arc::new(MemoryIdeationSessionRepository::new()),
        Arc::new(MemoryTaskProposalRepository::new()),
    )
}

#[tokio::test]
async fn provider_env_for_harness_reads_recovery_app_state_provider_settings() {
    let empty = provider_env_for_harness(&None, AgentHarnessKind::Claude)
        .await
        .expect("missing provider repository");
    assert!(empty.is_empty());

    let temp_dir = tempfile::tempdir().expect("temp dir");
    let env_path = temp_dir.path().join("claude.env");
    std::fs::write(
        &env_path,
        "CUSTOM_PROVIDER_TOKEN=from-recovery\nCLAUDE_MODEL=spoofed\n",
    )
    .expect("write env file");
    let app_state = AppState::new_test();
    let mut settings = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Claude);
    settings.custom_env_file_enabled = true;
    settings.custom_env_file_path = Some(env_path.to_string_lossy().into_owned());
    app_state
        .agent_provider_settings_repo
        .upsert(&settings)
        .await
        .expect("save provider settings");
    let provider_repo = Some(Arc::clone(&app_state.agent_provider_settings_repo));

    let provider_env = provider_env_for_harness(&provider_repo, AgentHarnessKind::Claude)
        .await
        .expect("load provider env");

    assert_eq!(
        provider_env
            .get("CUSTOM_PROVIDER_TOKEN")
            .map(String::as_str),
        Some("from-recovery")
    );
    assert!(!provider_env.contains_key("CLAUDE_MODEL"));
}

#[tokio::test]
async fn provider_env_for_harness_uses_recovery_explicit_provider_repo_without_app_handle() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let env_path = temp_dir.path().join("claude.env");
    std::fs::write(
        &env_path,
        "CUSTOM_PROVIDER_TOKEN=from-recovery-explicit\nCLAUDE_MODEL=spoofed\n",
    )
    .expect("write env file");
    let app_state = AppState::new_test();
    let mut settings = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Claude);
    settings.custom_env_file_enabled = true;
    settings.custom_env_file_path = Some(env_path.to_string_lossy().into_owned());
    app_state
        .agent_provider_settings_repo
        .upsert(&settings)
        .await
        .expect("save provider settings");
    let provider_repo = Some(Arc::clone(&app_state.agent_provider_settings_repo));

    let provider_env = provider_env_for_harness(&provider_repo, AgentHarnessKind::Claude)
        .await
        .expect("load provider env");

    assert_eq!(
        provider_env
            .get("CUSTOM_PROVIDER_TOKEN")
            .map(String::as_str),
        Some("from-recovery-explicit")
    );
    assert!(!provider_env.contains_key("CLAUDE_MODEL"));
}

#[tokio::test]
async fn session_recovery_provider_decision_fails_slot_without_provider_repo() {
    let block = session_recovery_provider_decision(
        &None,
        AgentHarnessKind::Claude,
        ChatContextType::Review,
    )
    .await
    .expect_err("slot recovery must fail closed without provider settings");

    assert_eq!(block, SessionRecoveryProviderBlock::MissingProviderSettings);
}

#[tokio::test]
async fn session_recovery_provider_decision_allows_non_slot_without_provider_repo() {
    let decision = session_recovery_provider_decision(
        &None,
        AgentHarnessKind::Claude,
        ChatContextType::Project,
    )
    .await
    .expect("non-slot recovery can preserve no-provider compatibility");

    assert_eq!(
        decision,
        SessionRecoveryProviderDecision::AllowWithoutProviderSettings
    );
}

#[tokio::test]
async fn session_recovery_provider_decision_blocks_disabled_slot_provider_without_app_handle() {
    let repo = Arc::new(MemoryAgentProviderSettingsRepository::new());
    let mut codex = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Codex);
    codex.enabled = true;
    codex.is_default = true;
    repo.upsert(&codex).await.expect("seed codex provider");
    let provider_repo: Arc<dyn AgentProviderSettingsRepository> = repo;
    let provider_repo = Some(provider_repo);

    let block = session_recovery_provider_decision(
        &provider_repo,
        AgentHarnessKind::Claude,
        ChatContextType::Review,
    )
    .await
    .expect_err("disabled Claude must block review recovery before spawn");

    match block {
        SessionRecoveryProviderBlock::Disabled(message) => {
            assert!(message.contains("claude is not enabled"), "{message}");
        }
        other => panic!("expected disabled-provider block, got {other:?}"),
    }
}

#[tokio::test]
async fn session_recovery_provider_decision_applies_explicit_provider_env() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let env_path = temp_dir.path().join("claude.env");
    std::fs::write(
        &env_path,
        "CUSTOM_PROVIDER_TOKEN=from-recovery-decision\nCLAUDE_MODEL=spoofed\n",
    )
    .expect("write env file");
    let app_state = AppState::new_test();
    let mut settings = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Claude);
    settings.enabled = true;
    settings.is_default = true;
    settings.custom_env_file_enabled = true;
    settings.custom_env_file_path = Some(env_path.to_string_lossy().into_owned());
    app_state
        .agent_provider_settings_repo
        .upsert(&settings)
        .await
        .expect("save provider settings");
    let provider_repo = Some(Arc::clone(&app_state.agent_provider_settings_repo));

    let decision = session_recovery_provider_decision(
        &provider_repo,
        AgentHarnessKind::Claude,
        ChatContextType::Review,
    )
    .await
    .expect("enabled provider should allow recovery");

    let SessionRecoveryProviderDecision::ApplyEnv(provider_env) = decision else {
        panic!("expected provider env application");
    };
    assert_eq!(
        provider_env
            .get("CUSTOM_PROVIDER_TOKEN")
            .map(String::as_str),
        Some("from-recovery-decision")
    );
    assert!(!provider_env.contains_key("CLAUDE_MODEL"));
}

#[test]
fn session_recovery_provider_block_error_maps_missing_provider_to_slot_message() {
    let error = session_recovery_provider_block_error(
        SessionRecoveryProviderBlock::MissingProviderSettings,
        ChatContextType::Review,
    );
    let message = error.to_string();

    assert!(
        message.contains("Provider settings were unavailable for review runtime"),
        "{message}"
    );
    assert!(
        message.contains("disabled-provider policy"),
        "message must explain the fail-closed policy: {message}"
    );
}

#[test]
fn session_recovery_provider_block_error_preserves_provider_errors() {
    let disabled = session_recovery_provider_block_error(
        SessionRecoveryProviderBlock::Disabled("disabled provider".to_string()),
        ChatContextType::Review,
    )
    .to_string();
    let env = session_recovery_provider_block_error(
        SessionRecoveryProviderBlock::Env("env failure".to_string()),
        ChatContextType::Review,
    )
    .to_string();

    assert!(disabled.contains("disabled provider"), "{disabled}");
    assert!(env.contains("env failure"), "{env}");
}

#[tokio::test]
async fn attempt_session_recovery_uses_managed_review_provider_settings_until_stream_failure() {
    let state = AppState::new_test();
    let task_id = TaskId::new();
    let conversation = ChatConversation::new_review(task_id.clone());
    let conversation_id = conversation.id.clone();
    state
        .chat_conversation_repo
        .create(conversation.clone())
        .await
        .expect("seed review conversation");
    let mut historical_message = ChatMessage::user_about_task(task_id.clone(), "prior review turn");
    historical_message.conversation_id = Some(conversation_id.clone());
    state
        .chat_message_repo
        .create(historical_message)
        .await
        .expect("seed conversation history");
    let chat_message_repo = Arc::clone(&state.chat_message_repo);
    let chat_conversation_repo = Arc::clone(&state.chat_conversation_repo);
    let chat_attachment_repo = Arc::clone(&state.chat_attachment_repo);
    let artifact_repo = Arc::clone(&state.artifact_repo);
    let agent_run_repo = Arc::clone(&state.agent_run_repo);
    let provider_repo = Some(Arc::clone(&state.agent_provider_settings_repo));
    let runtime_factory_deps = ChatRuntimeFactoryDeps::from_app_state(&state);
    let events = Arc::clone(&state.events);
    let temp_dir = tempfile::tempdir().expect("temp dir");

    let error = attempt_session_recovery(
        &conversation_id,
        &conversation,
        AgentHarnessKind::Claude,
        ChatContextType::Review,
        task_id.as_str(),
        "new review message",
        Path::new("/bin/echo"),
        temp_dir.path(),
        temp_dir.path(),
        None,
        chat_message_repo,
        chat_conversation_repo,
        chat_attachment_repo,
        artifact_repo,
        None,
        None,
        agent_run_repo,
        "recovery-run-id",
        provider_repo,
        false,
        false,
        "old-session",
        Some(&runtime_factory_deps),
        events.as_ref(),
    )
    .await
    .expect_err("review recovery should reach the inert stream with managed provider settings");
    let message = error.to_string();

    assert!(
        !message.contains("Provider settings were unavailable"),
        "managed review recovery must use AppState provider settings: {message}"
    );
    assert!(
        message.contains("Recovery failed")
            || message.contains("Recovery stream processing failed"),
        "review recovery should fail only after policy and provider gates: {message}"
    );
}

#[tokio::test]
async fn attempt_session_recovery_allows_project_without_provider_settings_until_stream_failure() {
    let state = AppState::new_test();
    let project_id = ProjectId::new();
    let conversation = ChatConversation::new_project(project_id.clone());
    let conversation_id = conversation.id.clone();
    state
        .chat_conversation_repo
        .create(conversation.clone())
        .await
        .expect("seed project conversation");
    let mut historical_message =
        ChatMessage::user_in_project(project_id.clone(), "prior project turn");
    historical_message.conversation_id = Some(conversation_id.clone());
    state
        .chat_message_repo
        .create(historical_message)
        .await
        .expect("seed conversation history");
    let chat_message_repo = Arc::clone(&state.chat_message_repo);
    let chat_conversation_repo = Arc::clone(&state.chat_conversation_repo);
    let chat_attachment_repo = Arc::clone(&state.chat_attachment_repo);
    let artifact_repo = Arc::clone(&state.artifact_repo);
    let agent_run_repo = Arc::clone(&state.agent_run_repo);
    let runtime_factory_deps = ChatRuntimeFactoryDeps::from_app_state(&state);
    let events = Arc::clone(&state.events);
    let temp_dir = tempfile::tempdir().expect("temp dir");

    let error = attempt_session_recovery(
        &conversation_id,
        &conversation,
        AgentHarnessKind::Claude,
        ChatContextType::Project,
        project_id.as_str(),
        "new project message",
        Path::new("/bin/echo"),
        temp_dir.path(),
        temp_dir.path(),
        None,
        chat_message_repo,
        chat_conversation_repo,
        chat_attachment_repo,
        artifact_repo,
        None,
        None,
        agent_run_repo,
        "recovery-run-id",
        None,
        false,
        false,
        "old-session",
        Some(&runtime_factory_deps),
        events.as_ref(),
    )
    .await
    .expect_err("project recovery should reach the inert stream without provider settings");
    let message = error.to_string();

    assert!(
        !message.contains("Provider settings were unavailable"),
        "project recovery must preserve no-provider compatibility: {message}"
    );
    assert!(
        message.contains("Recovery failed")
            || message.contains("Recovery stream processing failed"),
        "project recovery should fail only after the provider gate on the inert stream: {message}"
    );
}

#[tokio::test]
async fn attempt_session_recovery_rejects_authoritative_mode_only_builder_identity_before_replay() {
    let state = AppState::new_test();
    let task_id = TaskId::new();
    let mut conversation = ChatConversation::new_review(task_id.clone());
    conversation.agent_mode = Some(AgentConversationWorkspaceMode::PersonaBuilder);
    let conversation_id = conversation.id;
    state
        .chat_conversation_repo
        .create(conversation.clone())
        .await
        .expect("seed invalid-context builder row");
    let temp_dir = tempfile::tempdir().expect("temp dir");

    let events = NullEventSink;
    let error = attempt_session_recovery(
        &conversation_id,
        &conversation,
        AgentHarnessKind::Claude,
        ChatContextType::Review,
        task_id.as_str(),
        "must not recover as a persona builder",
        Path::new("/bin/echo"),
        temp_dir.path(),
        temp_dir.path(),
        None,
        Arc::clone(&state.chat_message_repo),
        Arc::clone(&state.chat_conversation_repo),
        Arc::clone(&state.chat_attachment_repo),
        Arc::clone(&state.artifact_repo),
        None,
        None,
        Arc::clone(&state.agent_run_repo),
        "invalid-builder-recovery-run",
        None,
        true,
        false,
        "invalid-builder-session",
        None,
        &events,
    )
    .await
    .expect_err("unsupported context must not acquire PersonaBuilder recovery authority");

    assert!(
        error.to_string().contains("Project or Standalone"),
        "unexpected recovery rejection: {error}"
    );
}

#[tokio::test]
async fn test_recovery_metadata_includes_verification_fields_when_in_progress() {
    let (session_repo, proposal_repo) = make_repos();
    let project_id = ProjectId::new();

    let mut session = IdeationSession::new(project_id);
    session.verification_status = VerificationStatus::Reviewing;
    session.verification_in_progress = true;
    session.verification_current_round = Some(2);

    let session_id = session.id.clone();
    session_repo.create(session).await.unwrap();

    let session_repo_dyn: Arc<dyn IdeationSessionRepository> =
        session_repo.clone() as Arc<dyn IdeationSessionRepository>;
    let proposal_repo_dyn: Arc<dyn TaskProposalRepository> =
        proposal_repo as Arc<dyn TaskProposalRepository>;
    let events = NullEventSink;

    let metadata = build_ideation_recovery_metadata(
        session_id.as_str(),
        Some(&session_repo_dyn),
        Some(&proposal_repo_dyn),
        &events,
    )
    .await;

    assert!(
        metadata.is_some(),
        "metadata must be returned for valid session"
    );
    let m = metadata.unwrap();
    assert_eq!(m.verification_status, "reviewing");
    assert!(
        m.verification_in_progress,
        "must capture in_progress=true before reset"
    );
    assert_eq!(
        m.current_round, 2,
        "must extract current_round from summary fields"
    );

    // Recovery resets verification state when in_progress=true
    let after = session_repo.get_by_id(&session_id).await.unwrap().unwrap();
    assert_eq!(
        after.verification_status,
        VerificationStatus::Unverified,
        "verification_status must be reset after recovery"
    );
    assert!(
        !after.verification_in_progress,
        "verification_in_progress must be cleared after recovery"
    );
}

#[tokio::test]
async fn test_recovery_metadata_no_reset_when_not_in_progress() {
    let (session_repo, proposal_repo) = make_repos();
    let project_id = ProjectId::new();

    let mut session = IdeationSession::new(project_id);
    session.verification_status = VerificationStatus::Verified;
    session.verification_in_progress = false;

    let session_id = session.id.clone();
    session_repo.create(session).await.unwrap();

    let session_repo_dyn: Arc<dyn IdeationSessionRepository> =
        session_repo.clone() as Arc<dyn IdeationSessionRepository>;
    let proposal_repo_dyn: Arc<dyn TaskProposalRepository> =
        proposal_repo as Arc<dyn TaskProposalRepository>;
    let events = NullEventSink;

    let metadata = build_ideation_recovery_metadata(
        session_id.as_str(),
        Some(&session_repo_dyn),
        Some(&proposal_repo_dyn),
        &events,
    )
    .await;

    assert!(metadata.is_some());
    let m = metadata.unwrap();
    assert_eq!(m.verification_status, "verified");
    assert!(!m.verification_in_progress);
    assert_eq!(
        m.current_round, 0,
        "current_round is 0 when no summary is present"
    );

    // Status must NOT be reset since verification was not in-progress
    let after = session_repo.get_by_id(&session_id).await.unwrap().unwrap();
    assert_eq!(
        after.verification_status,
        VerificationStatus::Verified,
        "verification_status must be preserved when not in-progress"
    );
}

#[tokio::test]
async fn test_recovery_metadata_returns_none_for_missing_session() {
    let (session_repo, proposal_repo) = make_repos();

    let session_repo_dyn: Arc<dyn IdeationSessionRepository> =
        session_repo as Arc<dyn IdeationSessionRepository>;
    let proposal_repo_dyn: Arc<dyn TaskProposalRepository> =
        proposal_repo as Arc<dyn TaskProposalRepository>;
    let events = NullEventSink;

    let metadata = build_ideation_recovery_metadata(
        "nonexistent-session-id",
        Some(&session_repo_dyn),
        Some(&proposal_repo_dyn),
        &events,
    )
    .await;

    assert!(metadata.is_none(), "must return None for missing sessions");
}

#[tokio::test]
async fn test_recovery_metadata_returns_none_when_repos_absent() {
    let metadata = build_ideation_recovery_metadata("any-id", None, None, &NullEventSink).await;
    assert!(
        metadata.is_none(),
        "must return None when repos are not provided"
    );
}
