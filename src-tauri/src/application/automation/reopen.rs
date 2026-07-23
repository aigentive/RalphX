use std::sync::Arc;

use chrono::Utc;
use tauri::Manager;

use crate::application::app_state::ApplicationExecutionState;
use crate::application::automation::api::automation_transition_service_for_state;
use crate::application::automation::plan_gate::clear_plan_phase_publication_metadata;
use crate::application::automation::transition::{
    AUTOMATION_RUN_UPDATED_EVENT, AUTOMATION_UPDATED_EVENT,
};
use crate::application::startup_background::resume_automation_run_with_prompt_via_chat_service;
use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentWorkspaceReviewGateStatus, AgentWorkspaceReviewMonitorStatus,
    AgentWorkspaceReviewOutcome, AutomationId, AutomationRun, AutomationRunId, AutomationRunStatus,
    AutomationStatus, ChatContextType, ChatConversationId,
};
use crate::domain::services::running_agent_registry::RunningAgentKey;
use crate::error::{AppError, AppResult};

pub(crate) const AUTOMATION_RUN_CONTINUATION_PROMPT: &str = "This run was interrupted and has been resumed in place. Your previous work is preserved in this same worktree (there are uncommitted changes and full conversation history above). Do NOT start over. Run git status to see what you already changed, review the goal and where the prior attempt stopped, then continue toward completion from here.";

struct ReopenContext {
    run: AutomationRun,
    automation_status: AutomationStatus,
    workspace: AgentConversationWorkspace,
    conversation_id: ChatConversationId,
}

#[async_trait::async_trait]
pub(crate) trait AutomationRunRedriver: Send + Sync {
    async fn redrive(
        &self,
        state: &AppState,
        conversation_id: &ChatConversationId,
        prompt: &str,
    ) -> AppResult<()>;
}

struct ChatServiceAutomationRunRedriver;

#[async_trait::async_trait]
impl AutomationRunRedriver for ChatServiceAutomationRunRedriver {
    async fn redrive(
        &self,
        state: &AppState,
        conversation_id: &ChatConversationId,
        prompt: &str,
    ) -> AppResult<()> {
        let chat_service = state.build_chat_service_with_managed_execution_state();
        resume_automation_run_with_prompt_via_chat_service(
            state,
            &chat_service,
            conversation_id,
            prompt,
        )
        .await?;
        Ok(())
    }
}

/// Reopen the latest agent-failed automation run and continue its existing conversation.
///
/// # Errors
///
/// Returns a typed not-found, validation, or conflict error when the run cannot be safely
/// reopened, and propagates repository or chat delivery failures.
pub(crate) async fn reopen_automation_run(
    state: &AppState,
    automation_id: &AutomationId,
    run_id: &AutomationRunId,
) -> AppResult<()> {
    reopen_automation_run_with_redriver(
        state,
        automation_id,
        run_id,
        &ChatServiceAutomationRunRedriver,
    )
    .await
}

pub(crate) async fn reopen_automation_run_with_redriver(
    state: &AppState,
    automation_id: &AutomationId,
    run_id: &AutomationRunId,
    redriver: &dyn AutomationRunRedriver,
) -> AppResult<()> {
    let context = load_reopen_context(state, automation_id, run_id).await?;
    let basis = Utc::now();

    // Claim authority first: the corrective transition is the atomic gate. Only after
    // it succeeds do we mutate the run's judge/publication/monitor/freshness state, so
    // a lost race (a concurrent transition, delete, or scheduler-spawned successor)
    // leaves the failed run untouched instead of partially reset.
    let transition_service = automation_transition_service_for_state(state);
    if !transition_service
        .reopen_run_corrective(run_id, AutomationRunStatus::AgentFailed)
        .await?
    {
        return Err(AppError::Conflict(
            "failed run changed before it could be resumed".to_string(),
        ));
    }

    if matches!(
        context.automation_status,
        AutomationStatus::Paused | AutomationStatus::Stopped
    ) {
        let _ = transition_service
            .transition_automation_status(
                automation_id,
                context.automation_status,
                AutomationStatus::Active,
                None,
                None,
            )
            .await?;
    }

    reset_reopen_state(state, &context).await?;
    state
        .automation_run_repo
        .set_agent_phase_started_at(run_id, Some(basis))
        .await?;
    state.automation_run_repo.clear_finished_at(run_id).await?;

    redriver
        .redrive(
            state,
            &context.conversation_id,
            AUTOMATION_RUN_CONTINUATION_PROMPT,
        )
        .await?;

    emit_reopen_events(state, automation_id, run_id);
    Ok(())
}

