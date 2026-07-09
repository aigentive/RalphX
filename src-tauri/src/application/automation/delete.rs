use std::str::FromStr;
use std::sync::Arc;

use chrono::Utc;

use crate::application::agent_conversation_archive::archive_agent_conversation_for_state;
use crate::application::automation::api::automation_service_for_state;
use crate::application::automation::transition::{
    AutomationEventEmitter, AutomationTransitionService, NoopAutomationEventEmitter,
    TauriAutomationEventEmitter,
};
use crate::application::AppState;
use crate::domain::entities::{
    ArtifactId, AutomationId, AutomationJudgeState, AutomationStatus, ChatConversation,
    IdeationAnalysisBaseRefKind, IdeationSessionFlow,
};
use crate::domain::repositories::PlanArtifactApprovalRepository;
use crate::error::{AppError, AppResult};
use crate::infrastructure::sqlite::SqlitePlanArtifactApprovalRepository;

/// Archive an automation's durable history and hard-delete its bookkeeping rows.
///
/// This is the application-level composition for automation deletion. It lives
/// here (not on `AutomationService`) because the conversation-archive
/// orchestrator takes `&AppState`, whereas `AutomationService` holds only repos
/// (critic G9). Ordering is fail-closed — destructive last — per the delete
/// spec:
///
/// 1. Gate on `Draft | Completed | Stopped` and the judge-lease predicate (E1/E2).
/// 2. If Draft, claim deletion authority by CAS `Draft -> Stopped` BEFORE any
///    side effect; a concurrent `finalize` then loses the race cleanly and no
///    archive ever commits against a survivor (E1).
/// 3. Archive every attached conversation (setup + runs) via the shared
///    orchestrator; the first failure aborts before any row deletion, and
///    already-archived conversations are skipped so a retry does not re-fire
///    stop-agent / close-PR side effects (E4/E5).
/// 4. Archive the linked spec artifact — never hard-deleted (E8); warn + continue.
/// 5. Hard-delete the automation + runs + attachments + context refs and emit
///    `AutomationDeleted` (E3/E6/E7/E9).
///
/// # Errors
///
/// Returns `NotFound` for an unknown automation, `Validation` when the status or
/// a live judge lease forbids deletion, `Conflict` when a concurrent finalize
/// wins the Draft CAS, or an `Infrastructure` error (naming the conversation)
/// when a conversation archive fails before any rows were deleted.
pub async fn delete_automation_with_archive(state: &AppState, id: &AutomationId) -> AppResult<()> {
    let automation = state
        .automation_repo
        .get_by_id(id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("automation {} not found", id.as_str())))?;

    if !matches!(
        automation.status,
        AutomationStatus::Draft | AutomationStatus::Completed | AutomationStatus::Stopped
    ) {
        return Err(AppError::Validation(
            "only draft, completed, or stopped automations can be deleted".to_string(),
        ));
    }

    // Judge-lease gate: only a *live* finalizing judge blocks deletion. A crashed
    // judge (InProgress with a NULL or expired lease, lazily self-healed by the
    // scheduler) must NOT block, otherwise delete would be permanently wedged (E2).
    if let Some(latest) = state.automation_run_repo.latest_for_automation(id).await? {
        let now = Utc::now();
        if latest.judge_state == AutomationJudgeState::InProgress
            && latest
                .judge_lease_expires_at
                .is_some_and(|expires_at| expires_at > now)
        {
            return Err(AppError::Validation(
                "judge is finalizing; retry shortly".to_string(),
            ));
        }
    }

    // Claim deletion authority for drafts BEFORE any archive side effect (E1). The
    // CAS is scoped to `Draft -> Stopped`; if a concurrent finalize already moved
    // the automation to Active the swap affects zero rows and we abort here with
    // nothing archived.
    if automation.status == AutomationStatus::Draft {
        let claimed = automation_transition_service(state)
            .transition_automation_status(
                id,
                AutomationStatus::Draft,
                AutomationStatus::Stopped,
                None,
                None,
            )
            .await?;
        if !claimed {
            return Err(AppError::Conflict(format!(
                "automation {} status changed before delete could claim authority",
                id.as_str()
            )));
        }
    }

    // Archive every attached conversation that is not already archived (E4/E5).
    // `list_by_automation_id` re-returns archived rows on retry, and `archive()`
    // itself is a no-op on re-run, but stop-agent / close-PR side effects would
    // still fire — so filter archived rows out here (critic G8).
    let conversations = state
        .chat_conversation_repo
        .list_by_automation_id(id)
        .await?;
    for conversation in &conversations {
        if conversation.archived_at.is_some() {
            continue;
        }
        archive_agent_conversation_for_state(&conversation.id, state)
            .await
            .map_err(|error| {
                AppError::Infrastructure(format!(
                    "failed to archive automation conversation {}: {error}",
                    conversation.id.as_str()
                ))
            })?;
    }
    cleanup_plan_gate_artifacts_for_run_conversations(state, id, &conversations).await?;

    // Archive the linked spec artifact — never hard-deleted; it is versioned and
    // may be `derived_from`-linked (E8). Warn + continue on failure.
    if let Some(spec_artifact_id) = automation
        .spec_artifact_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let artifact_id = ArtifactId::from_string(spec_artifact_id.to_string());
        if let Err(error) = state.artifact_repo.archive(&artifact_id).await {
            tracing::warn!(
                automation_id = id.as_str(),
                spec_artifact_id,
                error = %error,
                "delete_automation_with_archive: failed to archive spec artifact; continuing"
            );
        }
    }

    // Remote-branch cleanup (B4): best-effort delete of the automation-owned base
    // branch we newly pushed to origin (integration-branch model). Gated to
    // automation-owned local-branch bases; per-run pr_head branches are NOT touched
    // here. Fail-open — the branch may never have been pushed (automation never
    // published), and `delete_remote_branch` already no-ops on an absent remote, so
    // delete must still succeed for never-published automations. The base ref is
    // read from the `automation` row loaded above, before any row is deleted.
    cleanup_automation_remote_base_branch(state, &automation).await;

    // Destructive last: hard-delete the bookkeeping rows and emit
    // `AutomationDeleted` (the row-delete core owns the event so `project_id` is
    // captured before the row is gone).
    automation_service_for_state(state).delete(id).await
}

