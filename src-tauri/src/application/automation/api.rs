use std::sync::Arc;

use serde::Serialize;

use crate::application::automation::service::{
    AutomationDetail, AutomationScheduleOutcome, AutomationService, LOCAL_BRANCH_BASE_REF_KIND,
};
use crate::application::automation::transition::{
    AutomationEventEmitter, AutomationTransitionService, NoopAutomationEventEmitter,
    TauriAutomationEventEmitter,
};
use crate::application::AppState;
use crate::domain::entities::{
    is_open_automation_run, AgentConversationWorkspaceMode, AgentRun, Automation, AutomationRun,
    IdeationSessionStatus, InternalStatus,
};

use super::decomposition_verifier::{
    parse_authoring_state, AutomationAuthoringState, AutomationDecompositionVerificationStatus,
};
use super::plan_gate::{
    current_plan_artifact_ids_for_workspace, matching_plan_approval_for_workspace,
};

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
    pub base_target_ref: Option<String>,
    pub base_target_display_name: Option<String>,
    pub base_source_pull_request_json: Option<String>,
    pub goal_items_json: Option<String>,
    pub chain_mode: String,
    pub completion_signal: String,
    pub plan_approval_mode: String,
    pub pr_merge_mode: String,
    pub plan_deep_verification: bool,
    pub max_runs: i64,
    pub max_consecutive_failures: i64,
    pub first_run_prompt: Option<String>,
    pub setup_analysis_summary: Option<String>,
    pub spec_artifact_id: Option<String>,
    pub authoring_mode: String,
    pub decomposition_verification_status: String,
    pub decomposition_verification_verdict_json: Option<String>,
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
    pub plan_judge_state: String,
    pub plan_revision_round: i64,
    pub plan_revision_pending: bool,
    pub plan_phase: bool,
    pub plan_artifact_id: Option<String>,
    pub plan_blueprint_artifact_id: Option<String>,
    pub parked_plan_artifact_id: Option<String>,
    pub parked_plan_blueprint_artifact_id: Option<String>,
    pub plan_approved_by: Option<String>,
    pub plan_approved_artifact_version: Option<u32>,
    pub plan_approved_at: Option<String>,
    pub conversation_id: Option<String>,
    pub run_prompt: String,
    pub prompt_author: String,
    pub base_ref_kind: String,
    pub base_ref_used: String,
    pub base_from_run_id: Option<String>,
    pub goal_item_id: Option<String>,
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
    pub pipeline: Option<AutomationPipelineProgressResponse>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AutomationPipelineTaskResponse {
    pub id: String,
    pub title: String,
    pub status: String,
    pub blocked_by: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AutomationPipelineProgressResponse {
    pub deliverable: String,
    pub status: String,
    pub ideation_session_id: String,
    pub plan_artifact_id: Option<String>,
    pub proposal_count: u32,
    pub task_total: u32,
    pub task_merged: u32,
    pub task_terminal: u32,
    pub tasks: Vec<AutomationPipelineTaskResponse>,
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

pub fn automation_event_emitter_for_state(state: &AppState) -> Arc<dyn AutomationEventEmitter> {
    match state.app_handle.as_ref() {
        Some(app_handle) => Arc::new(TauriAutomationEventEmitter::new(app_handle.clone())),
        None => Arc::new(NoopAutomationEventEmitter),
    }
}

pub fn automation_service_for_state(state: &AppState) -> AutomationService {
    let event_emitter = automation_event_emitter_for_state(state);
    AutomationService::new(
        state.automation_repo.clone(),
        state.automation_run_repo.clone(),
        event_emitter,
        state.artifact_repo.clone(),
        state.notification_service(),
    )
    .with_pr_auto_merge_controls(
        state.agent_conversation_workspace_repo.clone(),
        state.github_service.clone(),
    )
}

pub fn automation_transition_service_for_state(state: &AppState) -> AutomationTransitionService {
    AutomationTransitionService::new(
        state.automation_repo.clone(),
        state.automation_run_repo.clone(),
        automation_event_emitter_for_state(state),
        state.notification_service(),
    )
}

pub async fn automation_detail_response_for_state(
    detail: AutomationDetail,
    state: &AppState,
) -> crate::error::AppResult<AutomationDetailResponse> {
    let setup_conversation_id = (detail.automation.base_ref_kind == LOCAL_BRANCH_BASE_REF_KIND)
        .then(|| detail.automation.setup_conversation_id.clone())
        .flatten();
    let usage = automation_usage_for_runs(&detail.runs, state).await?;
    let pipeline = automation_pipeline_progress_for_state(&detail, state).await?;
    let runs = automation_run_responses_for_state(detail.runs, state).await?;
    let mut automation = AutomationResponse::from(detail.automation);
    if let Some(setup_conversation_id) = setup_conversation_id {
        if let Some(workspace) = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&setup_conversation_id)
            .await?
        {
            let base_target_ref = workspace.base_ref;
            if !base_target_ref.trim().is_empty() && base_target_ref != automation.base_ref {
                automation.base_target_display_name = workspace
                    .base_display_name
                    .or_else(|| Some(base_target_ref.clone()));
                automation.base_target_ref = Some(base_target_ref);
            }
        }
    }
    Ok(AutomationDetailResponse {
        automation,
        runs,
        usage,
        pipeline,
    })
}

async fn automation_pipeline_progress_for_state(
    detail: &AutomationDetail,
    state: &AppState,
) -> crate::error::AppResult<Option<AutomationPipelineProgressResponse>> {
    if detail.automation.run_mode
        != crate::application::automation::service::IDEATION_BRIDGE_RUN_MODE
    {
        return Ok(None);
    }

    let mut linked_session_id = None;
    for run in detail.runs.iter().rev() {
        let Some(conversation_id) = run.conversation_id.as_ref() else {
            continue;
        };
        let Some(workspace) = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(conversation_id)
            .await?
        else {
            continue;
        };
        if workspace.linked_ideation_session_id.is_some() {
            linked_session_id = workspace.linked_ideation_session_id;
            break;
        }
    }
    let Some(session_id) = linked_session_id else {
        return Ok(None);
    };
    let Some(session) = state.ideation_session_repo.get_by_id(&session_id).await? else {
        return Ok(None);
    };

    let proposals = state.task_proposal_repo.get_by_session(&session_id).await?;
    let tasks = state.task_repo.get_by_ideation_session(&session_id).await?;
    let task_ids = tasks.iter().map(|task| task.id.clone()).collect::<Vec<_>>();
    let blockers = state
        .task_dependency_repo
        .get_blockers_batch(&task_ids)
        .await?;
    let task_merged = tasks
        .iter()
        .filter(|task| task.internal_status == InternalStatus::Merged)
        .count() as u32;
    let task_terminal = tasks.iter().filter(|task| task.is_terminal()).count() as u32;
    let has_failed_terminal = tasks
        .iter()
        .any(|task| task.is_terminal() && task.internal_status != InternalStatus::Merged);
    let status = if has_failed_terminal {
        "attention"
    } else if session.status == IdeationSessionStatus::Accepted
        && (tasks.is_empty() || task_merged == tasks.len() as u32)
    {
        "completed"
    } else if tasks.is_empty() {
        "authoring"
    } else {
        "executing"
    };
    let tasks = tasks
        .into_iter()
        .map(|task| AutomationPipelineTaskResponse {
            blocked_by: blockers
                .get(&task.id)
                .into_iter()
                .flatten()
                .map(ToString::to_string)
                .collect(),
            id: task.id.to_string(),
            title: task.title,
            status: task.internal_status.to_string(),
        })
        .collect();

    Ok(Some(AutomationPipelineProgressResponse {
        deliverable: "task_graph".to_string(),
        status: status.to_string(),
        ideation_session_id: session.id.to_string(),
        plan_artifact_id: session.plan_artifact_id.map(|id| id.to_string()),
        proposal_count: proposals
            .iter()
            .filter(|proposal| proposal.archived_at.is_none())
            .count() as u32,
        task_total: task_ids.len() as u32,
        task_merged,
        task_terminal,
        tasks,
    }))
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

#[derive(Default)]
struct AutomationRunPlanReadModel {
    plan_phase: bool,
    plan_artifact_id: Option<String>,
    plan_blueprint_artifact_id: Option<String>,
    plan_approved_by: Option<String>,
    plan_approved_artifact_version: Option<u32>,
    plan_approved_at: Option<String>,
}

async fn automation_run_responses_for_state(
    runs: Vec<AutomationRun>,
    state: &AppState,
) -> crate::error::AppResult<Vec<AutomationRunResponse>> {
    let mut responses = Vec::with_capacity(runs.len());
    for run in runs {
        responses.push(automation_run_response_for_state(run, state).await?);
    }
    Ok(responses)
}

pub(crate) async fn automation_run_response_for_state(
    run: AutomationRun,
    state: &AppState,
) -> crate::error::AppResult<AutomationRunResponse> {
    let plan = automation_run_plan_read_model_for_state(&run, state).await?;
    Ok(AutomationRunResponse::from_run_with_plan_read_model(
        run, plan,
    ))
}

async fn automation_run_plan_read_model_for_state(
    run: &AutomationRun,
    state: &AppState,
) -> crate::error::AppResult<AutomationRunPlanReadModel> {
    let parked_plan_artifact_id = run.plan_last_parked_artifact_id.clone();
    let parked_plan_blueprint_artifact_id = run.plan_last_parked_blueprint_artifact_id.clone();
    let Some(conversation_id) = run.conversation_id.as_ref() else {
        return Ok(AutomationRunPlanReadModel {
            plan_artifact_id: parked_plan_artifact_id,
            plan_blueprint_artifact_id: parked_plan_blueprint_artifact_id,
            ..AutomationRunPlanReadModel::default()
        });
    };
    let Some(workspace) = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(conversation_id)
        .await?
    else {
        return Ok(AutomationRunPlanReadModel {
            plan_artifact_id: parked_plan_artifact_id,
            plan_blueprint_artifact_id: parked_plan_blueprint_artifact_id,
            ..AutomationRunPlanReadModel::default()
        });
    };

    let open = is_open_automation_run(run.status, run.judge_state);
    let current_plan_artifacts =
        current_plan_artifact_ids_for_workspace(&state.ideation_session_repo, &workspace).await?;
    let plan_artifact_id = current_plan_artifacts
        .as_ref()
        .map(|artifacts| artifacts.overview_id.clone())
        .or(parked_plan_artifact_id);
    let plan_blueprint_artifact_id = current_plan_artifacts
        .and_then(|artifacts| artifacts.blueprint_id)
        .or(parked_plan_blueprint_artifact_id);
    let approval = if open {
        matching_plan_approval_for_workspace(
            &state.ideation_session_repo,
            &state.plan_approval_repo,
            &workspace,
        )
        .await?
    } else {
        None
    };

    Ok(AutomationRunPlanReadModel {
        plan_phase: open && workspace.mode == AgentConversationWorkspaceMode::Plan,
        plan_artifact_id,
        plan_blueprint_artifact_id,
        plan_approved_by: approval
            .as_ref()
            .map(|approval| approval.approved_by.clone()),
        plan_approved_artifact_version: approval.as_ref().map(|approval| approval.artifact_version),
        plan_approved_at: approval.map(|approval| approval.approved_at),
    })
}

impl From<Automation> for AutomationResponse {
    fn from(automation: Automation) -> Self {
        let authoring_state = parse_authoring_state(automation.authoring_state_json.as_deref())
            .unwrap_or_else(|_| AutomationAuthoringState {
                verification_status: AutomationDecompositionVerificationStatus::Failed,
                ..AutomationAuthoringState::default()
            });
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
            base_target_ref: None,
            base_target_display_name: None,
            base_source_pull_request_json: automation.base_source_pull_request_json,
            goal_items_json: automation.goal_items_json,
            chain_mode: automation.chain_mode,
            completion_signal: automation.completion_signal,
            plan_approval_mode: automation.plan_approval_mode.as_str().to_string(),
            pr_merge_mode: automation.pr_merge_mode.as_str().to_string(),
            plan_deep_verification: automation.plan_deep_verification,
            max_runs: automation.max_runs,
            max_consecutive_failures: automation.max_consecutive_failures,
            first_run_prompt: automation.first_run_prompt,
            setup_analysis_summary: automation.setup_analysis_summary,
            spec_artifact_id: automation.spec_artifact_id,
            authoring_mode: authoring_state.mode.as_str().to_string(),
            decomposition_verification_status: authoring_state
                .verification_status
                .as_str()
                .to_string(),
            decomposition_verification_verdict_json: authoring_state.verdict_json,
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
            plan_judge_state: run.plan_judge_state.as_str().to_string(),
            plan_revision_round: run.plan_revision_round,
            plan_revision_pending: run.plan_pending_instructions.is_some(),
            plan_phase: false,
            plan_artifact_id: None,
            plan_blueprint_artifact_id: None,
            parked_plan_artifact_id: run.plan_last_parked_artifact_id.clone(),
            parked_plan_blueprint_artifact_id: run.plan_last_parked_blueprint_artifact_id.clone(),
            plan_approved_by: None,
            plan_approved_artifact_version: None,
            plan_approved_at: None,
            conversation_id: run.conversation_id.map(|id| id.as_str()),
            run_prompt: run.run_prompt,
            prompt_author: run.prompt_author.as_str().to_string(),
            base_ref_kind: run.base_ref_kind,
            base_ref_used: run.base_ref_used,
            base_from_run_id: run.base_from_run_id.map(|id| id.as_str().to_string()),
            goal_item_id: run.goal_item_id,
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

impl AutomationRunResponse {
    fn from_run_with_plan_read_model(run: AutomationRun, plan: AutomationRunPlanReadModel) -> Self {
        let mut response = Self::from(run);
        response.plan_phase = plan.plan_phase;
        response.plan_artifact_id = plan.plan_artifact_id;
        response.plan_blueprint_artifact_id = plan.plan_blueprint_artifact_id;
        response.plan_approved_by = plan.plan_approved_by;
        response.plan_approved_artifact_version = plan.plan_approved_artifact_version;
        response.plan_approved_at = plan.plan_approved_at;
        response
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
