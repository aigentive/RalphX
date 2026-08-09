use crate::application::app_state::ApplicationExecutionState;
use crate::application::automation::api::automation_service_for_state;
use crate::application::automation::pause_recovery::is_actionable_paused_reason;
use crate::application::automation::reopen::{
    reopen_automation_run, reopen_automation_run_with_redriver, AutomationRunRedriver,
};
use crate::application::AppState;
use crate::domain::entities::{
    Automation, AutomationId, AutomationRun, AutomationRunId, AutomationRunStatus, AutomationStatus,
};
use crate::error::{AppError, AppResult};

/// Resume an automation using the latest durable run state.
///
/// Reopenable failed runs take precedence over creating a retry so their existing conversation,
/// worktree, and partial work remain authoritative.
///
/// # Errors
///
/// Returns a typed, user-facing error when the automation cannot be loaded or resumed safely.
pub(crate) async fn resume_automation_smart(
    state: &AppState,
    execution_state: &std::sync::Arc<ApplicationExecutionState>,
    automation_id: &AutomationId,
) -> AppResult<Automation> {
    resume_automation_smart_with_reopener(
        state,
        execution_state,
        automation_id,
        SmartResumeReopener::Production,
    )
    .await
}

// Test-only injection seam: exercised by resume_orchestrator_tests, so it reads as
// dead code in the `--no-default-features --lib` clippy lane (no test cfg).
#[allow(dead_code)]
pub(crate) async fn resume_automation_smart_with_redriver(
    state: &AppState,
    execution_state: &std::sync::Arc<ApplicationExecutionState>,
    automation_id: &AutomationId,
    redriver: &dyn AutomationRunRedriver,
) -> AppResult<Automation> {
    resume_automation_smart_with_reopener(
        state,
        execution_state,
        automation_id,
        SmartResumeReopener::Redriver(redriver),
    )
    .await
}

enum SmartResumeReopener<'a> {
    Production,
    // Constructed only by the test-only `resume_automation_smart_with_redriver` seam above.
    #[allow(dead_code)]
    Redriver(&'a dyn AutomationRunRedriver),
}

enum SmartResumeDecision {
    Reopen(AutomationRunId),
    Retry(String),
    Reject {
        reason: &'static str,
        error: AppError,
    },
}

async fn resume_automation_smart_with_reopener(
    state: &AppState,
    execution_state: &std::sync::Arc<ApplicationExecutionState>,
    automation_id: &AutomationId,
    reopener: SmartResumeReopener<'_>,
) -> AppResult<Automation> {
    let (automation, runs) = load_smart_resume_state(state, automation_id).await?;
    match select_smart_resume(&automation, &runs) {
        SmartResumeDecision::Reopen(run_id) => {
            tracing::info!(
                automation_id = %automation_id,
                run_id = %run_id,
                decision = "reopen_latest_failed_run",
                reason = "latest_failed_run_has_conversation",
                "Smart automation resume decision"
            );
            let result = match reopener {
                SmartResumeReopener::Production => {
                    reopen_automation_run(state, execution_state, automation_id, &run_id).await
                }
                SmartResumeReopener::Redriver(redriver) => {
                    reopen_automation_run_with_redriver(
                        state,
                        execution_state,
                        automation_id,
                        &run_id,
                        redriver,
                    )
                    .await
                }
            };
            result.map_err(with_resume_context)?;
            load_resumed_automation(state, automation_id).await
        }
        SmartResumeDecision::Retry(reason) => {
            tracing::info!(
                automation_id = %automation_id,
                decision = "spawn_retry",
                reason,
                "Smart automation resume decision"
            );
            automation_service_for_state(state)
                .resume(automation_id)
                .await
                .map_err(with_resume_context)
        }
        SmartResumeDecision::Reject { reason, error } => {
            tracing::info!(
                automation_id = %automation_id,
                decision = "reject",
                reason,
                "Smart automation resume decision"
            );
            Err(error)
        }
    }
}

