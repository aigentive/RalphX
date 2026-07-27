use std::path::Path;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use ralphx_domain::entities::automation::is_signal_terminal_automation_run;
use ralphx_domain::repositories::automation_run_repository::AutomationJudgeTransitionGuard;
use serde_json::Value;

use crate::application::agent_conversation_workspace::reject_persona_builder_workspace_mode;
use crate::application::automation::decomposition_verifier::{
    parse_authoring_state, AutomationAuthoringMode, AutomationAuthoringState,
    AutomationDecompositionInput, AutomationGoalReplanState, AutomationGoalReplanStatus,
};
use crate::application::automation::judge::{
    apply_updated_item_statuses, automation_judge_loop_suspected, current_goal_item_id,
    goal_items_proposal_json, parse_automation_judge_verdict,
    revert_in_progress_goal_items_to_pending, AutomationJudgeDecision,
    AutomationJudgeNextBaseBranch, AutomationJudgeValidationContext, AutomationJudgeVerdict,
};
use crate::application::automation::plan_gate::{
    is_plan_gate_pause_reason, AUTOMATION_PLAN_GATE_TRIGGER_RUN_NOW_ERROR_CODE,
    PLAN_JUDGE_FAILED_PAUSED_REASON_CODE,
};
use crate::application::automation::transition::{
    AutomationEvent, AutomationEventEmitter, AutomationTransitionService,
};
use crate::application::services::pr_auto_merge_status::{
    auto_merge_disable_failure_summary, AUTO_MERGE_SUPERVISION_STATUS_WAITING,
};
use crate::application::NotificationService;
use crate::domain::entities::{
    is_open_automation_run, AgentConversationWorkspace, Artifact, ArtifactBucketId,
    ArtifactContent, ArtifactId, ArtifactMetadata, ArtifactType, Automation, AutomationId,
    AutomationJudgeState, AutomationPlanApprovalMode, AutomationPlanJudgeState,
    AutomationPrMergeMode, AutomationPromptAuthor, AutomationRun, AutomationRunId,
    AutomationRunStatus, AutomationStatus, ChatConversationId, ProjectId,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, ArtifactRepository, AutomationConfigPatch,
    AutomationRepository, AutomationRunRepository, AutomationSettingsPatch,
};
use crate::domain::services::github_service::GithubServiceTrait;
use crate::error::{AppError, AppResult};
use crate::utils::path_safety::validate_absolute_non_root_path;

