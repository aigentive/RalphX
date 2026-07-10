use std::sync::Arc;

use chrono::{DateTime, Utc};
use ralphx_domain::repositories::automation_run_repository::AutomationJudgeTransitionGuard;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::application::NotificationService;
use crate::domain::entities::{
    automation_is_transition_allowed, automation_run_is_transition_allowed,
    judge_is_transition_allowed, plan_judge_is_transition_allowed, Automation, AutomationId,
    AutomationJudgeState, AutomationPlanJudgeState, AutomationRun, AutomationRunId,
    AutomationRunStatus, AutomationStatus, NewNotification, NotificationCategory,
    NotificationSeverity, NotificationTarget, NotificationTargetKind, ProjectId,
};
use crate::domain::repositories::{AutomationRepository, AutomationRunRepository};
use crate::error::{AppError, AppResult};

const ACTIONABLE_PAUSED_REASON_CODES: [&str; 6] = [
    "judge_failed",
    "plan_judge_failed",
    "plan_revision_exhausted",
    "workspace_review_blocked",
    "max_runs_exhausted",
    "max_consecutive_failures",
];

pub const AUTOMATION_UPDATED_EVENT: &str = "automation:updated";
pub const AUTOMATION_RUN_UPDATED_EVENT: &str = "automation:run:updated";
pub const AUTOMATION_DELETED_EVENT: &str = "automation:deleted";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutomationEvent {
    AutomationUpdated {
        automation_id: AutomationId,
    },
    AutomationRunUpdated {
        automation_id: AutomationId,
        run_id: AutomationRunId,
    },
    AutomationDeleted {
        automation_id: AutomationId,
        project_id: ProjectId,
    },
}