async fn load_smart_resume_state(
    state: &AppState,
    automation_id: &AutomationId,
) -> AppResult<(Automation, Vec<AutomationRun>)> {
    let automation = match state.automation_repo.get_by_id(automation_id).await {
        Ok(Some(automation)) => automation,
        Ok(None) => {
            tracing::info!(
                automation_id = %automation_id,
                decision = "reject",
                reason = "automation_not_found",
                "Smart automation resume decision"
            );
            return Err(AppError::NotFound(format!(
                "cannot resume automation {automation_id}: automation was not found"
            )));
        }
        Err(error) => {
            tracing::info!(
                automation_id = %automation_id,
                decision = "reject",
                reason = "automation_load_failed",
                "Smart automation resume decision"
            );
            return Err(with_resume_context(error));
        }
    };
    let runs = match state
        .automation_run_repo
        .list_for_automation(automation_id)
        .await
    {
        Ok(runs) => runs,
        Err(error) => {
            tracing::info!(
                automation_id = %automation_id,
                decision = "reject",
                reason = "run_history_load_failed",
                "Smart automation resume decision"
            );
            return Err(with_resume_context(error));
        }
    };
    Ok((automation, runs))
}

fn select_smart_resume(automation: &Automation, runs: &[AutomationRun]) -> SmartResumeDecision {
    if let Some(latest) = runs.last().filter(|run| {
        matches!(
            automation.status,
            AutomationStatus::Active | AutomationStatus::Paused | AutomationStatus::Stopped
        ) && run.status == AutomationRunStatus::AgentFailed
            && run.conversation_id.is_some()
    }) {
        return SmartResumeDecision::Reopen(latest.id.clone());
    }

    match (
        automation.status,
        automation.paused_reason_code.as_deref(),
    ) {
        (AutomationStatus::Paused, Some(reason)) if is_smart_resume_paused_reason(reason) => {
            SmartResumeDecision::Retry(reason.to_string())
        }
        (AutomationStatus::Active, _) => SmartResumeDecision::Reject {
            reason: "already_active",
            error: AppError::Conflict("automation is already active".to_string()),
        },
        (AutomationStatus::Paused, _) => SmartResumeDecision::Reject {
            reason: "paused_reason_not_actionable",
            error: AppError::Conflict(
                "automation is paused for a reason that cannot be resumed automatically"
                    .to_string(),
            ),
        },
        (AutomationStatus::Draft, _) => SmartResumeDecision::Reject {
            reason: "draft",
            error: AppError::Conflict(
                "automation is still a draft and cannot be resumed".to_string(),
            ),
        },
        (AutomationStatus::Completed, _) => SmartResumeDecision::Reject {
            reason: "completed",
            error: AppError::Conflict(
                "automation is completed and has no work to resume".to_string(),
            ),
        },
        (AutomationStatus::Stopped, _) => SmartResumeDecision::Reject {
            reason: "stopped_without_reopenable_run",
            error: AppError::Conflict(
                "automation is stopped and has no failed run that can be reopened; restart it to create a new run"
                    .to_string(),
            ),
        },
    }
}

fn is_smart_resume_paused_reason(reason: &str) -> bool {
    is_actionable_paused_reason(reason)
        || matches!(reason, "user" | "user_paused" | "setup_agent_paused")
}

async fn load_resumed_automation(
    state: &AppState,
    automation_id: &AutomationId,
) -> AppResult<Automation> {
    state
        .automation_repo
        .get_by_id(automation_id)
        .await
        .map_err(with_resume_context)?
        .ok_or_else(|| {
            AppError::NotFound(format!(
                "cannot return resumed automation {automation_id}: automation disappeared"
            ))
        })
}

fn with_resume_context(error: AppError) -> AppError {
    let context = |detail: String| format!("Cannot resume automation: {detail}");
    match error {
        AppError::Database(detail) => AppError::Database(context(detail)),
        AppError::Validation(detail) => AppError::Validation(context(detail)),
        AppError::NotFound(detail) => AppError::NotFound(context(detail)),
        AppError::Infrastructure(detail) => AppError::Infrastructure(context(detail)),
        AppError::Conflict(detail) => AppError::Conflict(context(detail)),
        AppError::Agent(detail) => AppError::Agent(context(detail)),
        AppError::ExecutionBlocked(detail) => AppError::ExecutionBlocked(context(detail)),
        AppError::GitOperation(detail) => AppError::GitOperation(context(detail)),
        AppError::GitAuth(detail) => AppError::GitAuth(context(detail)),
        AppError::InvalidTransition { from, to } => AppError::Conflict(context(format!(
            "automation state changed from {from} before it could transition to {to}"
        ))),
        other => AppError::Infrastructure(context(other.to_string())),
    }
}
