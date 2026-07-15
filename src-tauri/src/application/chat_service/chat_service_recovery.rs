// Session recovery logic for stale provider sessions.
//
// Extracted from chat_service_send_background.rs to reduce file size.
// Handles rebuilding conversation history and spawning fresh sessions.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use super::chat_service_context;
use super::chat_service_replay::{
    build_rehydration_prompt, IdeationRecoveryMetadata, ReplayBuilder,
};
use super::chat_service_streaming::process_stream_background;
use super::streaming_state_cache::StreamingStateCache;
use crate::application::persona_resolver::resolve_persona_for_send;
use crate::application::verification_event_emitters::{
    emit_verification_status_changed, event_sink_from_app_handle,
};
use crate::application::AppState;
use crate::domain::agents::{AgentHarnessKind, ProviderSessionRef};
use crate::domain::entities::VerificationStatus;
use crate::domain::entities::{
    ChatContextType, ChatConversation, ChatConversationId, PersonaDirective,
};
use crate::domain::repositories::{
    AgentProviderSettingsRepository, AgentRunRepository, ArtifactRepository,
    ChatAttachmentRepository, ChatConversationRepository, ChatMessageRepository,
    IdeationSessionRepository, TaskProposalRepository,
};
use crate::domain::services::{
    clear_verification_snapshot, load_current_verification_snapshot_or_default,
};
use crate::error::{AppError, AppResult};
use tauri::{Manager, Runtime};

async fn provider_env_for_harness<R: Runtime>(
    app_handle: Option<&tauri::AppHandle<R>>,
    agent_provider_settings_repo: &Option<Arc<dyn AgentProviderSettingsRepository>>,
    harness: AgentHarnessKind,
) -> Result<HashMap<String, String>, AppError> {
    let app_state_provider_repo = app_handle
        .and_then(|handle| handle.try_state::<AppState>())
        .map(|app_state| Arc::clone(&app_state.agent_provider_settings_repo));
    let provider_repo = agent_provider_settings_repo
        .as_ref()
        .map(Arc::clone)
        .or(app_state_provider_repo);
    crate::application::provider_env_file::load_provider_custom_env_file_for_harness(
        provider_repo.as_ref(),
        harness,
    )
    .await
    .map_err(AppError::Infrastructure)
}

#[derive(Debug, PartialEq, Eq)]
enum SessionRecoveryProviderDecision {
    ApplyEnv(HashMap<String, String>),
    AllowWithoutProviderSettings,
}

#[derive(Debug, PartialEq, Eq)]
enum SessionRecoveryProviderBlock {
    Disabled(String),
    Env(String),
    MissingProviderSettings,
}

fn session_recovery_missing_provider_settings_message(context_type: ChatContextType) -> String {
    format!(
        "Provider settings were unavailable for {} runtime; spawn blocked to avoid bypassing disabled-provider policy.",
        context_type
    )
}

fn session_recovery_provider_block_error(
    block: SessionRecoveryProviderBlock,
    context_type: ChatContextType,
) -> AppError {
    match block {
        SessionRecoveryProviderBlock::Disabled(error)
        | SessionRecoveryProviderBlock::Env(error) => AppError::Infrastructure(error),
        SessionRecoveryProviderBlock::MissingProviderSettings => AppError::Infrastructure(
            session_recovery_missing_provider_settings_message(context_type),
        ),
    }
}

