use serde::Serialize;

use crate::application::chat_service::{
    decode_pending_initial_prompt, ChatService, SendMessageOptions,
};
use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspaceMode, AgentConversationWorkspaceStatus, AgentRun, AgentRunActionKind,
    AgentRunId, AgentRunStatus, ChatContextType, ChatConversationId, IdeationSession,
    IdeationSessionId,
};
use crate::domain::services::{
    check_verification_gate, EffectiveGatePolicy, QueueKey, QueuedMessage,
};
use crate::error::{AppError, AppResult};

const VERIFY_PLAN_PROMPT: &str = "Verify the current linked plan now. Re-read the linked draft and relevant repository evidence; challenge goal alignment, assumptions, integration coverage, state transitions, failure and rollback edges, proof obligations, and testing. Choose context-specific reasoning lenses or allowed general-purpose exploration delegates only when useful. Update the same linked plan if you find material gaps. When the resulting current draft is genuinely implementation-ready, call complete_plan_verification exactly once. Report what changed or why no material changes were needed. Do not approve or implement the plan.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanVerificationRequestSource {
    Manual,
    Automatic,
    External,
}

pub(crate) const fn source_allows_verified_retry(source: PlanVerificationRequestSource) -> bool {
    matches!(source, PlanVerificationRequestSource::Manual)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanVerificationRequestOutcome {
    Queued,
    AlreadyQueued,
    AlreadyRunning,
    AlreadyVerified,
    NoPlan,
}

impl PlanVerificationRequestOutcome {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::AlreadyQueued => "already_queued",
            Self::AlreadyRunning => "already_running",
            Self::AlreadyVerified => "already_verified",
            Self::NoPlan => "no_plan",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanVerificationStatusKind {
    #[default]
    Unverified,
    Queued,
    Verifying,
    Verified,
    Failed,
    Cancelled,
}

impl PlanVerificationStatusKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unverified => "unverified",
            Self::Queued => "queued",
            Self::Verifying => "verifying",
            Self::Verified => "verified",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub const fn is_in_progress(self) -> bool {
        matches!(self, Self::Queued | Self::Verifying)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PlanVerificationStatus {
    pub session_id: String,
    pub status: PlanVerificationStatusKind,
    pub in_progress: bool,
    pub plan_artifact_id: Option<String>,
    pub verified_plan_artifact_id: Option<String>,
    pub agent_run_id: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanVerificationCompletion {
    pub artifact_id: String,
    pub newly_recorded: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutomaticPlanVerificationDisposition {
    NotEligible,
    VerificationPending,
}

impl AutomaticPlanVerificationDisposition {
    pub const fn verification_pending(self) -> bool {
        matches!(self, Self::VerificationPending)
    }
}

#[derive(Debug)]
struct ConversationTarget {
    context_type: ChatContextType,
    context_id: String,
    conversation_id: Option<ChatConversationId>,
    queue_key: QueueKey,
}

fn action_metadata(session_id: &str, artifact_id: &str) -> String {
    serde_json::json!({
        "ralphx_action_kind": "verify_plan",
        "ralphx_action_context_id": session_id,
        "ralphx_action_target_id": artifact_id,
    })
    .to_string()
}

fn queued_message_matches_action(
    message: &QueuedMessage,
    session_id: &str,
    artifact_id: &str,
) -> bool {
    let Some(metadata) = message.metadata_override.as_deref() else {
        return false;
    };
    metadata_matches_action(metadata, session_id, artifact_id)
}

fn metadata_matches_action(metadata: &str, session_id: &str, artifact_id: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(metadata) else {
        return false;
    };
    value
        .get("ralphx_action_kind")
        .and_then(|value| value.as_str())
        == Some("verify_plan")
        && value
            .get("ralphx_action_context_id")
            .and_then(|value| value.as_str())
            == Some(session_id)
        && value
            .get("ralphx_action_target_id")
            .and_then(|value| value.as_str())
            == Some(artifact_id)
}

fn run_matches_action(
    run: &AgentRun,
    conversation_id: Option<&ChatConversationId>,
    session_id: &str,
    artifact_id: &str,
) -> bool {
    conversation_id.is_none_or(|owner| run.conversation_id == *owner)
        && run.action_kind == Some(AgentRunActionKind::VerifyPlan)
        && run.action_context_id.as_deref() == Some(session_id)
        && run.action_target_id.as_deref() == Some(artifact_id)
}

async fn resolve_conversation_target(
    state: &AppState,
    session: &IdeationSession,
) -> AppResult<ConversationTarget> {
    if let Some(workspace) = state
        .agent_conversation_workspace_repo
        .get_by_linked_ideation_session_id(&session.id)
        .await?
    {
        if workspace.mode != AgentConversationWorkspaceMode::Plan {
            return Err(AppError::Validation(
                "Linked Agent conversation is not in Plan mode".to_string(),
            ));
        }
        let conversation_id = workspace.conversation_id;
        return Ok(ConversationTarget {
            context_type: ChatContextType::Project,
            context_id: workspace.project_id.as_str().to_string(),
            conversation_id: Some(conversation_id),
            queue_key: QueueKey::new(ChatContextType::Project, conversation_id.as_str()),
        });
    }

    Ok(ConversationTarget {
        context_type: ChatContextType::Ideation,
        context_id: session.id.as_str().to_string(),
        conversation_id: None,
        queue_key: QueueKey::ideation(session.id.as_str()),
    })
}

async fn action_is_queued(
    state: &AppState,
    key: &QueueKey,
    session: &IdeationSession,
    session_id: &str,
    artifact_id: &str,
) -> AppResult<bool> {
    if session
        .pending_initial_prompt
        .as_deref()
        .is_some_and(|payload| {
            let (_, metadata) = decode_pending_initial_prompt(payload);
            metadata
                .as_deref()
                .is_some_and(|value| metadata_matches_action(value, session_id, artifact_id))
        })
    {
        return Ok(true);
    }
    if state
        .message_queue
        .get_queued_with_key(key)
        .iter()
        .any(|message| queued_message_matches_action(message, session_id, artifact_id))
    {
        return Ok(true);
    }
    Ok(state
        .queued_message_repo
        .list(key)
        .await?
        .iter()
        .any(|message| queued_message_matches_action(message, session_id, artifact_id)))
}

fn exact_verified_status(session: &IdeationSession) -> Option<PlanVerificationStatus> {
    let current = session.plan_artifact_id.as_ref().map(ToString::to_string);
    let verified = session
        .verified_plan_artifact_id
        .as_ref()
        .map(ToString::to_string);
    if current.is_some() && current == verified {
        return Some(PlanVerificationStatus {
            session_id: session.id.as_str().to_string(),
            status: PlanVerificationStatusKind::Verified,
            in_progress: false,
            plan_artifact_id: current,
            verified_plan_artifact_id: verified,
            agent_run_id: session.verified_plan_agent_run_id.clone(),
            started_at: None,
            completed_at: None,
            error: None,
        });
    }
    None
}

fn status_from_run(session: &IdeationSession, run: Option<AgentRun>) -> PlanVerificationStatus {
    let current = session.plan_artifact_id.as_ref().map(ToString::to_string);
    let verified = session
        .verified_plan_artifact_id
        .as_ref()
        .map(ToString::to_string);

    let (status, error) = match run.as_ref().map(|run| run.status) {
        Some(AgentRunStatus::Running) => (PlanVerificationStatusKind::Verifying, None),
        Some(AgentRunStatus::Failed) => (
            PlanVerificationStatusKind::Failed,
            run.as_ref().and_then(|run| run.error_message.clone()),
        ),
        Some(AgentRunStatus::Cancelled) => (PlanVerificationStatusKind::Cancelled, None),
        Some(AgentRunStatus::Completed) => (
            PlanVerificationStatusKind::Failed,
            Some("Verification action completed without recording proof".to_string()),
        ),
        None => (PlanVerificationStatusKind::Unverified, None),
    };
    PlanVerificationStatus {
        session_id: session.id.as_str().to_string(),
        status,
        in_progress: status == PlanVerificationStatusKind::Verifying,
        plan_artifact_id: current,
        verified_plan_artifact_id: verified,
        agent_run_id: run.as_ref().map(|run| run.id.as_str()),
        started_at: run.as_ref().map(|run| run.started_at.to_rfc3339()),
        completed_at: run
            .as_ref()
            .and_then(|run| run.completed_at.map(|value| value.to_rfc3339())),
        error,
    }
}

pub async fn get_plan_verification_status(
    state: &AppState,
    session_id: &IdeationSessionId,
) -> AppResult<PlanVerificationStatus> {
    let session = state
        .ideation_session_repo
        .get_by_id(session_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Session {} not found", session_id)))?;
    let Some(artifact_id) = session.plan_artifact_id.as_ref() else {
        return Ok(
            exact_verified_status(&session).unwrap_or_else(|| status_from_run(&session, None))
        );
    };
    let target = resolve_conversation_target(state, &session).await?;
    if let Some(conversation_id) = target.conversation_id.as_ref() {
        if let Some(active_run) = state
            .agent_run_repo
            .get_active_action(
                conversation_id,
                AgentRunActionKind::VerifyPlan,
                session.id.as_str(),
                artifact_id.as_str(),
            )
            .await?
        {
            return Ok(status_from_run(&session, Some(active_run)));
        }
    }
    if action_is_queued(
        state,
        &target.queue_key,
        &session,
        session.id.as_str(),
        artifact_id.as_str(),
    )
    .await?
    {
        let mut status = status_from_run(&session, None);
        status.status = PlanVerificationStatusKind::Queued;
        status.in_progress = true;
        return Ok(status);
    }
    if let Some(verified) = exact_verified_status(&session) {
        return Ok(verified);
    }
    let latest = state
        .agent_run_repo
        .get_latest_action(
            AgentRunActionKind::VerifyPlan,
            session.id.as_str(),
            artifact_id.as_str(),
        )
        .await?;
    Ok(status_from_run(&session, latest))
}

pub async fn request_plan_verification<C: ChatService + ?Sized>(
    state: &AppState,
    chat_service: &C,
    session_id: &IdeationSessionId,
    source: PlanVerificationRequestSource,
) -> AppResult<PlanVerificationRequestOutcome> {
    let admission_key = session_id.as_str().to_string();
    let admission_lock = {
        state
            .plan_verification_locks
            .entry(admission_key.clone())
            .or_insert_with(|| std::sync::Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    };
    let _admission_guard = admission_lock.lock().await;
    let session = state
        .ideation_session_repo
        .get_by_id(session_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Session {} not found", session_id)))?;
    let Some(artifact_id) = session.plan_artifact_id.as_ref() else {
        return Ok(PlanVerificationRequestOutcome::NoPlan);
    };
    let target = resolve_conversation_target(state, &session).await?;
    if let Some(conversation_id) = target.conversation_id.as_ref() {
        if state
            .agent_run_repo
            .get_active_action(
                conversation_id,
                AgentRunActionKind::VerifyPlan,
                session.id.as_str(),
                artifact_id.as_str(),
            )
            .await?
            .is_some()
        {
            return Ok(PlanVerificationRequestOutcome::AlreadyRunning);
        }
    } else if state
        .agent_run_repo
        .get_latest_action(
            AgentRunActionKind::VerifyPlan,
            session.id.as_str(),
            artifact_id.as_str(),
        )
        .await?
        .is_some_and(|run| run.status == AgentRunStatus::Running)
    {
        return Ok(PlanVerificationRequestOutcome::AlreadyRunning);
    }
    if action_is_queued(
        state,
        &target.queue_key,
        &session,
        session.id.as_str(),
        artifact_id.as_str(),
    )
    .await?
    {
        state.plan_verification_admissions.remove(&admission_key);
        return Ok(PlanVerificationRequestOutcome::AlreadyQueued);
    }

    if session.verified_plan_artifact_id.as_ref() == Some(artifact_id)
        && !source_allows_verified_retry(source)
    {
        state.plan_verification_admissions.remove(&admission_key);
        return Ok(PlanVerificationRequestOutcome::AlreadyVerified);
    }

    if state
        .plan_verification_admissions
        .get(&admission_key)
        .is_some_and(|target| target.value() == artifact_id.as_str())
    {
        state.plan_verification_admissions.remove(&admission_key);
        return Ok(PlanVerificationRequestOutcome::AlreadyQueued);
    }

    state
        .plan_verification_admissions
        .insert(admission_key.clone(), artifact_id.as_str().to_string());
    let send_result = chat_service
        .send_message(
            target.context_type,
            &target.context_id,
            VERIFY_PLAN_PROMPT,
            SendMessageOptions {
                metadata: Some(action_metadata(session.id.as_str(), artifact_id.as_str())),
                conversation_id_override: target.conversation_id,
                is_external_mcp: source == PlanVerificationRequestSource::External,
                ..Default::default()
            },
        )
        .await;
    let send_result = match send_result {
        Ok(result) => result,
        Err(error) => {
            state.plan_verification_admissions.remove(&admission_key);
            return Err(AppError::Infrastructure(error.to_string()));
        }
    };
    let immediate_run = if send_result.agent_run_id.is_empty() {
        None
    } else {
        state
            .agent_run_repo
            .get_by_id(&crate::domain::entities::AgentRunId::from_string(
                send_result.agent_run_id.clone(),
            ))
            .await?
    };
    let fresh_session = state
        .ideation_session_repo
        .get_by_id(session_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Session {} not found", session_id)))?;
    let durable_queue = action_is_queued(
        state,
        &target.queue_key,
        &fresh_session,
        session.id.as_str(),
        artifact_id.as_str(),
    )
    .await?;
    if !immediate_run.as_ref().is_some_and(|run| {
        run_matches_action(
            run,
            target.conversation_id.as_ref(),
            session.id.as_str(),
            artifact_id.as_str(),
        )
    }) && !durable_queue
    {
        state.plan_verification_admissions.remove(&admission_key);
        return Err(AppError::Infrastructure(
            "Verify Plan admission did not produce a matching typed run or durable queue entry"
                .to_string(),
        ));
    }
    state.plan_verification_admissions.remove(&admission_key);
    Ok(PlanVerificationRequestOutcome::Queued)
}

/// Admit completion-triggered verification only from the authoritative successful finalizer.
pub async fn admit_automatic_plan_verification<C: ChatService + ?Sized>(
    state: &AppState,
    chat_service: &C,
    conversation_id: &ChatConversationId,
    run_id: &AgentRunId,
    completion_applied: bool,
) -> AppResult<AutomaticPlanVerificationDisposition> {
    if !completion_applied {
        return Ok(AutomaticPlanVerificationDisposition::NotEligible);
    }
    let Some(run) = state.agent_run_repo.get_by_id(run_id).await? else {
        return Ok(AutomaticPlanVerificationDisposition::NotEligible);
    };
    if run.conversation_id != *conversation_id
        || run.status != AgentRunStatus::Completed
        || run.action_kind.is_some()
    {
        return Ok(AutomaticPlanVerificationDisposition::NotEligible);
    }
    let latest = state
        .agent_run_repo
        .get_latest_for_conversation(conversation_id)
        .await?;
    if latest.as_ref().map(|candidate| candidate.id) != Some(*run_id) {
        return Ok(AutomaticPlanVerificationDisposition::NotEligible);
    }
    let Some(workspace) = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(conversation_id)
        .await?
    else {
        return Ok(AutomaticPlanVerificationDisposition::NotEligible);
    };
    if workspace.status != AgentConversationWorkspaceStatus::Active
        || workspace.mode != AgentConversationWorkspaceMode::Plan
    {
        return Ok(AutomaticPlanVerificationDisposition::NotEligible);
    }
    let Some(session_id) = workspace.linked_ideation_session_id else {
        return Ok(AutomaticPlanVerificationDisposition::NotEligible);
    };
    let settings = state
        .ideation_settings_repo
        .get_settings()
        .await
        .map_err(|error| AppError::Infrastructure(error.to_string()))?;
    if !settings.auto_verify_draft_plans {
        return Ok(AutomaticPlanVerificationDisposition::NotEligible);
    }

    let outcome = request_plan_verification(
        state,
        chat_service,
        &session_id,
        PlanVerificationRequestSource::Automatic,
    )
    .await?;
    Ok(match outcome {
        PlanVerificationRequestOutcome::Queued
        | PlanVerificationRequestOutcome::AlreadyQueued
        | PlanVerificationRequestOutcome::AlreadyRunning => {
            AutomaticPlanVerificationDisposition::VerificationPending
        }
        PlanVerificationRequestOutcome::AlreadyVerified
        | PlanVerificationRequestOutcome::NoPlan => {
            AutomaticPlanVerificationDisposition::NotEligible
        }
    })
}

/// Enforce the exact-plan verification policy at the acceptance boundary.
///
/// When verification is required and automatic triggering is enabled, the first
/// acceptance attempt queues the normal visible Verify Plan action and remains
/// blocked. The caller retries acceptance after that action records exact proof.
/// Draft creation and revision never call this path.
///
/// # Errors
///
/// Returns [`AppError::Validation`] while required proof is absent, including
/// after a verification action is queued or already in progress. Infrastructure
/// and persistence failures from verification request admission are propagated.
pub async fn ensure_plan_verification_for_acceptance<C: ChatService + ?Sized>(
    state: &AppState,
    chat_service: &C,
    session: &IdeationSession,
    policy: &EffectiveGatePolicy,
) -> AppResult<()> {
    let gate_error = match check_verification_gate(session, policy) {
        Ok(()) => return Ok(()),
        Err(error) => error,
    };

    if !policy.auto_verify_plans {
        return Err(AppError::Validation(gate_error.to_string()));
    }

    match request_plan_verification(
        state,
        chat_service,
        &session.id,
        PlanVerificationRequestSource::Automatic,
    )
    .await?
    {
        PlanVerificationRequestOutcome::Queued => Err(AppError::Validation(
            "Plan verification was queued. Accept the plan again after verification completes."
                .to_string(),
        )),
        PlanVerificationRequestOutcome::AlreadyQueued => Err(AppError::Validation(
            "Plan verification is already queued. Accept the plan again after verification completes."
                .to_string(),
        )),
        PlanVerificationRequestOutcome::AlreadyRunning => Err(AppError::Validation(
            "Plan verification is in progress. Accept the plan again after verification completes."
                .to_string(),
        )),
        PlanVerificationRequestOutcome::AlreadyVerified => {
            let current = state
                .ideation_session_repo
                .get_by_id(&session.id)
                .await?
                .ok_or_else(|| {
                    AppError::NotFound(format!("Session {} not found", session.id))
                })?;
            check_verification_gate(&current, policy)
                .map_err(|error| AppError::Validation(error.to_string()))
        }
        PlanVerificationRequestOutcome::NoPlan => {
            Err(AppError::Validation(gate_error.to_string()))
        }
    }
}

pub async fn complete_plan_verification(
    state: &AppState,
    session_id: &IdeationSessionId,
    agent_run_id: &str,
) -> AppResult<PlanVerificationCompletion> {
    let session = state
        .ideation_session_repo
        .get_by_id(session_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Session {} not found", session_id)))?;
    let artifact_id = session
        .plan_artifact_id
        .as_ref()
        .ok_or_else(|| AppError::Validation("Session has no linked plan".to_string()))?;
    let completed = state
        .ideation_session_repo
        .complete_plan_verification(session_id, agent_run_id, artifact_id.as_str())
        .await?;
    if completed {
        return Ok(PlanVerificationCompletion {
            artifact_id: artifact_id.as_str().to_string(),
            newly_recorded: true,
        });
    }
    let current = state
        .ideation_session_repo
        .get_by_id(session_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Session {} not found", session_id)))?;
    if current.plan_artifact_id.as_ref() == Some(artifact_id)
        && current.verified_plan_artifact_id.as_ref() == Some(artifact_id)
        && current.verified_plan_agent_run_id.as_deref() == Some(agent_run_id)
    {
        return Ok(PlanVerificationCompletion {
            artifact_id: artifact_id.as_str().to_string(),
            newly_recorded: false,
        });
    }
    Err(AppError::Validation(
        "Verification completion rejected: stale, failed, cancelled, ordinary, or mismatched action"
            .to_string(),
    ))
}