const DEFAULT_AUTOMATION_NAME: &str = "Automation setup";
const DEFAULT_PROVIDER_HARNESS: &str = "claude";
const DEFAULT_MODEL_ID: &str = "sonnet";
const DEFAULT_RUN_MODE: &str = "edit";
const DEFAULT_BASE_REF_KIND: &str = "project_default";
/// Base-ref kind for an automation whose base is its own integration branch.
pub(crate) const LOCAL_BRANCH_BASE_REF_KIND: &str = "local_branch";
const DEFAULT_CHAIN_MODE: &str = "merged_base";
const STACKED_CHAIN_MODE: &str = "pr_head_stacked";
const JUDGE_FAILED_PAUSED_REASON_CODE: &str = "judge_failed";
const JUDGE_STOPPED_UNMET_PAUSED_REASON_CODE: &str = "judge_stopped_unmet";
const DEFAULT_COMPLETION_SIGNAL: &str = "pr_merged";
const AGENT_COMPLETED_COMPLETION_SIGNAL: &str = "agent_completed";
pub(crate) const IDEATION_BRIDGE_RUN_MODE: &str = "ideation";
pub(crate) const IDEATION_FINALIZED_COMPLETION_SIGNAL: &str = "ideation_finalized";
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
    pub authoring_mode: Option<AutomationAuthoringMode>,
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
    pub judge_lease_expires_at: DateTime<Utc>,
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
    pub noop_reason: Option<AutomationJudgeApplyNoopReason>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutomationJudgeApplyNoopReason {
    NotCurrent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingGoalReplanApplyOutcome {
    None,
    Applied,
    Stale,
}

#[derive(Clone)]
pub struct AutomationService {
    automation_repo: Arc<dyn AutomationRepository>,
    run_repo: Arc<dyn AutomationRunRepository>,
    transition_service: AutomationTransitionService,
    event_emitter: Arc<dyn AutomationEventEmitter>,
    artifact_repo: Arc<dyn ArtifactRepository>,
    workspace_repo: Option<Arc<dyn AgentConversationWorkspaceRepository>>,
    github_service: Option<Arc<dyn GithubServiceTrait>>,
}

impl AutomationService {
    pub fn new(
        automation_repo: Arc<dyn AutomationRepository>,
        run_repo: Arc<dyn AutomationRunRepository>,
        event_emitter: Arc<dyn AutomationEventEmitter>,
        artifact_repo: Arc<dyn ArtifactRepository>,
        notification_service: Arc<NotificationService>,
    ) -> Self {
        let transition_service = AutomationTransitionService::new(
            Arc::clone(&automation_repo),
            Arc::clone(&run_repo),
            Arc::clone(&event_emitter),
            notification_service,
        );
        Self {
            automation_repo,
            run_repo,
            transition_service,
            event_emitter,
            artifact_repo,
            workspace_repo: None,
            github_service: None,
        }
    }

    pub fn with_pr_auto_merge_controls(
        mut self,
        workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
        github_service: Option<Arc<dyn GithubServiceTrait>>,
    ) -> Self {
        self.workspace_repo = Some(workspace_repo);
        self.github_service = github_service;
        self
    }

    pub async fn create_draft(&self, input: CreateAutomationDraftInput) -> AppResult<Automation> {
        let now = Utc::now();
        let authoring_state_json = match input.authoring_mode.unwrap_or_default() {
            AutomationAuthoringMode::Reviewed => None,
            AutomationAuthoringMode::TrustedAutoFinalize => Some(
                serde_json::to_string(&AutomationAuthoringState::trusted_unverified()).map_err(
                    |error| {
                        AppError::Infrastructure(format!(
                            "failed to serialize automation authoring state: {error}"
                        ))
                    },
                )?,
            ),
        };
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
            authoring_state_json,
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

    pub async fn update_goal_items_json_if_unchanged(
        &self,
        id: &AutomationId,
        expected_goal_items_json: Option<String>,
        next_goal_items_json: Option<String>,
    ) -> AppResult<bool> {
        let updated = self
            .automation_repo
            .update_goal_items_json_if_unchanged(id, expected_goal_items_json, next_goal_items_json)
            .await?;
        if updated.is_some() {
            self.event_emitter.emit(AutomationEvent::AutomationUpdated {
                automation_id: id.clone(),
            });
            return Ok(true);
        }
        Ok(false)
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
        if let Some(run_mode) = input.run_mode.as_deref() {
            reject_persona_builder_workspace_mode(run_mode).map_err(AppError::Validation)?;
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
        // The automation's base is fixed at creation to its own integration branch
        // (`local_branch`): runs base on it and their PRs merge into it, and the integration
        // branch later merges to the project default. The setup agent finalizing config must
        // NOT downgrade that base to the project default (the fork point, e.g. `main`) — doing
        // so silently makes every run open its PR against `main` instead of the integration
        // branch. Preserve the `local_branch` base against a `project_default` overwrite
        // (a `None` patch field keeps the current stored value via COALESCE).
        let downgrades_integration_base = automation.base_ref_kind == LOCAL_BRANCH_BASE_REF_KIND
            && input.base_ref_kind.as_deref() == Some(DEFAULT_BASE_REF_KIND);
        let (base_ref_kind, base_ref, base_display_name) = if downgrades_integration_base {
            (None, None, None)
        } else {
            (input.base_ref_kind, input.base_ref, input.base_display_name)
        };
        let goal_items_were_updated = input.goal_items_json.is_some();
        let patch = AutomationConfigPatch {
            goal_prompt: input.goal_prompt,
            first_run_prompt: input.first_run_prompt,
            provider_harness: input.provider_harness,
            model_id: input.model_id,
            logical_effort: input.logical_effort,
            run_mode: input.run_mode,
            base_ref_kind,
            base_ref,
            base_display_name,
            goal_items_json: input.goal_items_json,
            chain_mode: input.chain_mode,
            completion_signal,
            setup_analysis_summary: input.setup_analysis_summary,
            spec_artifact_id,
        };
        let mut updated = self
            .automation_repo
            .update_config(&input.id, patch)
            .await?
            .ok_or_else(|| automation_not_found(&input.id))?;
        if goal_items_were_updated {
            let mut state = parse_authoring_state(updated.authoring_state_json.as_deref())?;
            if let Some(replan) = state.pending_goal_replan.as_mut() {
                if replan.status == AutomationGoalReplanStatus::Pending {
                    replan.status = AutomationGoalReplanStatus::Rejected;
                    if self
                        .persist_authoring_state_if_unchanged(&updated, &state)
                        .await?
                    {
                        updated = self.require_automation(&updated.id).await?;
                    }
                }
            }
        }
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
        if automation.status != AutomationStatus::Paused {
            return Err(AppError::InvalidTransition {
                from: automation.status.as_str().to_string(),
                to: AutomationStatus::Active.as_str().to_string(),
            });
        }
        if automation.paused_reason_code.as_deref() == Some(JUDGE_STOPPED_UNMET_PAUSED_REASON_CODE)
        {
            return self.resume_after_judge_stopped_unmet(automation).await;
        }
        self.transition_automation_status_or_conflict(
            id,
            AutomationStatus::Paused,
            AutomationStatus::Active,
            None,
            None,
        )
        .await
    }

    async fn resume_after_judge_stopped_unmet(
        &self,
        automation: Automation,
    ) -> AppResult<Automation> {
        let runs = self.run_repo.list_for_automation(&automation.id).await?;
        let latest = current_unmet_stop_run(&automation, &runs)?;
        let previous_run_id = latest.id.clone();
        let retry = CreateAutomationRunInput {
            automation_id: automation.id.clone(),
            run_prompt: latest.run_prompt.clone(),
            prompt_author: latest.prompt_author,
            base_ref_kind: latest.base_ref_kind.clone(),
            base_ref_used: latest.base_ref_used.clone(),
            base_from_run_id: Some(previous_run_id.clone()),
        };
        let paused_reason_detail = automation.paused_reason_detail.clone();
        let resumed = self
            .transition_automation_status_or_conflict(
                &automation.id,
                AutomationStatus::Paused,
                AutomationStatus::Active,
                None,
                None,
            )
            .await?;

        if let Err(error) = self.create_run(retry).await {
            self.rollback_failed_activation_if_still_current(
                &automation.id,
                Some(previous_run_id),
                AutomationStatus::Paused,
                Some(JUDGE_STOPPED_UNMET_PAUSED_REASON_CODE.to_string()),
                paused_reason_detail,
                "judge-stopped-unmet resume",
            )
            .await;
            return Err(error);
        }
        Ok(resumed)
    }

    pub async fn stop(&self, id: &AutomationId) -> AppResult<Automation> {
        let automation = self.require_automation(id).await?;
        // Cancel the observed open work before committing the terminal automation state.
        // This keeps a failed sweep retryable and prevents a stopped automation from
        // silently retaining the work that was already visible to this request.
        self.cancel_open_runs(&automation).await?;
        let stopped = self
            .transition_automation_status_or_conflict(
                id,
                automation.status,
                AutomationStatus::Stopped,
                None,
                None,
            )
            .await?;

        // An Active automation can race a create_run call that passed its status check
        // before the stop CAS. Sweep once more after Stopped closes admission. If this
        // second sweep loses a run CAS, reactivate only through Stopped -> Active so the
        // scheduler can still observe and reconcile the open run.
        if automation.status == AutomationStatus::Active {
            if let Err(error) = self.cancel_open_runs(&automation).await {
                if let Err(rollback_error) = self
                    .transition_automation_status_or_conflict(
                        id,
                        AutomationStatus::Stopped,
                        AutomationStatus::Active,
                        None,
                        None,
                    )
                    .await
                {
                    tracing::error!(
                        automation_id = %id,
                        error = %error,
                        rollback_error = %rollback_error,
                        "Automation stop sweep failed and status reactivation also failed"
                    );
                }
                return Err(error);
            }
        }
        self.sync_goal_items_for_closed_run_without_successor(id)
            .await;
        Ok(stopped)
    }

    async fn cancel_open_runs(&self, automation: &Automation) -> AppResult<()> {
        for run in self.run_repo.list_for_automation(&automation.id).await? {
            if run_status_is_cancellable(run.status) {
                self.cancel_run_core(automation, &run).await?;
            }
        }
        Ok(())
    }

    pub async fn restart(&self, id: &AutomationId) -> AppResult<AutomationScheduleOutcome> {
        let automation = self.require_automation(id).await?;
        if automation.status != AutomationStatus::Stopped {
            return Err(AppError::InvalidTransition {
                from: automation.status.as_str().to_string(),
                to: AutomationStatus::Active.as_str().to_string(),
            });
        }
        validate_activation_configuration(&automation)?;

        let latest = self.run_repo.latest_for_automation(id).await?;
        if latest.as_ref().is_some_and(|run| {
            run_status_is_cancellable(run.status)
                || run.judge_state == AutomationJudgeState::InProgress
        }) {
            return Err(AppError::Conflict(format!(
                "automation {} still has work in flight",
                id.as_str()
            )));
        }
        let run_input = restart_run_input(&automation, latest.as_ref())?;
        let previous_run_id = latest.as_ref().map(|run| run.id.clone());

        self.transition_automation_status_or_conflict(
            id,
            AutomationStatus::Stopped,
            AutomationStatus::Active,
            None,
            None,
        )
        .await?;

        match self.create_run(run_input).await {
            Ok(_) => Ok(AutomationScheduleOutcome {
                scheduled: true,
                reason: None,
            }),
            Err(error) => {
                self.rollback_failed_activation_if_still_current(
                    id,
                    previous_run_id,
                    AutomationStatus::Stopped,
                    None,
                    None,
                    "automation restart",
                )
                .await;
                Err(error)
            }
        }
    }

    async fn rollback_failed_activation_if_still_current(
        &self,
        id: &AutomationId,
        previous_run_id: Option<AutomationRunId>,
        rollback_status: AutomationStatus,
        reason_code: Option<String>,
        reason_detail: Option<String>,
        operation: &'static str,
    ) {
        let latest_run_id = match self.run_repo.latest_for_automation(id).await {
            Ok(latest) => latest.map(|run| run.id),
            Err(error) => {
                tracing::error!(
                    automation_id = %id,
                    error = %error,
                    operation,
                    "Failed to read current run while rolling back automation activation"
                );
                return;
            }
        };
        if latest_run_id != previous_run_id {
            return;
        }
        if let Err(error) = self
            .transition_automation_status_or_conflict(
                id,
                AutomationStatus::Active,
                rollback_status,
                reason_code,
                reason_detail,
            )
            .await
        {
            tracing::error!(
                automation_id = %id,
                error = %error,
                operation,
                "Failed to roll back automation status after run creation failed"
            );
        }
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

        if latest.status == AutomationRunStatus::Cancelled {
            self.create_run(CreateAutomationRunInput {
                automation_id: automation.id.clone(),
                run_prompt: latest.run_prompt.clone(),
                prompt_author: latest.prompt_author,
                base_ref_kind: latest.base_ref_kind.clone(),
                base_ref_used: latest.base_ref_used.clone(),
                base_from_run_id: latest.base_from_run_id.clone(),
            })
            .await?;
            return Ok(AutomationRunNowAction::Outcome(AutomationScheduleOutcome {
                scheduled: true,
                reason: None,
            }));
        }

        if !is_signal_terminal_automation_run(latest.status) {
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
                if latest.judge_verdict_json.is_none() {
                    return Ok(AutomationRunNowAction::Outcome(schedule_not_scheduled(
                        "judge verdict is missing",
                    )));
                }
                let outcome = self
                    .apply_stored_judge_verdict(&automation.id, &latest.id)
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

    pub async fn retry_judge_action(&self, id: &AutomationId) -> AppResult<AutomationRunNowAction> {
        let latest = self.latest_run_for_automation(id).await?;
        if latest.judge_state != AutomationJudgeState::Failed
            || !is_signal_terminal_automation_run(latest.status)
        {
            return Ok(AutomationRunNowAction::Outcome(schedule_not_scheduled(
                "latest judge is not failed",
            )));
        }
        self.trigger_run_now_action(id).await
    }

    pub async fn retry_plan_judge(
        &self,
        id: &AutomationId,
        expected_artifact_id: &str,
    ) -> AppResult<AutomationScheduleOutcome> {
        let expected_artifact_id = expected_artifact_id.trim();
        if expected_artifact_id.is_empty() {
            return Err(AppError::Validation(
                "current plan artifact is required to retry plan judge".to_string(),
            ));
        }
        let automation = self.require_automation(id).await?;
        let paused_for_failed_plan_judge = automation.status == AutomationStatus::Paused
            && automation.paused_reason_code.as_deref()
                == Some(PLAN_JUDGE_FAILED_PAUSED_REASON_CODE);
        if automation.status != AutomationStatus::Active && !paused_for_failed_plan_judge {
            return Err(AppError::Validation(
                "automation must be active or paused for a failed plan judge".to_string(),
            ));
        }
        let run = self.latest_run_for_automation(id).await?;
        if run.status != AutomationRunStatus::AwaitingPlanApproval {
            return Ok(schedule_not_scheduled(
                "latest run is not awaiting plan approval",
            ));
        }
        if run.plan_last_parked_artifact_id.as_deref() != Some(expected_artifact_id) {
            return Err(AppError::Conflict(
                "current plan artifact does not match the parked judge attempt".to_string(),
            ));
        }
        if run.plan_judge_state != AutomationPlanJudgeState::Failed {
            return Ok(schedule_not_scheduled("latest plan judge is not failed"));
        }

        if paused_for_failed_plan_judge {
            self.transition_automation_status_or_conflict(
                id,
                AutomationStatus::Paused,
                AutomationStatus::Active,
                None,
                None,
            )
            .await?;
        }
        match self
            .transition_service
            .transition_plan_judge_state(
                &run.id,
                AutomationPlanJudgeState::Failed,
                AutomationPlanJudgeState::None,
                None,
                None,
            )
            .await
        {
            Ok(true) => Ok(AutomationScheduleOutcome {
                scheduled: true,
                reason: None,
            }),
            Ok(false) => Ok(schedule_not_scheduled("plan judge already retried")),
            Err(error) => {
                if paused_for_failed_plan_judge {
                    let _ = self
                        .transition_automation_status_or_conflict(
                            id,
                            AutomationStatus::Active,
                            AutomationStatus::Paused,
                            Some(PLAN_JUDGE_FAILED_PAUSED_REASON_CODE.to_string()),
                            Some("Plan judge retry could not be scheduled".to_string()),
                        )
                        .await;
                }
                Err(error)
            }
        }
    }

    pub async fn skip_judge(
        &self,
        id: &AutomationId,
        run_id: &AutomationRunId,
    ) -> AppResult<AutomationScheduleOutcome> {
        let mut automation = self.require_automation(id).await?;
        let resume_failed_judge = automation.status == AutomationStatus::Paused
            && automation.paused_reason_code.as_deref() == Some(JUDGE_FAILED_PAUSED_REASON_CODE);
        if automation.status != AutomationStatus::Active && !resume_failed_judge {
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
        let skip_failed_judge =
            resume_failed_judge && run.judge_state == AutomationJudgeState::Failed;
        if run.judge_state != AutomationJudgeState::None && !skip_failed_judge {
            return Ok(AutomationScheduleOutcome {
                scheduled: false,
                reason: Some("judge already started".to_string()),
            });
        }
        if !is_signal_terminal_automation_run(run.status) {
            return Ok(AutomationScheduleOutcome {
                scheduled: false,
                reason: Some("run is not ready for judge skipping".to_string()),
            });
        }
        if resume_failed_judge {
            automation = self
                .transition_automation_status_or_conflict(
                    &automation.id,
                    AutomationStatus::Paused,
                    AutomationStatus::Active,
                    None,
                    None,
                )
                .await?;
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
                    current_goal_item_id(automation.goal_items_json.as_deref()),
                );
                match self
                    .run_repo
                    .skip_judge_and_create_successor_run(&automation.id, &latest.id, successor)
                    .await?
                {
                    Some(run) => {
                        self.event_emitter
                            .emit(AutomationEvent::AutomationRunUpdated {
                                automation_id: automation.id.clone(),
                                run_id: latest.id,
                            });
                        self.event_emitter
                            .emit(AutomationEvent::AutomationRunUpdated {
                                automation_id: automation.id.clone(),
                                run_id: run.id,
                            });
                        Ok(AutomationScheduleOutcome {
                            scheduled: true,
                            reason: None,
                        })
                    }
                    None => Ok(schedule_not_scheduled(if skip_failed_judge {
                        "not skipped: judge redispatched"
                    } else {
                        "judge already started"
                    })),
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
        let automation = self.require_automation(id).await?;
        let run = self
            .run_repo
            .get_by_id(run_id)
            .await?
            .ok_or_else(|| automation_run_not_found(run_id))?;
        if run.automation_id != *id {
            return Err(AppError::Validation(
                "automation run is not owned by the requested automation".to_string(),
            ));
        }
        let cancelled = self.cancel_run_core(&automation, &run).await?;
        self.sync_goal_items_for_closed_run_without_successor(id)
            .await;
        Ok(cancelled)
    }

    async fn cancel_run_core(
        &self,
        automation: &Automation,
        run: &AutomationRun,
    ) -> AppResult<AutomationRun> {
        let cancelled = self
            .transition_run_status_or_conflict(
                &run.id,
                run.status,
                AutomationRunStatus::Cancelled,
                None,
                None,
            )
            .await?;
        if automation.pr_merge_mode == AutomationPrMergeMode::Automatic {
            self.disarm_cancelled_run_auto_merge(run).await;
        }
        self.run_repo.clear_plan_judge_state(&run.id).await?;
        Ok(cancelled)
    }

    pub async fn terminalize_blocked_run(
        &self,
        automation_id: &AutomationId,
        run: &AutomationRun,
        error_code: &str,
        error_detail: Option<String>,
    ) -> AppResult<bool> {
        if run.automation_id != *automation_id {
            return Err(AppError::Validation(
                "automation run is not owned by the requested automation".to_string(),
            ));
        }
        let changed = if matches!(
            run.status,
            AutomationRunStatus::Running | AutomationRunStatus::Provisioning
        ) {
            self.transition_service
                .transition_run_status(
                    &run.id,
                    run.status,
                    AutomationRunStatus::AgentFailed,
                    Some(error_code.to_string()),
                    error_detail,
                )
                .await?
        } else {
            false
        };
        if !changed
            && matches!(
                run.status,
                AutomationRunStatus::Running | AutomationRunStatus::Provisioning
            )
        {
            tracing::warn!(
                automation_id = %automation_id,
                run_id = %run.id,
                from_status = run.status.as_str(),
                "Discarded workspace-review-blocked run terminalization because run status changed"
            );
        }
        self.sync_goal_items_for_closed_run_without_successor(automation_id)
            .await;
        Ok(changed)
    }

    async fn disarm_cancelled_run_auto_merge(&self, run: &AutomationRun) {
        let Some(workspace_repo) = self.workspace_repo.as_ref() else {
            return;
        };
        let Some(conversation_id) = run.conversation_id.as_ref() else {
            return;
        };
        let workspace = match workspace_repo.get_by_conversation_id(conversation_id).await {
            Ok(Some(workspace)) => workspace,
            Ok(None) => return,
            Err(error) => {
                tracing::warn!(
                    run_id = run.id.as_str(),
                    conversation_id = conversation_id.as_str(),
                    error = %error,
                    "Automation cancel could not load workspace to disarm automatic PR auto-merge"
                );
                return;
            }
        };
        if !workspace.pr_auto_merge_desired {
            return;
        }
        if let Err(error) = workspace_repo
            .update_pr_supervision_preferences(
                conversation_id,
                workspace.pr_autofix_enabled,
                false,
                &workspace.pr_auto_merge_method,
            )
            .await
        {
            tracing::warn!(
                run_id = run.id.as_str(),
                conversation_id = conversation_id.as_str(),
                error = %error,
                "Automation cancel could not clear automatic PR auto-merge preference"
            );
        }
        self.disable_cancelled_run_remote_auto_merge(run, &workspace)
            .await;
    }

    async fn disable_cancelled_run_remote_auto_merge(
        &self,
        run: &AutomationRun,
        workspace: &AgentConversationWorkspace,
    ) {
        let Some(github) = self.github_service.as_ref() else {
            return;
        };
        let Some(pr_number) = run.pr_number.or(workspace.publication_pr_number) else {
            return;
        };
        let working_dir = match validate_absolute_non_root_path(
            Path::new(&workspace.worktree_path),
            "cancelled automation workspace",
        ) {
            Ok(path) => path,
            Err(error) => {
                tracing::warn!(
                    run_id = run.id.as_str(),
                    conversation_id = workspace.conversation_id.as_str(),
                    pr_number,
                    error = %error,
                    "Automation cancel could not disable GitHub auto-merge for an unsafe workspace path"
                );
                return;
            }
        };
        match github.disable_pr_auto_merge(&working_dir, pr_number).await {
            Ok(()) => {
                let Some(workspace_repo) = self.workspace_repo.as_ref() else {
                    return;
                };
                if let Err(error) = workspace_repo
                    .update_pr_auto_merge_state(
                        &workspace.conversation_id,
                        Some(false),
                        None,
                        Some("GitHub auto-merge is disabled."),
                    )
                    .await
                {
                    tracing::warn!(
                        run_id = run.id.as_str(),
                        conversation_id = workspace.conversation_id.as_str(),
                        pr_number,
                        error = %error,
                        "Automation cancel disabled GitHub auto-merge but could not persist workspace state"
                    );
                }
            }
            Err(error) => {
                tracing::warn!(
                    run_id = run.id.as_str(),
                    conversation_id = workspace.conversation_id.as_str(),
                    pr_number,
                    error = %error,
                    "Automation cancel could not disable GitHub auto-merge; preference was cleared"
                );
                let Some(workspace_repo) = self.workspace_repo.as_ref() else {
                    return;
                };
                if let Err(update_error) = workspace_repo
                    .update_pr_auto_merge_state(
                        &workspace.conversation_id,
                        Some(true),
                        Some(AUTO_MERGE_SUPERVISION_STATUS_WAITING),
                        Some(&auto_merge_disable_failure_summary(&error)),
                    )
                    .await
                {
                    tracing::warn!(
                        run_id = run.id.as_str(),
                        conversation_id = workspace.conversation_id.as_str(),
                        pr_number,
                        error = %update_error,
                        "Automation cancel could not persist GitHub auto-merge disable warning"
                    );
                }
            }
        }
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
        let authoring_state = parse_authoring_state(automation.authoring_state_json.as_deref())?;
        if authoring_state.mode == AutomationAuthoringMode::TrustedAutoFinalize {
            let input = self.load_decomposition_input(&automation).await?;
            if !authoring_state.is_verified_for(&input) {
                return Err(AppError::Validation(
                    "trusted auto-finalize requires a current verified decomposition".to_string(),
                ));
            }
        }
        self.transition_automation_status_or_conflict(
            id,
            automation.status,
            AutomationStatus::Active,
            None,
            None,
        )
        .await
    }

    pub(crate) async fn load_decomposition_input(
        &self,
        automation: &Automation,
    ) -> AppResult<AutomationDecompositionInput> {
        let trusted_edit_policy = automation.run_mode == DEFAULT_RUN_MODE
            && automation.completion_signal == DEFAULT_COMPLETION_SIGNAL
            && automation.chain_mode == DEFAULT_CHAIN_MODE
            && automation.plan_approval_mode == AutomationPlanApprovalMode::Automatic
            && automation.pr_merge_mode == AutomationPrMergeMode::Automatic;
        let trusted_ideation_policy = automation.run_mode == IDEATION_BRIDGE_RUN_MODE
            && automation.completion_signal == IDEATION_FINALIZED_COMPLETION_SIGNAL
            && automation.chain_mode == DEFAULT_CHAIN_MODE
            && automation.plan_approval_mode == AutomationPlanApprovalMode::Automatic
            && automation.pr_merge_mode == AutomationPrMergeMode::Manual
            && automation.plan_deep_verification;
        if !trusted_edit_policy && !trusted_ideation_policy {
            return Err(AppError::Validation(
                "trusted auto-finalize requires either the automatic edit/PR-merge policy or the verified ideation/task-graph policy"
                    .to_string(),
            ));
        }
        let goal_items_json = automation
            .goal_items_json
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                AppError::Validation(
                    "trusted auto-finalize requires structured goal items".to_string(),
                )
            })?
            .to_string();
        let first_run_prompt = automation
            .first_run_prompt
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                AppError::Validation(
                    "trusted auto-finalize requires a first run prompt".to_string(),
                )
            })?
            .to_string();
        let spec_artifact_id = automation
            .spec_artifact_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                AppError::Validation(
                    "trusted auto-finalize requires a linked specification".to_string(),
                )
            })?
            .to_string();
        let artifact = self
            .artifact_repo
            .get_by_id(&ArtifactId::from_string(spec_artifact_id.clone()))
            .await?
            .ok_or_else(|| {
                AppError::Validation(format!(
                    "trusted auto-finalize specification {spec_artifact_id} was not found"
                ))
            })?;
        let ArtifactContent::Inline { text: spec_content } = artifact.content else {
            return Err(AppError::Validation(
                "trusted auto-finalize requires an inline-readable specification".to_string(),
            ));
        };
        Ok(AutomationDecompositionInput {
            goal_prompt: automation.goal_prompt.trim().to_string(),
            goal_items_json,
            first_run_prompt,
            spec_artifact_id,
            spec_content,
            provider_harness: automation.provider_harness.clone(),
            model_id: automation.model_id.clone(),
            logical_effort: automation.logical_effort.clone(),
            run_mode: automation.run_mode.clone(),
            base_ref_kind: automation.base_ref_kind.clone(),
            base_ref: automation.base_ref.clone(),
            chain_mode: automation.chain_mode.clone(),
            completion_signal: automation.completion_signal.clone(),
            plan_approval_mode: automation.plan_approval_mode.as_str().to_string(),
            pr_merge_mode: automation.pr_merge_mode.as_str().to_string(),
            plan_deep_verification: automation.plan_deep_verification,
            max_runs: automation.max_runs,
            max_consecutive_failures: automation.max_consecutive_failures,
        })
    }

    pub(crate) async fn persist_authoring_state_if_unchanged(
        &self,
        automation: &Automation,
        state: &AutomationAuthoringState,
    ) -> AppResult<bool> {
        let serialized = serde_json::to_string(state).map_err(|error| {
            AppError::Infrastructure(format!(
                "failed to serialize automation authoring state: {error}"
            ))
        })?;
        let changed = self
            .automation_repo
            .update_authoring_state_if_unchanged(
                &automation.id,
                automation.updated_at,
                Some(serialized),
            )
            .await?;
        if changed {
            self.event_emitter.emit(AutomationEvent::AutomationUpdated {
                automation_id: automation.id.clone(),
            });
        }
        Ok(changed)
    }

    pub(crate) async fn apply_pending_goal_replan_for_run(
        &self,
        automation_id: &AutomationId,
        run: &AutomationRun,
    ) -> AppResult<PendingGoalReplanApplyOutcome> {
        let Some(source_run_id) = run.base_from_run_id.as_ref() else {
            return Ok(PendingGoalReplanApplyOutcome::None);
        };
        let current = self.require_automation(automation_id).await?;
        let state = parse_authoring_state(current.authoring_state_json.as_deref())?;
        let Some(replan) = state.pending_goal_replan.as_ref() else {
            return Ok(PendingGoalReplanApplyOutcome::None);
        };
        if replan.status != AutomationGoalReplanStatus::Pending
            || replan.source_run_id != source_run_id.as_str()
        {
            return Ok(PendingGoalReplanApplyOutcome::None);
        }
        if current.goal_items_json.as_deref() == Some(replan.proposed_goal_items_json.as_str()) {
            self.mark_goal_replan_applied(&current, state).await?;
            return Ok(PendingGoalReplanApplyOutcome::Applied);
        }
        if current.goal_items_json.as_deref() != Some(replan.base_goal_items_json.as_str()) {
            return Ok(PendingGoalReplanApplyOutcome::Stale);
        }
        if !self
            .update_goal_items_json_if_unchanged(
                automation_id,
                Some(replan.base_goal_items_json.clone()),
                Some(replan.proposed_goal_items_json.clone()),
            )
            .await?
        {
            let latest = self.require_automation(automation_id).await?;
            if latest.goal_items_json.as_deref() != Some(replan.proposed_goal_items_json.as_str()) {
                return Ok(PendingGoalReplanApplyOutcome::Stale);
            }
        }
        let latest = self.require_automation(automation_id).await?;
        let latest_state = parse_authoring_state(latest.authoring_state_json.as_deref())?;
        self.mark_goal_replan_applied(&latest, latest_state).await?;
        Ok(PendingGoalReplanApplyOutcome::Applied)
    }

    async fn mark_goal_replan_applied(
        &self,
        automation: &Automation,
        mut state: AutomationAuthoringState,
    ) -> AppResult<()> {
        if let Some(replan) = state.pending_goal_replan.as_mut() {
            replan.status = AutomationGoalReplanStatus::Applied;
            replan.applied_at = Some(Utc::now().to_rfc3339());
            let _ = self
                .persist_authoring_state_if_unchanged(automation, &state)
                .await?;
        }
        Ok(())
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
            plan_last_parked_blueprint_artifact_id: None,
            agent_phase_started_at: None,
            conversation_id: None,
            run_prompt: input.run_prompt,
            prompt_author: input.prompt_author,
            base_ref_kind: input.base_ref_kind,
            base_ref_used: input.base_ref_used,
            base_from_run_id: input.base_from_run_id,
            goal_item_id: current_goal_item_id(automation.goal_items_json.as_deref()),
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
                automation_id: created.automation_id.clone(),
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
        let changed = self
            .transition_service
            .transition_judge_state(
                &input.previous_run.id,
                AutomationJudgeState::InProgress,
                AutomationJudgeState::Done,
                AutomationJudgeTransitionGuard::Settle(input.judge_lease_expires_at),
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
                noop_reason: Some(AutomationJudgeApplyNoopReason::NotCurrent),
                reason: Some("judge state changed before verdict persistence".to_string()),
            });
        }

        self.apply_judge_verdict_effects(ApplyAutomationJudgeVerdictInput {
            automation: input.automation,
            previous_run: input.previous_run,
            verdict: input.verdict,
        })
        .await
    }

    pub async fn apply_persisted_judge_verdict(
        &self,
        input: ApplyAutomationJudgeVerdictInput,
    ) -> AppResult<AutomationJudgeApplyOutcome> {
        self.apply_judge_verdict_effects(input).await
    }

    pub async fn apply_stored_judge_verdict(
        &self,
        automation_id: &AutomationId,
        previous_run_id: &AutomationRunId,
    ) -> AppResult<AutomationJudgeApplyOutcome> {
        let Some((automation, latest)) = self
            .current_judge_verdict_authority(automation_id, previous_run_id)
            .await?
        else {
            return Ok(AutomationJudgeApplyOutcome {
                successor_run: None,
                terminal_automation_status: None,
                noop_reason: Some(AutomationJudgeApplyNoopReason::NotCurrent),
                reason: Some("judge verdict is no longer current".to_string()),
            });
        };
        let Some(verdict_json) = latest.judge_verdict_json.as_deref() else {
            return Ok(AutomationJudgeApplyOutcome {
                successor_run: None,
                terminal_automation_status: None,
                noop_reason: Some(AutomationJudgeApplyNoopReason::NotCurrent),
                reason: Some("judge verdict is missing".to_string()),
            });
        };
        let verdict = match parse_automation_judge_verdict(
            verdict_json,
            AutomationJudgeValidationContext {
                automation: &automation,
                previous_run: &latest,
            },
        ) {
            Ok(verdict) => verdict,
            Err(error) => {
                return self
                    .mark_stored_judge_verdict_failed(
                        &automation,
                        &latest,
                        format!("Automation stored judge verdict is invalid: {error}"),
                    )
                    .await;
            }
        };

        self.apply_judge_verdict_effects_with_current(automation, latest, verdict)
            .await
    }

    pub(crate) async fn sync_goal_items_for_closed_run_without_successor(
        &self,
        automation_id: &AutomationId,
    ) {
        let automation = match self.automation_repo.get_by_id(automation_id).await {
            Ok(Some(automation)) => automation,
            Ok(None) => {
                tracing::warn!(
                    automation_id = %automation_id,
                    "Failed to sync automation goal items after run close: automation not found"
                );
                return;
            }
            Err(error) => {
                tracing::warn!(
                    automation_id = %automation_id,
                    error = %error,
                    "Failed to sync automation goal items after run close"
                );
                return;
            }
        };

        let updated_goal_items_json =
            match revert_in_progress_goal_items_to_pending(automation.goal_items_json.as_deref()) {
                Ok(Some(updated)) => updated,
                Ok(None) => return,
                Err(error) => {
                    tracing::warn!(
                        automation_id = %automation_id,
                        error = %error,
                        "Failed to sync automation goal items after run close"
                    );
                    return;
                }
            };

        match self
            .automation_repo
            .update_goal_items_json_if_unchanged(
                automation_id,
                automation.goal_items_json,
                Some(updated_goal_items_json),
            )
            .await
        {
            Ok(Some(_)) => {
                self.event_emitter.emit(AutomationEvent::AutomationUpdated {
                    automation_id: automation_id.clone(),
                });
            }
            Ok(None) => {
                tracing::warn!(
                    automation_id = %automation_id,
                    "Skipped automation goal item close sync because stored goal items changed"
                );
            }
            Err(error) => {
                tracing::warn!(
                    automation_id = %automation_id,
                    error = %error,
                    "Failed to sync automation goal items after run close"
                );
            }
        }
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
    ) -> AppResult<AutomationJudgeApplyOutcome> {
        let Some((automation, latest)) = self
            .current_judge_verdict_authority(&input.automation.id, &input.previous_run.id)
            .await?
        else {
            return Ok(AutomationJudgeApplyOutcome {
                successor_run: None,
                terminal_automation_status: None,
                noop_reason: Some(AutomationJudgeApplyNoopReason::NotCurrent),
                reason: Some("judge verdict is no longer current".to_string()),
            });
        };

        self.apply_judge_verdict_effects_with_current(automation, latest, input.verdict)
            .await
    }

    async fn apply_judge_verdict_effects_with_current(
        &self,
        automation: Automation,
        latest: AutomationRun,
        verdict: AutomationJudgeVerdict,
    ) -> AppResult<AutomationJudgeApplyOutcome> {
        let applied_goal_items = apply_updated_item_statuses(
            automation.goal_items_json.as_deref(),
            verdict.updated_item_statuses.as_deref(),
        )?;
        if applied_goal_items.as_deref() != automation.goal_items_json.as_deref() {
            self.automation_repo
                .update_goal_items_json_if_unchanged(
                    &automation.id,
                    automation.goal_items_json.clone(),
                    applied_goal_items.clone(),
                )
                .await?
                .ok_or_else(|| {
                    AppError::Conflict(format!(
                        "automation {} goal items changed before judge verdict could apply",
                        automation.id.as_str()
                    ))
                })?;
        }

        match verdict.decision {
            AutomationJudgeDecision::Continue => {
                if automation_judge_loop_suspected(&latest, &verdict) {
                    self.transition_automation_status_or_conflict(
                        &automation.id,
                        AutomationStatus::Active,
                        AutomationStatus::Paused,
                        Some("judge_loop_suspected".to_string()),
                        Some(
                            "Automation judge produced the same next run prompt after a non-merged run"
                                .to_string(),
                        ),
                    )
                    .await?;
                    self.sync_goal_items_for_closed_run_without_successor(&automation.id)
                        .await;
                    return Ok(AutomationJudgeApplyOutcome {
                        successor_run: None,
                        terminal_automation_status: Some(AutomationStatus::Paused),
                        noop_reason: None,
                        reason: Some("judge_loop_suspected".to_string()),
                    });
                }
                let next_prompt = verdict
                    .next_run_prompt
                    .as_deref()
                    .unwrap_or("")
                    .trim()
                    .to_string();
                if let SuccessorReadiness::NotScheduled(outcome) = self
                    .successor_readiness(&automation, &latest, false)
                    .await?
                {
                    let outcome = *outcome;
                    return Ok(AutomationJudgeApplyOutcome {
                        successor_run: outcome.run,
                        terminal_automation_status: None,
                        noop_reason: None,
                        reason: outcome.reason,
                    });
                }

                let (base_ref_kind, base_ref_used) =
                    judge_successor_base(&automation, &latest, &verdict)?;

                if let Some(proposed_goal_items_json) = goal_items_proposal_json(&verdict)? {
                    self.persist_pending_goal_replan(
                        &automation,
                        &latest,
                        applied_goal_items.as_deref(),
                        proposed_goal_items_json,
                        &verdict.reason,
                    )
                    .await?;
                }

                // Stamp from the post-verdict goal items: the judge just marked
                // finished items done, so the current item is the one this
                // successor run will advance.
                let successor = pending_successor_run(
                    automation.id.clone(),
                    &latest,
                    latest.run_index + 1,
                    next_prompt,
                    AutomationPromptAuthor::Judge,
                    base_ref_kind,
                    base_ref_used,
                    current_goal_item_id(applied_goal_items.as_deref()),
                );
                match self
                    .create_judge_successor_run(&automation, &latest, successor)
                    .await?
                {
                    Some(run) => Ok(AutomationJudgeApplyOutcome {
                        successor_run: Some(run),
                        terminal_automation_status: None,
                        noop_reason: None,
                        reason: None,
                    }),
                    None => Ok(AutomationJudgeApplyOutcome {
                        successor_run: None,
                        terminal_automation_status: None,
                        noop_reason: None,
                        reason: Some(
                            self.judge_successor_not_scheduled_reason(&automation)
                                .await?,
                        ),
                    }),
                }
            }
            AutomationJudgeDecision::Stop if verdict.goal_met => {
                self.transition_automation_status_or_conflict(
                    &automation.id,
                    AutomationStatus::Active,
                    AutomationStatus::Completed,
                    None,
                    None,
                )
                .await?;
                Ok(AutomationJudgeApplyOutcome {
                    successor_run: None,
                    terminal_automation_status: Some(AutomationStatus::Completed),
                    noop_reason: None,
                    reason: None,
                })
            }
            AutomationJudgeDecision::Stop => {
                self.transition_automation_status_or_conflict(
                    &automation.id,
                    AutomationStatus::Active,
                    AutomationStatus::Paused,
                    Some("judge_stopped_unmet".to_string()),
                    Some(verdict.reason.clone()),
                )
                .await?;
                self.sync_goal_items_for_closed_run_without_successor(&automation.id)
                    .await;
                Ok(AutomationJudgeApplyOutcome {
                    successor_run: None,
                    terminal_automation_status: Some(AutomationStatus::Paused),
                    noop_reason: None,
                    reason: Some("judge_stopped_unmet".to_string()),
                })
            }
        }
    }

    async fn persist_pending_goal_replan(
        &self,
        automation: &Automation,
        source_run: &AutomationRun,
        base_goal_items_json: Option<&str>,
        proposed_goal_items_json: String,
        reason: &str,
    ) -> AppResult<()> {
        let base_goal_items_json = base_goal_items_json.ok_or_else(|| {
            AppError::Validation("goalItemsProposal requires stored goal items".to_string())
        })?;
        let current = self.require_automation(&automation.id).await?;
        if current.goal_items_json.as_deref() != Some(base_goal_items_json) {
            return Err(AppError::Conflict(format!(
                "automation {} goal items changed before re-plan proposal persistence",
                automation.id.as_str()
            )));
        }
        let mut state = parse_authoring_state(current.authoring_state_json.as_deref())?;
        let next = AutomationGoalReplanState {
            source_run_id: source_run.id.as_str().to_string(),
            base_goal_items_json: base_goal_items_json.to_string(),
            proposed_goal_items_json,
            reason: reason.to_string(),
            status: AutomationGoalReplanStatus::Pending,
            created_at: Utc::now().to_rfc3339(),
            applied_at: None,
        };
        if state.pending_goal_replan.as_ref().is_some_and(|stored| {
            stored.source_run_id == next.source_run_id
                && stored.base_goal_items_json == next.base_goal_items_json
                && stored.proposed_goal_items_json == next.proposed_goal_items_json
                && stored.status == AutomationGoalReplanStatus::Pending
        }) {
            return Ok(());
        }
        if state.pending_goal_replan.as_ref().is_some_and(|stored| {
            stored.status == AutomationGoalReplanStatus::Pending
                && stored.source_run_id != next.source_run_id
        }) {
            return Err(AppError::Conflict(
                "automation already has a pending goal re-plan proposal".to_string(),
            ));
        }
        state.pending_goal_replan = Some(next);
        if !self
            .persist_authoring_state_if_unchanged(&current, &state)
            .await?
        {
            return Err(AppError::Conflict(format!(
                "automation {} changed before re-plan proposal persistence",
                automation.id.as_str()
            )));
        }
        Ok(())
    }

    async fn create_judge_successor_run(
        &self,
        automation: &Automation,
        latest: &AutomationRun,
        successor: AutomationRun,
    ) -> AppResult<Option<AutomationRun>> {
        let created = self
            .run_repo
            .create_judge_successor_run(&automation.id, &latest.id, successor)
            .await?;
        if let Some(run) = created.as_ref() {
            self.event_emitter
                .emit(AutomationEvent::AutomationRunUpdated {
                    automation_id: automation.id.clone(),
                    run_id: latest.id.clone(),
                });
            self.event_emitter
                .emit(AutomationEvent::AutomationRunUpdated {
                    automation_id: automation.id.clone(),
                    run_id: run.id.clone(),
                });
        }
        Ok(created)
    }

    async fn judge_successor_not_scheduled_reason(
        &self,
        automation: &Automation,
    ) -> AppResult<String> {
        let Some(current) = self.automation_repo.get_by_id(&automation.id).await? else {
            return Ok("automation is not active".to_string());
        };
        if current.status != AutomationStatus::Active {
            return Ok("automation is not active".to_string());
        }
        Ok("successor already scheduled".to_string())
    }

    async fn mark_stored_judge_verdict_failed(
        &self,
        automation: &Automation,
        latest: &AutomationRun,
        detail: String,
    ) -> AppResult<AutomationJudgeApplyOutcome> {
        let changed = self
            .transition_service
            .transition_judge_state(
                &latest.id,
                AutomationJudgeState::Done,
                AutomationJudgeState::Failed,
                AutomationJudgeTransitionGuard::Dispatch,
                None,
                None,
                None,
                Some(detail.clone()),
            )
            .await?;
        if !changed {
            return Ok(AutomationJudgeApplyOutcome {
                successor_run: None,
                terminal_automation_status: None,
                noop_reason: Some(AutomationJudgeApplyNoopReason::NotCurrent),
                reason: Some("judge verdict is no longer current".to_string()),
            });
        }
        let paused = self
            .transition_service
            .transition_automation_status(
                &automation.id,
                AutomationStatus::Active,
                AutomationStatus::Paused,
                Some(JUDGE_FAILED_PAUSED_REASON_CODE.to_string()),
                Some(detail),
            )
            .await?;
        if paused {
            self.sync_goal_items_for_closed_run_without_successor(&automation.id)
                .await;
            return Ok(AutomationJudgeApplyOutcome {
                successor_run: None,
                terminal_automation_status: Some(AutomationStatus::Paused),
                noop_reason: None,
                reason: Some(JUDGE_FAILED_PAUSED_REASON_CODE.to_string()),
            });
        }
        Ok(AutomationJudgeApplyOutcome {
            successor_run: None,
            terminal_automation_status: None,
            noop_reason: Some(AutomationJudgeApplyNoopReason::NotCurrent),
            reason: Some("judge verdict is no longer current".to_string()),
        })
    }

    async fn current_judge_verdict_authority(
        &self,
        automation_id: &AutomationId,
        previous_run_id: &AutomationRunId,
    ) -> AppResult<Option<(Automation, AutomationRun)>> {
        let Some(automation) = self.automation_repo.get_by_id(automation_id).await? else {
            return Ok(None);
        };
        if automation.status != AutomationStatus::Active {
            return Ok(None);
        }
        let Some(latest) = self.run_repo.latest_for_automation(automation_id).await? else {
            return Ok(None);
        };
        if latest.id != *previous_run_id
            || latest.judge_state != AutomationJudgeState::Done
            || latest.judge_verdict_json.is_none()
        {
            return Ok(None);
        }
        Ok(Some((automation, latest)))
    }

    async fn successor_readiness(
        &self,
        automation: &Automation,
        latest: &AutomationRun,
        allow_unjudged_latest: bool,
    ) -> AppResult<SuccessorReadiness> {
        let skippable_terminal = allow_unjudged_latest
            && is_signal_terminal_automation_run(latest.status)
            && matches!(
                latest.judge_state,
                AutomationJudgeState::None | AutomationJudgeState::Failed
            );
        if is_open_automation_run(latest.status, latest.judge_state) && !skippable_terminal {
            return Ok(SuccessorReadiness::NotScheduled(Box::new(
                successor_not_scheduled("run in flight"),
            )));
        }
        if !is_signal_terminal_automation_run(latest.status) {
            return Err(AppError::Validation(
                "previous run is not signal-terminal".to_string(),
            ));
        }

        let runs = self.run_repo.list_for_automation(&automation.id).await?;
        if runs.len() as i64 >= automation.max_runs {
            let paused = self
                .transition_service
                .transition_automation_status(
                    &automation.id,
                    AutomationStatus::Active,
                    AutomationStatus::Paused,
                    Some("max_runs_exhausted".to_string()),
                    Some("Automation reached max_runs before scheduling a successor".to_string()),
                )
                .await?;
            if paused {
                self.sync_goal_items_for_closed_run_without_successor(&automation.id)
                    .await;
            }
            return Ok(SuccessorReadiness::NotScheduled(Box::new(
                successor_not_scheduled(if paused {
                    "max_runs_exhausted"
                } else {
                    "already settled"
                }),
            )));
        }
        if consecutive_failure_count(&runs) >= automation.max_consecutive_failures {
            let paused = self
                .transition_service
                .transition_automation_status(
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
            if paused {
                self.sync_goal_items_for_closed_run_without_successor(&automation.id)
                    .await;
            }
            return Ok(SuccessorReadiness::NotScheduled(Box::new(
                successor_not_scheduled(if paused {
                    "max_consecutive_failures"
                } else {
                    "already settled"
                }),
            )));
        }
        Ok(SuccessorReadiness::Ready)
    }
}

