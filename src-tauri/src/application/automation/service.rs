use std::sync::Arc;

use chrono::Utc;
use serde_json::Value;

use crate::application::automation::judge::{
    apply_updated_item_statuses, automation_judge_loop_suspected, parse_automation_judge_verdict,
    AutomationJudgeDecision, AutomationJudgeNextBaseBranch, AutomationJudgeValidationContext,
    AutomationJudgeVerdict,
};
use crate::application::automation::plan_gate::{
    is_plan_gate_pause_reason, AUTOMATION_PLAN_GATE_TRIGGER_RUN_NOW_ERROR_CODE,
};
use crate::application::automation::transition::{
    AutomationEvent, AutomationEventEmitter, AutomationTransitionService,
};
use crate::domain::entities::{
    is_open_automation_run, Artifact, ArtifactBucketId, ArtifactContent, ArtifactId,
    ArtifactMetadata, ArtifactType, Automation, AutomationId, AutomationJudgeState,
    AutomationPlanApprovalMode, AutomationPlanJudgeState, AutomationPrMergeMode,
    AutomationPromptAuthor, AutomationRun, AutomationRunId, AutomationRunStatus, AutomationStatus,
    ChatConversationId, ProjectId,
};
use crate::domain::repositories::{
    ArtifactRepository, AutomationConfigPatch, AutomationRepository, AutomationRunRepository,
    AutomationSettingsPatch,
};
use crate::error::{AppError, AppResult};

