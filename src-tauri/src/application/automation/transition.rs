use std::sync::Arc;

use chrono::{DateTime, Utc};

use crate::domain::entities::{
    automation_is_transition_allowed, automation_run_is_transition_allowed,
    judge_is_transition_allowed, AutomationId, AutomationJudgeState, AutomationRunId,
    AutomationRunStatus, AutomationStatus,
};
use crate::domain::repositories::{AutomationRepository, AutomationRunRepository};
use crate::error::{AppError, AppResult};

pub const AUTOMATION_UPDATED_EVENT: &str = "automation:updated";
pub const AUTOMATION_RUN_UPDATED_EVENT: &str = "automation:run:updated";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutomationEvent {
    AutomationUpdated { automation_id: AutomationId },
    AutomationRunUpdated { run_id: AutomationRunId },
}

impl AutomationEvent {
    pub fn event_name(&self) -> &'static str {
        match self {
            Self::AutomationUpdated { .. } => AUTOMATION_UPDATED_EVENT,
            Self::AutomationRunUpdated { .. } => AUTOMATION_RUN_UPDATED_EVENT,
        }
    }
}

pub trait AutomationEventEmitter: Send + Sync {
    fn emit(&self, event: AutomationEvent);
}

#[derive(Default)]
pub struct NoopAutomationEventEmitter;

impl AutomationEventEmitter for NoopAutomationEventEmitter {
    fn emit(&self, _event: AutomationEvent) {}
}

pub struct AutomationTransitionService {
    automation_repo: Arc<dyn AutomationRepository>,
    run_repo: Arc<dyn AutomationRunRepository>,
    event_emitter: Arc<dyn AutomationEventEmitter>,
}

impl AutomationTransitionService {
    pub fn new(
        automation_repo: Arc<dyn AutomationRepository>,
        run_repo: Arc<dyn AutomationRunRepository>,
        event_emitter: Arc<dyn AutomationEventEmitter>,
    ) -> Self {
        Self {
            automation_repo,
            run_repo,
            event_emitter,
        }
    }

    pub async fn transition_automation_status(
        &self,
        id: &AutomationId,
        from: AutomationStatus,
        to: AutomationStatus,
        paused_reason_code: Option<String>,
        paused_reason_detail: Option<String>,
    ) -> AppResult<bool> {
        if !automation_is_transition_allowed(from, to) {
            return Err(AppError::InvalidTransition {
                from: from.as_str().to_string(),
                to: to.as_str().to_string(),
            });
        }

        let changed = self
            .automation_repo
            .compare_and_swap_status(id, from, to, paused_reason_code, paused_reason_detail)
            .await?;
        if changed {
            self.event_emitter.emit(AutomationEvent::AutomationUpdated {
                automation_id: id.clone(),
            });
        }
        Ok(changed)
    }

    pub async fn transition_run_status(
        &self,
        id: &AutomationRunId,
        from: AutomationRunStatus,
        to: AutomationRunStatus,
        error_code: Option<String>,
        error_detail: Option<String>,
    ) -> AppResult<bool> {
        if !automation_run_is_transition_allowed(from, to) {
            return Err(AppError::InvalidTransition {
                from: from.as_str().to_string(),
                to: to.as_str().to_string(),
            });
        }

        let changed = self
            .run_repo
            .compare_and_swap_status(id, from, to, error_code, error_detail)
            .await?;
        if changed {
            self.event_emitter
                .emit(AutomationEvent::AutomationRunUpdated { run_id: id.clone() });
        }
        Ok(changed)
    }

    pub async fn transition_judge_state(
        &self,
        id: &AutomationRunId,
        from: AutomationJudgeState,
        to: AutomationJudgeState,
        judge_verdict_json: Option<String>,
        judge_model_id: Option<String>,
        judge_lease_expires_at: Option<DateTime<Utc>>,
        error_detail: Option<String>,
    ) -> AppResult<bool> {
        if !judge_is_transition_allowed(from, to) {
            return Err(AppError::InvalidTransition {
                from: from.as_str().to_string(),
                to: to.as_str().to_string(),
            });
        }

        let changed = self
            .run_repo
            .compare_and_swap_judge_state(
                id,
                from,
                to,
                judge_verdict_json,
                judge_model_id,
                judge_lease_expires_at,
                error_detail,
            )
            .await?;
        if changed {
            self.event_emitter
                .emit(AutomationEvent::AutomationRunUpdated { run_id: id.clone() });
        }
        Ok(changed)
    }
}
