use std::sync::Arc;

use chrono::Utc;

use crate::application::automation::transition::{
    AutomationEvent, AutomationEventEmitter, AutomationTransitionService,
};
use crate::domain::entities::{
    Automation, AutomationId, AutomationJudgeState, AutomationPromptAuthor, AutomationRun,
    AutomationRunId, AutomationRunStatus, AutomationStatus, ProjectId,
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
    pub project_id: ProjectId,
    pub name: Option<String>,
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
            id: AutomationId::new(),
            project_id: input.project_id,
            name: normalize_name(input.name.as_deref())?,
            status: AutomationStatus::Draft,
            paused_reason_code: None,
            paused_reason_detail: None,
            goal_prompt: String::new(),
            setup_conversation_id: None,
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
        self.transition_automation_status_or_conflict(
            id,
            automation.status,
            AutomationStatus::Stopped,
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

    async fn require_automation(&self, id: &AutomationId) -> AppResult<Automation> {
        self.automation_repo
            .get_by_id(id)
            .await?
            .ok_or_else(|| automation_not_found(id))
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

fn automation_not_found(id: &AutomationId) -> AppError {
    AppError::NotFound(format!("automation {} not found", id.as_str()))
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