async fn cleanup_plan_gate_artifacts_for_run_conversations(
    state: &AppState,
    automation_id: &AutomationId,
    conversations: &[ChatConversation],
) -> AppResult<()> {
    let approval_repo = SqlitePlanArtifactApprovalRepository::new(state.db.clone());
    for conversation in conversations
        .iter()
        .filter(|conversation| conversation.automation_run_id.is_some())
    {
        let Some(workspace) = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation.id)
            .await?
        else {
            continue;
        };
        let Some(session_id) = workspace.linked_ideation_session_id.as_ref() else {
            continue;
        };
        let Some(session) = state.ideation_session_repo.get_by_id(session_id).await? else {
            approval_repo.delete_by_session(session_id).await?;
            continue;
        };
        if session.session_flow != IdeationSessionFlow::Planning {
            continue;
        }
        if let Some(plan_artifact_id) = session.plan_artifact_id.as_ref() {
            archive_plan_artifact_chain(
                state,
                automation_id,
                session_id.as_str(),
                plan_artifact_id,
            )
            .await;
        }
        approval_repo.delete_by_session(session_id).await?;
        state.ideation_session_repo.delete(session_id).await?;
    }
    Ok(())
}

async fn archive_plan_artifact_chain(
    state: &AppState,
    automation_id: &AutomationId,
    session_id: &str,
    plan_artifact_id: &ArtifactId,
) {
    let latest_id = match state
        .artifact_repo
        .resolve_latest_artifact_id(plan_artifact_id)
        .await
    {
        Ok(latest_id) => latest_id,
        Err(error) => {
            tracing::warn!(
                automation_id = automation_id.as_str(),
                session_id,
                plan_artifact_id = plan_artifact_id.as_str(),
                error = %error,
                "delete_automation_with_archive: failed to resolve latest plan artifact; archiving known artifact"
            );
            plan_artifact_id.clone()
        }
    };

    let artifact_ids = match state.artifact_repo.get_version_history(&latest_id).await {
        Ok(history) if !history.is_empty() => {
            history.into_iter().map(|summary| summary.id).collect()
        }
        Ok(_) => vec![latest_id],
        Err(error) => {
            tracing::warn!(
                automation_id = automation_id.as_str(),
                session_id,
                plan_artifact_id = plan_artifact_id.as_str(),
                error = %error,
                "delete_automation_with_archive: failed to read plan artifact version chain; archiving known artifact"
            );
            vec![plan_artifact_id.clone()]
        }
    };

    for artifact_id in artifact_ids {
        if let Err(error) = state.artifact_repo.archive(&artifact_id).await {
            tracing::warn!(
                automation_id = automation_id.as_str(),
                session_id,
                plan_artifact_id = artifact_id.as_str(),
                error = %error,
                "delete_automation_with_archive: failed to archive plan artifact; continuing"
            );
        }
    }
}

/// Best-effort deletion of an automation's origin base branch. Never fails the
/// caller — every error path warns and returns. See B4 in the integration-branch
/// spec.
async fn cleanup_automation_remote_base_branch(
    state: &AppState,
    automation: &crate::domain::entities::Automation,
) {
    let is_local_branch = IdeationAnalysisBaseRefKind::from_str(automation.base_ref_kind.trim())
        .map(|kind| kind == IdeationAnalysisBaseRefKind::LocalBranch)
        .unwrap_or(false);
    if !is_local_branch {
        return;
    }
    let base_ref = automation.base_ref.trim();
    if base_ref.is_empty() {
        return;
    }
    let Some(github) = state.github_service.as_ref() else {
        return;
    };
    let project = match state.project_repo.get_by_id(&automation.project_id).await {
        Ok(Some(project)) => project,
        Ok(None) => {
            tracing::warn!(
                automation_id = automation.id.as_str(),
                project_id = automation.project_id.as_str(),
                "delete_automation_with_archive: project missing; skipping remote base branch cleanup"
            );
            return;
        }
        Err(error) => {
            tracing::warn!(
                automation_id = automation.id.as_str(),
                error = %error,
                "delete_automation_with_archive: failed to load project for remote base branch cleanup; continuing"
            );
            return;
        }
    };
    let working_dir = std::path::Path::new(&project.working_directory);
    if let Err(error) = github.delete_remote_branch(working_dir, base_ref).await {
        tracing::warn!(
            automation_id = automation.id.as_str(),
            base_ref,
            error = %error,
            "delete_automation_with_archive: failed to delete remote automation base branch; continuing"
        );
    }
}

fn automation_transition_service(state: &AppState) -> AutomationTransitionService {
    let event_emitter: Arc<dyn AutomationEventEmitter> = match state.app_handle.as_ref() {
        Some(app_handle) => Arc::new(TauriAutomationEventEmitter::new(app_handle.clone())),
        None => Arc::new(NoopAutomationEventEmitter),
    };
    AutomationTransitionService::new(
        state.automation_repo.clone(),
        state.automation_run_repo.clone(),
        event_emitter,
    )
}
