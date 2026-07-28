use std::sync::Arc;

use async_trait::async_trait;

use crate::application::plan_verification_service::PlanVerificationStatusKind;
use crate::domain::agents::AgentHarnessKind;
use crate::domain::entities::{
    AgentConversationWorkspace, AutomationPlanJudgeState, AutomationRun, ChatConversationId,
    IdeationSessionId, VerificationStatus,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, AutomationRunRepository, IdeationSessionRepository,
    PlanArtifactApproval, PlanArtifactApprovalRepository,
};
use crate::error::AppResult;

use super::transition::AutomationTransitionService;

pub(crate) const AUTOMATION_PLAN_REMINDER_PROMPT: &str = r#"The automation run is still in its planning phase. Continue from the current context, write the run plan artifact with the scope, files to inspect or change, approach, risks, and how it advances the current goal item, then end the turn. Do not begin implementation in this turn."#;
pub(crate) const PLAN_JUDGE_FAILED_PAUSED_REASON_CODE: &str = "plan_judge_failed";
pub(crate) const PLAN_REVISION_EXHAUSTED_PAUSED_REASON_CODE: &str = "plan_revision_exhausted";
pub(crate) const PLAN_RESUME_FAILED_ERROR_CODE: &str = "plan_resume_failed";
pub(crate) const AUTOMATION_PLAN_GATE_TRIGGER_RUN_NOW_ERROR_CODE: &str =
    "[ralphx:automation_plan_gate_paused]";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResumeDelivery {
    Delivered,
    QueuedAndPurged,
}

#[async_trait]
pub trait AutomationRunResumer: Send + Sync {
    async fn is_agent_running(&self, conversation_id: &ChatConversationId) -> AppResult<bool>;

    async fn is_ideation_agent_running(&self, session_id: &IdeationSessionId) -> AppResult<bool>;

    async fn launches_paused(&self) -> AppResult<bool>;

    async fn switch_to_edit(&self, conversation_id: &ChatConversationId) -> AppResult<()>;

    async fn switch_to_ideation(&self, conversation_id: &ChatConversationId) -> AppResult<()>;

    async fn resume_with_prompt(
        &self,
        conversation_id: &ChatConversationId,
        prompt: &str,
    ) -> AppResult<ResumeDelivery>;

    async fn resume_ideation_with_prompt(
        &self,
        session_id: &IdeationSessionId,
        prompt: &str,
    ) -> AppResult<ResumeDelivery>;
}

pub(crate) fn ideation_bridge_delivery_prompt(approval: &PlanArtifactApproval) -> String {
    format!(
        "<auto-propose>Automation plan v{} is verified and approved. Read the current plan, run the cross-project check, create atomic implementation task proposals with explicit dependencies, analyze the dependency graph, and finalize all proposals into executable tasks. Do not ask for another confirmation; this approved automation bridge is the authorization boundary.</auto-propose>",
        approval.artifact_version
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationPlanVerificationStartRequest {
    pub session_id: IdeationSessionId,
    pub artifact_id: String,
    pub provider_harness: Option<AgentHarnessKind>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutomationPlanVerificationStartOutcome {
    Started {
        generation: i32,
    },
    AlreadyInProgress {
        generation: i32,
    },
    AlreadyTerminal {
        generation: i32,
        status: VerificationStatus,
    },
    Unavailable {
        detail: String,
    },
}

#[async_trait]
pub trait AutomationPlanVerificationStarter: Send + Sync {
    async fn start_verification(
        &self,
        request: AutomationPlanVerificationStartRequest,
    ) -> AppResult<AutomationPlanVerificationStartOutcome>;

    async fn verification_status(
        &self,
        request: &AutomationPlanVerificationStartRequest,
    ) -> AppResult<PlanVerificationStatusKind>;
}

#[derive(Debug, Default)]
pub struct NoopAutomationPlanVerificationStarter;

#[async_trait]
impl AutomationPlanVerificationStarter for NoopAutomationPlanVerificationStarter {
    async fn start_verification(
        &self,
        _request: AutomationPlanVerificationStartRequest,
    ) -> AppResult<AutomationPlanVerificationStartOutcome> {
        Ok(AutomationPlanVerificationStartOutcome::Unavailable {
            detail: "automation plan verification starter is not configured".to_string(),
        })
    }

    async fn verification_status(
        &self,
        _request: &AutomationPlanVerificationStartRequest,
    ) -> AppResult<PlanVerificationStatusKind> {
        Ok(PlanVerificationStatusKind::Unverified)
    }
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
        && workspace.publication_pushed_sha.is_none()
    {
        return Ok(());
    }

    workspace_repo
        .update_publication(&workspace.conversation_id, None, None, None, None)
        .await?;
    workspace_repo
        .clear_publication_pushed_sha(&workspace.conversation_id)
        .await?;
    Ok(())
}

pub(crate) async fn refresh_plan_park_baseline(
    transition_service: &AutomationTransitionService,
    run_repo: &Arc<dyn AutomationRunRepository>,
    run: &AutomationRun,
    plan_artifact_id: Option<String>,
) -> AppResult<bool> {
    if run.plan_last_parked_artifact_id == plan_artifact_id {
        return Ok(false);
    }

    let next_round = run.plan_revision_round.saturating_add(1);
    run_repo
        .set_plan_last_parked_artifact_id(&run.id, plan_artifact_id)
        .await?;
    run_repo
        .set_plan_revision_round(&run.id, next_round)
        .await?;
    run_repo
        .set_plan_pending_instructions(&run.id, None)
        .await?;

    if run.plan_judge_state != AutomationPlanJudgeState::None {
        transition_service
            .transition_plan_judge_state(
                &run.id,
                run.plan_judge_state,
                AutomationPlanJudgeState::None,
                None,
                None,
            )
            .await?;
    }

    Ok(true)
}