async fn session_recovery_provider_decision<R: Runtime>(
    app_handle: Option<&tauri::AppHandle<R>>,
    agent_provider_settings_repo: &Option<Arc<dyn AgentProviderSettingsRepository>>,
    recovery_harness: AgentHarnessKind,
    context_type: ChatContextType,
) -> Result<SessionRecoveryProviderDecision, SessionRecoveryProviderBlock> {
    let app_state_provider_repo = app_handle
        .and_then(|handle| handle.try_state::<AppState>())
        .map(|app_state| Arc::clone(&app_state.agent_provider_settings_repo));
    let provider_repo = agent_provider_settings_repo
        .as_ref()
        .map(Arc::clone)
        .or(app_state_provider_repo);
    let Some(provider_repo) = provider_repo else {
        return if super::uses_execution_slot(context_type) {
            Err(SessionRecoveryProviderBlock::MissingProviderSettings)
        } else {
            Ok(SessionRecoveryProviderDecision::AllowWithoutProviderSettings)
        };
    };

    crate::application::ensure_provider_spawn_enabled(
        &provider_repo,
        recovery_harness,
        "session_recovery",
    )
    .await
    .map_err(SessionRecoveryProviderBlock::Disabled)?;

    let provider_env = provider_env_for_harness(
        app_handle,
        &Some(Arc::clone(&provider_repo)),
        recovery_harness,
    )
    .await
    .map_err(|error| SessionRecoveryProviderBlock::Env(error.to_string()))?;

    Ok(SessionRecoveryProviderDecision::ApplyEnv(provider_env))
}

