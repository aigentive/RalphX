use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;

use crate::domain::entities::{
    AgentConversationWorkspace, AutomationPlanJudgeState, AutomationRun, ChatConversationId,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, AutomationRunRepository, IdeationSessionRepository,
    PlanArtifactApproval, PlanArtifactApprovalRepository,
};
use crate::error::AppResult;

pub(crate) const AUTOMATION_PLAN_REMINDER_PROMPT: &str = r#"The automation run is still in its planning phase. Continue from the current context, write the run plan artifact with the scope, files to inspect or change, approach, risks, and how it advances the current goal item, then end the turn. Do not begin implementation in this turn."#;
pub(crate) const PLAN_JUDGE_FAILED_PAUSED_REASON_CODE: &str = "plan_judge_failed";
pub(crate) const PLAN_REVISION_EXHAUSTED_PAUSED_REASON_CODE: &str = "plan_revision_exhausted";
pub(crate) const PLAN_RESUME_FAILED_ERROR_CODE: &str = "plan_resume_failed";
pub(crate) const AUTOMATION_PLAN_GATE_TRIGGER_RUN_NOW_ERROR_CODE: &str =
    "[ralphx:automation_plan_gate_paused]";

#[async_trait]
pub trait AutomationRunResumer: Send + Sync {
    async fn is_agent_running(&self, conversation_id: &ChatConversationId) -> AppResult<bool>;

    async fn launches_paused(&self) -> AppResult<bool>;

    async fn switch_to_edit(&self, conversation_id: &ChatConversationId) -> AppResult<()>;

    async fn resume_with_prompt(
        &self,
        conversation_id: &ChatConversationId,
        prompt: &str,
    ) -> AppResult<()>;
}

pub(crate) fn is_plan_gate_pause_reason(reason: Option<&str>) -> bool {
    matches!(
        reason,
        Some(PLAN_JUDGE_FAILED_PAUSED_REASON_CODE | PLAN_REVISION_EXHAUSTED_PAUSED_REASON_CODE)
    )
}

pub(crate) fn approval_delivery_prompt(approval: &PlanArtifactApproval) -> String {
    format!(
        "Run plan v{} approved. Implement it now in this same run workspace. Follow the approved plan, keep the changes scoped, run targeted validation, and publish the run pull request when implementation is complete.",
        approval.artifact_version
    )
}

pub(crate) fn revision_delivery_prompt(instructions: &str) -> String {
    format!(
        "The plan gate requested revisions. Apply these instructions verbatim to the run plan artifact, then end the turn without implementing:\n\n{instructions}\n\nUpdate the plan artifact and end the turn."
    )
}

pub(crate) async fn current_plan_artifact_id_for_workspace(
    session_repo: &Arc<dyn IdeationSessionRepository>,
    workspace: &AgentConversationWorkspace,
) -> AppResult<Option<String>> {
    let Some(session_id) = workspace.linked_ideation_session_id.as_ref() else {
        return Ok(None);
    };
    let Some(session) = session_repo.get_by_id(session_id).await? else {
        return Ok(None);
    };
    Ok(session
        .plan_artifact_id
        .as_ref()
        .map(|artifact_id| artifact_id.as_str().to_string()))
}

pub(crate) async fn matching_plan_approval_for_workspace(
    session_repo: &Arc<dyn IdeationSessionRepository>,
    approval_repo: &Arc<dyn PlanArtifactApprovalRepository>,
    workspace: &AgentConversationWorkspace,
) -> AppResult<Option<PlanArtifactApproval>> {
    let Some(session_id) = workspace.linked_ideation_session_id.as_ref() else {
        return Ok(None);
    };
    let Some(session) = session_repo.get_by_id(session_id).await? else {
        return Ok(None);
    };
    let Some(plan_artifact_id) = session.plan_artifact_id.as_ref() else {
        return Ok(None);
    };
    let Some(approval) = approval_repo.get_by_session(session_id).await? else {
        return Ok(None);
    };

    if approval.artifact_id == *plan_artifact_id {
        Ok(Some(approval))
    } else {
        Ok(None)
    }
}

pub(crate) async fn clear_plan_phase_publication_metadata(
    run_repo: &Arc<dyn AutomationRunRepository>,
    workspace_repo: &Arc<dyn AgentConversationWorkspaceRepository>,
    run: &AutomationRun,
    workspace: &AgentConversationWorkspace,
) -> AppResult<()> {
    run_repo.clear_publication_metadata(&run.id).await?;

    if workspace.publication_pr_number.is_none()
        && workspace.publication_pr_url.is_none()
        && workspace.publication_pr_status.is_none()
        && workspace.publication_push_status.is_none()
    {
        return Ok(());
    }

    let mut updated = workspace.clone();
    updated.publication_pr_number = None;
    updated.publication_pr_url = None;
    updated.publication_pr_status = None;
    updated.publication_push_status = None;
    updated.updated_at = Utc::now();
    workspace_repo.create_or_update(updated).await?;
    Ok(())
}

pub(crate) async fn refresh_plan_park_baseline(
    run_repo: &Arc<dyn AutomationRunRepository>,
    run: &AutomationRun,
    plan_artifact_id: Option<String>,
) -> AppResult<()> {
    if run.plan_last_parked_artifact_id == plan_artifact_id {
        return Ok(());
    }

    let next_round = run.plan_revision_round.saturating_add(1);
    run_repo
        .set_plan_last_parked_artifact_id(&run.id, plan_artifact_id)
        .await?;
    run_repo
        .set_plan_revision_round(&run.id, next_round)
        .await?;

    if run.plan_judge_state != AutomationPlanJudgeState::None {
        run_repo
            .compare_and_swap_plan_judge_state(
                &run.id,
                run.plan_judge_state,
                AutomationPlanJudgeState::None,
                None,
                None,
            )
            .await?;
    }

    Ok(())
}

pub(crate) async fn arm_plan_reminder(
    run_repo: &Arc<dyn AutomationRunRepository>,
    run: &AutomationRun,
) -> AppResult<()> {
    run_repo
        .set_plan_reminder_count(&run.id, run.plan_reminder_count.saturating_add(1))
        .await?;
    run_repo
        .set_agent_phase_started_at(&run.id, Some(Utc::now()))
        .await?;
    Ok(())
}
