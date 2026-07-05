use std::sync::Arc;

use serde::Serialize;

use crate::application::automation::service::{
    AutomationDetail, AutomationScheduleOutcome, AutomationService,
};
use crate::application::automation::transition::NoopAutomationEventEmitter;
use crate::application::AppState;
use crate::domain::entities::{AgentRun, Automation, AutomationRun};

#[derive(Debug, Clone, Serialize)]
pub struct AutomationResponse {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub status: String,
    pub paused_reason_code: Option<String>,
    pub paused_reason_detail: Option<String>,
    pub goal_prompt: String,
    pub setup_conversation_id: Option<String>,
    pub provider_harness: String,
    pub model_id: String,
    pub logical_effort: Option<String>,
    pub run_mode: String,
    pub base_ref_kind: String,
    pub base_ref: String,
    pub base_display_name: Option<String>,
    pub base_source_pull_request_json: Option<String>,
    pub goal_items_json: Option<String>,
    pub chain_mode: String,
    pub completion_signal: String,
    pub max_runs: i64,
    pub max_consecutive_failures: i64,
    pub first_run_prompt: Option<String>,
    pub setup_analysis_summary: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AutomationRunResponse {
    pub id: String,
    pub automation_id: String,
    pub run_index: i64,
    pub status: String,
    pub judge_state: String,
    pub judge_lease_expires_at: Option<String>,
    pub conversation_id: Option<String>,
    pub run_prompt: String,
    pub prompt_author: String,
    pub base_ref_kind: String,
    pub base_ref_used: String,
    pub base_from_run_id: Option<String>,
    pub branch_name: Option<String>,
    pub pr_number: Option<i64>,
    pub pr_url: Option<String>,
    pub pr_title: Option<String>,
    pub pr_head_ref_name: Option<String>,
    pub pr_base_ref_name: Option<String>,
    pub pr_merged_at: Option<String>,
    pub merge_commit_sha: Option<String>,
    pub diff_stats_json: Option<String>,
    pub agent_summary: Option<String>,
    pub judge_verdict_json: Option<String>,
    pub judge_model_id: Option<String>,
    pub error_code: Option<String>,
    pub error_detail: Option<String>,
    pub signal_check_failures: i64,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AutomationUsageResponse {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_tokens: u64,
    pub cache_read_tokens: u64,
    pub estimated_usd: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AutomationDetailResponse {
    pub automation: AutomationResponse,
    pub runs: Vec<AutomationRunResponse>,
    pub usage: AutomationUsageResponse,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateAutomationDraftResponse {
    pub automation: AutomationResponse,
    pub setup_conversation_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AutomationScheduleResponse {
    pub scheduled: bool,
    pub reason: Option<String>,
}

pub fn automation_service_for_state(state: &AppState) -> AutomationService {
    AutomationService::new(
        state.automation_repo.clone(),
        state.automation_run_repo.clone(),
        Arc::new(NoopAutomationEventEmitter),
    )
}

pub async fn automation_detail_response_for_state(
    detail: AutomationDetail,
    state: &AppState,
) -> crate::error::AppResult<AutomationDetailResponse> {
    let usage = automation_usage_for_runs(&detail.runs, state).await?;
    Ok(AutomationDetailResponse::from_detail(detail, usage))
}

async fn automation_usage_for_runs(
    runs: &[AutomationRun],
    state: &AppState,
) -> crate::error::AppResult<AutomationUsageResponse> {
    let mut usage = AutomationUsageResponse::default();
    for run in runs {
        let Some(conversation_id) = run.conversation_id.as_ref() else {
            continue;
        };
        for agent_run in state
            .agent_run_repo
            .get_by_conversation(conversation_id)
            .await?
        {
            usage.add_agent_run(&agent_run);
        }
    }
    Ok(usage)
}

impl From<Automation> for AutomationResponse {
    fn from(automation: Automation) -> Self {
        Self {
            id: automation.id.as_str().to_string(),
            project_id: automation.project_id.as_str().to_string(),
            name: automation.name,
            status: automation.status.as_str().to_string(),
            paused_reason_code: automation.paused_reason_code,
            paused_reason_detail: automation.paused_reason_detail,
            goal_prompt: automation.goal_prompt,
            setup_conversation_id: automation.setup_conversation_id.map(|id| id.as_str()),
            provider_harness: automation.provider_harness,
            model_id: automation.model_id,
            logical_effort: automation.logical_effort,
            run_mode: automation.run_mode,
            base_ref_kind: automation.base_ref_kind,
            base_ref: automation.base_ref,
            base_display_name: automation.base_display_name,
            base_source_pull_request_json: automation.base_source_pull_request_json,
            goal_items_json: automation.goal_items_json,
            chain_mode: automation.chain_mode,
            completion_signal: automation.completion_signal,
            max_runs: automation.max_runs,
            max_consecutive_failures: automation.max_consecutive_failures,
            first_run_prompt: automation.first_run_prompt,
            setup_analysis_summary: automation.setup_analysis_summary,
            created_at: automation.created_at.to_rfc3339(),
            updated_at: automation.updated_at.to_rfc3339(),
        }
    }
}

impl From<AutomationRun> for AutomationRunResponse {
    fn from(run: AutomationRun) -> Self {
        Self {
            id: run.id.as_str().to_string(),
            automation_id: run.automation_id.as_str().to_string(),
            run_index: run.run_index,
            status: run.status.as_str().to_string(),
            judge_state: run.judge_state.as_str().to_string(),
            judge_lease_expires_at: run.judge_lease_expires_at.map(|dt| dt.to_rfc3339()),
            conversation_id: run.conversation_id.map(|id| id.as_str()),
            run_prompt: run.run_prompt,
            prompt_author: run.prompt_author.as_str().to_string(),
            base_ref_kind: run.base_ref_kind,
            base_ref_used: run.base_ref_used,
            base_from_run_id: run.base_from_run_id.map(|id| id.as_str().to_string()),
            branch_name: run.branch_name,
            pr_number: run.pr_number,
            pr_url: run.pr_url,
            pr_title: run.pr_title,
            pr_head_ref_name: run.pr_head_ref_name,
            pr_base_ref_name: run.pr_base_ref_name,
            pr_merged_at: run.pr_merged_at.map(|dt| dt.to_rfc3339()),
            merge_commit_sha: run.merge_commit_sha,
            diff_stats_json: run.diff_stats_json,
            agent_summary: run.agent_summary,
            judge_verdict_json: run.judge_verdict_json,
            judge_model_id: run.judge_model_id,
            error_code: run.error_code,
            error_detail: run.error_detail,
            signal_check_failures: run.signal_check_failures,
            started_at: run.started_at.map(|dt| dt.to_rfc3339()),
            finished_at: run.finished_at.map(|dt| dt.to_rfc3339()),
            created_at: run.created_at.to_rfc3339(),
            updated_at: run.updated_at.to_rfc3339(),
        }
    }
}

impl AutomationUsageResponse {
    fn add_agent_run(&mut self, run: &AgentRun) {
        self.input_tokens += run.input_tokens.unwrap_or(0);
        self.output_tokens += run.output_tokens.unwrap_or(0);
        self.cache_creation_tokens += run.cache_creation_tokens.unwrap_or(0);
        self.cache_read_tokens += run.cache_read_tokens.unwrap_or(0);
        if let Some(value) = run.estimated_usd {
            self.estimated_usd = Some(self.estimated_usd.unwrap_or(0.0) + value);
        }
    }
}

impl Default for AutomationUsageResponse {
    fn default() -> Self {
        Self {
            input_tokens: 0,
            output_tokens: 0,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
            estimated_usd: None,
        }
    }
}

impl AutomationDetailResponse {
    fn from_detail(detail: AutomationDetail, usage: AutomationUsageResponse) -> Self {
        Self {
            automation: AutomationResponse::from(detail.automation),
            runs: detail
                .runs
                .into_iter()
                .map(AutomationRunResponse::from)
                .collect(),
            usage,
        }
    }
}

impl From<AutomationDetail> for AutomationDetailResponse {
    fn from(detail: AutomationDetail) -> Self {
        Self::from_detail(detail, AutomationUsageResponse::default())
    }
}

impl From<Automation> for CreateAutomationDraftResponse {
    fn from(automation: Automation) -> Self {
        let setup_conversation_id = automation
            .setup_conversation_id
            .as_ref()
            .map(|id| id.as_str());
        Self {
            automation: AutomationResponse::from(automation),
            setup_conversation_id,
        }
    }
}

impl From<AutomationScheduleOutcome> for AutomationScheduleResponse {
    fn from(outcome: AutomationScheduleOutcome) -> Self {
        Self {
            scheduled: outcome.scheduled,
            reason: outcome.reason,
        }
    }
}