impl AutomationEvent {
    pub fn event_name(&self) -> &'static str {
        match self {
            Self::AutomationUpdated { .. } => AUTOMATION_UPDATED_EVENT,
            Self::AutomationRunUpdated { .. } => AUTOMATION_RUN_UPDATED_EVENT,
            Self::AutomationDeleted { .. } => AUTOMATION_DELETED_EVENT,
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

#[derive(Clone)]
pub struct TauriAutomationEventEmitter {
    app_handle: AppHandle,
}

impl TauriAutomationEventEmitter {
    pub fn new(app_handle: AppHandle) -> Self {
        Self { app_handle }
    }
}

#[derive(Clone, Debug, Serialize)]
struct AutomationUpdatedPayload {
    automation_id: String,
    #[serde(rename = "automationId")]
    automation_id_camel: String,
}

#[derive(Clone, Debug, Serialize)]
struct AutomationRunUpdatedPayload {
    automation_id: String,
    #[serde(rename = "automationId")]
    automation_id_camel: String,
    run_id: String,
    #[serde(rename = "runId")]
    run_id_camel: String,
}

#[derive(Clone, Debug, Serialize)]
struct AutomationDeletedPayload {
    automation_id: String,
    #[serde(rename = "automationId")]
    automation_id_camel: String,
    project_id: String,
    #[serde(rename = "projectId")]
    project_id_camel: String,
}

impl AutomationEventEmitter for TauriAutomationEventEmitter {
    fn emit(&self, event: AutomationEvent) {
        match event {
            AutomationEvent::AutomationUpdated { automation_id } => {
                let id = automation_id.as_str().to_string();
                let payload = AutomationUpdatedPayload {
                    automation_id: id.clone(),
                    automation_id_camel: id,
                };
                let _ = self.app_handle.emit(AUTOMATION_UPDATED_EVENT, payload);
            }
            AutomationEvent::AutomationRunUpdated {
                automation_id,
                run_id,
            } => {
                let automation_id = automation_id.as_str().to_string();
                let run_id = run_id.as_str().to_string();
                let payload = AutomationRunUpdatedPayload {
                    automation_id: automation_id.clone(),
                    automation_id_camel: automation_id,
                    run_id: run_id.clone(),
                    run_id_camel: run_id,
                };
                let _ = self.app_handle.emit(AUTOMATION_RUN_UPDATED_EVENT, payload);
            }
            AutomationEvent::AutomationDeleted {
                automation_id,
                project_id,
            } => {
                let automation_id = automation_id.as_str().to_string();
                let project_id = project_id.as_str().to_string();
                let payload = AutomationDeletedPayload {
                    automation_id: automation_id.clone(),
                    automation_id_camel: automation_id,
                    project_id: project_id.clone(),
                    project_id_camel: project_id,
                };
                let _ = self.app_handle.emit(AUTOMATION_DELETED_EVENT, payload);
            }
        }
    }
}

#[derive(Clone)]
pub struct AutomationTransitionService {
    automation_repo: Arc<dyn AutomationRepository>,
    run_repo: Arc<dyn AutomationRunRepository>,
    event_emitter: Arc<dyn AutomationEventEmitter>,
    notification_service: NotificationService,
}

impl AutomationTransitionService {
    pub fn new(
        automation_repo: Arc<dyn AutomationRepository>,
        run_repo: Arc<dyn AutomationRunRepository>,
        event_emitter: Arc<dyn AutomationEventEmitter>,
        notification_service: NotificationService,
    ) -> Self {
        Self {
            automation_repo,
            run_repo,
            event_emitter,
            notification_service,
        }
    }

    fn emit_run_updated(&self, automation_id: Option<AutomationId>, run_id: &AutomationRunId) {
        if let Some(automation_id) = automation_id {
            self.event_emitter
                .emit(AutomationEvent::AutomationRunUpdated {
                    automation_id,
                    run_id: run_id.clone(),
                });
        }
    }

    async fn record_automation_status_notification(&self, id: &AutomationId, to: AutomationStatus) {
        if to != AutomationStatus::Paused {
            return;
        }
        let automation = match self.automation_repo.get_by_id(id).await {
            Ok(Some(automation)) => automation,
            Ok(None) => return,
            Err(error) => {
                tracing::warn!(automation_id = %id, error = %error, "Failed to load paused automation for notification");
                return;
            }
        };
        let Some(reason) = automation.paused_reason_code.as_deref() else {
            return;
        };
        if !ACTIONABLE_PAUSED_REASON_CODES.contains(&reason) {
            return;
        }
        self.notification_service
            .record(NewNotification {
                project_id: Some(automation.project_id.to_string()),
                category: NotificationCategory::AutomationPaused,
                severity: NotificationSeverity::Warning,
                title: "Automation paused".to_string(),
                body: Some(format!(
                    "“{}” paused: {}",
                    automation.name,
                    paused_reason_label(reason)
                )),
                target: automation_target(&automation, None),
                dedupe_key: Some(format!(
                    "automation:{}:paused:{}:{}",
                    automation.id,
                    reason,
                    automation.updated_at.to_rfc3339()
                )),
            })
            .await;
    }

    async fn post_run_status_changed(
        &self,
        id: &AutomationRunId,
        to: AutomationRunStatus,
        error_code: Option<&str>,
    ) {
        let run = match self.run_repo.get_by_id(id).await {
            Ok(Some(run)) => run,
            Ok(None) => return,
            Err(error) => {
                tracing::warn!(run_id = %id, error = %error, "Failed to load automation run after transition");
                return;
            }
        };
        self.emit_run_updated(Some(run.automation_id.clone()), id);
        let automation = match self.automation_repo.get_by_id(&run.automation_id).await {
            Ok(Some(automation)) => automation,
            Ok(None) => return,
            Err(error) => {
                tracing::warn!(automation_id = %run.automation_id, error = %error, "Failed to load automation for run notification");
                return;
            }
        };
        let notification = run_status_notification(&automation, &run, to, error_code);
        if let Some(notification) = notification {
            self.notification_service.record(notification).await;
        }
    }

    async fn emit_run_updated_after_run_change(&self, id: &AutomationRunId) {
        match self.run_repo.get_by_id(id).await {
            Ok(Some(run)) => self.emit_run_updated(Some(run.automation_id), id),
            Ok(None) => {}
            Err(error) => {
                tracing::warn!(run_id = %id, error = %error, "Failed to load automation run for update event")
            }
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
            self.record_automation_status_notification(id, to).await;
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
            self.post_run_status_changed(id, to, None).await;
        }
        Ok(changed)
    }

    pub async fn transition_run_status_with_merge_metadata(
        &self,
        id: &AutomationRunId,
        from: AutomationRunStatus,
        to: AutomationRunStatus,
        merge_commit_sha: Option<String>,
        pr_merged_at: Option<DateTime<Utc>>,
    ) -> AppResult<bool> {
        if !automation_run_is_transition_allowed(from, to) {
            return Err(AppError::InvalidTransition {
                from: from.as_str().to_string(),
                to: to.as_str().to_string(),
            });
        }

        let changed = self
            .run_repo
            .compare_and_swap_status_with_merge_metadata(
                id,
                from,
                to,
                merge_commit_sha,
                pr_merged_at,
            )
            .await?;
        if changed {
            self.post_run_status_changed(id, to, None).await;
        }
        Ok(changed)
    }

    pub async fn transition_run_status_with_agent_phase_started_at(
        &self,
        id: &AutomationRunId,
        from: AutomationRunStatus,
        to: AutomationRunStatus,
        agent_phase_started_at: DateTime<Utc>,
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
            .compare_and_swap_status_with_agent_phase_started_at(
                id,
                from,
                to,
                agent_phase_started_at,
                error_code,
                error_detail,
            )
            .await?;
        if changed {
            self.post_run_status_changed(id, to, None).await;
        }
        Ok(changed)
    }

    pub async fn transition_run_status_clearing_plan_pending_instructions(
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
            .compare_and_swap_status_clearing_plan_pending_instructions(
                id,
                from,
                to,
                error_code,
                error_detail,
            )
            .await?;
        if changed {
            self.post_run_status_changed(id, to, None).await;
        }
        Ok(changed)
    }

    pub async fn transition_judge_state(
        &self,
        id: &AutomationRunId,
        from: AutomationJudgeState,
        to: AutomationJudgeState,
        guard: AutomationJudgeTransitionGuard,
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
        if from == AutomationJudgeState::InProgress {
            let guard_allows_settle = match to {
                AutomationJudgeState::Done => {
                    matches!(guard, AutomationJudgeTransitionGuard::Settle(_))
                }
                AutomationJudgeState::Failed => matches!(
                    guard,
                    AutomationJudgeTransitionGuard::Settle(_)
                        | AutomationJudgeTransitionGuard::LegacyNullLease
                ),
                _ => true,
            };
            if !guard_allows_settle {
                return Err(AppError::Validation(
                    "judge settle transitions require the dispatch lease".to_string(),
                ));
            }
        }

        let changed = self
            .run_repo
            .compare_and_swap_judge_state(
                id,
                from,
                to,
                guard,
                judge_verdict_json,
                judge_model_id,
                judge_lease_expires_at,
                error_detail,
            )
            .await?;
        if changed {
            self.emit_run_updated_after_run_change(id).await;
        }
        Ok(changed)
    }

    pub async fn transition_plan_judge_state(
        &self,
        id: &AutomationRunId,
        from: AutomationPlanJudgeState,
        to: AutomationPlanJudgeState,
        plan_judge_verdict_json: Option<String>,
        plan_judge_lease_expires_at: Option<DateTime<Utc>>,
    ) -> AppResult<bool> {
        if !plan_judge_is_transition_allowed(from, to) {
            return Err(AppError::InvalidTransition {
                from: from.as_str().to_string(),
                to: to.as_str().to_string(),
            });
        }

        let changed = self
            .run_repo
            .compare_and_swap_plan_judge_state(
                id,
                from,
                to,
                plan_judge_verdict_json,
                plan_judge_lease_expires_at,
            )
            .await?;
        if changed {
            self.emit_run_updated_after_run_change(id).await;
        }
        Ok(changed)
    }
}

fn automation_target(automation: &Automation, run: Option<&AutomationRun>) -> NotificationTarget {
    NotificationTarget {
        kind: NotificationTargetKind::AutomationRun,
        project_id: Some(automation.project_id.to_string()),
        task_id: None,
        conversation_id: run
            .and_then(|run| run.conversation_id.as_ref())
            .map(ToString::to_string),
        setup_conversation_id: automation
            .setup_conversation_id
            .as_ref()
            .map(ToString::to_string),
        automation_id: Some(automation.id.to_string()),
        run_id: run.map(|run| run.id.to_string()),
    }
}

fn paused_reason_label(reason: &str) -> &'static str {
    match reason {
        "judge_failed" => "judge failed",
        "plan_judge_failed" => "plan judge failed",
        "plan_revision_exhausted" => "plan revision limit reached",
        "workspace_review_blocked" => "workspace review blocked",
        "max_runs_exhausted" => "maximum run count reached",
        "max_consecutive_failures" => "too many consecutive failures",
        _ => "automation needs attention",
    }
}

fn run_error_label(error_code: Option<&str>) -> &'static str {
    match error_code {
        Some("no_changes") => "No changes to publish",
        Some("publish_failed") => "Publish failed",
        Some("timeout") => "Run timed out",
        Some("agent_failed") => "Agent run failed",
        Some("plan_not_submitted") => "Plan not submitted",
        Some("plan_reminder_failed") => "Plan reminder failed",
        Some("plan_resume_failed") => "Plan resume failed",
        _ => "Run failed",
    }
}

