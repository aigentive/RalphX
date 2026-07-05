use std::sync::Arc;

use chrono::Utc;

use crate::application::automation::judge::{
    apply_updated_item_statuses, automation_judge_loop_suspected, AutomationJudgeDecision,
    AutomationJudgeVerdict,
};
use crate::application::automation::transition::{
    AutomationEvent, AutomationEventEmitter, AutomationTransitionService,
};
use crate::domain::entities::{
    is_open_automation_run, Automation, AutomationId, AutomationJudgeState, AutomationPromptAuthor,
    AutomationRun, AutomationRunId, AutomationRunStatus, AutomationStatus, ChatConversationId,
    ProjectId,
};
use crate::domain::repositories::{
    AutomationRepository, AutomationRunRepository, AutomationSettingsPatch,
};
use crate::error::{AppError, AppResult};

const DEFAULT_AUTOMATION_NAME: &str = "Untitled automation";
const DEFAULT_PROVIDER_HARNESS: &str = "claude";
const DEFAULT_MODEL_ID: &str = "sonnet";
const DEFAULT_RUN_MODE: &str = "edit";
const DEFAULT_BASE_REF_KIND: &str = "project_default";
const DEFAULT_CHAIN_MODE: &str = "merged_base";
const DEFAULT_COMPLETION_SIGNAL: &str = "pr_merged";
const DEFAULT_MAX_RUNS: i64 = 25;
const DEFAULT_MAX_CONSECUTIVE_FAILURES: i64 = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationDetail {
    pub automation: Automation,
    pub runs: Vec<AutomationRun>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateAutomationDraftInput {
    pub id: Option<AutomationId>,
    pub project_id: ProjectId,
    pub name: Option<String>,
    pub setup_conversation_id: Option<ChatConversationId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateAutomationSettingsInput {
    pub id: AutomationId,
    pub name: Option<String>,
    pub max_runs: Option<i64>,
    pub max_consecutive_failures: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateAutomationRunInput {
    pub automation_id: AutomationId,
    pub run_prompt: String,
    pub prompt_author: AutomationPromptAuthor,
    pub base_ref_kind: String,
    pub base_ref_used: String,
    pub base_from_run_id: Option<AutomationRunId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateMergedBaseSuccessorRunInput {
    pub automation_id: AutomationId,
    pub previous_run_id: AutomationRunId,
    pub run_prompt: String,
    pub prompt_author: AutomationPromptAuthor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationSuccessorRunOutcome {
    pub scheduled: bool,
    pub reason: Option<String>,
    pub run: Option<AutomationRun>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationScheduleOutcome {
    pub scheduled: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompleteAutomationJudgeInput {
    pub automation: Automation,
    pub previous_run: AutomationRun,
    pub verdict: AutomationJudgeVerdict,
    pub verdict_json: String,
    pub judge_model_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ApplyAutomationJudgeVerdictInput {
    pub automation: Automation,
    pub previous_run: AutomationRun,
    pub verdict: AutomationJudgeVerdict,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AutomationJudgeApplyOutcome {
    pub successor_run: Option<AutomationRun>,
    pub terminal_automation_status: Option<AutomationStatus>,
    pub reason: Option<String>,
}

pub struct AutomationService {
    automation_repo: Arc<dyn AutomationRepository>,
    run_repo: Arc<dyn AutomationRunRepository>,
    transition_service: AutomationTransitionService,
    event_emitter: Arc<dyn AutomationEventEmitter>,
}

impl AutomationService {
    pub fn new(
        automation_repo: Arc<dyn AutomationRepository>,
        run_repo: Arc<dyn AutomationRunRepository>,
        event_emitter: Arc<dyn AutomationEventEmitter>,
    ) -> Self {
        let transition_service = AutomationTransitionService::new(
            Arc::clone(&automation_repo),
            Arc::clone(&run_repo),
            Arc::clone(&event_emitter),
        );
        Self {
            automation_repo,
            run_repo,
            transition_service,
            event_emitter,
        }
    }

    pub async fn create_draft(&self, input: CreateAutomationDraftInput) -> AppResult<Automation> {
        let now = Utc::now();
        let automation = Automation {
            id: input.id.unwrap_or_else(AutomationId::new),
            project_id: input.project_id,
            name: normalize_name(input.name.as_deref())?,
            status: AutomationStatus::Draft,
            paused_reason_code: None,
            paused_reason_detail: None,
            goal_prompt: String::new(),
            setup_conversation_id: input.setup_conversation_id,
            provider_harness: DEFAULT_PROVIDER_HARNESS.to_string(),
            model_id: DEFAULT_MODEL_ID.to_string(),
            logical_effort: None,
            run_mode: DEFAULT_RUN_MODE.to_string(),
            base_ref_kind: DEFAULT_BASE_REF_KIND.to_string(),
            base_ref: String::new(),
            base_display_name: None,
            base_source_pull_request_json: None,
            goal_items_json: None,
            chain_mode: DEFAULT_CHAIN_MODE.to_string(),
            completion_signal: DEFAULT_COMPLETION_SIGNAL.to_string(),
            max_runs: DEFAULT_MAX_RUNS,
            max_consecutive_failures: DEFAULT_MAX_CONSECUTIVE_FAILURES,
            first_run_prompt: None,
            setup_analysis_summary: None,
            created_at: now,
            updated_at: now,
        };
        self.automation_repo.create(automation).await
    }

    pub async fn list_automations(
        &self,
        project_id: Option<ProjectId>,
    ) -> AppResult<Vec<Automation>> {
        self.automation_repo.list(project_id).await
    }

    pub async fn get_automation_detail(&self, id: &AutomationId) -> AppResult<AutomationDetail> {
        let automation = self.require_automation(id).await?;
        let runs = self.run_repo.list_for_automation(id).await?;
        Ok(AutomationDetail { automation, runs })
    }

    pub async fn update_settings(
        &self,
        input: UpdateAutomationSettingsInput,
    ) -> AppResult<Automation> {
        validate_positive("max_runs", input.max_runs)?;
        validate_positive("max_consecutive_failures", input.max_consecutive_failures)?;
        let name = match input.name.as_deref() {
            Some(value) => Some(normalize_name(Some(value))?),
            None => None,
        };
        let patch = AutomationSettingsPatch {
            name,
            max_runs: input.max_runs,
            max_consecutive_failures: input.max_consecutive_failures,
        };
        let updated = self
            .automation_repo
            .update_settings(&input.id, patch)
            .await?
            .ok_or_else(|| automation_not_found(&input.id))?;
        self.event_emitter.emit(AutomationEvent::AutomationUpdated {
            automation_id: updated.id.clone(),
        });
        Ok(updated)
    }

    pub async fn pause(
        &self,
        id: &AutomationId,
        reason_code: &str,
        reason_detail: Option<&str>,
    ) -> AppResult<Automation> {
        let automation = self.require_automation(id).await?;
        self.transition_automation_status_or_conflict(
            id,
            automation.status,
            AutomationStatus::Paused,
            Some(reason_code.to_string()),
            reason_detail.map(str::to_string),
        )
        .await
    }

    pub async fn resume(&self, id: &AutomationId) -> AppResult<Automation> {
        let automation = self.require_automation(id).await?;
        self.transition_automation_status_or_conflict(
            id,
            automation.status,
            AutomationStatus::Active,
            None,
            None,
        )
        .await
    }

    pub async fn stop(&self, id: &AutomationId) -> AppResult<Automation> {
        let automation = self.require_automation(id).await?;
        let stopped = self
            .transition_automation_status_or_conflict(
                id,
                automation.status,
                AutomationStatus::Stopped,
                None,
                None,
            )
            .await?;
        if let Some(run) = self.run_repo.latest_for_automation(id).await? {
            if run_status_is_cancellable(run.status) {
                self.transition_run_status_or_conflict(
                    &run.id,
                    run.status,
                    AutomationRunStatus::Cancelled,
                    None,
                    None,
                )
                .await?;
            }
        }
        Ok(stopped)
    }

    pub async fn trigger_run_now(&self, id: &AutomationId) -> AppResult<AutomationScheduleOutcome> {
        self.require_automation(id).await?;
        Ok(deferred_schedule_outcome(
            "automation run-now scheduling is implemented in a later scheduler phase",
        ))
    }

    pub async fn skip_judge(
        &self,
        id: &AutomationId,
        run_id: &AutomationRunId,
    ) -> AppResult<AutomationScheduleOutcome> {
        let run = self.require_run_for_automation(id, run_id).await?;
        let latest = self.latest_run_for_automation(id).await?;
        if latest.id != run.id {
            return Err(AppError::Validation(
                "runId must reference the latest automation run".to_string(),
            ));
        }
        if run.judge_state != AutomationJudgeState::None {
            return Ok(AutomationScheduleOutcome {
                scheduled: false,
                reason: Some("judge already started".to_string()),
            });
        }
        if !run_status_is_signal_terminal(run.status) {
            return Ok(AutomationScheduleOutcome {
                scheduled: false,
                reason: Some("run is not ready for judge skipping".to_string()),
            });
        }
        Ok(deferred_schedule_outcome(
            "skip-judge successor scheduling is implemented in the judge phase",
        ))
    }

    pub async fn cancel_run(
        &self,
        id: &AutomationId,
        run_id: &AutomationRunId,
    ) -> AppResult<AutomationRun> {
        let run = self.require_run_for_automation(id, run_id).await?;
        self.transition_run_status_or_conflict(
            run_id,
            run.status,
            AutomationRunStatus::Cancelled,
            None,
            None,
        )
        .await
    }

    pub async fn delete(&self, id: &AutomationId) -> AppResult<()> {
        let automation = self.require_automation(id).await?;
        if !matches!(
            automation.status,
            AutomationStatus::Completed | AutomationStatus::Stopped
        ) {
            return Err(AppError::Validation(
                "only completed or stopped automations can be deleted".to_string(),
            ));
        }
        let deleted = self.automation_repo.delete_terminal(id).await?;
        if !deleted {
            return Err(automation_status_conflict(
                id,
                automation.status,
                AutomationStatus::Stopped,
            ));
        }
        self.run_repo.delete_for_automation(id).await?;
        self.event_emitter.emit(AutomationEvent::AutomationUpdated {
            automation_id: id.clone(),
        });
        Ok(())
    }

    pub async fn finalize(&self, id: &AutomationId) -> AppResult<Automation> {
        let automation = self.require_automation(id).await?;
        validate_finalizable(&automation)?;
        self.transition_automation_status_or_conflict(
            id,
            automation.status,
            AutomationStatus::Active,
            None,
            None,
        )
        .await
    }

    pub async fn create_run(&self, input: CreateAutomationRunInput) -> AppResult<AutomationRun> {
        let automation = self.require_automation(&input.automation_id).await?;
        if automation.status != AutomationStatus::Active {
            return Err(AppError::Validation(
                "automation must be active to create a run".to_string(),
            ));
        }
        if input.run_prompt.trim().is_empty() {
            return Err(AppError::Validation(
                "automation run prompt cannot be empty".to_string(),
            ));
        }

        let run_index = self
            .run_repo
            .latest_for_automation(&input.automation_id)
            .await?
            .map_or(1, |run| run.run_index + 1);
        let now = Utc::now();
        let run = AutomationRun {
            id: AutomationRunId::new(),
            automation_id: input.automation_id,
            run_index,
            status: AutomationRunStatus::Pending,
            judge_state: AutomationJudgeState::None,
            judge_lease_expires_at: None,
            conversation_id: None,
            run_prompt: input.run_prompt,
            prompt_author: input.prompt_author,
            base_ref_kind: input.base_ref_kind,
            base_ref_used: input.base_ref_used,
            base_from_run_id: input.base_from_run_id,
            branch_name: None,
            pr_number: None,
            pr_url: None,
            pr_title: None,
            pr_head_ref_name: None,
            pr_base_ref_name: None,
            pr_merged_at: None,
            merge_commit_sha: None,
            diff_stats_json: None,
            agent_summary: None,
            judge_verdict_json: None,
            judge_model_id: None,
            error_code: None,
            error_detail: None,
            signal_check_failures: 0,
            started_at: None,
            finished_at: None,
            created_at: now,
            updated_at: now,
        };
        let created = self.run_repo.create_run(run).await?;
        self.event_emitter
            .emit(AutomationEvent::AutomationRunUpdated {
                run_id: created.id.clone(),
            });
        Ok(created)
    }

    pub async fn create_merged_base_successor_run(
        &self,
        input: CreateMergedBaseSuccessorRunInput,
    ) -> AppResult<AutomationSuccessorRunOutcome> {
        let automation = self.require_automation(&input.automation_id).await?;
        if automation.status != AutomationStatus::Active {
            return Err(AppError::Validation(
                "automation must be active to schedule a successor run".to_string(),
            ));
        }
        if automation.chain_mode != DEFAULT_CHAIN_MODE {
            return Err(AppError::Validation(format!(
                "automation chain_mode {} is not supported in merged_base scheduling",
                automation.chain_mode
            )));
        }
        if input.run_prompt.trim().is_empty() {
            return Err(AppError::Validation(
                "automation successor run prompt cannot be empty".to_string(),
            ));
        }

        let previous = self
            .require_run_for_automation(&input.automation_id, &input.previous_run_id)
            .await?;
        let latest = self.latest_run_for_automation(&input.automation_id).await?;
        if latest.id != previous.id {
            return Err(AppError::Validation(
                "previousRunId must reference the latest automation run".to_string(),
            ));
        }
        if is_open_automation_run(latest.status, latest.judge_state) {
            return Ok(successor_not_scheduled("run in flight"));
        }
        if !run_status_is_signal_terminal(latest.status) {
            return Err(AppError::Validation(
                "previous run is not signal-terminal".to_string(),
            ));
        }

        let runs = self
            .run_repo
            .list_for_automation(&input.automation_id)
            .await?;
        if runs.len() as i64 >= automation.max_runs {
            self.transition_automation_status_or_conflict(
                &automation.id,
                AutomationStatus::Active,
                AutomationStatus::Paused,
                Some("max_runs_exhausted".to_string()),
                Some("Automation reached max_runs before scheduling a successor".to_string()),
            )
            .await?;
            return Ok(successor_not_scheduled("max_runs_exhausted"));
        }
        if consecutive_failure_count(&runs) >= automation.max_consecutive_failures {
            self.transition_automation_status_or_conflict(
                &automation.id,
                AutomationStatus::Active,
                AutomationStatus::Paused,
                Some("max_consecutive_failures".to_string()),
                Some(
                    "Automation reached max_consecutive_failures before scheduling a successor"
                        .to_string(),
                ),
            )
            .await?;
            return Ok(successor_not_scheduled("max_consecutive_failures"));
        }

        let (base_ref_kind, base_ref_used) = merged_base_successor_base(&automation, &latest)?;
        let run = self
            .create_run(CreateAutomationRunInput {
                automation_id: automation.id,
                run_prompt: input.run_prompt,
                prompt_author: input.prompt_author,
                base_ref_kind,
                base_ref_used,
                base_from_run_id: Some(latest.id),
            })
            .await?;
        Ok(AutomationSuccessorRunOutcome {
            scheduled: true,
            reason: None,
            run: Some(run),
        })
    }

    pub async fn complete_judge_verdict(
        &self,
        input: CompleteAutomationJudgeInput,
    ) -> AppResult<AutomationJudgeApplyOutcome> {
        let applied_goal_items = apply_updated_item_statuses(
            input.automation.goal_items_json.as_deref(),
            input.verdict.updated_item_statuses.as_deref(),
        )?;
        let changed = self
            .transition_service
            .transition_judge_state(
                &input.previous_run.id,
                AutomationJudgeState::InProgress,
                AutomationJudgeState::Done,
                Some(input.verdict_json),
                input.judge_model_id,
                None,
                None,
            )
            .await?;
        if !changed {
            return Ok(AutomationJudgeApplyOutcome {
                successor_run: None,
                terminal_automation_status: None,
                reason: Some("judge state changed before verdict persistence".to_string()),
            });
        }

        self.apply_judge_verdict_effects(
            ApplyAutomationJudgeVerdictInput {
                automation: input.automation,
                previous_run: input.previous_run,
                verdict: input.verdict,
            },
            applied_goal_items,
        )
        .await
    }

    pub async fn apply_persisted_judge_verdict(
        &self,
        input: ApplyAutomationJudgeVerdictInput,
    ) -> AppResult<AutomationJudgeApplyOutcome> {
        let applied_goal_items = apply_updated_item_statuses(
            input.automation.goal_items_json.as_deref(),
            input.verdict.updated_item_statuses.as_deref(),
        )?;
        self.apply_judge_verdict_effects(input, applied_goal_items)
            .await
    }

    async fn require_automation(&self, id: &AutomationId) -> AppResult<Automation> {
        self.automation_repo
            .get_by_id(id)
            .await?
            .ok_or_else(|| automation_not_found(id))
    }

    async fn require_run_for_automation(
        &self,
        automation_id: &AutomationId,
        run_id: &AutomationRunId,
    ) -> AppResult<AutomationRun> {
        self.require_automation(automation_id).await?;
        let run = self
            .run_repo
            .get_by_id(run_id)
            .await?
            .ok_or_else(|| automation_run_not_found(run_id))?;
        if run.automation_id != *automation_id {
            return Err(AppError::Validation(
                "automation run is not owned by the requested automation".to_string(),
            ));
        }
        Ok(run)
    }

    async fn latest_run_for_automation(&self, id: &AutomationId) -> AppResult<AutomationRun> {
        self.run_repo
            .latest_for_automation(id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("automation {} has no runs", id.as_str())))
    }

    async fn transition_automation_status_or_conflict(
        &self,
        id: &AutomationId,
        from: AutomationStatus,
        to: AutomationStatus,
        paused_reason_code: Option<String>,
        paused_reason_detail: Option<String>,
    ) -> AppResult<Automation> {
        let changed = self
            .transition_service
            .transition_automation_status(id, from, to, paused_reason_code, paused_reason_detail)
            .await?;
        if !changed {
            return Err(automation_status_conflict(id, from, to));
        }
        self.require_automation(id).await
    }

    async fn transition_run_status_or_conflict(
        &self,
        id: &AutomationRunId,
        from: AutomationRunStatus,
        to: AutomationRunStatus,
        error_code: Option<String>,
        error_detail: Option<String>,
    ) -> AppResult<AutomationRun> {
        let changed = self
            .transition_service
            .transition_run_status(id, from, to, error_code, error_detail)
            .await?;
        if !changed {
            return Err(automation_run_status_conflict(id, from, to));
        }
        self.run_repo
            .get_by_id(id)
            .await?
            .ok_or_else(|| automation_run_not_found(id))
    }

    async fn apply_judge_verdict_effects(
        &self,
        input: ApplyAutomationJudgeVerdictInput,
        applied_goal_items: Option<String>,
    ) -> AppResult<AutomationJudgeApplyOutcome> {
        if applied_goal_items.as_deref() != input.automation.goal_items_json.as_deref() {
            self.automation_repo
                .update_goal_items_json(&input.automation.id, applied_goal_items.clone())
                .await?
                .ok_or_else(|| automation_not_found(&input.automation.id))?;
        }

        match input.verdict.decision {
            AutomationJudgeDecision::Continue => {
                if automation_judge_loop_suspected(&input.previous_run, &input.verdict) {
                    self.transition_automation_status_or_conflict(
                        &input.automation.id,
                        AutomationStatus::Active,
                        AutomationStatus::Paused,
                        Some("judge_loop_suspected".to_string()),
                        Some(
                            "Automation judge produced the same next run prompt after a non-merged run"
                                .to_string(),
                        ),
                    )
                    .await?;
                    return Ok(AutomationJudgeApplyOutcome {
                        successor_run: None,
                        terminal_automation_status: Some(AutomationStatus::Paused),
                        reason: Some("judge_loop_suspected".to_string()),
                    });
                }
                let next_prompt = input
                    .verdict
                    .next_run_prompt
                    .as_deref()
                    .unwrap_or("")
                    .trim()
                    .to_string();
                let outcome = self
                    .create_merged_base_successor_run(CreateMergedBaseSuccessorRunInput {
                        automation_id: input.automation.id,
                        previous_run_id: input.previous_run.id,
                        run_prompt: next_prompt,
                        prompt_author: AutomationPromptAuthor::Judge,
                    })
                    .await?;
                Ok(AutomationJudgeApplyOutcome {
                    successor_run: outcome.run,
                    terminal_automation_status: None,
                    reason: outcome.reason,
                })
            }
            AutomationJudgeDecision::Stop if input.verdict.goal_met => {
                self.transition_automation_status_or_conflict(
                    &input.automation.id,
                    AutomationStatus::Active,
                    AutomationStatus::Completed,
                    None,
                    None,
                )
                .await?;
                Ok(AutomationJudgeApplyOutcome {
                    successor_run: None,
                    terminal_automation_status: Some(AutomationStatus::Completed),
                    reason: None,
                })
            }
            AutomationJudgeDecision::Stop => {
                self.transition_automation_status_or_conflict(
                    &input.automation.id,
                    AutomationStatus::Active,
                    AutomationStatus::Paused,
                    Some("judge_stopped_unmet".to_string()),
                    Some(input.verdict.reason.clone()),
                )
                .await?;
                Ok(AutomationJudgeApplyOutcome {
                    successor_run: None,
                    terminal_automation_status: Some(AutomationStatus::Paused),
                    reason: Some("judge_stopped_unmet".to_string()),
                })
            }
        }
    }
}

fn deferred_schedule_outcome(reason: &str) -> AutomationScheduleOutcome {
    AutomationScheduleOutcome {
        scheduled: false,
        reason: Some(reason.to_string()),
    }
}

fn successor_not_scheduled(reason: &str) -> AutomationSuccessorRunOutcome {
    AutomationSuccessorRunOutcome {
        scheduled: false,
        reason: Some(reason.to_string()),
        run: None,
    }
}

fn run_status_is_cancellable(status: AutomationRunStatus) -> bool {
    matches!(
        status,
        AutomationRunStatus::Pending
            | AutomationRunStatus::Provisioning
            | AutomationRunStatus::Running
            | AutomationRunStatus::Published
    )
}

fn consecutive_failure_count(runs: &[AutomationRun]) -> i64 {
    let mut count = 0;
    for run in runs.iter().rev() {
        match run.status {
            AutomationRunStatus::AgentFailed | AutomationRunStatus::PrClosed => count += 1,
            AutomationRunStatus::Merged => break,
            _ => {}
        }
    }
    count
}

fn merged_base_successor_base(
    automation: &Automation,
    previous_run: &AutomationRun,
) -> AppResult<(String, String)> {
    if previous_run.run_index == 1 && automation.base_source_pull_request_json.is_some() {
        let pr_base = previous_run
            .pr_base_ref_name
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                AppError::Validation(
                    "source-PR automation successor requires previous run pr_base_ref_name"
                        .to_string(),
                )
            })?;
        return Ok(("local_branch".to_string(), pr_base.to_string()));
    }

    Ok((
        automation.base_ref_kind.clone(),
        automation.base_ref.clone(),
    ))
}

fn run_status_is_signal_terminal(status: AutomationRunStatus) -> bool {
    matches!(
        status,
        AutomationRunStatus::Merged
            | AutomationRunStatus::PrClosed
            | AutomationRunStatus::AgentFailed
    )
}

fn normalize_name(value: Option<&str>) -> AppResult<String> {
    let trimmed = value.unwrap_or(DEFAULT_AUTOMATION_NAME).trim();
    if trimmed.is_empty() {
        return Err(AppError::Validation(
            "automation name cannot be empty".to_string(),
        ));
    }
    Ok(trimmed.to_string())
}

fn validate_positive(field: &str, value: Option<i64>) -> AppResult<()> {
    if value.is_some_and(|value| value <= 0) {
        return Err(AppError::Validation(format!("{field} must be positive")));
    }
    Ok(())
}

fn validate_finalizable(automation: &Automation) -> AppResult<()> {
    if automation.status != AutomationStatus::Draft {
        return Err(AppError::InvalidTransition {
            from: automation.status.as_str().to_string(),
            to: AutomationStatus::Active.as_str().to_string(),
        });
    }
    if automation.goal_prompt.trim().is_empty() {
        return Err(AppError::Validation(
            "automation goal_prompt is required before activation".to_string(),
        ));
    }
    if automation.provider_harness.trim().is_empty() {
        return Err(AppError::Validation(
            "automation provider_harness is required before activation".to_string(),
        ));
    }
    if automation.model_id.trim().is_empty() {
        return Err(AppError::Validation(
            "automation model_id is required before activation".to_string(),
        ));
    }
    if automation
        .first_run_prompt
        .as_deref()
        .unwrap_or("")
        .trim()
        .is_empty()
    {
        return Err(AppError::Validation(
            "automation first_run_prompt is required before activation".to_string(),
        ));
    }
    if automation.completion_signal == DEFAULT_COMPLETION_SIGNAL
        && automation.run_mode != DEFAULT_RUN_MODE
    {
        return Err(AppError::Validation(
            "pr_merged automations require edit run_mode".to_string(),
        ));
    }
    match automation.base_ref_kind.as_str() {
        DEFAULT_BASE_REF_KIND => {}
        "local_branch" if !automation.base_ref.trim().is_empty() => {}
        "current_branch" => {
            return Err(AppError::Validation(
                "current_branch must be resolved before activation".to_string(),
            ))
        }
        _ => {
            return Err(AppError::Validation(
                "automation base_ref_kind/base_ref is not activation-ready".to_string(),
            ))
        }
    }
    validate_positive("max_runs", Some(automation.max_runs))?;
    validate_positive(
        "max_consecutive_failures",
        Some(automation.max_consecutive_failures),
    )?;
    Ok(())
}

fn automation_not_found(id: &AutomationId) -> AppError {
    AppError::NotFound(format!("automation {} not found", id.as_str()))
}

fn automation_run_not_found(id: &AutomationRunId) -> AppError {
    AppError::NotFound(format!("automation run {} not found", id.as_str()))
}

fn automation_status_conflict(
    id: &AutomationId,
    from: AutomationStatus,
    to: AutomationStatus,
) -> AppError {
    AppError::Conflict(format!(
        "automation {} status changed before transition {} -> {}",
        id.as_str(),
        from.as_str(),
        to.as_str()
    ))
}

fn automation_run_status_conflict(
    id: &AutomationRunId,
    from: AutomationRunStatus,
    to: AutomationRunStatus,
) -> AppError {
    AppError::Conflict(format!(
        "automation run {} status changed before transition {} -> {}",
        id.as_str(),
        from.as_str(),
        to.as_str()
    ))
}
