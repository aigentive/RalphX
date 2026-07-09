use chrono::Utc;
use serde_json::json;

use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspaceMode, ChatContextType, ChatConversationId, IdeationSessionId,
    ProposalGenerationPhase, ProposalGenerationProgress, ProposalGenerationStatus,
};
use crate::error::{AppError, AppResult};

pub const PROPOSAL_GENERATION_PROGRESS_EVENT: &str = "ideation:proposal_generation_progress";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProposalGenerationProgressTransition {
    Queued { expected_count: Option<u32> },
    CreatingProposals { expected_count: Option<u32> },
    AnalyzingDependencies,
    FinalizingProposals,
    WaitingForConfirmation,
    Completed,
    Failed { error: String },
    Cancelled { error: Option<String> },
}

pub async fn write_proposal_generation_progress(
    state: &AppState,
    session_id: &IdeationSessionId,
    transition: ProposalGenerationProgressTransition,
) -> AppResult<ProposalGenerationProgress> {
    let session = state
        .ideation_session_repo
        .get_by_id(session_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Session {} not found", session_id)))?;

    let existing = session.proposal_generation_progress;
    let created_count = saturating_usize_to_u32(
        state
            .ideation_session_repo
            .count_active_proposals(session_id)
            .await?,
    );
    let dependency_count = saturating_usize_to_u32(
        state
            .proposal_dependency_repo
            .get_all_for_session(session_id)
            .await?
            .len(),
    );

    let now = Utc::now();
    let (status, phase) = status_and_phase(&transition);
    let is_terminal = matches!(
        status,
        ProposalGenerationStatus::Completed
            | ProposalGenerationStatus::Failed
            | ProposalGenerationStatus::Cancelled
    );
    let starts_new_operation = matches!(
        transition,
        ProposalGenerationProgressTransition::Queued { .. }
            | ProposalGenerationProgressTransition::CreatingProposals { .. }
    ) && matches!(
        existing.status,
        ProposalGenerationStatus::Idle
            | ProposalGenerationStatus::Completed
            | ProposalGenerationStatus::Failed
            | ProposalGenerationStatus::Cancelled
    );
    let expected_count = explicit_expected_count(&transition)
        .or(session.expected_proposal_count)
        .or(existing.expected_count);
    let dependency_count = if should_include_dependency_count(
        &transition,
        dependency_count,
        existing.dependency_count,
    ) {
        Some(dependency_count)
    } else {
        None
    };

    let progress = ProposalGenerationProgress {
        status,
        phase,
        expected_count,
        created_count,
        dependency_count,
        error: transition_error(transition),
        started_at: if starts_new_operation {
            Some(now)
        } else {
            existing.started_at.or(Some(now))
        },
        updated_at: Some(now),
        completed_at: if is_terminal { Some(now) } else { None },
    };

    state
        .ideation_session_repo
        .update_proposal_generation_progress(session_id.as_str(), progress.clone())
        .await?;

    state.events.emit(
        PROPOSAL_GENERATION_PROGRESS_EVENT,
        json!({
            "sessionId": session_id.as_str(),
            "progress": progress_payload(&progress),
        }),
    );

    Ok(progress)
}

pub async fn write_active_proposal_generation_progress_for_context(
    state: &AppState,
    context_type: ChatContextType,
    context_id: &str,
    conversation_id: Option<&ChatConversationId>,
    transition: ProposalGenerationProgressTransition,
) -> AppResult<bool> {
    let Some(session_id) =
        resolve_proposal_generation_session_id(state, context_type, context_id, conversation_id)
            .await?
    else {
        return Ok(false);
    };
    let Some(session) = state.ideation_session_repo.get_by_id(&session_id).await? else {
        return Ok(false);
    };
    if !matches!(
        session.proposal_generation_progress.status,
        ProposalGenerationStatus::Queued
            | ProposalGenerationStatus::Running
            | ProposalGenerationStatus::WaitingForConfirmation
    ) {
        return Ok(false);
    }

    write_proposal_generation_progress(state, &session_id, transition).await?;
    Ok(true)
}

async fn resolve_proposal_generation_session_id(
    state: &AppState,
    context_type: ChatContextType,
    context_id: &str,
    conversation_id: Option<&ChatConversationId>,
) -> AppResult<Option<IdeationSessionId>> {
    if context_type == ChatContextType::Ideation {
        return Ok(Some(IdeationSessionId::from_string(context_id)));
    }
    if context_type != ChatContextType::Project {
        return Ok(None);
    }

    let conversation_id = conversation_id
        .cloned()
        .unwrap_or_else(|| ChatConversationId::from_string(context_id));
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await?;
    Ok(workspace.and_then(|workspace| {
        if workspace.mode == AgentConversationWorkspaceMode::Plan {
            workspace.linked_ideation_session_id
        } else {
            None
        }
    }))
}

fn status_and_phase(
    transition: &ProposalGenerationProgressTransition,
) -> (ProposalGenerationStatus, Option<ProposalGenerationPhase>) {
    match transition {
        ProposalGenerationProgressTransition::Queued { .. } => (
            ProposalGenerationStatus::Queued,
            Some(ProposalGenerationPhase::Queued),
        ),
        ProposalGenerationProgressTransition::CreatingProposals { .. } => (
            ProposalGenerationStatus::Running,
            Some(ProposalGenerationPhase::CreatingProposals),
        ),
        ProposalGenerationProgressTransition::AnalyzingDependencies => (
            ProposalGenerationStatus::Running,
            Some(ProposalGenerationPhase::AnalyzingDependencies),
        ),
        ProposalGenerationProgressTransition::FinalizingProposals => (
            ProposalGenerationStatus::Running,
            Some(ProposalGenerationPhase::FinalizingProposals),
        ),
        ProposalGenerationProgressTransition::WaitingForConfirmation => (
            ProposalGenerationStatus::WaitingForConfirmation,
            Some(ProposalGenerationPhase::WaitingForConfirmation),
        ),
        ProposalGenerationProgressTransition::Completed => (
            ProposalGenerationStatus::Completed,
            Some(ProposalGenerationPhase::Completed),
        ),
        ProposalGenerationProgressTransition::Failed { .. } => (
            ProposalGenerationStatus::Failed,
            Some(ProposalGenerationPhase::Failed),
        ),
        ProposalGenerationProgressTransition::Cancelled { .. } => (
            ProposalGenerationStatus::Cancelled,
            Some(ProposalGenerationPhase::Cancelled),
        ),
    }
}

fn explicit_expected_count(transition: &ProposalGenerationProgressTransition) -> Option<u32> {
    match transition {
        ProposalGenerationProgressTransition::Queued { expected_count }
        | ProposalGenerationProgressTransition::CreatingProposals { expected_count } => {
            *expected_count
        }
        _ => None,
    }
}

fn should_include_dependency_count(
    transition: &ProposalGenerationProgressTransition,
    current_count: u32,
    existing_count: Option<u32>,
) -> bool {
    match transition {
        ProposalGenerationProgressTransition::Queued { .. }
        | ProposalGenerationProgressTransition::CreatingProposals { .. } => {
            current_count > 0 || existing_count.is_some()
        }
        _ => true,
    }
}

fn transition_error(transition: ProposalGenerationProgressTransition) -> Option<String> {
    match transition {
        ProposalGenerationProgressTransition::Failed { error } => Some(short_error(error)),
        ProposalGenerationProgressTransition::Cancelled { error } => error.map(short_error),
        _ => None,
    }
}

fn short_error(error: String) -> String {
    const MAX_ERROR_CHARS: usize = 500;
    if error.chars().count() <= MAX_ERROR_CHARS {
        return error;
    }
    error.chars().take(MAX_ERROR_CHARS).collect()
}

fn progress_payload(progress: &ProposalGenerationProgress) -> serde_json::Value {
    json!({
        "status": progress.status.to_string(),
        "phase": progress.phase.map(|phase| phase.to_string()),
        "expected_count": progress.expected_count,
        "created_count": progress.created_count,
        "dependency_count": progress.dependency_count,
        "error": progress.error.as_deref(),
        "started_at": progress.started_at.as_ref().map(|dt| dt.to_rfc3339()),
        "updated_at": progress.updated_at.as_ref().map(|dt| dt.to_rfc3339()),
        "completed_at": progress.completed_at.as_ref().map(|dt| dt.to_rfc3339()),
    })
}

fn saturating_usize_to_u32(value: usize) -> u32 {
    if value > u32::MAX as usize {
        u32::MAX
    } else {
        value as u32
    }
}