enum SuccessorReadiness {
    Ready,
    NotScheduled(Box<AutomationSuccessorRunOutcome>),
}

fn current_unmet_stop_run<'a>(
    automation: &Automation,
    runs: &'a [AutomationRun],
) -> AppResult<&'a AutomationRun> {
    let invalid = |detail: &str| {
        AppError::Validation(format!(
            "judge_stopped_unmet resume requires a current unmet stop verdict: {detail}"
        ))
    };
    let latest = runs
        .last()
        .ok_or_else(|| invalid("automation has no runs"))?;
    if !is_signal_terminal_automation_run(latest.status)
        || latest.judge_state != AutomationJudgeState::Done
    {
        return Err(invalid("latest run is not terminal with a completed judge"));
    }
    let verdict_json = latest
        .judge_verdict_json
        .as_deref()
        .ok_or_else(|| invalid("latest run has no stored judge verdict"))?;
    let verdict = parse_automation_judge_verdict(
        verdict_json,
        AutomationJudgeValidationContext {
            automation,
            previous_run: latest,
        },
    )
    .map_err(|error| invalid(&format!("latest verdict is invalid: {error}")))?;
    if verdict.decision != AutomationJudgeDecision::Stop || verdict.goal_met {
        return Err(invalid("latest verdict is not stop with goalMet=false"));
    }
    if latest.run_prompt.trim().is_empty() {
        return Err(invalid("latest run prompt is empty"));
    }
    let run_count = runs.len() as i64;
    if run_count >= automation.max_runs {
        return Err(AppError::Validation(format!(
            "{run_count} runs already reached the configured limit {}. Reopen the last run to continue in place, or raise maxRuns.",
            automation.max_runs
        )));
    }
    let failure_count = consecutive_failure_count(runs);
    if failure_count >= automation.max_consecutive_failures {
        return Err(AppError::Validation(format!(
            "{failure_count} consecutive runs failed (limit {}). Reopen the last run to continue in place, or raise maxConsecutiveFailures.",
            automation.max_consecutive_failures
        )));
    }
    Ok(latest)
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
    goal_item_id: Option<String>,
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
        plan_last_parked_blueprint_artifact_id: None,
        agent_phase_started_at: None,
        conversation_id: None,
        run_prompt,
        prompt_author,
        base_ref_kind,
        base_ref_used,
        base_from_run_id: Some(previous_run.id.clone()),
        goal_item_id,
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