const DEFAULT_AUTOMATION_NAME: &str = "Automation setup";
const DEFAULT_PROVIDER_HARNESS: &str = "claude";
const DEFAULT_MODEL_ID: &str = "sonnet";
const DEFAULT_RUN_MODE: &str = "edit";
const DEFAULT_BASE_REF_KIND: &str = "project_default";
const DEFAULT_CHAIN_MODE: &str = "merged_base";
const STACKED_CHAIN_MODE: &str = "pr_head_stacked";
const DEFAULT_COMPLETION_SIGNAL: &str = "pr_merged";
const AGENT_COMPLETED_COMPLETION_SIGNAL: &str = "agent_completed";
const DEFAULT_MAX_RUNS: i64 = 25;
const DEFAULT_MAX_CONSECUTIVE_FAILURES: i64 = 3;
const SPEC_ARTIFACT_BUCKET: &str = "prd-library";
const SPEC_ARTIFACT_CREATED_BY: &str = "automation-setup";
pub const AUTOMATION_STACKED_AUTO_MERGE_ERROR_CODE: &str =
    "automation_stacked_auto_merge_unsupported";

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
    pub base_ref_kind: Option<String>,
    pub base_ref: Option<String>,
    pub base_display_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateAutomationSettingsInput {
    pub id: AutomationId,
    pub name: Option<String>,
    pub max_runs: Option<i64>,
    pub max_consecutive_failures: Option<i64>,
    pub plan_approval_mode: Option<AutomationPlanApprovalMode>,
    pub pr_merge_mode: Option<AutomationPrMergeMode>,
    pub plan_deep_verification: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateAutomationConfigInput {
    pub id: AutomationId,
    pub goal_prompt: Option<String>,
    pub first_run_prompt: Option<String>,
    pub provider_harness: Option<String>,
    pub model_id: Option<String>,
    pub logical_effort: Option<String>,
    pub run_mode: Option<String>,
    pub base_ref_kind: Option<String>,
    pub base_ref: Option<String>,
    pub base_display_name: Option<String>,
    pub goal_items_json: Option<String>,
    pub chain_mode: Option<String>,
    pub completion_signal: Option<String>,
    pub setup_analysis_summary: Option<String>,
    pub spec_artifact_id: Option<String>,
    pub spec_content: Option<String>,
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
pub enum AutomationRunNowAction {
    Outcome(AutomationScheduleOutcome),
    StartJudge {
        automation: Box<Automation>,
        runs: Vec<AutomationRun>,
        run: Box<AutomationRun>,
    },
}

impl AutomationRunNowAction {
    pub fn into_schedule_outcome(self) -> AutomationScheduleOutcome {
        match self {
            Self::Outcome(outcome) => outcome,
            Self::StartJudge { .. } => AutomationScheduleOutcome {
                scheduled: false,
                reason: Some("judge dispatcher required".to_string()),
            },
        }
    }
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

#[derive(Clone)]
pub struct AutomationService {
    automation_repo: Arc<dyn AutomationRepository>,
    run_repo: Arc<dyn AutomationRunRepository>,
    transition_service: AutomationTransitionService,
    event_emitter: Arc<dyn AutomationEventEmitter>,
    artifact_repo: Arc<dyn ArtifactRepository>,
}

impl AutomationService {
    pub fn new(
        automation_repo: Arc<dyn AutomationRepository>,
        run_repo: Arc<dyn AutomationRunRepository>,
        event_emitter: Arc<dyn AutomationEventEmitter>,
        artifact_repo: Arc<dyn ArtifactRepository>,
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
            artifact_repo,
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
            base_ref_kind: input
                .base_ref_kind
                .unwrap_or_else(|| DEFAULT_BASE_REF_KIND.to_string()),
            base_ref: input.base_ref.unwrap_or_default(),
            base_display_name: input.base_display_name,
            base_source_pull_request_json: None,
            goal_items_json: None,
            chain_mode: DEFAULT_CHAIN_MODE.to_string(),
            completion_signal: DEFAULT_COMPLETION_SIGNAL.to_string(),
            plan_approval_mode: AutomationPlanApprovalMode::Manual,
            pr_merge_mode: AutomationPrMergeMode::Manual,
            plan_deep_verification: false,
            max_runs: DEFAULT_MAX_RUNS,
            max_consecutive_failures: DEFAULT_MAX_CONSECUTIVE_FAILURES,
            first_run_prompt: None,
            setup_analysis_summary: None,
            spec_artifact_id: None,
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
        let automation = self.require_automation(&input.id).await?;
        if let Some(pr_merge_mode) = input.pr_merge_mode {
            validate_stacked_chain_merge_mode(automation.chain_mode.as_str(), pr_merge_mode)?;
        }
        let name = match input.name.as_deref() {
            Some(value) => Some(normalize_name(Some(value))?),
            None => None,
        };
        let patch = AutomationSettingsPatch {
            name,
            max_runs: input.max_runs,
            max_consecutive_failures: input.max_consecutive_failures,
            plan_approval_mode: input.plan_approval_mode,
            pr_merge_mode: input.pr_merge_mode,
            plan_deep_verification: input.plan_deep_verification,
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

    /// Persist the setup-agent config patch (goal/prompt/provider/model/base).
    ///
    /// This is a configuration write, NOT a status change, so it never routes
    /// through `AutomationTransitionService`. It is only permitted while the
    /// automation is still editable (`Draft` or `Paused`); `Active`,
    /// `Completed`, and `Stopped` automations reject the write.
    pub async fn update_config(&self, input: UpdateAutomationConfigInput) -> AppResult<Automation> {
        let automation = self.require_automation(&input.id).await?;
        if !matches!(
            automation.status,
            AutomationStatus::Draft | AutomationStatus::Paused
        ) {
            return Err(AppError::Validation(format!(
                "automation config can only be updated while draft or paused, not {}",
                automation.status.as_str()
            )));
        }
        let completion_signal = input.completion_signal.or_else(|| {
            input
                .run_mode
                .as_deref()
                .map(completion_signal_for_run_mode)
                .map(str::to_string)
        });
        if let Some(chain_mode) = input.chain_mode.as_deref() {
            validate_stacked_chain_merge_mode(chain_mode, automation.pr_merge_mode)?;
        }
        let spec_artifact_id = self
            .resolve_spec_artifact_id(&automation, input.spec_content, input.spec_artifact_id)
            .await?;
        let patch = AutomationConfigPatch {
            goal_prompt: input.goal_prompt,
            first_run_prompt: input.first_run_prompt,
            provider_harness: input.provider_harness,
            model_id: input.model_id,
            logical_effort: input.logical_effort,
            run_mode: input.run_mode,
            base_ref_kind: input.base_ref_kind,
            base_ref: input.base_ref,
            base_display_name: input.base_display_name,
            goal_items_json: input.goal_items_json,
            chain_mode: input.chain_mode,
            completion_signal,
            setup_analysis_summary: input.setup_analysis_summary,
            spec_artifact_id,
        };
        let updated = self
            .automation_repo
            .update_config(&input.id, patch)
            .await?
            .ok_or_else(|| automation_not_found(&input.id))?;
        self.event_emitter.emit(AutomationEvent::AutomationUpdated {
            automation_id: updated.id.clone(),
        });
        Ok(updated)
    }

    /// Resolve the `spec_artifact_id` patch value for a config write.
    ///
    /// - `spec_content` present and non-empty: materialize a new `Specification`
    ///   artifact (versioned off the current spec if one exists) and link its id.
    /// - Otherwise, if `spec_artifact_id` is present and non-empty: validate the
    ///   artifact exists (fail closed) and pass it through.
    /// - Otherwise: `None`, leaving the existing linkage untouched (COALESCE).
    async fn resolve_spec_artifact_id(
        &self,
        automation: &Automation,
        spec_content: Option<String>,
        spec_artifact_id: Option<String>,
    ) -> AppResult<Option<String>> {
        if let Some(content) = spec_content
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let new_id = self.materialize_spec_artifact(automation, content).await?;
            return Ok(Some(new_id));
        }

        if let Some(existing_id) = spec_artifact_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let artifact_id = ArtifactId::from_string(existing_id.to_string());
            if self.artifact_repo.get_by_id(&artifact_id).await?.is_none() {
                return Err(AppError::Validation(format!(
                    "spec_artifact_id {existing_id} does not reference an existing artifact"
                )));
            }
            return Ok(Some(existing_id.to_string()));
        }

        Ok(None)
    }

    /// Persist automation spec markdown as a `Specification` artifact, chaining a
    /// new version off the current spec artifact if one is already linked.
    async fn materialize_spec_artifact(
        &self,
        automation: &Automation,
        content: &str,
    ) -> AppResult<String> {
        let previous_version_id = automation
            .spec_artifact_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| ArtifactId::from_string(value.to_string()));

        let next_version = match previous_version_id.as_ref() {
            Some(previous) => self
                .artifact_repo
                .get_by_id(previous)
                .await?
                .map_or(1, |artifact| artifact.metadata.version.saturating_add(1)),
            None => 1,
        };

        let artifact = Artifact {
            id: ArtifactId::new(),
            artifact_type: ArtifactType::Specification,
            name: format!("{} spec", automation.name),
            content: ArtifactContent::inline(content),
            metadata: ArtifactMetadata::new(SPEC_ARTIFACT_CREATED_BY).with_version(next_version),
            derived_from: vec![],
            bucket_id: Some(ArtifactBucketId::from_string(
                SPEC_ARTIFACT_BUCKET.to_string(),
            )),
            archived_at: None,
        };

        let created = match previous_version_id {
            Some(previous) => {
                self.artifact_repo
                    .create_with_previous_version(artifact, previous)
                    .await?
            }
            None => self.artifact_repo.create(artifact).await?,
        };
        Ok(created.id.as_str().to_string())
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
        self.trigger_run_now_action(id)
            .await
            .map(AutomationRunNowAction::into_schedule_outcome)
    }

    pub async fn trigger_run_now_action(
        &self,
        id: &AutomationId,
    ) -> AppResult<AutomationRunNowAction> {
        let mut automation = self.require_automation(id).await?;
        match automation.status {
            AutomationStatus::Active => {}
            AutomationStatus::Paused => {
                if is_plan_gate_pause_reason(automation.paused_reason_code.as_deref()) {
                    return Err(AppError::Validation(format!(
                        "{AUTOMATION_PLAN_GATE_TRIGGER_RUN_NOW_ERROR_CODE} This automation is paused at the plan gate. Review the run plan and approve it from the plan artifact pane."
                    )));
                }
                automation = self
                    .transition_automation_status_or_conflict(
                        id,
                        AutomationStatus::Paused,
                        AutomationStatus::Active,
                        None,
                        None,
                    )
                    .await?;
            }
            _ => {
                return Ok(AutomationRunNowAction::Outcome(schedule_not_scheduled(
                    "automation is not active",
                )))
            }
        }

        let runs = self.run_repo.list_for_automation(id).await?;
        let Some(latest) = runs.last().cloned() else {
            return Ok(AutomationRunNowAction::Outcome(schedule_not_scheduled(
                "automation has no runs",
            )));
        };

        if run_status_blocks_trigger_run_now(latest.status)
            || latest.judge_state == AutomationJudgeState::InProgress
        {
            return Ok(AutomationRunNowAction::Outcome(schedule_not_scheduled(
                "run in flight",
            )));
        }

        if !run_status_is_signal_terminal(latest.status) {
            return Ok(AutomationRunNowAction::Outcome(schedule_not_scheduled(
                "latest run is not ready",
            )));
        }

        match latest.judge_state {
            AutomationJudgeState::None | AutomationJudgeState::Failed => {
                Ok(AutomationRunNowAction::StartJudge {
                    automation: Box::new(automation),
                    runs,
                    run: Box::new(latest),
                })
            }
            AutomationJudgeState::Done => {
                let Some(verdict_json) = latest.judge_verdict_json.as_deref() else {
                    return Ok(AutomationRunNowAction::Outcome(schedule_not_scheduled(
                        "judge verdict is missing",
                    )));
                };
                let verdict = parse_automation_judge_verdict(
                    verdict_json,
                    AutomationJudgeValidationContext {
                        automation: &automation,
                        previous_run: &latest,
                    },
                )?;
                let outcome = self
                    .apply_persisted_judge_verdict(ApplyAutomationJudgeVerdictInput {
                        automation,
                        previous_run: latest,
                        verdict,
                    })
                    .await?;
                Ok(AutomationRunNowAction::Outcome(schedule_from_judge_apply(
                    outcome,
                )))
            }
            AutomationJudgeState::Skipped => Ok(AutomationRunNowAction::Outcome(
                schedule_not_scheduled("judge already skipped"),
            )),
            AutomationJudgeState::InProgress => Ok(AutomationRunNowAction::Outcome(
                schedule_not_scheduled("run in flight"),
            )),
        }
    }

    pub async fn skip_judge(
        &self,
        id: &AutomationId,
        run_id: &AutomationRunId,
    ) -> AppResult<AutomationScheduleOutcome> {
        let automation = self.require_automation(id).await?;
        if automation.status != AutomationStatus::Active {
            return Err(AppError::Validation(
                "automation must be active to skip judge".to_string(),
            ));
        }
        if automation.chain_mode != DEFAULT_CHAIN_MODE {
            return Err(AppError::Validation(format!(
                "automation chain_mode {} is not supported in skip-judge scheduling",
                automation.chain_mode
            )));
        }
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
        match self.successor_readiness(&automation, &latest, true).await? {
            SuccessorReadiness::Ready => {
                let (base_ref_kind, base_ref_used) =
                    merged_base_successor_base(&automation, &latest)?;
                let successor = pending_successor_run(
                    automation.id.clone(),
                    &latest,
                    latest.run_index + 1,
                    skip_judge_template_prompt(&latest),
                    AutomationPromptAuthor::SkipJudgeTemplate,
                    base_ref_kind,
                    base_ref_used,
                );
                match self
                    .run_repo
                    .skip_judge_and_create_successor_run(&automation.id, &latest.id, successor)
                    .await?
                {
                    Some(run) => {
                        self.event_emitter
                            .emit(AutomationEvent::AutomationRunUpdated { run_id: latest.id });
                        self.event_emitter
                            .emit(AutomationEvent::AutomationRunUpdated { run_id: run.id });
                        Ok(AutomationScheduleOutcome {
                            scheduled: true,
                            reason: None,
                        })
                    }
                    None => Ok(schedule_not_scheduled("judge already started")),
                }
            }
            SuccessorReadiness::NotScheduled(outcome) => {
                let outcome = *outcome;
                Ok(AutomationScheduleOutcome {
                    scheduled: false,
                    reason: outcome.reason,
                })
            }
        }
    }

    pub async fn cancel_run(
        &self,
        id: &AutomationId,
        run_id: &AutomationRunId,
    ) -> AppResult<AutomationRun> {
        let run = self.require_run_for_automation(id, run_id).await?;
        let cancelled = self
            .transition_run_status_or_conflict(
                run_id,
                run.status,
                AutomationRunStatus::Cancelled,
                None,
                None,
            )
            .await?;
        self.run_repo.clear_plan_judge_state(run_id).await?;
        Ok(cancelled)
    }

    /// Row-deletion core for automation deletion.
    ///
    /// This is the terminal step of the `delete_automation_with_archive`
    /// composition (durable history is archived first). It hard-deletes the
    /// automation bookkeeping rows — the automation itself (via the
    /// `status IN ('completed','stopped')` SQL predicate), its runs, attachments,
    /// and context refs (FK cascades are OFF in production) — then emits
    /// `AutomationDeleted`. Drafts are CAS'd to `Stopped` by the composition
    /// before this runs, so the terminal-only predicate still applies here.
    pub async fn delete(&self, id: &AutomationId) -> AppResult<()> {
        let automation = self.require_automation(id).await?;
        if !matches!(
            automation.status,
            AutomationStatus::Completed | AutomationStatus::Stopped
        ) {
            return Err(AppError::Validation(
                "only draft, completed, or stopped automations can be deleted".to_string(),
            ));
        }
        // Capture the project id before the row is gone; the deleted event needs
        // it so the frontend can evict caches and navigate away (critic G3/E7).
        let project_id = automation.project_id.clone();
        let deleted = self.automation_repo.delete_terminal(id).await?;
        if !deleted {
            return Err(automation_status_conflict(
                id,
                automation.status,
                AutomationStatus::Stopped,
            ));
        }
        self.run_repo.delete_for_automation(id).await?;
        self.automation_repo
            .delete_attachments_for_automation(id)
            .await?;
        self.automation_repo
            .delete_context_refs_for_automation(id)
            .await?;
        self.event_emitter.emit(AutomationEvent::AutomationDeleted {
            automation_id: id.clone(),
            project_id,
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
            plan_judge_state: AutomationPlanJudgeState::None,
            plan_judge_lease_expires_at: None,
            plan_judge_verdict_json: None,
            plan_revision_round: 0,
            plan_reminder_count: 0,
            plan_pending_instructions: None,
            plan_last_parked_artifact_id: None,
            agent_phase_started_at: None,
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
        if let SuccessorReadiness::NotScheduled(outcome) = self
            .successor_readiness(&automation, &latest, false)
            .await?
        {
            return Ok(*outcome);
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
                let latest = self.latest_run_for_automation(&input.automation.id).await?;
                if latest.id != input.previous_run.id {
                    return Err(AppError::Validation(
                        "previousRunId must reference the latest automation run".to_string(),
                    ));
                }
                if let SuccessorReadiness::NotScheduled(outcome) = self
                    .successor_readiness(&input.automation, &latest, false)
                    .await?
                {
                    let outcome = *outcome;
                    return Ok(AutomationJudgeApplyOutcome {
                        successor_run: outcome.run,
                        terminal_automation_status: None,
                        reason: outcome.reason,
                    });
                }

                let (base_ref_kind, base_ref_used) =
                    judge_successor_base(&input.automation, &latest, &input.verdict)?;
                let run = self
                    .create_run(CreateAutomationRunInput {
                        automation_id: input.automation.id.clone(),
                        run_prompt: next_prompt,
                        prompt_author: AutomationPromptAuthor::Judge,
                        base_ref_kind,
                        base_ref_used,
                        base_from_run_id: Some(latest.id),
                    })
                    .await?;
                Ok(AutomationJudgeApplyOutcome {
                    successor_run: Some(run),
                    terminal_automation_status: None,
                    reason: None,
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

    async fn successor_readiness(
        &self,
        automation: &Automation,
        latest: &AutomationRun,
        allow_unjudged_latest: bool,
    ) -> AppResult<SuccessorReadiness> {
        let unjudged_terminal = allow_unjudged_latest
            && run_status_is_signal_terminal(latest.status)
            && latest.judge_state == AutomationJudgeState::None;
        if is_open_automation_run(latest.status, latest.judge_state) && !unjudged_terminal {
            return Ok(SuccessorReadiness::NotScheduled(Box::new(
                successor_not_scheduled("run in flight"),
            )));
        }
        if !run_status_is_signal_terminal(latest.status) {
            return Err(AppError::Validation(
                "previous run is not signal-terminal".to_string(),
            ));
        }

        let runs = self.run_repo.list_for_automation(&automation.id).await?;
        if runs.len() as i64 >= automation.max_runs {
            self.transition_automation_status_or_conflict(
                &automation.id,
                AutomationStatus::Active,
                AutomationStatus::Paused,
                Some("max_runs_exhausted".to_string()),
                Some("Automation reached max_runs before scheduling a successor".to_string()),
            )
            .await?;
            return Ok(SuccessorReadiness::NotScheduled(Box::new(
                successor_not_scheduled("max_runs_exhausted"),
            )));
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
            return Ok(SuccessorReadiness::NotScheduled(Box::new(
                successor_not_scheduled("max_consecutive_failures"),
            )));
        }
        Ok(SuccessorReadiness::Ready)
    }
}

enum SuccessorReadiness {
    Ready,
    NotScheduled(Box<AutomationSuccessorRunOutcome>),
}

fn schedule_not_scheduled(reason: &str) -> AutomationScheduleOutcome {
    AutomationScheduleOutcome {
        scheduled: false,
        reason: Some(reason.to_string()),
    }
}

fn schedule_from_judge_apply(outcome: AutomationJudgeApplyOutcome) -> AutomationScheduleOutcome {
    AutomationScheduleOutcome {
        scheduled: outcome.successor_run.is_some(),
        reason: outcome.reason,
    }
}

fn successor_not_scheduled(reason: &str) -> AutomationSuccessorRunOutcome {
    AutomationSuccessorRunOutcome {
        scheduled: false,
        reason: Some(reason.to_string()),
        run: None,
    }
}

pub(crate) fn run_status_is_cancellable(status: AutomationRunStatus) -> bool {
    matches!(
        status,
        AutomationRunStatus::Pending
            | AutomationRunStatus::Provisioning
            | AutomationRunStatus::Running
            | AutomationRunStatus::AwaitingPlanApproval
            | AutomationRunStatus::Published
    )
}

pub(crate) fn run_status_blocks_trigger_run_now(status: AutomationRunStatus) -> bool {
    matches!(
        status,
        AutomationRunStatus::Pending
            | AutomationRunStatus::Provisioning
            | AutomationRunStatus::Running
            | AutomationRunStatus::AwaitingPlanApproval
            | AutomationRunStatus::Published
    )
}

fn consecutive_failure_count(runs: &[AutomationRun]) -> i64 {
    let mut count = 0;
    for run in runs.iter().rev() {
        // Workspace-review-gate blocks terminalize the run as AgentFailed but are user-actionable,
        // not agent failures, so they must not count toward max_consecutive_failures.
        if crate::application::automation::review_gate::run_is_workspace_review_blocked(run) {
            continue;
        }
        match run.status {
            AutomationRunStatus::AgentFailed | AutomationRunStatus::PrClosed => count += 1,
            AutomationRunStatus::Completed | AutomationRunStatus::Merged => break,
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

fn judge_successor_base(
    automation: &Automation,
    previous_run: &AutomationRun,
    verdict: &AutomationJudgeVerdict,
) -> AppResult<(String, String)> {
    match verdict.next_base_branch {
        Some(AutomationJudgeNextBaseBranch::AutomationBase) => {
            if automation.chain_mode == STACKED_CHAIN_MODE {
                return Err(AppError::Validation(
                    "stacked automation judge verdict must use previous_pr_head".to_string(),
                ));
            }
            if automation.chain_mode != DEFAULT_CHAIN_MODE {
                return Err(AppError::Validation(format!(
                    "automation chain_mode {} is not supported for automation_base successors",
                    automation.chain_mode
                )));
            }
            merged_base_successor_base(automation, previous_run)
        }
        Some(AutomationJudgeNextBaseBranch::PreviousPrHead) => {
            if automation.chain_mode != STACKED_CHAIN_MODE {
                return Err(AppError::Validation(
                    "previous_pr_head is only valid for stacked automations".to_string(),
                ));
            }
            let pr_head = previous_run
                .pr_head_ref_name
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    AppError::Validation(
                        "stacked automation successor requires previous run pr_head_ref_name"
                            .to_string(),
                    )
                })?;
            Ok(("local_branch".to_string(), pr_head.to_string()))
        }
        None => Err(AppError::Validation(
            "judge verdict continue requires nextBaseBranch".to_string(),
        )),
    }
}

fn pending_successor_run(
    automation_id: AutomationId,
    previous_run: &AutomationRun,
    run_index: i64,
    run_prompt: String,
    prompt_author: AutomationPromptAuthor,
    base_ref_kind: String,
    base_ref_used: String,
) -> AutomationRun {
    let now = Utc::now();
    AutomationRun {
        id: AutomationRunId::new(),
        automation_id,
        run_index,
        status: AutomationRunStatus::Pending,
        judge_state: AutomationJudgeState::None,
        judge_lease_expires_at: None,
        plan_judge_state: AutomationPlanJudgeState::None,
        plan_judge_lease_expires_at: None,
        plan_judge_verdict_json: None,
        plan_revision_round: 0,
        plan_reminder_count: 0,
        plan_pending_instructions: None,
        plan_last_parked_artifact_id: None,
        agent_phase_started_at: None,
        conversation_id: None,
        run_prompt,
        prompt_author,
        base_ref_kind,
        base_ref_used,
        base_from_run_id: Some(previous_run.id.clone()),
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
    }
}

fn skip_judge_template_prompt(previous_run: &AutomationRun) -> String {
    match (previous_run.status, previous_run.pr_number) {
        (AutomationRunStatus::Merged, Some(pr_number)) => {
            format!("Continue the goal; previous run merged PR #{pr_number}.")
        }
        (_, Some(pr_number)) => {
            format!("Continue the goal; previous run finished with PR #{pr_number}.")
        }
        _ => "Continue the goal; previous run finished without a pull request.".to_string(),
    }
}

fn run_status_is_signal_terminal(status: AutomationRunStatus) -> bool {
    matches!(
        status,
        AutomationRunStatus::Completed
            | AutomationRunStatus::Merged
            | AutomationRunStatus::PrClosed
            | AutomationRunStatus::AgentFailed
    )
}

fn completion_signal_for_run_mode(run_mode: &str) -> &'static str {
    match run_mode.trim() {
        DEFAULT_RUN_MODE => DEFAULT_COMPLETION_SIGNAL,
        _ => AGENT_COMPLETED_COMPLETION_SIGNAL,
    }
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

fn validate_stacked_chain_merge_mode(
    chain_mode: &str,
    pr_merge_mode: AutomationPrMergeMode,
) -> AppResult<()> {
    if chain_mode == STACKED_CHAIN_MODE && pr_merge_mode == AutomationPrMergeMode::Automatic {
        return Err(AppError::Validation(format!(
            "{AUTOMATION_STACKED_AUTO_MERGE_ERROR_CODE}: automatic PR merge is not supported for stacked PR chains"
        )));
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
            "automation goal_prompt is required before approval".to_string(),
        ));
    }
    if automation.provider_harness.trim().is_empty() {
        return Err(AppError::Validation(
            "automation provider_harness is required before approval".to_string(),
        ));
    }
    if automation.model_id.trim().is_empty() {
        return Err(AppError::Validation(
            "automation model_id is required before approval".to_string(),
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
            "automation first_run_prompt is required before approval".to_string(),
        ));
    }
    validate_goal_items_json(automation.goal_items_json.as_deref())?;
    match automation.completion_signal.as_str() {
        DEFAULT_COMPLETION_SIGNAL if automation.run_mode != DEFAULT_RUN_MODE => {
            return Err(AppError::Validation(
                "pr_merged automations require edit run_mode".to_string(),
            ));
        }
        DEFAULT_COMPLETION_SIGNAL | AGENT_COMPLETED_COMPLETION_SIGNAL => {}
        value => {
            return Err(AppError::Validation(format!(
                "automation completion_signal is not supported: {value}"
            )));
        }
    }
    match automation.base_ref_kind.as_str() {
        DEFAULT_BASE_REF_KIND => {}
        "local_branch" if !automation.base_ref.trim().is_empty() => {}
        "current_branch" => {
            return Err(AppError::Validation(
                "current_branch must be resolved before approval".to_string(),
            ))
        }
        _ => {
            return Err(AppError::Validation(
                "automation base_ref_kind/base_ref is not approval-ready".to_string(),
            ))
        }
    }
    validate_positive("max_runs", Some(automation.max_runs))?;
    validate_positive(
        "max_consecutive_failures",
        Some(automation.max_consecutive_failures),
    )?;
    validate_stacked_chain_merge_mode(&automation.chain_mode, automation.pr_merge_mode)?;
    Ok(())
}

fn validate_goal_items_json(goal_items_json: Option<&str>) -> AppResult<()> {
    let Some(goal_items_json) = goal_items_json
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Err(AppError::Validation(
            "automation phase spec is required before approval".to_string(),
        ));
    };
    let parsed = serde_json::from_str::<Value>(goal_items_json).map_err(|error| {
        AppError::Validation(format!(
            "automation phase spec must be valid JSON before approval: {error}"
        ))
    })?;
    match parsed {
        Value::Array(items) if !items.is_empty() => Ok(()),
        _ => Err(AppError::Validation(
            "automation phase spec must include at least one phase before approval".to_string(),
        )),
    }
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