fn run_status_notification(
    automation: &Automation,
    run: &AutomationRun,
    status: AutomationRunStatus,
    error_code: Option<&str>,
) -> Option<NewNotification> {
    let (category, severity, title, body, dedupe_key) = match status {
        AutomationRunStatus::AwaitingPlanApproval => (
            NotificationCategory::AutomationPlanApproval,
            NotificationSeverity::ActionRequired,
            "Plan approval needed",
            format!(
                "Run #{} of “{}” is waiting on plan approval",
                run.run_index, automation.name
            ),
            format!("run:{}:plan_approval", run.id),
        ),
        AutomationRunStatus::AgentFailed => {
            let error_code = error_code
                .or(run.error_code.as_deref())
                .unwrap_or("unknown");
            (
                NotificationCategory::AutomationRunFailed,
                NotificationSeverity::ActionRequired,
                "Automation run failed",
                format!(
                    "Run #{} of “{}”: {}",
                    run.run_index,
                    automation.name,
                    run_error_label(Some(error_code))
                ),
                format!("run:{}:failed:{}", run.id, error_code),
            )
        }
        AutomationRunStatus::Merged | AutomationRunStatus::Completed => (
            NotificationCategory::AutomationRunCompleted,
            NotificationSeverity::Info,
            "Automation run completed",
            format!("Run #{} of “{}” completed", run.run_index, automation.name),
            format!("run:{}:completed", run.id),
        ),
        AutomationRunStatus::PrClosed => (
            NotificationCategory::AutomationRunCompleted,
            NotificationSeverity::Warning,
            "Automation run closed",
            format!(
                "Run #{} of “{}” had its pull request closed",
                run.run_index, automation.name
            ),
            format!("run:{}:pr_closed", run.id),
        ),
        _ => return None,
    };
    Some(NewNotification {
        project_id: Some(automation.project_id.to_string()),
        category,
        severity,
        title: title.to_string(),
        body: Some(body),
        target: automation_target(automation, Some(run)),
        dedupe_key: Some(dedupe_key),
    })
}