fn completion_signal_for_run_mode(run_mode: &str) -> &'static str {
    match run_mode.trim() {
        DEFAULT_RUN_MODE => DEFAULT_COMPLETION_SIGNAL,
        IDEATION_BRIDGE_RUN_MODE => IDEATION_FINALIZED_COMPLETION_SIGNAL,
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

pub(crate) fn validate_finalizable(automation: &Automation) -> AppResult<()> {
    if automation.status != AutomationStatus::Draft {
        return Err(AppError::InvalidTransition {
            from: automation.status.as_str().to_string(),
            to: AutomationStatus::Active.as_str().to_string(),
        });
    }
    validate_activation_configuration(automation)
}

fn validate_activation_configuration(automation: &Automation) -> AppResult<()> {
    reject_persona_builder_workspace_mode(&automation.run_mode).map_err(AppError::Validation)?;
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
    if automation.run_mode == IDEATION_BRIDGE_RUN_MODE && !automation.plan_deep_verification {
        return Err(AppError::Validation(
            "ideation bridge automations require deep plan verification".to_string(),
        ));
    }
    if automation.run_mode == IDEATION_BRIDGE_RUN_MODE
        && automation.completion_signal != IDEATION_FINALIZED_COMPLETION_SIGNAL
    {
        return Err(AppError::Validation(
            "ideation bridge automations require ideation_finalized completion".to_string(),
        ));
    }
    match automation.completion_signal.as_str() {
        DEFAULT_COMPLETION_SIGNAL if automation.run_mode != DEFAULT_RUN_MODE => {
            return Err(AppError::Validation(
                "pr_merged automations require edit run_mode".to_string(),
            ));
        }
        IDEATION_FINALIZED_COMPLETION_SIGNAL if automation.run_mode == IDEATION_BRIDGE_RUN_MODE => {
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

fn restart_run_input(
    automation: &Automation,
    latest: Option<&AutomationRun>,
) -> AppResult<CreateAutomationRunInput> {
    if let Some(latest) = latest {
        return Ok(CreateAutomationRunInput {
            automation_id: automation.id.clone(),
            run_prompt: latest.run_prompt.clone(),
            prompt_author: latest.prompt_author,
            base_ref_kind: latest.base_ref_kind.clone(),
            base_ref_used: latest.base_ref_used.clone(),
            base_from_run_id: latest.base_from_run_id.clone(),
        });
    }
    let run_prompt = automation
        .first_run_prompt
        .as_deref()
        .map(str::trim)
        .filter(|prompt| !prompt.is_empty())
        .ok_or_else(|| {
            AppError::Validation(
                "automation first_run_prompt is required before restart".to_string(),
            )
        })?
        .to_string();
    Ok(CreateAutomationRunInput {
        automation_id: automation.id.clone(),
        run_prompt,
        prompt_author: AutomationPromptAuthor::SetupAgent,
        base_ref_kind: automation.base_ref_kind.clone(),
        base_ref_used: automation.base_ref.clone(),
        base_from_run_id: None,
    })
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
