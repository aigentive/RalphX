use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;

use crate::domain::entities::{
    AgentConversationWorkspace, AutomationPlanJudgeState, AutomationRun, ChatConversationId,
};
use crate::domain::repositories::{AutomationRunRepository, IdeationSessionRepository};
use crate::error::AppResult;

pub(crate) const AUTOMATION_PLAN_REMINDER_PROMPT: &str = r#"The automation run is still in its planning phase. Continue from the current context, write the run plan artifact with the scope, files to inspect or change, approach, risks, and how it advances the current goal item, then end the turn. Do not begin implementation in this turn."#;

#[async_trait]
pub trait AutomationRunResumer: Send + Sync {
    async fn is_agent_running(&self, conversation_id: &ChatConversationId) -> AppResult<bool>;

    async fn launches_paused(&self) -> AppResult<bool>;

    async fn resume_with_prompt(
        &self,
        conversation_id: &ChatConversationId,
        prompt: &str,
    ) -> AppResult<()>;
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
