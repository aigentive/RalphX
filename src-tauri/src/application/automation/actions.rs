use std::sync::Arc;

use chrono::{DateTime, Utc};
use ralphx_domain::repositories::automation_run_repository::AutomationJudgeTransitionGuard;

use crate::application::automation::api::{
    automation_service_for_state, automation_transition_service_for_state,
};
use crate::application::automation::plan_gate::current_plan_artifact_id_for_workspace;
use crate::application::automation::scheduler::{
    automation_judge_lease_expires_at, spawn_automation_judge_task, AutomationSchedulerConfig,
    HarnessAutomationJudgeInvoker,
};
use crate::application::automation::service::{
    AutomationRunNowAction, AutomationScheduleOutcome, AutomationService,
};
use crate::application::automation::transition::AutomationTransitionService;
use crate::application::AppState;
use crate::domain::entities::{Automation, AutomationId, AutomationJudgeState, AutomationRun};
use crate::error::{AppError, AppResult};
use crate::infrastructure::agents::claude::automations_config;

pub(crate) struct AutomationJudgeStartDispatch {
    pub service: AutomationService,
    pub transition_service: AutomationTransitionService,
    pub config: AutomationSchedulerConfig,
    pub automation: Automation,
    pub runs: Vec<AutomationRun>,
    pub run: AutomationRun,
    pub judge_lease_expires_at: DateTime<Utc>,
}

pub async fn trigger_automation_run_now_for_state(
    id: &AutomationId,
    state: &AppState,
) -> AppResult<AutomationScheduleOutcome> {
    let service = automation_service_for_state(state);
    let action = service.trigger_run_now_action(id).await?;
    dispatch_automation_run_now_action(id, state, service, action).await
}

pub async fn retry_automation_judge_for_state(
    id: &AutomationId,
    state: &AppState,
) -> AppResult<AutomationScheduleOutcome> {
    let service = automation_service_for_state(state);
    let action = service.retry_judge_action(id).await?;
    dispatch_automation_run_now_action(id, state, service, action).await
}

async fn dispatch_automation_run_now_action(
    id: &AutomationId,
    state: &AppState,
    service: AutomationService,
    action: AutomationRunNowAction,
) -> AppResult<AutomationScheduleOutcome> {
    dispatch_automation_run_now_action_with_spawner(id, state, service, action, |dispatch| {
        spawn_automation_judge_task(
            dispatch.service,
            dispatch.transition_service,
            Arc::new(HarnessAutomationJudgeInvoker::new(state.clone())),
            dispatch.config,
            dispatch.automation,
            dispatch.runs,
            dispatch.run,
            dispatch.judge_lease_expires_at,
        );
    })
    .await
}

pub(crate) async fn dispatch_automation_run_now_action_with_spawner<F>(
    id: &AutomationId,
    state: &AppState,
    service: AutomationService,
    action: AutomationRunNowAction,
    spawn_judge: F,
) -> AppResult<AutomationScheduleOutcome>
where
    F: FnOnce(AutomationJudgeStartDispatch),
{
    match action {
        AutomationRunNowAction::Outcome(outcome) => Ok(outcome),
        AutomationRunNowAction::StartJudge {
            automation,
            runs,
            run,
        } => {
            let automation = *automation;
            let run = *run;
            let config = AutomationSchedulerConfig::from_runtime(automations_config());
            let judge_lease_expires_at = automation_judge_lease_expires_at(config.judge_timeout);
            let transition_service = automation_transition_service_for_state(state);
            let changed = transition_service
                .transition_judge_state(
                    &run.id,
                    run.judge_state,
                    AutomationJudgeState::InProgress,
                    AutomationJudgeTransitionGuard::Dispatch,
                    None,
                    None,
                    Some(judge_lease_expires_at),
                    None,
                )
                .await?;
            if !changed {
                tracing::warn!(
                    automation_id = %id,
                    run_id = %run.id,
                    from_judge_state = run.judge_state.as_str(),
                    "Discarded Run Now judge start because judge state changed"
                );
                return Ok(AutomationScheduleOutcome {
                    scheduled: false,
                    reason: Some("run in flight".to_string()),
                });
            }
            spawn_judge(AutomationJudgeStartDispatch {
                service,
                transition_service,
                config,
                automation,
                runs,
                run,
                judge_lease_expires_at,
            });
            Ok(AutomationScheduleOutcome {
                scheduled: true,
                reason: None,
            })
        }
    }
}

pub async fn retry_automation_plan_judge_for_state(
    id: &AutomationId,
    state: &AppState,
) -> AppResult<AutomationScheduleOutcome> {
    let service = automation_service_for_state(state);
    let detail = service.get_automation_detail(id).await?;
    let run = detail
        .runs
        .last()
        .ok_or_else(|| AppError::Validation("automation has no runs".to_string()))?;
    let conversation_id = run.conversation_id.as_ref().ok_or_else(|| {
        AppError::Validation("latest automation run has no plan conversation".to_string())
    })?;
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(conversation_id)
        .await?
        .ok_or_else(|| {
            AppError::NotFound(format!(
                "Automation plan workspace not found: {conversation_id}"
            ))
        })?;
    let artifact_id =
        current_plan_artifact_id_for_workspace(&state.ideation_session_repo, &workspace)
            .await?
            .ok_or_else(|| {
                AppError::Validation("current automation plan artifact is unavailable".to_string())
            })?;
    service.retry_plan_judge(id, &artifact_id).await
}