async fn load_reopen_context(
    state: &AppState,
    automation_id: &AutomationId,
    run_id: &AutomationRunId,
) -> AppResult<ReopenContext> {
    let automation = state
        .automation_repo
        .get_by_id(automation_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("automation {automation_id} not found")))?;
    let runs = state
        .automation_run_repo
        .list_for_automation(automation_id)
        .await?;
    let run = runs
        .into_iter()
        .find(|run| run.id == *run_id)
        .ok_or_else(|| AppError::NotFound(format!("automation run {run_id} not found")))?;
    let latest = state
        .automation_run_repo
        .latest_for_automation(automation_id)
        .await?;
    if !latest.is_some_and(|latest| latest.id == *run_id) {
        return Err(AppError::Conflict(
            "only the latest run can be resumed".to_string(),
        ));
    }
    if !matches!(run.status, AutomationRunStatus::AgentFailed) {
        return Err(AppError::Conflict(
            "only a failed run can be resumed".to_string(),
        ));
    }
    let conversation_id = run.conversation_id.clone().ok_or_else(|| {
        AppError::Validation("failed run has no conversation to resume".to_string())
    })?;
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await?
        .ok_or_else(|| {
            AppError::Validation("failed run has no existing workspace to resume".to_string())
        })?;

    let running_key = RunningAgentKey::new(
        ChatContextType::Project.to_string(),
        conversation_id.as_str(),
    );
    let running_agents = state
        .running_agent_registry
        .list_by_context_type(&running_key.context_type)
        .await
        .map_err(AppError::Infrastructure)?;
    if running_agents.iter().any(|(key, _)| key == &running_key) {
        return Err(AppError::Conflict(
            "the run agent is already running".to_string(),
        ));
    }
    if launches_paused(state) {
        return Err(AppError::Conflict("agent launches are paused".to_string()));
    }

    Ok(ReopenContext {
        run,
        automation_status: automation.status,
        workspace,
        conversation_id,
    })
}

async fn reset_reopen_state(state: &AppState, context: &ReopenContext) -> AppResult<()> {
    state
        .automation_run_repo
        .clear_judge_state(&context.run.id)
        .await?;
    clear_plan_phase_publication_metadata(
        &state.automation_run_repo,
        &state.agent_conversation_workspace_repo,
        &context.run,
        &context.workspace,
    )
    .await?;
    rearm_workspace_review_monitor(state, &context.conversation_id).await?;
    state
        .automation_run_repo
        .set_plan_reminder_count(&context.run.id, 0)
        .await?;
    Ok(())
}

fn emit_reopen_events(state: &AppState, automation_id: &AutomationId, run_id: &AutomationRunId) {
    let automation_id = automation_id.as_str();
    let run_id = run_id.as_str();
    state.events.emit(
        AUTOMATION_RUN_UPDATED_EVENT,
        serde_json::json!({
            "automation_id": automation_id,
            "automationId": automation_id,
            "run_id": run_id,
            "runId": run_id,
        }),
    );
    state.events.emit(
        AUTOMATION_UPDATED_EVENT,
        serde_json::json!({"automation_id": automation_id, "automationId": automation_id}),
    );
}

fn launches_paused(state: &AppState) -> bool {
    state
        .app_handle
        .as_ref()
        .and_then(|handle| handle.try_state::<Arc<ApplicationExecutionState>>())
        .is_some_and(|execution_state| execution_state.is_paused())
}

async fn rearm_workspace_review_monitor(
    state: &AppState,
    conversation_id: &ChatConversationId,
) -> AppResult<()> {
    let Some(mut monitor) = state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(conversation_id)
        .await?
    else {
        return Ok(());
    };
    // Local Workspace Review represents terminal outcomes as `Ready`. Re-arm both terminal and
    // failed monitors, including the stale failure authority that would otherwise immediately
    // restore a `Failed` gate after setting only the status back to `Idle`.
    if !matches!(
        monitor.status,
        AgentWorkspaceReviewMonitorStatus::Blocked | AgentWorkspaceReviewMonitorStatus::Ready
    ) {
        return Ok(());
    }
    monitor.status = AgentWorkspaceReviewMonitorStatus::Idle;
    monitor.review_outcome = AgentWorkspaceReviewOutcome::None;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::NotRequired;
    monitor.clear_review_gate_bypass();
    monitor.review_blocking_summary = None;
    monitor.review_blocking_fingerprint = None;
    monitor.review_fixer_run_id = None;
    monitor.review_fixer_conversation_id = None;
    monitor.review_fixer_status = None;
    monitor.last_run_id = None;
    monitor.last_error = None;
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await?;
    Ok(())
}