/// Attempt to recover from a stale provider session by rebuilding conversation history
/// and spawning a fresh session.
///
/// # Arguments
/// - `conversation_id`: The conversation ID
/// - `conversation`: The conversation entity with stale session
/// - `context_type`: The chat context type
/// - `context_id`: The context ID
/// - `new_message`: The user message that triggered the recovery
/// - `cli_path`: Path to Claude CLI
/// - `plugin_dir`: Path to plugin directory
/// - `working_directory`: Working directory for spawned commands
/// - `resolved_project_id`: Optional project ID for RALPHX_PROJECT_ID
/// - `chat_message_repo`: Message repository
/// - `conversation_repo`: Conversation repository
/// - `ideation_session_repo`: Optional ideation session repository for Ideation context
/// - `task_proposal_repo`: Optional proposal repository for Ideation context
///
/// # Returns
/// - `Ok(new_session_id)`: Recovery succeeded, new session ID
/// - `Err(AppError)`: Recovery failed
#[allow(clippy::too_many_arguments)]
#[doc(hidden)]
pub async fn attempt_session_recovery<R: Runtime>(
    conversation_id: &ChatConversationId,
    conversation: &ChatConversation,
    harness: AgentHarnessKind,
    context_type: ChatContextType,
    context_id: &str,
    new_message: &str,
    cli_path: &Path,
    plugin_dir: &Path,
    working_directory: &Path,
    _resolved_project_id: Option<String>,
    team_mode: bool,
    chat_message_repo: Arc<dyn ChatMessageRepository>,
    conversation_repo: Arc<dyn ChatConversationRepository>,
    chat_attachment_repo: Arc<dyn ChatAttachmentRepository>,
    artifact_repo: Arc<dyn ArtifactRepository>,
    ideation_session_repo: Option<Arc<dyn IdeationSessionRepository>>,
    task_proposal_repo: Option<Arc<dyn TaskProposalRepository>>,
    agent_run_repo: Arc<dyn AgentRunRepository>,
    agent_run_id: &str,
    agent_provider_settings_repo: Option<Arc<dyn AgentProviderSettingsRepository>>,
    persona_feature_enabled: bool,
    agent_name_override_set: bool,
    old_session_id: &str,
    app_handle: Option<&tauri::AppHandle<R>>,
) -> AppResult<String> {
    let recovery_start = std::time::Instant::now();

    // Helper closure to log failure with duration
    let log_failure = |error: &AppError| {
        tracing::error!(
            event = "rehydrate_failure",
            conversation_id = conversation_id.as_str(),
            error = %error,
            duration_ms = recovery_start.elapsed().as_millis(),
            "Session recovery failed"
        );
    };

    // 1. Build replay from history
    let replay_builder = ReplayBuilder::new(100_000); // 100K token budget
    let replay = match replay_builder
        .build_replay(&chat_message_repo, conversation_id)
        .await
    {
        Ok(r) => r,
        Err(e) => {
            log_failure(&e);
            return Err(e);
        }
    };

    tracing::debug!(
        conversation_id = conversation_id.as_str(),
        turns = replay.turns.len(),
        estimated_tokens = replay.total_tokens,
        truncated = replay.is_truncated,
        "Built conversation replay for rehydration"
    );

    // 2. Build ideation recovery metadata if context is Ideation
    let ideation_metadata = if context_type == ChatContextType::Ideation {
        build_ideation_recovery_metadata(
            context_id,
            ideation_session_repo.as_ref(),
            task_proposal_repo.as_ref(),
            app_handle,
        )
        .await
    } else {
        None
    };

    // 3. Generate rehydration prompt
    let bootstrap_prompt = build_rehydration_prompt(
        &replay,
        context_type,
        context_id,
        new_message,
        ideation_metadata.as_ref(),
    );

    let ideation_model_settings_repo = app_handle.map(|handle| {
        let app_state = handle.state::<AppState>();
        Arc::clone(&app_state.ideation_model_settings_repo)
    });
    let agent_lane_settings_repo = app_handle.map(|handle| {
        let app_state = handle.state::<AppState>();
        Arc::clone(&app_state.agent_lane_settings_repo)
    });
    let ideation_effort_settings_repo = app_handle.map(|handle| {
        let app_state = handle.state::<AppState>();
        Arc::clone(&app_state.ideation_effort_settings_repo)
    });
    let task_repo = app_handle.map(|handle| {
        let app_state = handle.state::<AppState>();
        Arc::clone(&app_state.task_repo)
    });
    let delegated_session_repo = app_handle.map(|handle| {
        let app_state = handle.state::<AppState>();
        Arc::clone(&app_state.delegated_session_repo)
    });
    let entity_status = if let (Some(ideation_repo), Some(delegated_repo), Some(task_repo)) = (
        ideation_session_repo.as_ref(),
        delegated_session_repo.as_ref(),
        task_repo.as_ref(),
    ) {
        chat_service_context::get_entity_status_for_resume(
            conversation.context_type,
            conversation.context_id.as_str(),
            Arc::clone(ideation_repo),
            Arc::clone(delegated_repo),
            Arc::clone(task_repo),
        )
        .await
    } else {
        None
    };
    let resolved_persona = if persona_feature_enabled {
        if let Some(app_state) = app_handle.and_then(|handle| handle.try_state::<AppState>()) {
            let workspace_mode = app_state
                .agent_conversation_workspace_repo
                .get_by_conversation_id(&conversation.id)
                .await
                .map_err(|error| {
                    AppError::PersonaUnavailable(format!("[Persona unavailable: {error}]"))
                })?
                .map(|workspace| workspace.mode);
            resolve_persona_for_send(
                conversation,
                &PersonaDirective::Inherit,
                super::persona_resolve_flags_for_conversation(
                    persona_feature_enabled,
                    false,
                    agent_name_override_set,
                    context_type,
                    conversation,
                    workspace_mode,
                ),
                Arc::clone(&app_state.persona_repo),
            )
            .await
            .map_err(|error| {
                AppError::PersonaUnavailable(format!("[Persona unavailable: {error}]"))
            })?
        } else {
            None
        }
    } else {
        None
    };

    let persona_for_attribution = resolved_persona.clone();

    // 4. Spawn fresh provider session with history
    let provider_spawnable = match chat_service_context::build_command_for_harness(
        harness,
        cli_path,
        plugin_dir,
        conversation,
        &bootstrap_prompt,
        resolved_persona,
        working_directory,
        entity_status.as_deref(),
        _resolved_project_id.as_deref(),
        &[],
        team_mode,
        chat_attachment_repo,
        artifact_repo,
        agent_lane_settings_repo,
        ideation_effort_settings_repo,
        ideation_model_settings_repo,
        &[],
        0,
        None,
        None,
        false,
        None,
    )
    .await
    {
        Ok(spawnable) => spawnable,
        Err(error) => {
            let err =
                AppError::Infrastructure(format!("Failed to build recovery command: {error}"));
            log_failure(&err);
            return Err(err);
        }
    };
    let persona_injected = provider_spawnable.spawnable.persona_injected();
    let persona_injection_skipped_reason = provider_spawnable
        .spawnable
        .persona_injection_skipped_reason();
    let provider_env = match session_recovery_provider_decision(
        app_handle,
        &agent_provider_settings_repo,
        harness,
        context_type,
    )
    .await
    {
        Ok(SessionRecoveryProviderDecision::ApplyEnv(provider_env)) => provider_env,
        Ok(SessionRecoveryProviderDecision::AllowWithoutProviderSettings) => HashMap::new(),
        Err(block) => {
            let error = session_recovery_provider_block_error(block, context_type);
            log_failure(&error);
            return Err(error);
        }
    };
    let mut provider_spawnable = provider_spawnable;
    provider_spawnable.apply_provider_env(&provider_env);
    let spawnable = provider_spawnable.spawnable;

    let child = match spawnable.spawn().await {
        Ok(c) => c,
        Err(e) => {
            let err = AppError::Infrastructure(format!("Failed to spawn recovery session: {}", e));
            log_failure(&err);
            return Err(err);
        }
    };
    super::record_persona_run_attribution(
        &agent_run_repo,
        app_handle,
        conversation_id,
        agent_run_id,
        harness,
        persona_for_attribution.as_ref(),
        persona_injected,
        persona_injection_skipped_reason,
    )
    .await;

    // 5. Process stream to capture new session ID
    let outcome = match process_stream_background::<tauri::Wry>(
        child,
        harness,
        context_type,
        context_id,
        conversation_id,
        None,                                       // no app_handle, silent recovery
        None,                                       // no activity persistence
        None,                                       // no task repo
        None,                                       // no incremental message update
        None,                                       // no timeline persistence
        None,                                       // no assistant message ID
        None,                                       // no question state
        tokio_util::sync::CancellationToken::new(), // standalone token for recovery
        None,                                       // no team tracker for recovery
        false,                                      // not team mode
        StreamingStateCache::new(),                 // fresh cache for recovery (no UI to hydrate)
        None,                                       // no heartbeat for recovery sessions
        None,                                       // no agent_run_repo for recovery
        None,                                       // no agent_run_id for recovery
        None,                                       // no execution state for recovery
        None,                                       // no conversation_repo for recovery
        false,                                      // no verification transcript splitting
        true, // recovery may persist any replacement session externally
    )
    .await
    {
        Ok(o) => o,
        Err(e) => {
            let err = AppError::Infrastructure(format!("Recovery stream processing failed: {}", e));
            log_failure(&err);
            return Err(err);
        }
    };

    let new_session_id = match outcome.session_id {
        Some(id) => id,
        None => {
            let err = AppError::Infrastructure("Recovery failed: no session ID captured".into());
            log_failure(&err);
            return Err(err);
        }
    };

    // 6. Update conversation with new provider session ID
    if let Err(e) = conversation_repo
        .update_provider_session_ref(
            conversation_id,
            &ProviderSessionRef {
                harness,
                provider_session_id: new_session_id.clone(),
            },
        )
        .await
    {
        let err = AppError::Database(format!("Failed to update session ID: {}", e));
        log_failure(&err);
        return Err(err);
    }

    // 7. Log telemetry
    tracing::info!(
        event = "rehydrate_success",
        conversation_id = conversation_id.as_str(),
        harness = %harness,
        old_session_id = old_session_id,
        new_session_id = %new_session_id,
        replay_turns = replay.turns.len(),
        estimated_tokens = replay.total_tokens,
        duration_ms = recovery_start.elapsed().as_millis(),
    );

    Ok(new_session_id)
}

/// Build ideation recovery metadata from repositories.
///
/// Fetches the ideation session and counts proposals to populate metadata
/// for enriching the recovery prompt with ideation-specific context.
async fn build_ideation_recovery_metadata<R: Runtime>(
    context_id: &str,
    ideation_session_repo: Option<&Arc<dyn IdeationSessionRepository>>,
    task_proposal_repo: Option<&Arc<dyn TaskProposalRepository>>,
    app_handle: Option<&tauri::AppHandle<R>>,
) -> Option<IdeationRecoveryMetadata> {
    // Both repositories are required for ideation metadata
    let (session_repo, proposal_repo) = (ideation_session_repo?, task_proposal_repo?);

    // Parse context_id as IdeationSessionId
    let session_id =
        crate::domain::entities::IdeationSessionId::from_string(context_id.to_string());

    // Fetch the session
    let session = match session_repo.get_by_id(&session_id).await {
        Ok(Some(s)) => s,
        Ok(None) => {
            tracing::warn!(
                session_id = session_id.as_str(),
                "Ideation session not found for recovery metadata"
            );
            return None;
        }
        Err(e) => {
            tracing::error!(
                session_id = session_id.as_str(),
                error = %e,
                "Failed to fetch ideation session for recovery metadata"
            );
            return None;
        }
    };

    // Count proposals for this session
    let proposal_count = match proposal_repo.count_by_session(&session_id).await {
        Ok(count) => count,
        Err(e) => {
            tracing::warn!(
                session_id = session_id.as_str(),
                error = %e,
                "Failed to count proposals for recovery metadata, using 0"
            );
            0
        }
    };

    // Extract verification state before (potentially) resetting it
    let verification_was_in_progress = session.verification_in_progress;
    let verification_status_str = session.verification_status.to_string();
    let current_round = session.verification_current_round.unwrap_or(0);

    // If verification was in-progress when the session crashed, force-reset it.
    // A stuck `verification_in_progress=1` would block reconciliation and confuse the recovered agent.
    // Reset the authoritative snapshot so the session summary and current-generation state stay aligned.
    if verification_was_in_progress {
        let reset_result = async {
            let mut snapshot = load_current_verification_snapshot_or_default(
                session_repo.as_ref(),
                &session,
                VerificationStatus::Unverified,
                false,
            )
            .await?;
            clear_verification_snapshot(&mut snapshot, VerificationStatus::Unverified, false);
            session_repo
                .save_verification_run_snapshot(&session_id, &snapshot)
                .await
        }
        .await;

        if let Err(e) = reset_result {
            tracing::warn!(
                session_id = session_id.as_str(),
                error = %e,
                "Failed to reset verification state during session recovery"
            );
        } else {
            tracing::info!(
                session_id = session_id.as_str(),
                round = current_round,
                "Verification in-progress reset during session recovery"
            );
            // Emit UI event so the frontend reflects the reset immediately (B2)
            if let Some(event_sink) = app_handle.and_then(event_sink_from_app_handle) {
                emit_verification_status_changed(
                    event_sink.as_ref(),
                    session_id.as_str(),
                    VerificationStatus::Unverified,
                    false,
                    None,
                    None,
                    Some(session.verification_generation),
                );
            }
        }
    }

    Some(IdeationRecoveryMetadata {
        session_status: session.status.to_string(),
        plan_artifact_id: session.plan_artifact_id.map(|id| id.to_string()),
        proposal_count,
        parent_session_id: session.parent_session_id.map(|id| id.to_string()),
        team_mode: session.team_mode,
        session_title: session.title,
        verification_status: verification_status_str,
        verification_in_progress: verification_was_in_progress,
        current_round,
    })
}

#[cfg(test)]
#[path = "chat_service_recovery_tests.rs"]
mod tests;
